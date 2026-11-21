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
//! Not built: the same on macOS (Seatbelt) and Windows. There a command runs unsandboxed, and so every
//! level but Autonomous asks first. Not built either: a filter on what the program may ask of the kernel
//! (seccomp), which the harnesses above add on top.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncReadExt;
use utoipa::ToSchema;

/// What a program's output is cut to before it is kept: more than a person scrolls, less than fills a disk.
const MOST_KEPT: usize = 200_000;
/// What a model is shown of it: its start and its end.
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
    /// What the model is told: how it ended, and the start and the end of what it printed.
    pub fn told(&self) -> String {
        let how = match (&self.stopped, self.code) {
            (Some(why), _) => why.clone(),
            (None, Some(0)) => "It ran, and ended without an error (exit code 0).".into(),
            (None, Some(code)) => format!("It ran, and ended with an error (exit code {code})."),
            (None, None) => "It was ended from outside before it finished.".into(),
        };
        let output = self.output.trim();
        if output.is_empty() { format!("{how} It printed nothing.") } else { format!("{how}\n{}", shortened(output, MOST_SHOWN)) }
    }
}

/// A long text as its start and its end, with what was left out counted.
pub fn shortened(text: &str, at_most: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= at_most {
        return text.to_string();
    }
    let half = at_most / 2;
    let (start, end): (String, String) = (chars[..half].iter().collect(), chars[chars.len() - half..].iter().collect());
    // Cut at line ends where there are any, so that no line is shown as half of itself.
    let start = start.rfind('\n').map_or(start.as_str(), |at| &start[..at]);
    let end = end.find('\n').map_or(end.as_str(), |at| &end[at + 1..]);
    let left_out = text.lines().count().saturating_sub(start.lines().count() + end.lines().count());
    format!("{start}\n… ({left_out} lines are not shown) …\n{end}")
}

impl Sandbox {
    fn none(why: impl Into<String>) -> Self {
        Self { kind: "none".into(), program: None, why_not: Some(why.into()) }
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
            let found = Self { kind: "bubblewrap".into(), program: Some(program.to_string_lossy().into_owned()), why_not: None };
            // Installed is not the same as working: some systems do not let ordinary programs make the walls.
            let folder = std::env::temp_dir();
            let tried = found.command(&folder, "exit 0").and_then(|mut c| c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string()));
            match tried {
                Ok(out) if out.status.success() => found,
                Ok(out) => Self::none(format!(
                    "bubblewrap is installed, and this system does not let it work: {}. On Ubuntu 24.04 and later that is the system's rule about user namespaces.",
                    String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("it said nothing")
                )),
                Err(e) => Self::none(format!("bubblewrap is installed, and could not be started: {e}.")),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::none("A sandbox is built for Linux only so far. On this system a command runs without one, so Demido asks before each.")
        }
    }

    /// The command line that runs a shell script in `folder`, inside the sandbox when there is one.
    fn command(&self, folder: &Path, script: &str) -> Result<std::process::Command, String> {
        let folder = folder.canonicalize().map_err(|e| format!("the session's folder cannot be opened: {e}"))?;
        #[cfg(target_os = "linux")]
        if let Some(program) = &self.program {
            let mut command = std::process::Command::new(program);
            command.args(walls(&folder)).args(["--", "/bin/sh", "-c", script]);
            return Ok(command);
        }
        // No sandbox: the plain shell, in the folder, with the person's own environment. Asked for first.
        let mut command = if cfg!(windows) { std::process::Command::new("cmd") } else { std::process::Command::new("/bin/sh") };
        command.args([if cfg!(windows) { "/C" } else { "-c" }, script]).current_dir(&folder);
        Ok(command)
    }

    /// Runs a shell script in the folder and waits for it, for no longer than `at_most`. Setting `stop`
    /// ends it early. What it printed is kept up to a size; the rest is dropped, and said to be.
    pub async fn run(&self, folder: &Path, script: &str, at_most: Duration, stop: &AtomicBool) -> Result<Ran, String> {
        let started = Instant::now();
        let mut command = tokio::process::Command::from(self.command(folder, script)?);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        // A group of its own, so that ending it ends everything it started. Without this, and without a
        // sandbox, a stopped script would leave what it started running.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|e| format!("the command could not be started: {e}"))?;
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
#[cfg(target_os = "linux")]
const TOOLS_AT_HOME: [&str; 14] =
    [".cargo/bin", ".cargo/registry", ".cargo/git", ".rustup", ".nvm", ".pyenv", ".bun", ".deno", ".volta", "go", ".local/bin", ".local/lib", ".local/share/pnpm", ".sdkman"];

/// The arguments that tell bubblewrap what is inside.
#[cfg(target_os = "linux")]
fn walls(folder: &Path) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut args: Vec<OsString> = Vec::new();
    let mut add = |parts: &[&std::ffi::OsStr]| args.extend(parts.iter().map(|p| p.to_os_string()));
    let s = std::ffi::OsStr::new;
    // Its own view of processes, users, and the network (none); gone when Demido is gone.
    add(&[s("--unshare-all"), s("--die-with-parent"), s("--new-session"), s("--clearenv")]);
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

    #[cfg(unix)]
    static NOT_STOPPED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn a_long_output_is_shown_as_its_start_and_its_end() {
        let long: String = (1..=2000).map(|i| format!("line {i}\n")).collect();
        let shown = shortened(&long, 400);
        assert!(shown.starts_with("line 1\nline 2\n") && shown.trim_end().ends_with("line 2000"), "{shown}");
        assert!(shown.contains("lines are not shown) …") && shown.len() < 600);
        assert_eq!(shortened("short", 400), "short");
        let ran = Ran { output: "ok\n".into(), code: Some(0), stopped: None, took_ms: 3 };
        assert_eq!(ran.told(), "It ran, and ended without an error (exit code 0).\nok");
        assert_eq!(Ran { output: String::new(), code: Some(2), ..ran.clone() }.told(), "It ran, and ended with an error (exit code 2). It printed nothing.");
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
            let flood = sandbox.run(&dir, "yes abcdefghij | head -c 1000000", Duration::from_secs(20), &NOT_STOPPED).await.unwrap();
            assert!(flood.code == Some(0) && flood.output.len() < MOST_KEPT + 200 && flood.output.contains("more bytes, which were not kept"), "{}", flood.output.len());
        }
        let _ = sandbox;
        std::fs::remove_dir_all(dir).ok();
    }

    /// The walls themselves. Only where there is a sandbox: on a computer without one this test says so and passes.
    #[cfg(target_os = "linux")]
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

        // The system cannot be changed, there is no network, and no variable of the person's shell came along.
        // (Set before the run in this process, the way a key would be set in a shell.)
        unsafe { std::env::set_var("DEMIDO_TEST_SECRET", "a-key") };
        let closed = run("touch /usr/planted 2>&1; touch /etc/planted 2>&1; echo outside-links=$(grep -c -v -e 'lo:' -e '|' /proc/net/dev); echo secret=[$DEMIDO_TEST_SECRET]").await;
        assert!(!std::path::Path::new("/usr/planted").exists() && !std::path::Path::new("/etc/planted").exists());
        assert!(closed.output.contains("secret=[]"), "{}", closed.output);
        // The only way out of the network inside is the one that leads back to itself.
        assert!(closed.output.contains("outside-links=0"), "{}", closed.output);

        std::fs::remove_file(&private).ok();
        std::fs::remove_file(&beside).ok();
        std::fs::remove_dir_all(dir).ok();
    }
}
