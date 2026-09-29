//! Whether this copy of the app can install updates itself, and starting the installer that does.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use demido_core::InstallScope;
use demido_core::setup_args::UpdateArgs;

use crate::paths::AppPaths;

pub const DEV_BUILD: &str = "This is a development build; update it with git.";
pub const NOT_INSTALLED: &str = "This copy of Demido Studio was not installed by setup.";
pub const NOT_WINDOWS: &str = "Updates install on Windows only for now.";

/// Where this copy of the app is installed, and whether it can update itself.
#[derive(Clone, Debug)]
pub struct InstallTarget {
    pub dir: Option<PathBuf>,
    pub scope: Option<InstallScope>,
    /// Why this copy cannot install updates itself; `None` when it can.
    pub unsupported: Option<String>,
}

impl InstallTarget {
    pub fn detect(paths: &AppPaths) -> Self {
        let exe = std::env::current_exe().ok();
        Self {
            dir: paths.install_dir.clone(),
            scope: paths.manifest.as_ref().map(|m| m.scope),
            unsupported: unsupported_reason(
                cfg!(debug_assertions),
                cfg!(windows),
                paths.install_dir.as_deref(),
                paths.manifest.is_some(),
                exe.as_deref(),
            ),
        }
    }

    /// A machine-wide installation lives in Program Files: Windows asks for permission.
    pub fn needs_admin(&self) -> bool {
        self.scope == Some(InstallScope::Machine)
    }
}

/// Why a copy cannot update itself. Only an installation made by setup can: the installer needs
/// its `install.json` to keep every choice, and replaces the files the running app came from, so
/// that app must be the one inside the installation (not a build pointed at one elsewhere).
pub fn unsupported_reason(
    debug: bool,
    windows: bool,
    install_dir: Option<&Path>,
    has_manifest: bool,
    exe: Option<&Path>,
) -> Option<String> {
    if debug {
        return Some(DEV_BUILD.into());
    }
    let installed = match (install_dir, exe) {
        (Some(dir), Some(exe)) => has_manifest && is_inside(exe, dir),
        _ => false,
    };
    if !installed {
        return Some(NOT_INSTALLED.into());
    }
    if !windows {
        return Some(NOT_WINDOWS.into());
    }
    None
}

fn is_inside(path: &Path, dir: &Path) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canonical(path).starts_with(canonical(dir))
}

/// Whether setup is installing or updating right now: it holds `brand::SETUP_LOCK` while it does.
/// An app opened then would run from files that are being replaced.
#[cfg(windows)]
pub fn setup_is_running() -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    use windows::core::HSTRING;

    let name = HSTRING::from(demido_core::brand::SETUP_LOCK);
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) } {
        Ok(handle) => {
            unsafe {
                let _ = CloseHandle(handle);
            }
            true
        }
        // A setup running elevated holds it, and this process may not open what it created.
        Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
    }
}

#[cfg(not(windows))]
pub fn setup_is_running() -> bool {
    false
}

/// Starts `installer` to update the installation in `install_dir` once this process has exited,
/// and to start the app again afterwards. It runs detached, so it outlives the app.
pub fn launch_installer(installer: &Path, install_dir: &Path, working_dir: &Path) -> std::io::Result<()> {
    let args = UpdateArgs {
        dir: Some(install_dir.to_path_buf()),
        wait_pid: Some(std::process::id()),
        relaunch: true,
    }
    .to_args();
    tracing::info!("starting {} {}", installer.display(), args.join(" "));
    spawn_detached(installer, &args, working_dir)
}

/// The installer's command. It does not inherit a test feed ([`super::feed::FEED_ENV`]): a
/// per-user update would pass it on to the app it starts again at the end, which would keep
/// reading it, while an elevated, machine-wide one starts with a fresh environment. Without it
/// both go back to GitHub.
fn command(program: &Path, args: &[String], working_dir: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_remove(super::feed::FEED_ENV)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

#[cfg(windows)]
fn spawn_detached(program: &Path, args: &[String], working_dir: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    // Out of any job the app was started in, so closing that job cannot end the update halfway.
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    match command(program, args, working_dir)
        .creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(err) => {
            // A job that forbids breaking away refuses the flag; inside it is still better than
            // no update.
            tracing::info!("the installer could not leave the app's job ({err}); starting it inside");
            command(program, args, working_dir)
                .creation_flags(detached)
                .spawn()
                .map(drop)
        }
    }
}

#[cfg(not(windows))]
fn spawn_detached(program: &Path, args: &[String], working_dir: &Path) -> std::io::Result<()> {
    command(program, args, working_dir).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_development_build_never_updates_itself() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("demido-studio.exe");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(
            unsupported_reason(true, true, Some(dir.path()), true, Some(&exe)).as_deref(),
            Some(DEV_BUILD)
        );
    }

    #[test]
    fn only_the_app_inside_an_installation_updates_itself() {
        let install = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let inside = install.path().join("demido-studio.exe");
        let outside = elsewhere.path().join("demido-studio.exe");
        std::fs::write(&inside, b"").unwrap();
        std::fs::write(&outside, b"").unwrap();

        assert_eq!(
            unsupported_reason(false, true, Some(install.path()), true, Some(&inside)),
            None
        );
        for (dir, manifest, exe) in [
            (Some(install.path()), true, Some(outside.as_path())),
            (Some(install.path()), false, Some(inside.as_path())),
            (None, true, Some(inside.as_path())),
            (Some(install.path()), true, None),
        ] {
            assert_eq!(
                unsupported_reason(false, true, dir, manifest, exe).as_deref(),
                Some(NOT_INSTALLED)
            );
        }
    }

    #[test]
    fn installations_elsewhere_than_windows_wait() {
        let install = tempfile::tempdir().unwrap();
        let exe = install.path().join("demido-studio");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(
            unsupported_reason(false, false, Some(install.path()), true, Some(&exe)).as_deref(),
            Some(NOT_WINDOWS)
        );
    }

    #[test]
    fn the_installer_does_not_inherit_a_test_feed() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = command(Path::new("setup.exe"), &["--update".to_string()], dir.path());
        let feed = std::ffi::OsStr::new(super::super::feed::FEED_ENV);
        assert!(
            cmd.get_envs().any(|(key, value)| key == feed && value.is_none()),
            "{} must be removed from the installer's environment",
            super::super::feed::FEED_ENV
        );
        assert_eq!(cmd.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn a_sibling_folder_with_the_same_prefix_is_not_inside() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Demido Studio");
        let sibling = root.path().join("Demido Studio Old");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let exe = sibling.join("demido-studio.exe");
        std::fs::write(&exe, b"").unwrap();
        let own = install.join("demido-studio.exe");
        std::fs::write(&own, b"").unwrap();
        assert!(!is_inside(&exe, &install));
        assert!(is_inside(&own, &install));
    }
}
