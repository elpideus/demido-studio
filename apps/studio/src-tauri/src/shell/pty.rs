//! A command running in a pseudo-terminal (ConPTY on Windows), as in a terminal window: programs
//! see a console, so they print what they print for a person and full-screen ones run. Nothing
//! types into it; a command that waits for input simply runs until its timeout.

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// It finished by itself, with this exit code.
    Exited(u32),
    /// It was still running at the timeout and was stopped.
    TimedOut,
    /// The person stopped it.
    Stopped,
}

#[derive(Debug)]
pub struct Outcome {
    pub ending: Ending,
    /// What the terminal showed at the end.
    pub shown: Shown,
    pub elapsed: Duration,
}

/// Runs the command. `on_output` gets what the terminal shows while it changes. `stop` ends the
/// command and keeps what it printed; `cancel` (the turn was stopped) ends it and returns `None`.
pub async fn run(
    req: Request<'_>,
    stop: &CancellationToken,
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
    cmd.env("TERM", "xterm-256color");
    for (key, value) in &req.env {
        cmd.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start {}: {e}", req.shell.name))?;
    drop(pair.slave);
    let tree = tree::Tree::adopt(&*child);
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

    let deadline = tokio::time::sleep(req.timeout);
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let ending = loop {
        tokio::select! {
            code = &mut exited => break Some(Ending::Exited(code.ok().flatten().unwrap_or(1))),
            _ = &mut deadline => break Some(Ending::TimedOut),
            _ = stop.cancelled() => break Some(Ending::Stopped),
            _ = cancel.cancelled() => break None,
            _ = tick.tick() => {
                if changed.swap(false, Ordering::AcqRel) {
                    on_output(&screen.lock().shown());
                }
            }
        }
    };

    let master = pair.master;
    let close = move || drop(master);
    let shown = match ending {
        Some(Ending::Exited(_)) => {
            // Closing the pseudo console ends what is still attached to it and lets the last
            // output through; a program the command opened in its own window stays open.
            tree.release();
            let _ = tokio::time::timeout(DRAIN, tokio::task::spawn_blocking(close)).await;
            let _ = tokio::time::timeout(DRAIN, drained).await;
            screen.lock().shown()
        }
        _ => {
            // The screen as it is, before a full-screen program is torn down.
            let shown = screen.lock().shown();
            tree.kill();
            let _ = killer.kill();
            let _ = tokio::time::timeout(DRAIN, exited).await;
            tokio::task::spawn_blocking(close);
            shown
        }
    };
    Ok(ending.map(|ending| Outcome {
        ending,
        shown,
        elapsed: started.elapsed(),
    }))
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
    }

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
            }
        }

        pub fn release(&self) {}
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
            },
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
            },
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
        let (stop, cancel) = (CancellationToken::new(), CancellationToken::new());
        let request = |command| Request {
            shell,
            command,
            dir: dir.path(),
            env: Vec::new(),
            timeout: Duration::from_secs(60),
        };
        let stopper = stop.clone();
        let mut seen = String::new();
        let out = run(
            request("Write-Output working; Start-Sleep 30"),
            &stop,
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
        let never = CancellationToken::new();
        let cancelled = run(request("Start-Sleep 30"), &never, &cancel, |_| {}).await.unwrap();
        assert!(cancelled.is_none());
    }
}
