//! Operating system helpers the installer needs: elevation, launching the app, disk space and
//! finding, waiting for and closing a running instance.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Processes whose executable lives inside `dir`, other than this one and other copies of this
/// executable (a non-elevated uninstaller waits for the elevated copy of itself it started).
pub fn running_app_pids(dir: &Path) -> Vec<u32> {
    running_from(dir).into_iter().map(|(pid, _)| pid).collect()
}

/// [`running_app_pids`], with each process's executable.
fn running_from(dir: &Path) -> Vec<(u32, PathBuf)> {
    let own = std::process::id();
    let Ok(dir) = dir.canonicalize() else {
        return Vec::new();
    };
    let own_exe = std::env::current_exe().and_then(|p| p.canonicalize()).ok();
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .iter()
        .filter(|(pid, _)| pid.as_u32() != own)
        .filter_map(|(pid, p)| {
            let exe = p.exe()?.canonicalize().ok()?;
            runs_from(&exe, &dir, own_exe.as_deref()).then_some((pid.as_u32(), exe))
        })
        .collect()
}

fn runs_from(exe: &Path, dir: &Path, own_exe: Option<&Path>) -> bool {
    exe.starts_with(dir) && own_exe != Some(exe)
}

/// How often the waits below look again.
const POLL: Duration = Duration::from_millis(250);

/// Whether process `pid` is running.
pub fn is_running(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    sys.process(pid).is_some()
}

/// Waits up to `timeout` for process `pid` to exit. Returns whether it has.
pub fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    imp::wait_for_exit(pid, timeout)
}

/// Checks `done` until it holds or `timeout` has passed. Returns whether it held.
fn poll_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// Waits up to `timeout` for everything running from `dir` to exit. Returns whether nothing runs
/// from there any more.
pub fn wait_until_closed(dir: &Path, timeout: Duration) -> bool {
    poll_until(timeout, || running_app_pids(dir).is_empty())
}

/// Closes Demido Studio running from `dir`. Each app window is asked to close first, so the app
/// shuts its services down cleanly (a forced exit could leave a download or the database half
/// written); whatever still runs from `dir` after `grace`, the app's helper processes included, is
/// then ended. Returns whether nothing runs from `dir` any more. Fails only when the operating
/// system's tool for it cannot be started.
pub fn close_app(dir: &Path, grace: Duration) -> anyhow::Result<bool> {
    let running = running_from(dir);
    if running.is_empty() {
        return Ok(true);
    }
    for pid in app_windows(&running) {
        imp::ask_to_close(pid)?;
    }
    if wait_until_closed(dir, grace) {
        return Ok(true);
    }
    for (pid, _) in running_from(dir) {
        imp::end_tree(pid)?;
    }
    Ok(wait_until_closed(dir, Duration::from_secs(5)))
}

/// The processes among `running` that are the app itself, which has windows to close; its helpers
/// (the runtime, Node) have none and exit with it.
fn app_windows(running: &[(u32, PathBuf)]) -> Vec<u32> {
    running
        .iter()
        .filter(|(_, exe)| {
            exe.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(demido_core::brand::STUDIO_BIN))
        })
        .map(|(pid, _)| *pid)
        .collect()
}

/// A volume (drive) and the bytes free on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub mount: PathBuf,
    pub free_bytes: u64,
}

/// The volume that holds `path` (or its closest existing ancestor).
pub fn volume(path: &Path) -> Option<Volume> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let probe = probe.canonicalize().ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| {
            d.mount_point()
                .canonicalize()
                .map(|m| probe.starts_with(m))
                .unwrap_or(false)
        })
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| Volume {
            mount: d.mount_point().to_path_buf(),
            free_bytes: d.available_space(),
        })
}

/// Free bytes on the volume that holds `path` (or its closest existing ancestor).
pub fn free_space(path: &Path) -> Option<u64> {
    volume(path).map(|v| v.free_bytes)
}

/// How much an installation writes to one volume, next to what is free there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeNeed {
    pub mount: PathBuf,
    pub free_bytes: u64,
    pub needed_bytes: u64,
}

/// Space needed per volume. `needs` pairs a folder with the bytes written into it; folders on the
/// same volume add up. Folders whose volume cannot be found are left out.
pub fn space_needed(needs: &[(PathBuf, u64)]) -> Vec<VolumeNeed> {
    sum_by_volume(needs.iter().filter_map(|(path, bytes)| Some((volume(path)?, *bytes))))
}

fn sum_by_volume(needs: impl IntoIterator<Item = (Volume, u64)>) -> Vec<VolumeNeed> {
    let mut out: Vec<VolumeNeed> = Vec::new();
    for (volume, bytes) in needs {
        match out.iter_mut().find(|v| v.mount == volume.mount) {
            Some(v) => v.needed_bytes += bytes,
            None => out.push(VolumeNeed {
                mount: volume.mount,
                free_bytes: volume.free_bytes,
                needed_bytes: bytes,
            }),
        }
    }
    out
}

/// Whether this process can write into `dir` (creating it if needed).
pub fn can_write(dir: &Path) -> bool {
    let mut probe = dir.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(p) => probe = p.to_path_buf(),
            None => return false,
        }
    }
    let test = probe.join(format!(".demido-write-test-{}", std::process::id()));
    match std::fs::write(&test, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(test);
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use std::process::Command;

    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use std::time::Duration;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcess, OpenProcessToken, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR};

    pub fn is_elevated() -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION::default();
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(token);
            ok && elevation.TokenIsElevated != 0
        }
    }

    /// Starts this executable again with administrator rights. `Ok(false)` when the person
    /// declined the prompt.
    pub fn relaunch_elevated(args: &str) -> anyhow::Result<bool> {
        match start_elevated(args)? {
            Some(process) => {
                if !process.is_invalid() {
                    unsafe {
                        let _ = CloseHandle(process);
                    }
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Runs this executable again with administrator rights and waits for it. Returns its exit
    /// code, or `None` when the person declined the prompt.
    pub fn run_elevated(args: &str) -> anyhow::Result<Option<u32>> {
        let Some(process) = start_elevated(args)? else {
            return Ok(None);
        };
        if process.is_invalid() {
            anyhow::bail!("Windows started the elevated copy but did not say which process it is.");
        }
        let mut code = 1u32;
        let result = unsafe {
            WaitForSingleObject(process, INFINITE);
            let result = GetExitCodeProcess(process, &mut code);
            let _ = CloseHandle(process);
            result
        };
        result?;
        Ok(Some(code))
    }

    /// The started process (the caller closes it), or `None` when the prompt was declined.
    fn start_elevated(args: &str) -> anyhow::Result<Option<HANDLE>> {
        let exe = std::env::current_exe()?;
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(args);
        let verb = HSTRING::from("runas");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe {
            match ShellExecuteExW(&mut info) {
                Ok(()) => Ok(Some(info.hProcess)),
                // ERROR_CANCELLED: the UAC prompt was dismissed.
                Err(e) if e.code().0 as u32 == 0x8007_04C7 => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
    }

    /// Launches the app as the signed-in user even from an elevated installer, so a machine-wide
    /// installation's app never inherits administrator rights. Started through Explorer that way,
    /// it gets none of `args`; the only one, `--skip-update`, matters only for per-user
    /// installations, which never go through Explorer: when setup runs elevated for one of those
    /// (UAC turned off, or the app itself run as administrator), the app already had those rights.
    pub fn launch_app(exe: &Path, args: &[&str]) -> anyhow::Result<()> {
        let per_user = exe
            .parent()
            .and_then(crate::folder::installation)
            .is_some_and(|m| m.scope == demido_core::InstallScope::User);
        if is_elevated() && !per_user {
            Command::new("explorer.exe").arg(exe).spawn()?;
        } else {
            super::app_command(exe, args).spawn()?;
        }
        Ok(())
    }

    /// Deletes `file` once this process has exited, then `dir` if that left it empty. Used by
    /// the uninstaller, which cannot delete its own executable while it runs, and may stay open
    /// for a while showing its result.
    pub fn delete_after_exit(file: &Path, dir: &Path) {
        let _ = delete_after_exit_command(std::process::id(), file, dir).spawn();
    }

    /// Waits for the process, then retries the file for a few seconds (the image can stay locked
    /// briefly after exit), then removes the folder only if it is empty. The paths arrive in
    /// environment variables, so nothing in them needs quoting.
    const DELETE_AFTER_EXIT: &str = "Wait-Process -Id ([int]$env:DEMIDO_WAIT_PID) -ErrorAction SilentlyContinue; \
         for ($i = 0; $i -lt 40 -and (Test-Path -LiteralPath $env:DEMIDO_DELETE_FILE); $i++) { \
         Start-Sleep -Milliseconds 250; \
         Remove-Item -LiteralPath $env:DEMIDO_DELETE_FILE -Force -ErrorAction SilentlyContinue }; \
         try { [System.IO.Directory]::Delete($env:DEMIDO_DELETE_DIR) } catch { }; exit 0";

    /// Keeps a console program from flashing a window when a GUI process starts it.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
        // A handle is signalled once the process has exited, even while something else still
        // holds one to it (then it lingers in the process list, which a poll would take for
        // running).
        let Ok(process) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
            // Gone already, or not ours to open: the process list tells.
            return super::poll_until(timeout, || !super::is_running(pid));
        };
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        unsafe {
            let result = WaitForSingleObject(process, millis);
            let _ = CloseHandle(process);
            result == WAIT_OBJECT_0
        }
    }

    /// Asks `pid` to close its windows, as clicking their close button would.
    pub fn ask_to_close(pid: u32) -> anyhow::Result<()> {
        run_taskkill(taskkill(pid, false))
    }

    /// Ends `pid` and every process it started.
    pub fn end_tree(pid: u32) -> anyhow::Result<()> {
        run_taskkill(taskkill(pid, true))
    }

    /// `taskkill` for `pid`: without `/F` it sends the process's windows `WM_CLOSE`, and the app
    /// shuts down as it does when closed; with it, the process and its children end at once.
    pub(super) fn taskkill(pid: u32, force: bool) -> Command {
        let mut cmd = Command::new("taskkill.exe");
        cmd.creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if force {
            cmd.args(["/F", "/T"]);
        }
        cmd.args(["/PID", &pid.to_string()]);
        cmd
    }

    /// Runs `taskkill` to the end. Its exit code is not an error: the process may have exited
    /// meanwhile, and the caller checks what still runs.
    fn run_taskkill(mut cmd: Command) -> anyhow::Result<()> {
        cmd.status()
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("could not start taskkill: {e}"))
    }

    pub(super) fn delete_after_exit_command(pid: u32, file: &Path, dir: &Path) -> Command {
        // No window, but not DETACHED_PROCESS: a detached PowerShell exits without running its
        // command. Without a job object here it outlives the uninstaller anyway.
        let mut cmd = Command::new("powershell.exe");
        cmd.creation_flags(CREATE_NO_WINDOW)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                DELETE_AFTER_EXIT,
            ])
            .env("DEMIDO_WAIT_PID", pid.to_string())
            .env("DEMIDO_DELETE_FILE", file)
            .env("DEMIDO_DELETE_DIR", dir);
        cmd
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    use std::process::Command;

    pub fn is_elevated() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        unsafe { geteuid() == 0 }
    }

    pub fn relaunch_elevated(_args: &str) -> anyhow::Result<bool> {
        anyhow::bail!("run the installer with sudo to install for everyone")
    }

    pub fn run_elevated(_args: &str) -> anyhow::Result<Option<u32>> {
        anyhow::bail!("run the uninstaller with sudo to remove an installation for everyone")
    }

    pub fn launch_app(exe: &Path, args: &[&str]) -> anyhow::Result<()> {
        super::app_command(exe, args).spawn()?;
        Ok(())
    }

    /// A running executable can be deleted here, so nothing needs to wait.
    pub fn delete_after_exit(file: &Path, dir: &Path) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir(dir);
    }

    pub fn wait_for_exit(pid: u32, timeout: std::time::Duration) -> bool {
        super::poll_until(timeout, || !super::is_running(pid))
    }

    /// Asks `pid` to shut down, as closing it would.
    pub fn ask_to_close(pid: u32) -> anyhow::Result<()> {
        kill(pid, "-TERM")
    }

    /// Ends `pid` at once. Its helpers run from the install folder too, so the caller ends each.
    pub fn end_tree(pid: u32) -> anyhow::Result<()> {
        kill(pid, "-KILL")
    }

    /// Sends `signal` to `pid`. The exit code is not an error: the process may have exited
    /// meanwhile, and the caller checks what still runs.
    fn kill(pid: u32, signal: &str) -> anyhow::Result<()> {
        Command::new("kill")
            .args([signal, &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("could not start kill: {e}"))
    }
}

pub use imp::{delete_after_exit, is_elevated, launch_app, relaunch_elevated, run_elevated};

/// The app at `exe` started with `args`, from its own folder.
fn app_command(exe: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd
}

/// Held while setup installs or updates (see `demido_core::brand::SETUP_LOCK`). Released when
/// dropped, and by Windows when the process ends however it ends.
pub struct SetupLock {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: the mutex handle is only closed, once, on drop; Windows handles are process-wide.
unsafe impl Send for SetupLock {}
unsafe impl Sync for SetupLock {}

impl SetupLock {
    /// Takes the lock, or `None` when another setup holds it. Where named locks do not exist the
    /// lock is always granted.
    #[cfg(windows)]
    pub fn acquire() -> Option<Self> {
        use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::HSTRING;

        let name = HSTRING::from(demido_core::brand::SETUP_LOCK);
        unsafe {
            let handle = match CreateMutexW(None, false, &name) {
                Ok(handle) => handle,
                // Created by another setup running elevated, which this one may not open.
                Err(e) => {
                    tracing::warn!("could not take the setup lock: {e}");
                    return None;
                }
            };
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
    }

    #[cfg(not(windows))]
    pub fn acquire() -> Option<Self> {
        Some(Self {})
    }
}

impl Drop for SetupLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn only_one_setup_holds_the_lock_until_it_lets_go() {
        let first = SetupLock::acquire().expect("nothing else holds the setup lock");
        assert!(SetupLock::acquire().is_none(), "a second setup got the lock");
        drop(first);
        assert!(SetupLock::acquire().is_some(), "the lock was not released");
    }

    fn vol(mount: &str, free_bytes: u64) -> Volume {
        Volume {
            mount: PathBuf::from(mount),
            free_bytes,
        }
    }

    #[test]
    fn folders_on_the_same_volume_add_up() {
        let needs = sum_by_volume([(vol("C:\\", 100), 30), (vol("D:\\", 50), 70), (vol("C:\\", 100), 20)]);
        assert_eq!(
            needs,
            [
                VolumeNeed {
                    mount: "C:\\".into(),
                    free_bytes: 100,
                    needed_bytes: 50
                },
                VolumeNeed {
                    mount: "D:\\".into(),
                    free_bytes: 50,
                    needed_bytes: 70
                },
            ]
        );
    }

    #[test]
    fn two_folders_side_by_side_share_their_volume() {
        let root = tempfile::tempdir().unwrap();
        let needs = space_needed(&[(root.path().join("app"), 3), (root.path().join("models"), 4)]);
        // Some sandboxes hide their disks; when the volume is known, it is counted once.
        assert!(needs.len() <= 1, "{needs:?}");
        if let Some(need) = needs.first() {
            assert_eq!(need.needed_bytes, 7);
        }
    }

    #[test]
    fn only_the_app_itself_is_asked_to_close_its_windows() {
        let dir = Path::new("/opt/demido");
        let running = [
            (1, dir.join(demido_core::platform::exe("demido-studio"))),
            (2, dir.join("runtime").join("llama").join("llama-server")),
            (3, dir.join("runtime").join("node").join("node")),
            (4, dir.join("Demido-Studio.EXE")),
        ];
        assert_eq!(app_windows(&running), [1, 4]);
    }

    #[test]
    fn the_app_starts_from_its_folder_with_the_arguments_given() {
        let exe = Path::new("/opt/demido").join(demido_core::platform::exe("demido-studio"));
        let cmd = app_command(&exe, &[demido_core::setup_args::SKIP_UPDATE]);
        assert_eq!(cmd.get_program(), exe.as_os_str());
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["--skip-update"]);
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/opt/demido")));
        assert_eq!(app_command(&exe, &[]).get_args().count(), 0);
    }

    #[test]
    fn closing_nothing_succeeds_at_once() {
        let root = tempfile::tempdir().unwrap();
        assert!(close_app(root.path(), Duration::from_secs(10)).unwrap());
        assert!(close_app(&root.path().join("missing"), Duration::from_secs(10)).unwrap());
        assert!(wait_until_closed(root.path(), Duration::ZERO));
    }

    #[test]
    fn waiting_for_a_process_that_has_exited_returns_at_once() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd.exe").args(["/C", "exit 0"]).spawn()
        } else {
            std::process::Command::new("true").spawn()
        }
        .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        drop(child);
        let started = Instant::now();
        assert!(wait_for_exit(pid, Duration::from_secs(30)));
        assert!(started.elapsed() < Duration::from_secs(10));
        // This process runs on, so waiting for it times out.
        assert!(is_running(std::process::id()));
        assert!(!wait_for_exit(std::process::id(), Duration::from_millis(300)));
    }

    #[cfg(windows)]
    #[test]
    fn taskkill_asks_first_and_forces_only_when_told() {
        let args = |cmd: std::process::Command| -> Vec<String> {
            cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
        };
        assert_eq!(args(imp::taskkill(42, false)), ["/PID", "42"]);
        assert_eq!(args(imp::taskkill(42, true)), ["/F", "/T", "/PID", "42"]);
    }

    #[test]
    fn copies_of_this_executable_do_not_count_as_running_apps() {
        let dir = Path::new("/opt/demido");
        let app = dir.join("demido-studio");
        let uninstaller = dir.join("uninstall");
        assert!(runs_from(&app, dir, Some(&uninstaller)));
        assert!(!runs_from(&uninstaller, dir, Some(&uninstaller)));
        assert!(!runs_from(Path::new("/usr/bin/other"), dir, None));
    }

    #[cfg(windows)]
    #[test]
    fn the_cleanup_removes_only_the_uninstaller_then_the_folder_if_empty() {
        use std::process::Command;

        // A process that has already exited, so the script does not wait.
        let mut done = Command::new("cmd.exe").args(["/C", "exit 0"]).spawn().unwrap();
        let pid = done.id();
        done.wait().unwrap();

        let root = tempfile::tempdir().unwrap();
        // The ASCII apostrophe and U+2019, which PowerShell also reads as a quote, plus a `$`.
        let shared = root.path().join("O'Neil\u{2019}s $HOME Tools");
        let alone = root.path().join("O'Neil\u{2019}s Demido Studio");
        for dir in [&shared, &alone] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("uninstall.exe"), b"x").unwrap();
        }
        std::fs::write(shared.join("notes.txt"), b"not ours").unwrap();

        // Spawned exactly as the uninstaller spawns it, flags and inherited handles included.
        for dir in [&shared, &alone] {
            let status = imp::delete_after_exit_command(pid, &dir.join("uninstall.exe"), dir)
                .spawn()
                .unwrap()
                .wait()
                .unwrap();
            assert!(status.success());
        }
        assert!(
            !shared.join("uninstall.exe").exists(),
            "the uninstaller was not removed"
        );
        assert!(shared.join("notes.txt").is_file(), "an unrelated file was removed");
        assert!(
            !alone.exists(),
            "{} was left behind although it is empty",
            alone.display()
        );
    }
}
