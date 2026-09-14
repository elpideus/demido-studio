//! Killing a command, and everything it started.
//!
//! `Child::kill` kills the process Demido spawned, and for `run_command` that is
//! `cmd.exe`, which is almost never the thing doing the work. `npm test` is
//! `npm.cmd`, which is `cmd.exe` starting `node`, which starts whatever the test
//! runner starts. Killing what Demido spawned kills the first link and leaves
//! the rest running, holding the pipes, with nothing in the window able to see
//! it, let alone stop it.
//!
//! A job object closes that. A process in a job whose limits say
//! `KILL_ON_JOB_CLOSE` dies with the job, and so does everything it started,
//! because a child is born in its parent's job. So the tree goes when the handle
//! goes, which covers the three ways a call ends: it returns, it is killed at its
//! deadline, and it is dropped because a generation was stopped mid call. It
//! also covers the one nothing else could: if Demido itself dies, the operating
//! system closes its handles, and the job takes the tree with it.
//!
//! **There is a window, and it is written down rather than closed.** The child
//! is spawned first and put in the job on the next line, so anything it starts
//! in between is born outside the job. Closing it needs the child created
//! suspended and its thread resumed by hand, which the standard library does
//! not expose. `cmd.exe` takes milliseconds to get as far as starting anything,
//! so the window is real and narrow, and a process that deliberately breaks
//! away from its job is outside this module's reach entirely.
//!
//! Elsewhere this is a no-op, and so is the promise: Demido is Windows only
//! until close to 1.0, and a process group is the Unix shape of the same idea
//! when it is needed.
//!
//! Carried from v2's `demido-core::tree`, where it was written for MCP servers.
//! It is here rather than in v3's `demido-core` because that crate's own rule is
//! that nothing which touches a process belongs in it, and `run_command` is the
//! only thing in v3 that starts a tree (`AGENTS.md`, the port review).

use tokio::process::Child;

/// Everything one command started, killed together.
pub struct Tree(Inner);

impl Tree {
    /// Put a child and its descendants under one handle.
    ///
    /// Never fails loudly. A job object that could not be created is a command
    /// that cleans up worse, which is not a reason to refuse to run it: the
    /// child itself is still killed on drop, exactly as before.
    pub fn around(child: &Child) -> Self {
        Self(Inner::around(child))
    }

    /// Kill the tree now, rather than when this is dropped.
    pub fn kill(&self) {
        self.0.kill();
    }
}

#[cfg(windows)]
use windows_impl::Inner;

#[cfg(not(windows))]
use elsewhere::Inner;

#[cfg(not(windows))]
mod elsewhere {
    use tokio::process::Child;

    pub struct Inner;

    impl Inner {
        pub fn around(_child: &Child) -> Self {
            Self
        }

        pub fn kill(&self) {}
    }
}

#[cfg(windows)]
mod windows_impl {
    use tokio::process::Child;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A job handle, or nothing if the operating system would not give one.
    pub struct Inner(Option<Handle>);

    /// A kernel handle. Kernel handles are not tied to the thread that made
    /// them, which is what makes this sound: the only operations performed on
    /// it are terminate and close, both of which any thread may do.
    struct Handle(HANDLE);

    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Inner {
        pub fn around(child: &Child) -> Self {
            Self(assign(child))
        }

        pub fn kill(&self) {
            if let Some(handle) = &self.0 {
                // Exit code 1: this is a kill, and a tree that reported success
                // would be a lie to anything reading exit codes.
                unsafe {
                    let _ = TerminateJobObject(handle.0, 1);
                }
            }
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            if let Some(handle) = &self.0 {
                // Closing the last handle to a KILL_ON_JOB_CLOSE job kills
                // everything in it. This is the line that makes a dropped call
                // take its whole tree with it.
                unsafe {
                    let _ = CloseHandle(handle.0);
                }
            }
        }
    }

    /// Create a job that kills its members when it closes, and put the child in
    /// it.
    fn assign(child: &Child) -> Option<Handle> {
        // Borrowed for this call only, and the child outlives the borrow: this
        // takes `&Child`, so tokio cannot have reaped it and closed the handle.
        let process = HANDLE(child.raw_handle()?);

        // SAFETY: no pointers are passed; a failure is an error value, not UB.
        let job = unsafe { CreateJobObjectW(None, None) }.ok()?;

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        // SAFETY: `limits` lives on this stack frame for the whole call, and the
        // length passed is exactly its size.
        let set = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        // SAFETY (both closes below): `job` was opened above and is closed once,
        // on the path that gives up on it, so no handle is used after closing.
        if set.is_err() {
            let _ = unsafe { CloseHandle(job) };
            return None;
        }

        // Almost always refused because the process is already in a job that
        // forbids nesting. The command still runs; only its cleanup is worse.
        // SAFETY: both handles are open for the duration of the call.
        if unsafe { AssignProcessToJobObject(job, process) }.is_err() {
            let _ = unsafe { CloseHandle(job) };
            return None;
        }

        Some(Handle(job))
    }
}
