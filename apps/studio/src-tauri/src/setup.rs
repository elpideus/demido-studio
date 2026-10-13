//! Setup where a package or an AppImage installs the app (Linux). Setup there does not carry the
//! app: it lays down the runtime, Node, uv, Python and the models in the person's own folder
//! (`demido_core::paths::default_install_dir`), where the app finds them (`paths`). It travels
//! beside the app, and the app starts it: for the first setup, once per version of the app when
//! that version pins newer runtimes than are installed, and when the person asks.
//!
//! An AppImage's files exist only while it runs, so there setup is started through the AppImage
//! itself (`$APPIMAGE --setup …`), which hands over to the setup inside it (`main.rs`).

use std::path::PathBuf;
use std::process::Command;

use demido_catalog::Catalog;
use demido_core::InstallManifest;
use demido_core::setup_args::{SETUP, SKIP_SETUP, SKIP_UPDATE, UpdateArgs};

use crate::paths::AppPaths;
use crate::settings::{Settings, SettingsStore};

/// The setup executable beside this app's, if there is one.
pub fn beside_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let setup = exe
        .parent()?
        .join(demido_core::platform::exe(demido_core::brand::SETUP_BIN));
    setup.is_file().then_some(setup)
}

/// Whether this app can start setup: on Linux, with setup beside it.
pub fn available() -> bool {
    cfg!(target_os = "linux") && beside_app().is_some()
}

/// How the person adds a missing part, for messages that say what is missing: "Python is not
/// installed. {how}."
pub fn how_to_add() -> &'static str {
    if available() {
        "Open setup from Settings, General, to add it"
    } else {
        "Run the Demido Studio installer again to add it"
    }
}

/// Setup with `args`: through the AppImage when the app runs from one, else beside the app.
fn command(args: &[String]) -> Option<Command> {
    let setup = beside_app()?;
    let mut cmd = match std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()) {
        Some(appimage) => {
            let mut cmd = Command::new(appimage);
            cmd.arg(SETUP);
            cmd
        }
        None => Command::new(setup),
    };
    cmd.args(args);
    // The AppImage sets its own environment up afresh; setup beside the app needs none of it.
    demido_core::appimage::use_host_env(&mut cmd);
    Some(cmd)
}

/// Starts setup with `args`, on its own: it outlives the app, which closes for it.
pub fn start(args: &[String]) -> std::io::Result<()> {
    let mut cmd = command(args).ok_or_else(|| std::io::Error::other("setup is not beside the app"))?;
    cmd.stdin(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a Ctrl+C meant for the app, or the app's end, leaves it running.
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}

/// What the app does at launch about setup.
#[derive(Debug, PartialEq, Eq)]
enum AtLaunch {
    Open,
    /// The first setup: no runtimes are installed.
    Install,
    /// The runtimes in this folder are older than this version pins.
    Update(PathBuf),
}

fn decide(
    manifest: Option<(&PathBuf, &InstallManifest)>,
    settings: &Settings,
    catalog: &Catalog,
    skip: bool,
) -> AtLaunch {
    if skip {
        return AtLaunch::Open;
    }
    match manifest {
        None if settings.local_setup_declined => AtLaunch::Open,
        None => AtLaunch::Install,
        Some((dir, manifest)) => {
            let tried = settings.runtime_update_tried.as_deref() == Some(demido_core::brand::VERSION);
            if !tried && !outdated(manifest, catalog).is_empty() {
                AtLaunch::Update(dir.clone())
            } else {
                AtLaunch::Open
            }
        }
    }
}

/// What in `manifest` is not what `catalog` pins: missing, or another version.
fn outdated(manifest: &InstallManifest, catalog: &Catalog) -> Vec<&'static str> {
    let rt = &catalog.runtimes;
    let mut out = Vec::new();
    if manifest
        .runtime
        .as_ref()
        .is_none_or(|r| r.release != rt.llama_cpp.release)
    {
        out.push("runtime");
    }
    if manifest.node.as_ref().is_none_or(|t| t.version != rt.node.version) {
        out.push("node");
    }
    if manifest.uv.as_ref().is_none_or(|t| t.version != rt.uv.version) {
        out.push("uv");
    }
    // The catalog pins a minor version ("3.13"), and uv installs its latest patch ("3.13.15").
    let python = &rt.python.version;
    if manifest
        .python
        .as_ref()
        .is_none_or(|t| t.version != *python && !t.version.starts_with(&format!("{python}.")))
    {
        out.push("python");
    }
    out
}

/// Starts setup when this launch needs it, and says so: the caller then exits without a window,
/// and setup opens the app when it is done. Where setup does not travel beside the app, and in
/// development, nothing happens.
pub fn at_launch(paths: &AppPaths, settings: &SettingsStore) -> bool {
    if cfg!(debug_assertions) || !available() {
        return false;
    }
    let args: Vec<String> = std::env::args().collect();
    let skip = args.iter().any(|a| a == SKIP_SETUP || a == SKIP_UPDATE);
    if args.iter().any(|a| a == SKIP_SETUP) && paths.manifest.is_none() {
        // "Not now" in setup: online models only, and setup does not open on its own again.
        let _ = settings.patch(serde_json::json!({ "localSetupDeclined": true }));
    }
    let manifest = paths.install_dir.as_ref().zip(paths.manifest.as_ref());
    let setup_args = match decide(manifest, &settings.get(), demido_catalog::catalog(), skip) {
        AtLaunch::Open => return false,
        AtLaunch::Install => {
            tracing::info!("no runtimes are installed: starting setup");
            Vec::new()
        }
        AtLaunch::Update(dir) => {
            tracing::info!(
                "the runtimes in {} are older than this version pins: starting setup to update them",
                dir.display()
            );
            let _ = settings.patch(serde_json::json!({ "runtimeUpdateTried": demido_core::brand::VERSION }));
            UpdateArgs {
                dir: Some(dir),
                wait_pid: Some(std::process::id()),
                relaunch: true,
            }
            .to_args()
        }
    };
    match start(&setup_args) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("could not start setup: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use demido_core::manifest::{RuntimeInfo, ToolInfo};

    fn manifest(catalog: &Catalog) -> InstallManifest {
        let rt = &catalog.runtimes;
        let tool = |version: &str| {
            Some(ToolInfo {
                version: version.into(),
                exe: PathBuf::from("x"),
            })
        };
        let mut m = InstallManifest::new(demido_core::InstallScope::User);
        m.runtime = Some(RuntimeInfo {
            backend: demido_core::Backend::Cuda,
            variant: "linux-cuda13".into(),
            release: rt.llama_cpp.release.clone(),
            dir: PathBuf::from("runtime/llama"),
            server: PathBuf::from("runtime/llama/llama-server"),
        });
        m.node = tool(&rt.node.version);
        m.uv = tool(&rt.uv.version);
        m.python = tool(&format!("{}.15", rt.python.version));
        m
    }

    #[test]
    fn what_this_version_pins_is_up_to_date() {
        let catalog = demido_catalog::catalog();
        assert!(outdated(&manifest(catalog), catalog).is_empty());
    }

    #[test]
    fn another_version_or_a_missing_part_is_outdated() {
        let catalog = demido_catalog::catalog();
        let mut m = manifest(catalog);
        m.runtime.as_mut().unwrap().release = "b1".into();
        m.uv = None;
        m.python.as_mut().unwrap().version = "3.12.9".into();
        assert_eq!(outdated(&m, catalog), ["runtime", "uv", "python"]);
    }

    #[test]
    fn launch_starts_setup_only_when_needed_and_once_per_version() {
        let catalog = demido_catalog::catalog();
        let dir = PathBuf::from("/home/x/.local/opt/demido-studio");
        let current = manifest(catalog);
        let mut old = manifest(catalog);
        old.node.as_mut().unwrap().version = "1.0.0".into();
        let fresh = Settings::default();

        assert_eq!(decide(None, &fresh, catalog, false), AtLaunch::Install);
        assert_eq!(decide(Some((&dir, &current)), &fresh, catalog, false), AtLaunch::Open);
        assert_eq!(
            decide(Some((&dir, &old)), &fresh, catalog, false),
            AtLaunch::Update(dir.clone())
        );
        // --skip-setup or --skip-update from setup: open whatever is installed.
        assert_eq!(decide(None, &fresh, catalog, true), AtLaunch::Open);
        assert_eq!(decide(Some((&dir, &old)), &fresh, catalog, true), AtLaunch::Open);

        let declined = Settings {
            local_setup_declined: true,
            ..Settings::default()
        };
        assert_eq!(decide(None, &declined, catalog, false), AtLaunch::Open);
        let tried = Settings {
            runtime_update_tried: Some(demido_core::brand::VERSION.into()),
            ..Settings::default()
        };
        assert_eq!(decide(Some((&dir, &old)), &tried, catalog, false), AtLaunch::Open);
        let tried_before = Settings {
            runtime_update_tried: Some("0.0.1".into()),
            ..Settings::default()
        };
        assert_eq!(
            decide(Some((&dir, &old)), &tried_before, catalog, false),
            AtLaunch::Update(dir.clone())
        );
    }
}
