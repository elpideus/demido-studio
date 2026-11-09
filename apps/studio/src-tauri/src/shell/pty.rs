//! A command running in a pseudo-terminal (ConPTY on Windows), as in a terminal window: programs
//! see a console, so they print what they print for a person and full-screen ones run. Nothing
//! types into it; a command that waits for input simply runs until its timeout.
//!
//! A command can also be left running: the model asks for that for a server or a watcher, and
//! the person can choose it for a command that keeps the chat waiting (a program that opened a
//! window, say). Its terminal then stays open, read and thrown away, until it ends or the app
//! closes.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::Shell;
use super::screen::{Screen, Shown};

/// The terminal's size: wide enough that tables and paths rarely wrap, tall enough for btop.
pub const COLS: u16 = 160;
pub const ROWS: u16 = 40;
/// How often a running command's output is passed on.
const REFRESH: Duration = Duration::from_millis(400);
/// How long the output may take to drain once the command has ended.
const DRAIN: Duration = Duration::from_secs(3);

pub struct Request<'a> {
    pub shell: &'a Shell,
    pub command: &'a str,
    pub dir: &'a Path,
    /// Added to the environment a new terminal window would get.
    pub env: Vec<(&'static str, OsString)>,
    pub timeout: Duration,
    /// Leave the command running after this long instead of stopping it at the timeout: a
    /// server, a watcher, anything meant to keep running.
    pub background: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// It finished by itself, with this exit code.
    Exited(u32),
    /// It was still running at the timeout and was stopped.
    TimedOut,
    /// The person stopped it.
    Stopped,
    /// It was still running and was left to run on: the request asked for that, or the person
    /// did. It runs until it ends or the app closes.
    LeftRunning,
}

/// A program a command started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub pid: u32,
    pub name: String,
}

#[derive(Debug)]
pub struct Outcome {
    pub ending: Ending,
    /// What the terminal showed at the end.
    pub shown: Shown,
    pub elapsed: Duration,
    /// The shell's process id, which on Linux and macOS is also its process group's.
    pub pid: Option<u32>,
    /// Programs the command left running when it finished that stay open: in a window of their
    /// own, or away from its terminal (nohup, output to a file). Linux only.
    pub left_open: Vec<Program>,
    /// Programs it left running in its terminal, which ended with it. Linux only.
    pub ended: Vec<Program>,
}

/// Runs the command. `on_output` gets what the terminal shows while it changes. `stop` ends the
/// command and keeps what it printed; `release` leaves it running and returns what it printed so
/// far; `cancel` (the turn was stopped) ends it and returns `None`.
pub async fn run(
    req: Request<'_>,
    stop: &CancellationToken,
    release: &CancellationToken,
    cancel: &CancellationToken,
    mut on_output: impl FnMut(&Shown),
) -> Result<Option<Outcome>, String> {
    let started = Instant::now();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("could not open a terminal: {e}"))?;

    let mut cmd = CommandBuilder::new(&req.shell.program);
    cmd.args(req.shell.args(req.command));
    cmd.cwd(req.dir);
    // No pager or password prompt waiting for keys nobody will press.
    cmd.env("GIT_PAGER", "cat");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(unix)]
    {
        cmd.env("TERM", "xterm-256color");
        // man, journalctl and systemctl page through less in a terminal: "cat" turns that off.
        cmd.env("PAGER", "cat");
        cmd.env("MANPAGER", "cat");
        cmd.env("SYSTEMD_PAGER", "cat");
    }
    for key in demido_core::platform::set_for_webview() {
        cmd.env_remove(key);
    }
    // From inside an AppImage, the person's shell gets their own environment, not the AppImage's.
    for (key, value) in demido_core::appimage::host_env() {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        }
    }
    for (key, value) in &req.env {
        cmd.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start {}: {e}", req.shell.name))?;
    drop(pair.slave);
    let tree = tree::Tree::adopt(&*child);
    let pid = child.process_id();
    // The terminal's own name (/dev/pts/N), to tell the programs still using it from those
    // that left it. Windows has no such name, and needs none (`leftovers`).
    #[cfg(unix)]
    let tty = pair.master.tty_name();
    #[cfg(not(unix))]
    let tty: Option<std::path::PathBuf> = None;
    let mut killer = child.clone_killer();

    let screen = Arc::new(Mutex::new(Screen::new(COLS.into(), ROWS.into())));
    let changed = Arc::new(AtomicBool::new(false));
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (drained_tx, drained) = oneshot::channel::<()>();
    {
        let (screen, changed) = (screen.clone(), changed.clone());
        std::thread::spawn(move || {
            pump(reader, writer, &screen, &changed);
            let _ = drained_tx.send(());
        });
    }
    let (exit_tx, mut exited) = oneshot::channel::<Option<u32>>();
    std::thread::spawn(move || {
        let mut child = child;
        let _ = exit_tx.send(child.wait().ok().map(|s| s.exit_code()));
    });

    // In the background, the time is how long to wait before leaving it running.
    let deadline = tokio::time::sleep(req.background.unwrap_or(req.timeout));
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let ending = loop {
        tokio::select! {
            code = &mut exited => break Some(Ending::Exited(code.ok().flatten().unwrap_or(1))),
            _ = &mut deadline => break Some(match req.background {
                Some(_) => Ending::LeftRunning,
                None => Ending::TimedOut,
            }),
            _ = stop.cancelled() => break Some(Ending::Stopped),
            _ = release.cancelled() => break Some(Ending::LeftRunning),
            _ = cancel.cancelled() => break None,
            _ = tick.tick() => {
                if changed.swap(false, Ordering::AcqRel) {
                    on_output(&screen.lock().shown());
                }
            }
        }
    };

    let master = pair.master;
    let mut left_open = Vec::new();
    let mut ended = Vec::new();
    let shown = match ending {
        Some(Ending::Exited(_)) => {
            (left_open, ended) = leftovers(&tree, tty.as_deref()).await;
            // A program the command opened in its own window stays open.
            tree.release();
            if left_open.is_empty() {
                // Closing the pseudo console ends what is still attached to it and lets the last
                // output through.
                let _ = tokio::time::timeout(DRAIN, tokio::task::spawn_blocking(move || drop(master))).await;
                let _ = tokio::time::timeout(DRAIN, drained).await;
            } else {
                // The programs left open keep a terminal that works, so writing to it does not
                // fail, until the last of them closes it.
                tokio::time::sleep(REFRESH).await;
                tokio::spawn(close_when_unused(master, drained));
            }
            screen.lock().shown()
        }
        Some(Ending::LeftRunning) => {
            let shown = screen.lock().shown();
            tree.end_with_app();
            tokio::spawn(async move {
                let _ = exited.await;
                let (left_open, _) = leftovers(&tree, tty.as_deref()).await;
                tree.ended();
                tree.release();
                if left_open.is_empty() {
                    let _ = tokio::task::spawn_blocking(move || drop(master)).await;
                } else {
                    close_when_unused(master, drained).await;
                }
            });
            shown
        }
        _ => {
            // The screen as it is, before a full-screen program is torn down.
            let shown = screen.lock().shown();
            tree.kill();
            let _ = killer.kill();
            let _ = tokio::time::timeout(DRAIN, exited).await;
            tokio::task::spawn_blocking(move || drop(master));
            shown
        }
    };
    Ok(ending.map(|ending| Outcome {
        ending,
        shown,
        elapsed: started.elapsed(),
        pid,
        left_open,
        ended,
    }))
}

/// Ends the commands left running; for when the app closes.
pub fn end_left_running() {
    tree::end_left_running();
}

/// Closes the terminal once nothing uses it any more: the reader has seen its end.
async fn close_when_unused(master: Box<dyn portable_pty::MasterPty + Send>, drained: oneshot::Receiver<()>) {
    let _ = drained.await;
    let _ = tokio::task::spawn_blocking(move || drop(master)).await;
}

/// How long a program a finished command left running in its terminal has to open a window
/// before it is ended: long enough for a Python or Java program to load its toolkit.
#[cfg(target_os = "linux")]
const WINDOW_WAIT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 5 });

/// Sorts out what a command left running when its shell ended (`cmd &`), as Windows does by
/// itself: a program in a window of its own stays open, and so does one that left the terminal
/// (nohup, its output sent to a file) or started a program in a session of its own, with whatever
/// belongs with it; the rest, still writing to the terminal, end with the command. A program that is still starting gets [`WINDOW_WAIT`] to open
/// its window. Returns those left open and those ended.
#[cfg(target_os = "linux")]
async fn leftovers(tree: &tree::Tree, tty: Option<&Path>) -> (Vec<Program>, Vec<Program>) {
    let Some(session) = tree.session() else {
        return Default::default();
    };
    let program = |p: &procs::Proc| Program {
        pid: p.pid as u32,
        name: p.name.clone(),
    };
    let deadline = Instant::now() + WINDOW_WAIT;
    loop {
        let (all, others): (Vec<procs::Proc>, Vec<procs::Proc>) =
            procs::all().into_iter().partition(|p| p.sid == session);
        // A launcher whose program went off into a session of its own (a sandbox, say) stays
        // with it.
        let launchers: std::collections::HashSet<i32> = others.iter().map(|p| p.ppid).collect();
        // Without the terminal's name every program counts as using it.
        let roots: Vec<i32> = all
            .iter()
            .filter(|p| {
                procs::has_window(p.pid)
                    || launchers.contains(&p.pid)
                    || !tty.is_none_or(|tty| procs::on_terminal(p.pid, tty))
            })
            .map(|p| p.pid)
            .collect();
        let kept = procs::with_relatives(&all, &roots);
        let rest: Vec<&procs::Proc> = all.iter().filter(|p| !kept.contains(&p.pid)).collect();
        if rest.is_empty() || Instant::now() >= deadline {
            for p in &rest {
                // SAFETY: signals one process of the command's own session; no memory is touched.
                unsafe {
                    libc::kill(p.pid, libc::SIGKILL);
                }
            }
            let left_open = all.iter().filter(|p| kept.contains(&p.pid)).map(program).collect();
            return (left_open, rest.into_iter().map(program).collect());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(not(target_os = "linux"))]
async fn leftovers(_tree: &tree::Tree, _tty: Option<&Path>) -> (Vec<Program>, Vec<Program>) {
    Default::default()
}

/// Plays the terminal's output onto the screen, answering its cursor position queries.
fn pump(
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    screen: &Mutex<Screen>,
    changed: &AtomicBool,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let reply = {
                    let mut screen = screen.lock();
                    let queries = screen.feed(&buf[..n]);
                    (queries > 0).then(|| screen.cursor_report().repeat(queries))
                };
                if let Some(reply) = reply {
                    let _ = writer.write_all(reply.as_bytes());
                    let _ = writer.flush();
                }
                changed.store(true, Ordering::Release);
            }
        }
    }
}

#[cfg(windows)]
mod tree {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        TerminateJobObject,
    };

    /// The command's processes, in a job object: stopping the command ends every program it
    /// started, and so does the app closing, however it closes. A command that finishes by
    /// itself releases them, so a program it opened in a window of its own stays open.
    pub struct Tree(Option<HANDLE>);

    // SAFETY: a job handle is a process-wide kernel handle, usable from any thread.
    unsafe impl Send for Tree {}
    unsafe impl Sync for Tree {}

    impl Tree {
        pub fn adopt(child: &(dyn portable_pty::Child + Send + Sync)) -> Self {
            let Some(process) = child.as_raw_handle() else {
                return Self(None);
            };
            unsafe {
                let Ok(job) = CreateJobObjectW(None, None) else {
                    return Self(None);
                };
                if set_limits(job, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)
                    && AssignProcessToJobObject(job, HANDLE(process)).is_ok()
                {
                    return Self(Some(job));
                }
                tracing::warn!("could not group a command's processes; stopping it may leave some running");
                let _ = CloseHandle(job);
                Self(None)
            }
        }

        pub fn kill(&self) {
            if let Some(job) = self.0 {
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                }
            }
        }

        pub fn release(&self) {
            if let Some(job) = self.0 {
                set_limits(job, JOB_OBJECT_LIMIT(0));
            }
        }

        /// The job ends the command when the app closes: nothing to do.
        pub fn end_with_app(&self) {}

        pub fn ended(&self) {}
    }

    /// The jobs end the commands left running as the app's handles to them close.
    pub fn end_left_running() {}

    impl Drop for Tree {
        fn drop(&mut self) {
            if let Some(job) = self.0.take() {
                unsafe {
                    let _ = CloseHandle(job);
                }
            }
        }
    }

    fn set_limits(job: HANDLE, flags: JOB_OBJECT_LIMIT) -> bool {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = flags;
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .is_ok()
        }
    }
}

#[cfg(unix)]
mod tree {
    /// The command's process group: the pseudo-terminal made the shell the leader of a session
    /// of its own, and the programs it runs belong to it.
    pub struct Tree(Option<i32>);

    /// The commands left running, by their shell's process id. Their programs ignore the hang-up
    /// a closing terminal sends (see the shell's preamble), so the app ends them itself.
    static LEFT_RUNNING: parking_lot::Mutex<Vec<i32>> = parking_lot::Mutex::new(Vec::new());

    pub fn end_left_running() {
        for pid in std::mem::take(&mut *LEFT_RUNNING.lock()) {
            Tree(Some(pid)).kill();
        }
    }

    impl Tree {
        pub fn adopt(child: &(dyn portable_pty::Child + Send + Sync)) -> Self {
            Self(child.process_id().and_then(|pid| i32::try_from(pid).ok()))
        }

        pub fn kill(&self) {
            if let Some(pid) = self.0 {
                // SAFETY: signals the group the shell leads; no memory is touched.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                // And what moved to a group of its own in the same session.
                #[cfg(target_os = "linux")]
                for p in super::procs::session(pid) {
                    // SAFETY: as above, one process of the session the shell leads.
                    unsafe {
                        libc::kill(p.pid, libc::SIGKILL);
                    }
                }
            }
        }

        pub fn release(&self) {}

        /// Ends the command when the app closes, as closing a terminal window would: it was
        /// left running.
        pub fn end_with_app(&self) {
            if let Some(pid) = self.0 {
                LEFT_RUNNING.lock().push(pid);
            }
        }

        /// It ended by itself.
        pub fn ended(&self) {
            if let Some(pid) = self.0 {
                LEFT_RUNNING.lock().retain(|&p| p != pid);
            }
        }

        /// The session the shell leads: its process id.
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        pub fn session(&self) -> Option<i32> {
            self.0
        }
    }
}

/// The processes of a command's session, read from /proc.
#[cfg(target_os = "linux")]
mod procs {
    use std::collections::{HashMap, HashSet};
    use std::path::Path;

    #[derive(Debug)]
    pub struct Proc {
        pub pid: i32,
        pub ppid: i32,
        pub sid: i32,
        pub name: String,
    }

    /// The live processes (zombies are done already).
    pub fn all() -> Vec<Proc> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let (name, state, ppid, sid) = parse_stat(&stat)?;
                (state != 'Z' && state != 'X').then_some(Proc { pid, ppid, sid, name })
            })
            .collect()
    }

    /// The live processes of `session`.
    pub fn session(session: i32) -> Vec<Proc> {
        all().into_iter().filter(|p| p.sid == session).collect()
    }

    /// The name, state, parent and session in a /proc/<pid>/stat line:
    /// `pid (name) state ppid pgrp session …`, where the name may hold spaces and parentheses.
    pub fn parse_stat(stat: &str) -> Option<(String, char, i32, i32)> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat.get(open + 1..close)?.to_string();
        let mut rest = stat.get(close + 1..)?.split_whitespace();
        let state = rest.next()?.chars().next()?;
        let ppid = rest.next()?.parse().ok()?;
        let _pgrp = rest.next()?;
        let session = rest.next()?.parse().ok()?;
        Some((name, state, ppid, session))
    }

    /// Whether the process reads or writes the terminal `tty`: its input, output or errors.
    pub fn on_terminal(pid: i32, tty: &Path) -> bool {
        (0..3).any(|fd| std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).is_ok_and(|target| target == tty))
    }

    /// Whether the process talks to the display server: it loaded the Wayland or X11 client
    /// library, which programs load to open windows.
    pub fn has_window(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/maps")).is_ok_and(|maps| maps_have_window(&maps))
    }

    pub fn maps_have_window(maps: &str) -> bool {
        ["/libwayland-client.so", "/libX11.so", "/libxcb.so"]
            .iter()
            .any(|lib| maps.contains(lib))
    }

    /// `roots` with their ancestors and descendants among `all`: a program's launcher and its
    /// helper processes go with it.
    pub fn with_relatives(all: &[Proc], roots: &[i32]) -> HashSet<i32> {
        let parent: HashMap<i32, i32> = all.iter().map(|p| (p.pid, p.ppid)).collect();
        let mut kept = HashSet::new();
        for &root in roots {
            let mut pid = root;
            while parent.contains_key(&pid) && kept.insert(pid) {
                pid = parent[&pid];
            }
        }
        loop {
            let before = kept.len();
            for p in all {
                if kept.contains(&p.ppid) {
                    kept.insert(p.pid);
                }
            }
            if kept.len() == before {
                return kept;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    async fn run_command(command: &str, timeout: Duration) -> Outcome {
        let shell = super::super::detect().expect("a PowerShell");
        let dir = tempfile::tempdir().unwrap();
        let never = CancellationToken::new();
        run(
            Request {
                shell,
                command,
                dir: dir.path(),
                env: vec![("DEMIDO_TEST", "from Demido".into())],
                timeout,
                background: None,
            },
            &never,
            &never,
            &never,
            |_| {},
        )
        .await
        .unwrap()
        .expect("not cancelled")
    }

    #[tokio::test]
    async fn output_and_exit_code_come_back() {
        let out = run_command(
            "Write-Output \"héllo $env:DEMIDO_TEST\"; cmd /c exit 3",
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(out.ending, Ending::Exited(3));
        assert_eq!(out.shown.text, "héllo from Demido");
    }

    #[tokio::test]
    async fn windows_powershell_runs_commands_the_same_way() {
        let shell = super::super::detect::windows_powershell();
        let dir = tempfile::tempdir().unwrap();
        let never = CancellationToken::new();
        let out = run(
            Request {
                shell: &shell,
                command: "Write-Output \"héllo ✓ $($PSVersionTable.PSVersion.Major)\"; cmd /c exit 4",
                dir: dir.path(),
                env: Vec::new(),
                timeout: Duration::from_secs(60),
                background: None,
            },
            &never,
            &never,
            &never,
            |_| {},
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out.ending, Ending::Exited(4));
        assert_eq!(out.shown.text, "héllo ✓ 5");
    }

    #[tokio::test]
    async fn a_failing_cmdlet_exits_with_one_and_says_why() {
        let out = run_command("Get-Item C:\\does-not-exist-demido", Duration::from_secs(60)).await;
        assert_eq!(out.ending, Ending::Exited(1));
        assert!(out.shown.text.contains("does-not-exist-demido"), "{}", out.shown.text);
        let ok = run_command("Get-Date | Out-Null", Duration::from_secs(60)).await;
        assert_eq!(ok.ending, Ending::Exited(0));
    }

    #[tokio::test]
    async fn a_timeout_stops_the_whole_tree_and_keeps_the_output() {
        let marker = format!("demido-{}", std::process::id());
        let out = run_command(
            &format!("Write-Output started; ping -n 30 127.0.0.1 | Out-Null; Write-Output {marker}"),
            Duration::from_secs(4),
        )
        .await;
        assert_eq!(out.ending, Ending::TimedOut);
        assert!(out.shown.text.starts_with("started"), "{}", out.shown.text);
        assert!(!out.shown.text.contains(&marker));
    }

    #[tokio::test]
    async fn stop_and_cancel_end_a_running_command() {
        let shell = super::super::detect().expect("a PowerShell");
        let dir = tempfile::tempdir().unwrap();
        let (stop, cancel, never) = (
            CancellationToken::new(),
            CancellationToken::new(),
            CancellationToken::new(),
        );
        let request = |command| Request {
            shell,
            command,
            dir: dir.path(),
            env: Vec::new(),
            timeout: Duration::from_secs(60),
            background: None,
        };
        let stopper = stop.clone();
        let mut seen = String::new();
        let out = run(
            request("Write-Output working; Start-Sleep 30"),
            &stop,
            &never,
            &cancel,
            |shown| {
                seen = shown.text.clone();
                if shown.text.contains("working") {
                    stopper.cancel();
                }
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out.ending, Ending::Stopped);
        assert_eq!(out.shown.text, "working");
        assert_eq!(seen, "working");
        assert!(out.elapsed < Duration::from_secs(20));

        let canceller = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            canceller.cancel();
        });
        let cancelled = run(request("Start-Sleep 30"), &never, &never, &cancel, |_| {})
            .await
            .unwrap();
        assert!(cancelled.is_none());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    fn request<'a>(command: &'a str, dir: &'a std::path::Path, timeout: Duration) -> Request<'a> {
        Request {
            shell: super::super::detect().expect("bash"),
            command,
            dir,
            env: vec![("DEMIDO_TEST", "from Demido".into())],
            timeout,
            background: None,
        }
    }

    async fn run_command(command: &str, timeout: Duration) -> (Outcome, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let never = CancellationToken::new();
        let out = run(request(command, dir.path(), timeout), &never, &never, &never, |_| {})
            .await
            .unwrap()
            .expect("not cancelled");
        (out, dir)
    }

    #[tokio::test]
    async fn output_and_exit_code_come_back() {
        let (out, _dir) = run_command("echo \"héllo $DEMIDO_TEST\"; exit 3", Duration::from_secs(60)).await;
        assert_eq!(out.ending, Ending::Exited(3));
        assert_eq!(out.shown.text, "héllo from Demido");
    }

    #[tokio::test]
    async fn a_failing_program_exits_with_its_own_code_and_says_why() {
        let (out, _dir) = run_command("ls /does-not-exist-demido", Duration::from_secs(60)).await;
        assert_eq!(out.ending, Ending::Exited(2));
        assert!(out.shown.text.contains("does-not-exist-demido"), "{}", out.shown.text);
    }

    #[tokio::test]
    async fn a_timeout_stops_the_whole_tree_and_keeps_the_output() {
        let marker = format!("demido-{}", std::process::id());
        // A program the command left running in the background must end too.
        let (out, dir) = run_command(
            &format!("echo started; sleep 300 & echo $! > sleeper; wait; echo {marker}"),
            Duration::from_secs(3),
        )
        .await;
        assert_eq!(out.ending, Ending::TimedOut);
        assert!(out.shown.text.starts_with("started"), "{}", out.shown.text);
        assert!(!out.shown.text.contains(&marker));
        let pid: libc::pid_t = std::fs::read_to_string(dir.path().join("sleeper"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only asks whether the process exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(Instant::now() < deadline, "the background sleep outlived the command");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[tokio::test]
    async fn stop_and_cancel_end_a_running_command() {
        let dir = tempfile::tempdir().unwrap();
        let (stop, cancel, never) = (
            CancellationToken::new(),
            CancellationToken::new(),
            CancellationToken::new(),
        );
        let stopper = stop.clone();
        let mut seen = String::new();
        let out = run(
            request("echo working; sleep 30", dir.path(), Duration::from_secs(60)),
            &stop,
            &never,
            &cancel,
            |shown| {
                seen = shown.text.clone();
                if shown.text.contains("working") {
                    stopper.cancel();
                }
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out.ending, Ending::Stopped);
        assert_eq!(out.shown.text, "working");
        assert_eq!(seen, "working");
        assert!(out.elapsed < Duration::from_secs(20));

        let canceller = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            canceller.cancel();
        });
        let cancelled = run(
            request("sleep 30", dir.path(), Duration::from_secs(60)),
            &never,
            &never,
            &cancel,
            |_| {},
        )
        .await
        .unwrap();
        assert!(cancelled.is_none());
    }

    #[tokio::test]
    async fn pagers_never_wait_for_keys() {
        let (out, _dir) = run_command("echo \"$PAGER $MANPAGER $SYSTEMD_PAGER\"", Duration::from_secs(60)).await;
        assert_eq!(out.shown.text, "cat cat cat");
    }

    fn pid_in(dir: &std::path::Path, file: &str) -> libc::pid_t {
        std::fs::read_to_string(dir.join(file)).unwrap().trim().parse().unwrap()
    }

    fn alive(pid: libc::pid_t) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| procs::parse_stat(&stat))
            .is_some_and(|(_, state, _, _)| state != 'Z')
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[tokio::test]
    async fn a_program_left_in_the_terminal_ends_with_the_command() {
        let (out, dir) = run_command("sleep 300 & echo $! > sleeper; echo done", Duration::from_secs(60)).await;
        assert_eq!(out.ending, Ending::Exited(0));
        assert_eq!(out.shown.text, "done");
        let pid = pid_in(dir.path(), "sleeper");
        assert_eq!(
            out.ended,
            vec![Program {
                pid: pid as u32,
                name: "sleep".into()
            }]
        );
        assert!(out.left_open.is_empty());
        wait_for("the sleep outlived the command", || !alive(pid));
    }

    #[tokio::test]
    async fn a_program_that_left_the_terminal_stays_running() {
        // As a program in a window of its own does; nohup and a log file are how servers leave.
        // The command waits for the sleep to start: the program that left the terminal is kept
        // as it is when the shell ends, and until then it can still be the shell's fork (bash).
        let (out, dir) = run_command(
            "nohup sleep 300 > /dev/null 2>&1 & echo $! > sleeper; \
             until [ \"$(cat /proc/$!/comm)\" = sleep ]; do sleep 0.01; done",
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(out.ending, Ending::Exited(0));
        let pid = pid_in(dir.path(), "sleeper");
        assert_eq!(
            out.left_open,
            vec![Program {
                pid: pid as u32,
                name: "sleep".into()
            }]
        );
        assert!(out.ended.is_empty());
        std::thread::sleep(Duration::from_millis(300));
        assert!(alive(pid), "the sleep that left the terminal was ended");
        // SAFETY: ends the test's own sleep.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }

    #[tokio::test]
    async fn a_command_in_the_background_runs_on_after_it_returns() {
        let dir = tempfile::tempdir().unwrap();
        let never = CancellationToken::new();
        let mut req = request(
            "echo started; sleep 2; echo later > marker",
            dir.path(),
            Duration::from_secs(1),
        );
        req.background = Some(Duration::from_secs(1));
        let out = run(req, &never, &never, &never, |_| {}).await.unwrap().unwrap();
        assert_eq!(out.ending, Ending::LeftRunning);
        assert_eq!(out.shown.text, "started");
        assert!(out.elapsed < Duration::from_secs(2));
        assert!(!dir.path().join("marker").exists());
        wait_for("the command did not run on", || dir.path().join("marker").exists());

        // A command that ends sooner is done as usual.
        let mut req = request("echo quick", dir.path(), Duration::from_secs(60));
        req.background = Some(Duration::from_secs(10));
        let out = run(req, &never, &never, &never, |_| {}).await.unwrap().unwrap();
        assert_eq!(out.ending, Ending::Exited(0));
        assert_eq!(out.shown.text, "quick");
    }

    #[tokio::test]
    async fn the_person_can_leave_a_command_running() {
        let dir = tempfile::tempdir().unwrap();
        let (never, release) = (CancellationToken::new(), CancellationToken::new());
        let releaser = release.clone();
        let out = run(
            request(
                "echo working; sleep 2; echo later > marker",
                dir.path(),
                Duration::from_secs(60),
            ),
            &never,
            &release,
            &never,
            |shown| {
                if shown.text.contains("working") {
                    releaser.cancel();
                }
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(out.ending, Ending::LeftRunning);
        assert_eq!(out.shown.text, "working");
        assert!(out.pid.is_some());
        wait_for("the command did not run on", || dir.path().join("marker").exists());
    }

    #[test]
    fn stat_lines_with_odd_names_are_read() {
        assert_eq!(
            procs::parse_stat("4242 (my (odd) prog) S 4200 4242 4200 34816 4242 4194304 1 0"),
            Some(("my (odd) prog".into(), 'S', 4200, 4200))
        );
        assert_eq!(procs::parse_stat("garbage"), None);
    }

    #[test]
    fn a_window_is_told_by_the_display_libraries() {
        let gtk = "7f00-7f01 r-xp 00000000 00:1f 123 /usr/lib/libwayland-client.so.0.23.1\n";
        let x11 = "7f00-7f01 r-xp 00000000 00:1f 123 /usr/lib/libX11.so.6.4.0\n";
        let console = "7f00-7f01 r-xp 00000000 00:1f 123 /usr/lib/libc.so.6\n";
        assert!(procs::maps_have_window(gtk));
        assert!(procs::maps_have_window(x11));
        assert!(!procs::maps_have_window(console));
    }

    #[test]
    fn a_kept_program_keeps_its_launcher_and_helpers() {
        let p = |pid, ppid| procs::Proc {
            pid,
            ppid,
            sid: 10,
            name: String::new(),
        };
        // 10 launches 11 (the window), which starts 12; 20 is unrelated.
        let all = [p(10, 1), p(11, 10), p(12, 11), p(20, 1)];
        let mut kept: Vec<i32> = procs::with_relatives(&all, &[11]).into_iter().collect();
        kept.sort();
        assert_eq!(kept, [10, 11, 12]);
        assert!(procs::with_relatives(&all, &[]).is_empty());
    }
}
