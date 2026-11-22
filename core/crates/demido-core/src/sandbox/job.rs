//! A Windows job: everything a command starts is in it, and can be ended together.
//!
//! The command is started suspended, put into the job, and only then let go on, so that nothing it starts
//! can begin outside the job.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// What a process is started with so that it waits to be put into its job (`CREATE_SUSPENDED`).
pub(super) const STARTED_WAITING: u32 = windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

pub(super) struct Job(HANDLE);

// A job's handle may be used from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// Puts a process that was started waiting into a new job, then lets it go on. It is let go on even when
    /// the job could not be made: then only the process itself can be ended, as before.
    pub(super) fn holding(process: Option<HANDLE>, pid: u32) -> Option<Job> {
        // SAFETY: plain Win32 calls on handles this process owns; each handle made here is closed here or by Drop.
        let job = unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            match process {
                Some(process) if !job.is_null() && AssignProcessToJobObject(job, process) != 0 => Some(Job(job)),
                _ => {
                    if !job.is_null() {
                        CloseHandle(job);
                    }
                    None
                }
            }
        };
        resume(pid);
        job
    }

    /// Ends everything in the job.
    pub(super) fn end(&self) {
        // SAFETY: the handle is this job's, open until Drop.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is this job's, and is closed once. What is in the job goes on running.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Lets every thread of a process that was started waiting go on.
fn resume(pid: u32) {
    // SAFETY: a snapshot of the system's threads, read and closed here; each thread opened is closed here.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return;
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    ResumeThread(thread);
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
}
