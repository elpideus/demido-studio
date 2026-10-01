//! Ties helper processes (llama-server, the market service) to the app's lifetime.
//!
//! On Windows every child is placed in a job object created with "kill on close": when Demido
//! Studio exits for any reason, including a crash, the OS closes the job and ends the children,
//! so a model never keeps holding the GPU after the app is gone.

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

    /// The same for a child started with `std::process` (files read for attachments).
    pub fn adopt_std(child: &std::process::Child) {
        use std::os::windows::io::AsRawHandle;
        init();
        let Some(Some(job)) = JOB.get() else {
            return;
        };
        unsafe {
            if let Err(e) = AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) {
                tracing::warn!("could not tie child process to the app: {e}");
            }
        }
    }

    pub fn adopt(child: &tokio::process::Child) {
        let Some(Some(job)) = JOB.get() else {
            return;
        };
        if let Some(raw) = child.raw_handle() {
            unsafe {
                if let Err(e) = AssignProcessToJobObject(job.0, HANDLE(raw)) {
                    tracing::warn!("could not tie child process to the app: {e}");
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn init() {}
    pub fn adopt(_child: &tokio::process::Child) {}
    pub fn adopt_std(_child: &std::process::Child) {}
}

pub use imp::{adopt, adopt_std, init};
