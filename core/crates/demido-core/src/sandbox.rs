//! Running a program where it can do little harm.
//!
//! A model asks to run a command; Demido runs it inside a sandbox made by the operating system. What is
//! inside is decided here, in code, and a program cannot argue with it:
//!
//! - **The session's folder**, to read and to change. Nothing else can be changed.
//! - **The system's own programs** (`/usr`, `/etc`, `/opt` and the like), to read and run.
//! - **Not the rest of the person's home.** Their documents, keys, browser and mail are not there at all.
//!   The folders that hold programs they installed for themselves (what is on their `PATH`, and the usual
//!   homes of Rust, Node, Python and Go) are there to read, so that a build or a test still runs.
//! - **No network.** Not a filter: there is no network inside.
//! - **A clean environment.** No variable of the person's shell comes along; that is where keys live.
//!
//! On Linux this is bubblewrap, a small program most distributions ship, which makes these walls from the
//! kernel's own namespaces without any special rights (research note 14: it is what the two harnesses I
//! could check use). It is not carried inside Demido: it is found on the computer, and if it is not there,
//! or the system does not let it work, there is no sandbox, Demido says so, and the levels ask instead.
//!
//! On macOS it is the system's own Seatbelt, through `/usr/bin/sandbox-exec` with a profile written here
//! (`seatbelt`), as every harness checked in note 05 does it. There the walls are rules rather than an
//! empty world: everything on the computer can be read except where people keep their things (the homes,
//! other disks, the shared and per-user temporary folders), the folder and a temporary folder of the
//! command's own can be changed, and nothing else; no network at all. Apple marks `sandbox-exec` as
//! deprecated and still ships it; if a later macOS drops it, there is no sandbox, and the levels ask.
//!
//! On Linux there is also a filter on what a program may ask of the kernel (seccomp, `kernel_filter`): the
//! parts of it that programs a model runs have no need of, and that have been the way out of sandboxes, say
//! no. Nor can it make namespaces of its own inside (`--disable-userns`).
//!
//! On Windows it is a user of Demido's own, made once at setup with an administrator's rights, and a
//! write-restricted token (`windows`, DECISIONS T27): the person's profile is not there to read, nothing
//! outside the folder can be changed, and the firewall shuts that user's network. Weaker than the other two
//! in what it lets be read: what Windows lets every user read stays readable. Until it is set up, a command
//! runs unsandboxed, and so every level but Autonomous asks first.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncReadExt;
use utoipa::ToSchema;

#[cfg(windows)]
mod job;
#[cfg(windows)]
pub mod windows;

/// What a program's output is cut to before it is kept: far more than a person scrolls, less than fills a disk.
const MOST_KEPT: usize = 2_000_000;
/// What a model is shown of it (`tools::output`): what went wrong, the ending, the beginning.
const MOST_SHOWN: usize = 6_000;

/// The sandbox this computer has, if it has one.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Sandbox {
    /// `bubblewrap`, or `none`.
    pub kind: String,
    /// The program that makes it. `null` when there is none.
    pub program: Option<String>,
    /// When there is none: why, and what would give this computer one.
    pub why_not: Option<String>,
    /// Whether the person can set one up from the window (Windows: once, with an administrator's rights).
    #[serde(default)]
    pub can_set_up: bool,
    /// Demido's folder, where a sandbox that is set up keeps what it needs (Windows).
    #[serde(skip)]
    pub(crate) home: Option<PathBuf>,
}

/// What came of running a program.
#[derive(Clone, Debug, PartialEq)]
pub struct Ran {
    /// What it printed, both streams, cut to what is kept.
    pub output: String,
    /// The number it ended with. `None`: it was stopped before it ended.
    pub code: Option<i32>,
    /// Why it was stopped, if it was: it ran too long, or the person stopped the answer.
    pub stopped: Option<String>,
    pub took_ms: u64,
}

impl Ran {
    /// What the model is told: how it ended, and the short view of what it printed (`tools::output`), with
    /// what was left out marked by its line numbers.
    pub fn told(&self) -> String {
        let how = match (&self.stopped, self.code) {
            (Some(why), _) => why.clone(),
            (None, Some(0)) => "It ran, and ended without an error (exit code 0).".into(),
            (None, Some(code)) => format!("It ran, and ended with an error (exit code {code})."),
            (None, None) => "It was ended from outside before it finished.".into(),
        };
        let whole = crate::tools::output::as_a_terminal_shows(&self.output);
        if whole.trim().is_empty() {
            return format!("{how} It printed nothing.");
        }
        let shown = crate::tools::output::shortened(&whole, MOST_SHOWN);
        if shown.left_out == 0 {
            return format!("{how}\n{}", shown.text);
        }
        format!("{how}\n{}\n({} of its {} lines are not shown here. {}(from_line) gives them.)", shown.text, shown.left_out, shown.lines, crate::tools::READ_OUTPUT)
    }
}

impl Sandbox {
    fn none(why: impl Into<String>) -> Self {
        Self { kind: "none".into(), program: None, why_not: Some(why.into()), can_set_up: false, home: None }
    }

    /// No sandbox, because nobody has looked for one yet.
    pub fn not_looked_for() -> Self {
        Self::none("No sandbox was looked for.")
    }

    /// Whether a program run through this cannot leave the folder or reach the network.
    pub fn contains(&self) -> bool {
        self.kind != "none"
    }

    /// Looks for a sandbox on this computer and tries it once. Costs a few milliseconds.
    pub fn find() -> Self {
        #[cfg(target_os = "linux")]
        {
            let Some(program) = on_path("bwrap") else {
                return Self::none("bubblewrap is not installed. Your distribution has it as a package called bubblewrap.");
            };
            let found = Self { kind: "bubblewrap".into(), program: Some(program.to_string_lossy().into_owned()), why_not: None, can_set_up: false, home: None };
            // Installed is not the same as working: some systems do not let ordinary programs make the walls.
            let folder = std::env::temp_dir();
            let tried = found.command(&folder, "exit 0").and_then(|(mut c, _)| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string()));
            match tried {
                Ok(out) if out.status.success() => found,
                Ok(out) => Self::none(format!(
                    "bubblewrap is installed, and this system does not let it work: {}. On Ubuntu 24.04 and later that is the system's rule about user namespaces.",
                    String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("it said nothing")
                )),
                Err(e) => Self::none(format!("bubblewrap is installed, and could not be started: {e}.")),
            }
        }
        #[cfg(target_os = "macos")]
        {
            if !Path::new(SANDBOX_EXEC).is_file() {
                return Self::none("This Mac has no /usr/bin/sandbox-exec, which macOS has long had: Demido cannot make walls here.");
            }
            let found = Self { kind: "seatbelt".into(), program: Some(SANDBOX_EXEC.into()), why_not: None, can_set_up: false, home: None };
            // Its rules are read when it starts: a rule this macOS does not know is a failure here, not later.
            let folder = std::env::temp_dir();
            let tried = found.command(&folder, "exit 0").and_then(|(mut c, _scratch)| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string()));
            match tried {
                Ok(out) if out.status.success() => found,
                Ok(out) => Self::none(format!(
                    "sandbox-exec did not take Demido's rules on this version of macOS: {}.",
                    String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("it said nothing")
                )),
                Err(e) => Self::none(format!("sandbox-exec could not be started: {e}.")),
            }
        }
        #[cfg(windows)]
        {
            Self::none("The sandbox on Windows is looked for with Demido's folder.")
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Self::none("A sandbox is built for Linux, macOS and Windows. On this system a command runs without one, so Demido asks before each.")
        }
    }

    /// As `find`, and on Windows the sandbox set up for Demido's folder `home`, run through `program` (Demido's
    /// own: `demido sandbox-launch`). Tried once, as on the other systems.
    pub fn find_for(home: &Path, program: Option<PathBuf>) -> Self {
        #[cfg(windows)]
        {
            let unset = |why: String| Self { can_set_up: true, home: Some(home.to_path_buf()), ..Self::none(why) };
            let Some(program) = program else { return Self::none("Demido cannot find its own program, which the sandbox runs through.") };
            if let Err(why) = windows::ready(home) {
                return unset(format!(
                    "{why} Setting it up takes a minute, once, and Windows asks for an administrator: Demido then makes a user of its own that programs run as, which cannot read your profile, change anything outside the session's folder, or reach the network."
                ));
            }
            let found = Self { kind: "windows-user".into(), program: Some(program.to_string_lossy().into_owned()), why_not: None, can_set_up: true, home: Some(home.to_path_buf()) };
            let folder = home.join("sandbox").join("tried");
            let _ = std::fs::create_dir_all(&folder);
            let tried = found.command(&folder, "exit 0").and_then(|(mut c, _)| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string()));
            match tried {
                Ok(out) if out.status.success() => found,
                Ok(out) => unset(format!(
                    "The sandbox is set up and did not work: {}. Setting it up again may mend it.",
                    String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("it said nothing")
                )),
                Err(e) => unset(format!("The sandbox is set up and could not be started: {e}.")),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (home, program);
            Self::find()
        }
    }

    /// Sets the sandbox up, where the person can (Windows: Windows asks for an administrator first), and gives
    /// back the sandbox there is now. Waits for it: call it where waiting is fine.
    pub fn set_up(&self) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let Some(home) = &self.home else { return Err("This sandbox cannot be set up from here.".into()) };
            windows::start_setup(home)?;
            Ok(Self::find_for(home, self.program.clone().map(PathBuf::from).or_else(|| std::env::current_exe().ok())))
        }
        #[cfg(not(windows))]
        {
            Err("This system's sandbox is not set up from Demido.".into())
        }
    }

    /// Takes a folder back from the sandbox, when no session uses it any more (Windows: the entries that shared
    /// it come off, and what the sandbox made in it is given to the person). In the background: on a large
    /// folder it takes a while. Elsewhere there is nothing to take back.
    pub fn give_back(&self, folder: &Path) {
        #[cfg(windows)]
        if let Some(home) = self.home.clone() {
            let folder = folder.to_path_buf();
            std::thread::spawn(move || {
                if let Err(why) = windows::give_back(&home, &folder) {
                    tracing::warn!("{} was not all given back from the sandbox: {why}", folder.display());
                }
            });
        }
        #[cfg(not(windows))]
        let _ = folder;
    }

    /// The command line that runs a shell script in `folder`, inside the sandbox when there is one, with the
    /// temporary folder made for it (on macOS), which goes when it is dropped.
    fn command(&self, folder: &Path, script: &str) -> Result<(std::process::Command, Option<Scratch>), String> {
        let folder = folder.canonicalize().map_err(|e| format!("the session's folder cannot be opened: {e}"))?;
        #[cfg(target_os = "linux")]
        if let Some(program) = &self.program {
            let mut command = std::process::Command::new(program);
            command.args(walls(&folder));
            // The filter goes to bubblewrap on a pipe, as its descriptor 3, and is put in place just before
            // the program starts. Where none can be made (a processor it is not written for), there is none.
            let mut kept = Scratch::none();
            if let Ok(filter) = kernel_filter() {
                let read = filter_pipe(&filter).map_err(|e| format!("the kernel filter could not be handed over: {e}"))?;
                let fd = std::os::fd::AsRawFd::as_raw_fd(&read);
                // SAFETY: only async-signal-safe calls between fork and exec: dup2 and fcntl.
                unsafe {
                    std::os::unix::process::CommandExt::pre_exec(&mut command, move || {
                        if fd == 3 {
                            if libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                                return Err(std::io::Error::last_os_error());
                            }
                        } else if libc::dup2(fd, 3) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                command.args(["--seccomp", "3"]);
                kept.1 = Some(read);
            }
            command.args(["--", "/bin/sh", "-c", script]);
            return Ok((command, Some(kept)));
        }
        #[cfg(target_os = "macos")]
        if let Some(program) = &self.program {
            // A temporary folder of its own, as /tmp is one on Linux: the shared ones are not inside.
            let scratch = Scratch::new()?;
            let home = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute());
            let tools = tools_at_home(home.as_deref());
            let walls = seatbelt::walls(&folder, scratch.folder(), home.as_deref(), &tools);
            let mut command = std::process::Command::new(program);
            command.arg("-p").arg(&walls.profile);
            for (name, value) in &walls.params {
                let mut define = std::ffi::OsString::from(format!("-D{name}="));
                define.push(value);
                command.arg(define);
            }
            command.args(["--", "/bin/sh", "-c", script]).current_dir(&folder).env_clear();
            let path_inside: Vec<PathBuf> = std::env::var_os("PATH").iter().flat_map(std::env::split_paths).filter(|dir| seatbelt::readable(dir, home.as_deref(), &tools)).collect();
            command.env("PATH", std::env::join_paths(path_inside).unwrap_or_else(|_| "/usr/bin:/bin".into())).env("TERM", "dumb").env("TMPDIR", scratch.folder());
            if let Some(home) = &home {
                command.env("HOME", home);
            }
            for kept in ["LANG", "LC_ALL", "USER", "LOGNAME"] {
                if let Some(value) = std::env::var_os(kept) {
                    command.env(kept, value);
                }
            }
            return Ok((command, Some(scratch)));
        }
        // The sandbox of Windows: Demido itself, as the person, starts the command as the sandbox's user.
        #[cfg(windows)]
        if self.kind == "windows-user"
            && let (Some(program), Some(home)) = (&self.program, &self.home)
        {
            use std::os::windows::process::CommandExt;
            let mut command = std::process::Command::new(program);
            command
                .arg("sandbox-launch")
                .arg("--home")
                .arg(home)
                .arg("--folder")
                .arg(windows::plain(&folder))
                .arg("--script")
                .arg(windows::hex(script.as_bytes()))
                .current_dir(&folder)
                .creation_flags(job::STARTED_WAITING);
            return Ok((command, None));
        }
        // No sandbox: the plain shell, in the folder, with the person's own environment. Asked for first.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // `cmd` reads its line its own way, not as programs usually do: the script is given to it as it
            // is (`/S` takes off only the outer quotes), and the person's AutoRun commands are not run (`/D`).
            // Started waiting, so that it is in its job (`run`) before it can start anything.
            let mut command = std::process::Command::new("cmd");
            command.args(["/D", "/S", "/C"]).raw_arg(format!("\"{script}\"")).current_dir(&folder).creation_flags(job::STARTED_WAITING);
            Ok((command, None))
        }
        #[cfg(not(windows))]
        {
            let mut command = std::process::Command::new("/bin/sh");
            command.args(["-c", script]).current_dir(&folder);
            Ok((command, None))
        }
    }

    /// Runs a shell script in the folder and waits for it, for no longer than `at_most`. Setting `stop`
    /// ends it early. What it printed is kept up to a size; the rest is dropped, and said to be.
    pub async fn run(&self, folder: &Path, script: &str, at_most: Duration, stop: &AtomicBool) -> Result<Ran, String> {
        let started = Instant::now();
        // The temporary folder made for it, if any, goes when this ends, however it ends.
        let (command, _scratch) = self.command(folder, script)?;
        let mut command = tokio::process::Command::from(command);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        // A group of its own, so that ending it ends everything it started. Without this, and without a
        // sandbox, a stopped script would leave what it started running.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|e| format!("the command could not be started: {e}"))?;
        // On Windows everything it starts goes into a job of its own, so that ending it ends all of that.
        #[cfg(windows)]
        let held = job::Job::holding(child.raw_handle().map(|h| h as _), child.id().unwrap_or(0));
        let (mut out, mut err) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
        // Both streams are read as they come, so that a program that prints a lot does not wait on a full
        // pipe. What was read is kept where it can be taken even if a stream never closes.
        let read = std::sync::Arc::new(std::sync::Mutex::new((Vec::<u8>::new(), 0usize)));
        let reading = tokio::spawn({
            let read = read.clone();
            async move {
                let (mut a, mut b) = ([0u8; 8192], [0u8; 8192]);
                let (mut out_open, mut err_open) = (true, true);
                while out_open || err_open {
                    let got = tokio::select! {
                        n = out.read(&mut a), if out_open => { let n = n.unwrap_or(0); out_open = n > 0; &a[..n] }
                        n = err.read(&mut b), if err_open => { let n = n.unwrap_or(0); err_open = n > 0; &b[..n] }
                    };
                    let mut read = read.lock().unwrap_or_else(|e| e.into_inner());
                    let room = MOST_KEPT.saturating_sub(read.0.len()).min(got.len());
                    read.0.extend_from_slice(&got[..room]);
                    read.1 += got.len() - room;
                }
            }
        });
        let mut stopped = None;
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.ok(),
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if stop.load(Ordering::Relaxed) {
                        stopped = Some("It was stopped before it finished, because the answer was stopped.".to_string());
                    } else if started.elapsed() >= at_most {
                        stopped = Some(format!("It ran for {} seconds and was stopped: that is as long as a command may run.", at_most.as_secs()));
                    }
                    if stopped.is_some() {
                        // The whole group, then the program itself. A sandbox also ends everything inside it when its first program is ended.
                        #[cfg(unix)]
                        if let Some(pid) = child.id() {
                            let _ = std::process::Command::new("kill").args(["-KILL", "--", &format!("-{pid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
                        }
                        #[cfg(windows)]
                        if let Some(job) = &held {
                            job.end();
                        }
                        let _ = child.kill().await;
                        break None;
                    }
                }
            }
        };
        // What it printed last is given a moment to arrive. A stream that something still holds open is not waited for.
        let _ = tokio::time::timeout(Duration::from_millis(if stopped.is_some() { 500 } else { 2_000 }), reading).await;
        let (kept, dropped) = std::mem::take(&mut *read.lock().unwrap_or_else(|e| e.into_inner()));
        let mut output = String::from_utf8_lossy(&kept).into_owned();
        if dropped > 0 {
            output.push_str(&format!("\n… (it printed {dropped} more bytes, which were not kept)"));
        }
        Ok(Ran { output, code: status.and_then(|s| s.code()), stopped, took_ms: started.elapsed().as_millis() as u64 })
    }
}

/// What one command needs kept until it has started and ended: a temporary folder of its own (macOS),
/// deleted with everything in it when this is dropped, and the pipe its kernel filter comes on (Linux).
pub(crate) struct Scratch(#[allow(dead_code)] Option<std::path::PathBuf>, #[cfg(target_os = "linux")] Option<std::os::fd::OwnedFd>);

impl Scratch {
    #[cfg(target_os = "macos")]
    fn new() -> Result<Self, String> {
        let place = std::env::temp_dir().join(format!("demido-sandbox-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&place).map_err(|e| format!("a temporary folder for it could not be made: {e}"))?;
        // Its real path: Seatbelt's rules are about real paths, and /var is /private/var on macOS.
        let real = place.canonicalize().map_err(|e| format!("its temporary folder could not be found again: {e}"))?;
        Ok(Self(Some(real)))
    }

    #[cfg(target_os = "linux")]
    fn none() -> Self {
        Self(None, None)
    }

    /// The temporary folder made for the command.
    #[cfg(target_os = "macos")]
    fn folder(&self) -> &std::path::Path {
        self.0.as_deref().expect("made with a folder")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(place) = &self.0 {
            let _ = std::fs::remove_dir_all(place);
        }
    }
}

/// Where Seatbelt is. Only this one: one found on the `PATH` could be anything.
#[cfg(target_os = "macos")]
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The folders of a person's home that are there to read (`TOOLS_AT_HOME`, and what of their `PATH` is in
/// their home), as their real paths, the ones that exist.
#[cfg(target_os = "macos")]
fn tools_at_home(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return vec![] };
    let mut shown: Vec<PathBuf> = TOOLS_AT_HOME.iter().chain(TOOLS_AT_HOME_MACOS.iter()).map(|t| home.join(t)).collect();
    shown.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths).filter(|p| p.starts_with(home) && p != home));
    let mut real: Vec<PathBuf> = shown.iter().filter_map(|d| d.canonicalize().ok()).filter(|d| d.is_dir()).collect();
    real.sort();
    real.dedup();
    real
}

/// What a program in the sandbox may not ask of the kernel: parts of it that programs a model runs have no
/// need of, and that have often been the way out of sandboxes. Asked anyway, the kernel says no (EPERM).
/// io_uring; following and reading other processes (ptrace: no debugger runs inside); the kernel's keyring,
/// which can hold the person's own secrets; BPF, performance counters and userfaultfd; mounting; loading
/// kernel code; reopening files by handle; and sockets to virtual machines (vsock).
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
const NOT_ASKED: [i64; 26] = [
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_ptrace,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_keyctl,
    libc::SYS_add_key,
    libc::SYS_request_key,
    libc::SYS_bpf,
    libc::SYS_perf_event_open,
    libc::SYS_userfaultfd,
    libc::SYS_mount,
    libc::SYS_umount2,
    libc::SYS_pivot_root,
    libc::SYS_fsopen,
    libc::SYS_fsmount,
    libc::SYS_fsconfig,
    libc::SYS_fspick,
    libc::SYS_move_mount,
    libc::SYS_open_tree,
    libc::SYS_kexec_load,
    libc::SYS_init_module,
    libc::SYS_finit_module,
    libc::SYS_delete_module,
    libc::SYS_open_by_handle_at,
];

/// The kernel filter, as the compiled program bubblewrap's `--seccomp` takes.
#[cfg(target_os = "linux")]
fn kernel_filter() -> Result<Vec<u8>, String> {
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Err("no kernel filter is written for this processor".into())
    }
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule, TargetArch};
        let arch = if cfg!(target_arch = "x86_64") { TargetArch::x86_64 } else { TargetArch::aarch64 };
        let mut rules: std::collections::BTreeMap<i64, Vec<SeccompRule>> = NOT_ASKED.iter().map(|n| (*n, vec![])).collect();
        let vsock = SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, libc::AF_VSOCK as u64).and_then(|c| SeccompRule::new(vec![c])).map_err(|e| e.to_string())?;
        rules.insert(libc::SYS_socket, vec![vsock]);
        let filter = SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch).map_err(|e| e.to_string())?;
        let program: BpfProgram = filter.try_into().map_err(|e: seccompiler::BackendError| e.to_string())?;
        Ok(program.iter().flat_map(|f| f.code.to_ne_bytes().into_iter().chain([f.jt, f.jf]).chain(f.k.to_ne_bytes())).collect())
    }
}

/// A pipe holding the filter, for the program to read: its reading end, the writing end already closed.
#[cfg(target_os = "linux")]
fn filter_pipe(filter: &[u8]) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    let mut fds = [0i32; 2];
    // SAFETY: pipe2 fills two descriptors, each owned once below.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: both were just made by pipe2 and are owned by nothing else.
    let (read, write) = unsafe { (std::os::fd::OwnedFd::from_raw_fd(fds[0]), std::os::fd::OwnedFd::from_raw_fd(fds[1])) };
    // A few hundred bytes: far less than a pipe holds, so this never waits.
    std::fs::File::from(write).write_all(filter)?;
    Ok(read)
}

/// The full path of a program, if it is on the `PATH`.
#[cfg(target_os = "linux")]
fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(program)).find(|p| p.is_file()))
}

/// What is at the top of the computer and is not shown to a program: where people keep their things, and
/// what the sandbox makes afresh.
#[cfg(target_os = "linux")]
const NOT_INSIDE: [&str; 13] = ["home", "root", "mnt", "media", "srv", "tmp", "proc", "dev", "run", "var", "boot", "efi", "lost+found"];

/// Folders in a person's home that hold programs they installed for themselves, and nothing private that I
/// know of. `.cargo` is not here as a whole: it holds the key to crates.io. Its parts that a build reads are.
#[cfg(unix)]
const TOOLS_AT_HOME: [&str; 14] =
    [".cargo/bin", ".cargo/registry", ".cargo/git", ".rustup", ".nvm", ".pyenv", ".bun", ".deno", ".volta", "go", ".local/bin", ".local/lib", ".local/share/pnpm", ".sdkman"];

/// The same, where macOS keeps them: pnpm's home.
#[cfg(target_os = "macos")]
const TOOLS_AT_HOME_MACOS: [&str; 1] = ["Library/pnpm"];

/// The walls on macOS: a Seatbelt profile, and the paths it names, given to `sandbox-exec` as parameters
/// so that no path is ever written into the profile's own text.
///
/// Written for Demido. Codex's Seatbelt policy (openai/codex, Apache-2.0) was read first, for which of
/// macOS's services and controls a program needs and which are best shut; none of its text is copied.
/// Plain text, so that it is tested on every system; used on macOS.
#[cfg(any(test, target_os = "macos"))]
mod seatbelt {
    use std::path::{Path, PathBuf};

    /// Where people keep their things, and what is shared with other programs: not inside, to read or to
    /// change. The same places are also reached through `/System/Volumes/Data`, and are shut there too.
    pub(super) const NOT_INSIDE: [&str; 9] =
        ["/Users", "/Volumes", "/private/var/folders", "/private/var/tmp", "/private/tmp", "/private/var/root", "/private/var/log", "/Library/Keychains", "/Network"];

    /// The data volume's name for the places above.
    const DATA: &str = "/System/Volumes/Data";

    pub(super) struct Walls {
        pub profile: String,
        pub params: Vec<(String, PathBuf)>,
    }

    /// Whether a program in `dir` can be read inside, so that it belongs on the `PATH` there.
    #[cfg_attr(test, allow(dead_code))]
    pub(super) fn readable(dir: &Path, home: Option<&Path>, tools: &[PathBuf]) -> bool {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if tools.iter().any(|t| dir.starts_with(t)) {
            return true;
        }
        let shut = NOT_INSIDE.iter().any(|p| dir.starts_with(p) || dir.starts_with(format!("{DATA}{p}")));
        !shut && !home.is_some_and(|h| dir.starts_with(h))
    }

    /// The profile for a command in `folder` (its real path), with `scratch` as its temporary folder and
    /// `tools` the folders of the home it may read.
    pub(super) fn walls(folder: &Path, scratch: &Path, home: Option<&Path>, tools: &[PathBuf]) -> Walls {
        let mut params: Vec<(String, PathBuf)> = vec![("FOLDER".into(), folder.to_path_buf()), ("SCRATCH".into(), scratch.to_path_buf())];
        if let Some(home) = home {
            params.push(("HOME".into(), home.to_path_buf()));
        }
        for (i, tool) in tools.iter().enumerate() {
            params.push((format!("TOOL_{i}"), tool.clone()));
        }
        // The folders on the way to what may be read, so that a program can find its way there: their
        // names and dates only, never what is in them.
        let mut ancestors: Vec<PathBuf> =
            [folder, scratch].into_iter().chain(tools.iter().map(PathBuf::as_path)).flat_map(|p| p.ancestors().skip(1).map(Path::to_path_buf)).filter(|a| a.as_os_str() != "/").collect();
        ancestors.sort();
        ancestors.dedup();
        for (i, ancestor) in ancestors.iter().enumerate() {
            params.push((format!("ON_THE_WAY_{i}"), ancestor.clone()));
        }
        let quoted = |name: &str| format!("(subpath (param \"{name}\"))");
        let tools_read: String = (0..tools.len()).map(|i| format!(" {}", quoted(&format!("TOOL_{i}")))).collect();
        let on_the_way: String = (0..ancestors.len()).map(|i| format!(" (literal (param \"ON_THE_WAY_{i}\"))")).collect();
        let mut shut: Vec<String> = NOT_INSIDE.iter().flat_map(|p| [format!("(require-not (subpath \"{p}\"))"), format!("(require-not (subpath \"{DATA}{p}\"))")]).collect();
        if home.is_some() {
            shut.push("(require-not (subpath (param \"HOME\")))".into());
        }
        let shut = shut.join(" ");
        let profile = format!(
            r#"(version 1)
; Demido's walls for a program a model runs. Nothing is allowed that is not allowed here.
(deny default)

; It may start programs, which keep these walls, and signal and look at what it started.
(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))

; What the system says about itself (its version, its processors), and who the user is.
(allow sysctl-read)
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.system.DirectoryService.libinfo_v1")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.logd")
  (global-name "com.apple.diagnosticd"))
(allow ipc-posix-sem)
(allow ipc-posix-shm-read* (ipc-posix-name "apple.shm.notification_center"))

; To read, and to load programs from: the whole computer but where people keep their things.
(allow file-read* file-test-existence file-map-executable
  (require-all (subpath "/") {shut}))
; Of those, what it needs: the folder, its own temporary folder, the person's own programs.
(allow file-read* file-test-existence file-map-executable {folder} {scratch}{tools_read})
(allow file-read-metadata file-test-existence{on_the_way})

; To change: the folder and its own temporary folder, and nothing else.
(allow file-write* {folder} {scratch})
(allow file-write-data file-ioctl
  (literal "/dev/null") (literal "/dev/zero") (literal "/dev/dtracehelper") (subpath "/dev/fd"))

; The folder itself stays where it is: what is in it may change, not the folder.
(deny file-write-unlink (literal (param "FOLDER")))
; These change a file through a descriptor opened only to read it.
(deny system-fcntl (fcntl-command 80 110))
; No network of any kind.
(deny network*)
"#,
            folder = quoted("FOLDER"),
            scratch = quoted("SCRATCH"),
        );
        Walls { profile, params }
    }
}

/// The arguments that tell bubblewrap what is inside.
#[cfg(target_os = "linux")]
fn walls(folder: &Path) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut args: Vec<OsString> = Vec::new();
    let mut add = |parts: &[&std::ffi::OsStr]| args.extend(parts.iter().map(|p| p.to_os_string()));
    let s = std::ffi::OsStr::new;
    // Its own view of processes, users, and the network (none); gone when Demido is gone. It cannot make
    // namespaces of its own inside: they are much of the kernel a program is not otherwise given.
    add(&[s("--unshare-all"), s("--unshare-user"), s("--disable-userns"), s("--die-with-parent"), s("--new-session"), s("--clearenv")]);
    add(&[s("--proc"), s("/proc"), s("--dev"), s("/dev"), s("--tmpfs"), s("/tmp"), s("--tmpfs"), s("/run"), s("--tmpfs"), s("/var"), s("--dir"), s("/var/tmp")]);
    // The system, to read: every folder at the top of the computer but the ones people keep their things in.
    let mut top: Vec<_> = std::fs::read_dir("/").into_iter().flatten().filter_map(|e| e.ok()).collect();
    top.sort_by_key(|e| e.file_name());
    for entry in top {
        let name = entry.file_name();
        if name.to_str().is_none_or(|n| n.starts_with('.') || NOT_INSIDE.contains(&n)) {
            continue;
        }
        let path = entry.path();
        match std::fs::read_link(&path) {
            // /bin -> usr/bin and the like stay what they are.
            Ok(target) => add(&[s("--symlink"), target.as_os_str(), path.as_os_str()]),
            Err(_) if path.is_dir() => add(&[s("--ro-bind"), path.as_os_str(), path.as_os_str()]),
            Err(_) => {}
        }
    }
    // A home that is there and empty, but for the programs the person installed for themselves.
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute());
    let mut path_inside: Vec<PathBuf> = Vec::new();
    if let Some(home) = &home {
        add(&[s("--dir"), home.as_os_str()]);
        let mut shown: Vec<PathBuf> = TOOLS_AT_HOME.iter().map(|t| home.join(t)).collect();
        // What is on the person's PATH inside their home, they run from a terminal too.
        shown.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths).filter(|p| p.starts_with(home) && p != home));
        shown.sort();
        shown.dedup();
        for dir in shown.iter().filter(|d| d.is_dir()) {
            add(&[s("--ro-bind"), dir.as_os_str(), dir.as_os_str()]);
        }
    }
    for dir in std::env::var_os("PATH").iter().flat_map(std::env::split_paths) {
        let hidden = dir.components().nth(1).and_then(|c| c.as_os_str().to_str()).is_some_and(|top| NOT_INSIDE.contains(&top));
        if (!hidden || home.as_ref().is_some_and(|h| dir.starts_with(h))) && !path_inside.contains(&dir) {
            path_inside.push(dir);
        }
    }
    // The folder, to read and to change, and to start in. Last, so that it is there wherever it lies.
    add(&[s("--bind"), folder.as_os_str(), folder.as_os_str(), s("--chdir"), folder.as_os_str()]);
    let path = std::env::join_paths(path_inside).unwrap_or_else(|_| "/usr/bin:/bin".into());
    add(&[s("--setenv"), s("PATH"), path.as_os_str(), s("--setenv"), s("TERM"), s("dumb"), s("--setenv"), s("TMPDIR"), s("/tmp")]);
    if let Some(home) = &home {
        add(&[s("--setenv"), s("HOME"), home.as_os_str()]);
    }
    for kept in ["LANG", "LC_ALL", "USER", "LOGNAME"] {
        if let Some(value) = std::env::var_os(kept) {
            add(&[s("--setenv"), s(kept), value.as_os_str()]);
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("demido-sandbox-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "hello\n").unwrap();
        dir
    }

    static NOT_STOPPED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn a_long_output_is_told_short_and_says_how_to_read_the_rest() {
        let long: String = (1..=2000).map(|i| format!("line {i} of a long story\n")).collect();
        let told = Ran { output: long, code: Some(0), stopped: None, took_ms: 3 }.told();
        assert!(told.starts_with("It ran, and ended without an error (exit code 0).\nline 1 of a long story\n(… 1998 more lines like these …)\nline 2000 of a long story"), "{told}");
        let ran = Ran { output: "\u{1b}[32mok\u{1b}[0m\n".into(), code: Some(0), stopped: None, took_ms: 3 };
        assert_eq!(ran.told(), "It ran, and ended without an error (exit code 0).\nok", "a short output is told whole, without colour codes");
        let varied: String = (1..=2000).map(|i| format!("{} {i}\n", ["alpha", "beta", "gamma"][i % 3])).collect();
        let told = Ran { output: varied, code: Some(1), stopped: None, took_ms: 3 }.told();
        assert!(told.contains("are left out]") && told.ends_with("lines are not shown here. read_output(from_line) gives them.)"), "{}", &told[told.len() - 200..]);
    }

    #[tokio::test]
    async fn a_command_runs_in_the_folder_and_its_output_and_ending_are_kept() {
        let (dir, sandbox) = (folder("run"), Sandbox::find());
        #[cfg(unix)]
        {
            let ran = sandbox.run(&dir, "cat hello.txt; echo to-stderr 1>&2; echo made > made.txt; exit 3", Duration::from_secs(20), &NOT_STOPPED).await.unwrap();
            assert_eq!((ran.code, ran.stopped.clone()), (Some(3), None), "{}", ran.output);
            assert!(ran.output.contains("hello") && ran.output.contains("to-stderr"), "{}", ran.output);
            assert_eq!(std::fs::read_to_string(dir.join("made.txt")).unwrap(), "made\n", "the folder can be changed");
            assert!(ran.told().starts_with("It ran, and ended with an error (exit code 3).\n"));
            // One that runs too long is ended, and what it printed until then is kept.
            let slow = sandbox.run(&dir, "echo started; sleep 30; echo never", Duration::from_millis(400), &NOT_STOPPED).await.unwrap();
            assert!(slow.code.is_none() && slow.stopped.as_deref().is_some_and(|why| why.contains("was stopped")) && slow.took_ms < 5_000, "{slow:?}");
            assert!(slow.output.contains("started") && !slow.output.contains("never"));
            // What a script started is ended with it: a file that something it started would write later never
            // appears. With a sandbox and without one: where there is none, the script's own group is ended.
            for (name, held) in [("found", sandbox.clone()), ("none", Sandbox::not_looked_for())] {
                let orphan = held.run(&dir, "(sleep 2; echo late > late.txt) & echo begun; sleep 30", Duration::from_millis(300), &NOT_STOPPED).await.unwrap();
                assert!(orphan.stopped.is_some() && orphan.output.contains("begun") && orphan.took_ms < 5_000, "{name}: {orphan:?}");
                tokio::time::sleep(Duration::from_millis(2_500)).await;
                assert!(!dir.join("late.txt").exists(), "{name}: what the script started was still running after it was stopped");
            }
            // And one that the person stops.
            let stop = AtomicBool::new(true);
            let ended = sandbox.run(&dir, "sleep 30", Duration::from_secs(60), &stop).await.unwrap();
            assert!(ended.stopped.unwrap().contains("the answer was stopped") && ended.took_ms < 5_000);
            // A flood of output does not hang it, and is not all kept.
            let flood = sandbox.run(&dir, "yes abcdefghij | head -c 3000000", Duration::from_secs(20), &NOT_STOPPED).await.unwrap();
            assert!(flood.code == Some(0) && flood.output.len() < MOST_KEPT + 200 && flood.output.contains("more bytes, which were not kept"), "{}", flood.output.len());
        }
        // On Windows, with no sandbox: `cmd`, given the script as it is written, quotes and all.
        #[cfg(windows)]
        {
            let ran = sandbox.run(&dir, "type hello.txt & echo to-stderr 1>&2 & echo \"quoted words\" & echo made> made.txt & exit /b 3", Duration::from_secs(20), &NOT_STOPPED).await.unwrap();
            assert_eq!((ran.code, ran.stopped.clone()), (Some(3), None), "{}", ran.output);
            assert!(ran.output.contains("hello") && ran.output.contains("to-stderr") && ran.output.contains("\"quoted words\""), "{}", ran.output);
            assert_eq!(std::fs::read_to_string(dir.join("made.txt")).unwrap().trim_end(), "made");
            // One that runs too long is ended, with what it printed until then.
            let slow = sandbox.run(&dir, "echo started & ping -n 30 127.0.0.1 > nul & echo never", Duration::from_millis(400), &NOT_STOPPED).await.unwrap();
            assert!(slow.code.is_none() && slow.stopped.is_some() && slow.took_ms < 5_000, "{slow:?}");
            assert!(slow.output.contains("started") && !slow.output.contains("never"), "{}", slow.output);
            // What it started is ended with it: a file that something it started would write later never appears.
            std::fs::write(dir.join("late.cmd"), "@ping -n 3 127.0.0.1 > nul\r\n@echo late> late.txt\r\n").unwrap();
            let orphan = sandbox.run(&dir, "start /b late.cmd & echo begun & ping -n 30 127.0.0.1 > nul", Duration::from_millis(500), &NOT_STOPPED).await.unwrap();
            assert!(orphan.stopped.is_some() && orphan.output.contains("begun") && orphan.took_ms < 5_000, "{orphan:?}");
            tokio::time::sleep(Duration::from_millis(4_000)).await;
            assert!(!dir.join("late.txt").exists(), "what the command started was still running after it was stopped");
        }
        let _ = sandbox;
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_macos_profile_names_no_path_in_its_text_and_shuts_the_homes() {
        let tools = [std::path::PathBuf::from("/Users/ada/.cargo/bin")];
        let w = seatbelt::walls(Path::new("/Users/ada/work \"q\""), Path::new("/private/var/folders/x/T/demido-sandbox-1"), Some(Path::new("/Users/ada")), &tools);
        // Paths go in as parameters: a quote in a folder's name cannot change a rule.
        assert!(!w.profile.contains("/Users/ada") && !w.profile.contains("work \\\"q") && !w.profile.contains("demido-sandbox-1"), "{}", w.profile);
        let named: Vec<(&str, &str)> = w.params.iter().map(|(k, v)| (k.as_str(), v.to_str().unwrap())).collect();
        assert!(named.contains(&("FOLDER", "/Users/ada/work \"q\"")) && named.contains(&("TOOL_0", "/Users/ada/.cargo/bin")) && named.contains(&("HOME", "/Users/ada")));
        // The way to the folder is there by name, not the rest of the home.
        assert!(named.contains(&("ON_THE_WAY_0", "/Users")) || named.iter().any(|(k, v)| k.starts_with("ON_THE_WAY") && *v == "/Users"));
        assert!(!named.iter().any(|(k, v)| k.starts_with("ON_THE_WAY") && *v == "/"));
        // Closed by default; the homes, other disks and shared temporary folders shut, by both their names.
        assert!(w.profile.contains("(deny default)") && w.profile.contains("(deny network*)"));
        for shut in ["/Users", "/Volumes", "/private/tmp", "/private/var/folders", "/System/Volumes/Data/Users"] {
            assert!(w.profile.contains(&format!("(require-not (subpath \"{shut}\"))")), "{shut}");
        }
        // Brackets balance: sandbox-exec reads it as one expression after another.
        let (open, close) = (w.profile.matches('(').count(), w.profile.matches(')').count());
        assert_eq!(open, close);
        assert!(seatbelt::readable(Path::new("/opt/homebrew/bin"), Some(Path::new("/Users/ada")), &tools));
        assert!(!seatbelt::readable(Path::new("/Users/ada/bin"), Some(Path::new("/Users/ada")), &tools));
        assert!(seatbelt::readable(Path::new("/Users/ada/.cargo/bin"), Some(Path::new("/Users/ada")), &tools));
    }

    /// The walls themselves. Only where there is a sandbox: on a computer without one this test says so and passes.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn inside_the_sandbox_there_is_the_folder_the_system_and_nothing_else() {
        let sandbox = Sandbox::find();
        if !sandbox.contains() {
            // Where the checks are run, a sandbox is set up first, and its absence is a failure.
            assert!(std::env::var_os("DEMIDO_EXPECT_SANDBOX").is_none(), "a sandbox was expected here: {}", sandbox.why_not.clone().unwrap_or_default());
            eprintln!("no sandbox on this computer, so its walls were not tried: {}", sandbox.why_not.unwrap_or_default());
            return;
        }
        let dir = folder("walls");
        let run = |script: &'static str| {
            let (sandbox, dir) = (sandbox.clone(), dir.clone());
            async move { sandbox.run(&dir, script, Duration::from_secs(20), &NOT_STOPPED).await.unwrap() }
        };
        // Something private in the home, and something outside the folder beside it.
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let private = home.join(format!(".demido-sandbox-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&private, "the key").unwrap();
        let beside = dir.parent().unwrap().join(format!("beside-{}", uuid::Uuid::new_v4()));
        std::fs::write(&beside, "not yours").unwrap();

        // The folder is there to read and change. The system's programs run.
        let inside = run("pwd; cat hello.txt; echo more >> hello.txt; ls /usr/bin | head -1").await;
        assert_eq!(inside.code, Some(0), "{}", inside.output);
        assert!(inside.output.contains(dir.canonicalize().unwrap().to_str().unwrap()) && inside.output.contains("hello"));
        assert_eq!(std::fs::read_to_string(dir.join("hello.txt")).unwrap(), "hello\nmore\n");

        // The person's home is empty; what lies beside the folder is not there; neither can be written.
        let hidden = run(&*Box::leak(
            format!("cat {} 2>&1; cat {} 2>&1; ls -A \"$HOME\" | wc -l; echo x > \"$HOME/planted\" 2>&1; echo x > {}.new 2>&1; echo done", private.display(), beside.display(), beside.display())
                .into_boxed_str(),
        ))
        .await;
        assert!(!hidden.output.contains("the key") && !hidden.output.contains("not yours"), "{}", hidden.output);
        assert!(!home.join("planted").exists() && !PathBuf::from(format!("{}.new", beside.display())).exists(), "nothing outside the folder was written");
        // On macOS the same files are also reached through the data volume's own name, and the homes can
        // not be listed.
        #[cfg(target_os = "macos")]
        {
            let script = format!("cat /System/Volumes/Data{} 2>&1; ls /Users 2>&1; echo listed=$?", private.display());
            let other = run(&*Box::leak(script.into_boxed_str())).await;
            assert!(!other.output.contains("the key") && !other.output.contains("listed=0"), "{}", other.output);
        }
        // The person's own programs still run: a build or a test is what a model runs most.
        if let Some(cargo) = std::env::var_os("PATH").iter().flat_map(std::env::split_paths).map(|d| d.join("cargo")).find(|p| p.is_file()) {
            let built = run(&*Box::leak(format!("{} --version", cargo.display()).into_boxed_str())).await;
            assert_eq!(built.code, Some(0), "cargo did not run inside: {}", built.output);
        }

        // The system cannot be changed, there is no network, and no variable of the person's shell came along.
        // (Set before the run in this process, the way a key would be set in a shell.)
        unsafe { std::env::set_var("DEMIDO_TEST_SECRET", "a-key") };
        let closed = run("touch /usr/planted 2>&1; touch /etc/planted 2>&1; echo secret=[$DEMIDO_TEST_SECRET]").await;
        assert!(!std::path::Path::new("/usr/planted").exists() && !std::path::Path::new("/etc/planted").exists());
        assert!(closed.output.contains("secret=[]"), "{}", closed.output);
        // On Linux, what a program may not ask of the kernel is refused, and it cannot make namespaces of
        // its own. (io_uring_setup is syscall 425 on both processors the filter is written for.)
        #[cfg(target_os = "linux")]
        if Path::new("/usr/bin/python3").exists() {
            let asked = run("/usr/bin/python3 -c \"import ctypes; c = ctypes.CDLL(None, use_errno=True); print('uring', c.syscall(425, 1, 0), ctypes.get_errno()); print('ptrace', c.ptrace(0, 0, 0, 0), ctypes.get_errno())\"; unshare -U true 2>&1; echo userns=$?").await;
            assert!(asked.output.contains("uring -1 1") && asked.output.contains("ptrace -1 1"), "{}", asked.output);
            assert!(!asked.output.contains("userns=0"), "{}", asked.output);
        }
        // What is written to /tmp inside never reaches the computer's own /tmp.
        let planted = format!("demido-sandbox-planted-{}", uuid::Uuid::new_v4());
        let tmp = run(&*Box::leak(format!("echo x > /tmp/{planted} 2>&1; echo inside > \"$TMPDIR/own\" && cat \"$TMPDIR/own\"").into_boxed_str())).await;
        assert!(!Path::new("/tmp").join(&planted).exists(), "{}", tmp.output);
        assert!(tmp.output.contains("inside"), "a temporary folder of its own works: {}", tmp.output);
        // The only way out of the network inside is the one that leads back to itself (Linux); on macOS no
        // connection can be made at all.
        #[cfg(target_os = "linux")]
        {
            let net = run("echo outside-links=$(grep -c -v -e 'lo:' -e '|' /proc/net/dev)").await;
            assert!(net.output.contains("outside-links=0"), "{}", net.output);
        }
        #[cfg(target_os = "macos")]
        {
            let net = run("/usr/bin/curl -s -m 5 -o /dev/null http://1.1.1.1/; echo net=$?").await;
            assert!(net.output.contains("net=") && !net.output.contains("net=0"), "{}", net.output);
        }

        std::fs::remove_file(&private).ok();
        std::fs::remove_file(&beside).ok();
        std::fs::remove_dir_all(dir).ok();
    }
}
