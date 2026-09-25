//! Operating system helpers the installer needs: elevation, launching the app, disk space and
//! finding a running instance.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Processes whose executable lives inside `dir`, other than this one and other copies of this
/// executable (a non-elevated uninstaller waits for the elevated copy of itself it started).
pub fn running_app_pids(dir: &Path) -> Vec<u32> {
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
            runs_from(&exe, &dir, own_exe.as_deref()).then_some(pid.as_u32())
        })
        .collect()
}

fn runs_from(exe: &Path, dir: &Path, own_exe: Option<&Path>) -> bool {
    exe.starts_with(dir) && own_exe != Some(exe)
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

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
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

    /// Launches the app as the signed-in user even from an elevated installer, so the app
    /// never inherits administrator rights.
    pub fn launch_app(exe: &Path) -> anyhow::Result<()> {
        if is_elevated() {
            Command::new("explorer.exe").arg(exe).spawn()?;
        } else {
            let mut cmd = Command::new(exe);
            if let Some(dir) = exe.parent() {
                cmd.current_dir(dir);
            }
            cmd.spawn()?;
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

    pub(super) fn delete_after_exit_command(pid: u32, file: &Path, dir: &Path) -> Command {
        use std::os::windows::process::CommandExt;
        // No window, but not DETACHED_PROCESS: a detached PowerShell exits without running its
        // command. Without a job object here it outlives the uninstaller anyway.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
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

    pub fn launch_app(exe: &Path) -> anyhow::Result<()> {
        Command::new(exe).spawn()?;
        Ok(())
    }

    /// A running executable can be deleted here, so nothing needs to wait.
    pub fn delete_after_exit(file: &Path, dir: &Path) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir(dir);
    }
}

pub use imp::{delete_after_exit, is_elevated, launch_app, relaunch_elevated, run_elevated};

#[cfg(test)]
mod tests {
    use super::*;

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
