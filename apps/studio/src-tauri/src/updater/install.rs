//! Whether this copy of the app can install updates itself, and starting the installer that does
//! (on Linux, putting the new AppImage in place of the running one).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use demido_core::InstallScope;
use demido_core::setup_args::UpdateArgs;

use crate::paths::AppPaths;

pub const DEV_BUILD: &str = "This is a development build; update it with git.";
pub const NOT_INSTALLED: &str = "This copy of Demido Studio was not installed by setup.";
pub const PACKAGED: &str = "Demido Studio was installed by your package manager, which updates it: update it there (pacman -Syu, or your AUR helper).";
pub const BY_HAND: &str =
    "This copy of Demido Studio does not update itself: download the new version from its release page.";

/// Where this copy of the app is installed, and whether it can update itself.
#[derive(Clone, Debug)]
pub struct InstallTarget {
    pub dir: Option<PathBuf>,
    pub scope: Option<InstallScope>,
    /// The AppImage this copy runs from (`$APPIMAGE`): an update replaces it.
    pub appimage: Option<PathBuf>,
    /// Why this copy cannot install updates itself; `None` when it can.
    pub unsupported: Option<String>,
}

impl InstallTarget {
    pub fn detect(paths: &AppPaths) -> Self {
        let exe = std::env::current_exe().ok();
        let appimage = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .filter(|p| cfg!(target_os = "linux") && p.is_file());
        Self {
            dir: paths.install_dir.clone(),
            scope: paths.manifest.as_ref().map(|m| m.scope),
            unsupported: unsupported_reason(
                cfg!(debug_assertions),
                cfg!(windows),
                paths.install_dir.as_deref(),
                paths.manifest.is_some(),
                exe.as_deref(),
                appimage.as_deref(),
            ),
            appimage,
        }
    }

    /// A machine-wide installation lives in Program Files: Windows asks for permission.
    pub fn needs_admin(&self) -> bool {
        self.appimage.is_none() && self.scope == Some(InstallScope::Machine)
    }

    /// Whether an update installs without asking anyone: into an installation for this person
    /// only, or over the AppImage this person runs.
    pub fn installs_quietly(&self) -> bool {
        self.appimage.is_some() || self.scope == Some(InstallScope::User)
    }
}

/// Why a copy cannot update itself. On Windows only an installation made by setup can: the
/// installer needs its `install.json` to keep every choice, and replaces the files the running app
/// came from, so that app must be the one inside the installation (not a build pointed at one
/// elsewhere). Elsewhere only an AppImage can, by replacing itself: a package's files are its
/// package manager's, and an unpacked copy is updated by hand.
pub fn unsupported_reason(
    debug: bool,
    windows: bool,
    install_dir: Option<&Path>,
    has_manifest: bool,
    exe: Option<&Path>,
    appimage: Option<&Path>,
) -> Option<String> {
    if debug {
        return Some(DEV_BUILD.into());
    }
    if !windows {
        return match (appimage, exe) {
            (Some(_), _) => None,
            (None, Some(exe)) if exe.starts_with("/usr") || exe.starts_with("/opt") => Some(PACKAGED.into()),
            _ => Some(BY_HAND.into()),
        };
    }
    let installed = match (install_dir, exe) {
        (Some(dir), Some(exe)) => has_manifest && is_inside(exe, dir),
        _ => false,
    };
    if !installed {
        return Some(NOT_INSTALLED.into());
    }
    None
}

#[cfg(unix)]
/// Puts the verified `update` in place of the AppImage at `appimage`: copied beside it first,
/// then renamed over it, so the AppImage is whole at every moment (the running one keeps the
/// file it opened). The path stays, so whatever starts it (a menu entry, say) starts the new one.
pub fn replace_appimage(update: &Path, appimage: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = appimage
        .parent()
        .ok_or_else(|| std::io::Error::other("the AppImage has no folder"))?;
    let name = appimage.file_name().unwrap_or_default().to_string_lossy();
    let next = dir.join(format!(".{name}.update"));
    let result = std::fs::copy(update, &next).and_then(|_| {
        std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o755))?;
        std::fs::File::open(&next)?.sync_all()?;
        std::fs::rename(&next, appimage)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&next);
    }
    result.map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "Could not replace {}: {e}. Move the AppImage to a folder you can write to.",
                appimage.display()
            ),
        )
    })
}

#[cfg(unix)]
/// Starts the AppImage at `appimage` once this process has ended: started earlier, it would find
/// this copy still open and hand itself over to it.
pub fn relaunch_after_exit(appimage: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("/bin/sh");
    demido_core::appimage::use_host_env(&mut cmd);
    cmd.arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(appimage)
        .env_remove(super::feed::FEED_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    cmd.spawn().map(drop)
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

#[cfg(unix)]
pub fn setup_is_running() -> bool {
    demido_core::lock::is_held(&demido_core::lock::path())
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
            unsupported_reason(true, true, Some(dir.path()), true, Some(&exe), None).as_deref(),
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
            unsupported_reason(false, true, Some(install.path()), true, Some(&inside), None),
            None
        );
        for (dir, manifest, exe) in [
            (Some(install.path()), true, Some(outside.as_path())),
            (Some(install.path()), false, Some(inside.as_path())),
            (None, true, Some(inside.as_path())),
            (Some(install.path()), true, None),
        ] {
            assert_eq!(
                unsupported_reason(false, true, dir, manifest, exe, None).as_deref(),
                Some(NOT_INSTALLED)
            );
        }
    }

    #[test]
    fn elsewhere_than_windows_only_an_appimage_updates_itself() {
        let runtimes = Path::new("/home/x/.local/opt/demido-studio");
        let appimage = Path::new("/home/x/Apps/Demido-Studio-0.5.0-x86_64.AppImage");
        let mounted = Path::new("/tmp/.mount_DemidoX/usr/bin/demido-studio");
        assert_eq!(
            unsupported_reason(false, false, Some(runtimes), true, Some(mounted), Some(appimage)),
            None
        );
        // Without its runtimes set up yet, too.
        assert_eq!(
            unsupported_reason(false, false, None, false, Some(mounted), Some(appimage)),
            None
        );
        for exe in [
            "/usr/lib/demido-studio/demido-studio",
            "/opt/demido-studio/demido-studio",
        ] {
            assert_eq!(
                unsupported_reason(false, false, Some(runtimes), true, Some(Path::new(exe)), None).as_deref(),
                Some(PACKAGED)
            );
        }
        let unpacked = Path::new("/home/x/demido-studio-0.5.0/usr/lib/demido-studio/demido-studio");
        assert_eq!(
            unsupported_reason(false, false, Some(runtimes), true, Some(unpacked), None).as_deref(),
            Some(BY_HAND)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_appimage_is_replaced_whole_and_stays_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let appimage = dir.path().join("Demido-Studio.AppImage");
        std::fs::write(&appimage, b"old").unwrap();
        let update = dir.path().join("downloaded");
        std::fs::write(&update, b"new").unwrap();
        replace_appimage(&update, &appimage).unwrap();
        assert_eq!(std::fs::read(&appimage).unwrap(), b"new");
        assert_eq!(
            std::fs::metadata(&appimage).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // Nothing left beside it, and the download stays where it was.
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(update.is_file());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_appimage_in_a_folder_it_cannot_write_says_where_to_move_it() {
        let missing = Path::new("/nonexistent-demido/Demido-Studio.AppImage");
        let dir = tempfile::tempdir().unwrap();
        let update = dir.path().join("downloaded");
        std::fs::write(&update, b"new").unwrap();
        let err = replace_appimage(&update, missing).unwrap_err().to_string();
        assert!(err.contains("Move the AppImage"), "{err}");
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
