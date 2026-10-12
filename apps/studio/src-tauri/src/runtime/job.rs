//! Ties helper processes (llama-server, the market service) to the app's lifetime.
//!
//! Every helper is started with [`spawn`] (or [`spawn_std`]), so that when Demido Studio exits for
//! any reason, including a crash, its helpers end too and a model never keeps holding the GPU
//! after the app is gone.
//!
//! On Windows every child is placed in a job object created with "kill on close": the OS closes
//! the job with the app and ends the children.
//!
//! On Linux every child asks the kernel for SIGKILL when its parent goes (`PR_SET_PDEATHSIG`),
//! set between fork and exec. The kernel sends it when the *thread* that started the child ends,
//! not the process, so a helper must be started from a thread that outlives it: a tokio worker
//! (they live as long as the app), or a thread that waits for the child, as the file reader's
//! does. Never from `spawn_blocking`, whose threads end after a few idle seconds.

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    struct Job(HANDLE);
    // SAFETY: a job handle is a process-wide kernel handle; it is only used for assignment.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    pub fn init() {
        JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(Job(job))
        });
    }

    pub fn spawn(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
        init();
        let child = cmd.spawn()?;
        let Some(Some(job)) = JOB.get() else {
            return Ok(child);
        };
        if let Some(raw) = child.raw_handle() {
            unsafe {
                if let Err(e) = AssignProcessToJobObject(job.0, HANDLE(raw)) {
                    tracing::warn!("could not tie child process to the app: {e}");
                }
            }
        }
        Ok(child)
    }

    pub fn spawn_std(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        use std::os::windows::io::AsRawHandle;
        init();
        let child = cmd.spawn()?;
        let Some(Some(job)) = JOB.get() else {
            return Ok(child);
        };
        unsafe {
            if let Err(e) = AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) {
                tracing::warn!("could not tie child process to the app: {e}");
            }
        }
        Ok(child)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::os::unix::process::CommandExt;

    pub fn init() {}

    pub fn spawn(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
        die_with_parent(cmd.as_std_mut());
        cmd.spawn()
    }

    pub fn spawn_std(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        die_with_parent(cmd);
        cmd.spawn()
    }

    fn die_with_parent(cmd: &mut std::process::Command) {
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: runs in the child between fork and exec, where only async-signal-safe calls
        // are allowed: prctl, getppid and _exit are.
        unsafe {
            cmd.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The app may have ended between fork and prctl, and then nothing would send it.
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    pub fn init() {}

    pub fn spawn(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
        cmd.spawn()
    }

    pub fn spawn_std(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        cmd.spawn()
    }
}

pub use imp::{init, spawn, spawn_std};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_helper_still_runs_while_the_app_does() {
        let mut cmd = tokio::process::Command::new("sleep");
        cmd.arg("5").kill_on_drop(true);
        let mut child = spawn(&mut cmd).expect("sleep starts");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(child.try_wait().unwrap().is_none(), "the helper ended on its own");
    }

    #[test]
    fn a_helper_ends_with_the_thread_that_started_it() {
        // The kernel signals the child when its parent thread ends: a thread that exits right
        // after starting `sleep` stands in for an app that is gone.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new("sleep");
            cmd.arg("30");
            tx.send(spawn_std(&mut cmd).expect("sleep starts")).unwrap();
        })
        .join()
        .unwrap();
        let mut child = rx.recv().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(std::time::Instant::now() < deadline, "the helper outlived its parent");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }
}
