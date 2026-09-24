//! Operating system helpers the installer needs: elevation, launching the app, disk space and
//! finding a running instance.

use std::path::Path;

/// Processes whose executable lives inside `dir` (other than this one).
pub fn running_app_pids(dir: &Path) -> Vec<u32> {
    let own = std::process::id();
    let Ok(dir) = dir.canonicalize() else {
        return Vec::new();
    };
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
            exe.starts_with(&dir).then_some(pid.as_u32())
        })
        .collect()
}

/// Free bytes on the volume that holds `path` (or its closest existing ancestor).
pub fn free_space(path: &Path) -> Option<u64> {
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
        .map(|d| d.available_space())
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
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
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
                Ok(()) => {
                    if !info.hProcess.is_invalid() {
                        let _ = CloseHandle(info.hProcess);
                    }
                    Ok(true)
                }
                // ERROR_CANCELLED: the UAC prompt was dismissed.
                Err(e) if e.code().0 as u32 == 0x8007_04C7 => Ok(false),
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

    /// Deletes `dir` a few seconds after this process exits (used by the uninstaller, which
    /// cannot delete its own executable while it runs).
    pub fn delete_after_exit(dir: &Path) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let script = format!(
            "ping 127.0.0.1 -n 4 > nul & rmdir /s /q \"{}\"",
            dir.display()
        );
        let _ = Command::new("cmd.exe")
            .args(["/C", &script])
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn();
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

    pub fn launch_app(exe: &Path) -> anyhow::Result<()> {
        Command::new(exe).spawn()?;
        Ok(())
    }

    pub fn delete_after_exit(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub use imp::{delete_after_exit, is_elevated, launch_app, relaunch_elevated};
