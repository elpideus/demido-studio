//! The setup wizard's backend. The installation itself is `demido-provision`; this crate adds
//! the window, the choices the wizard offers, elevation for machine-wide installs, launching
//! the app at the end, and the uninstaller (the same executable, run with `--uninstall`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use demido_catalog::{BackendChoice, Family, ModelRecommendation};
use demido_core::{Backend, InstallManifest, InstallScope};
use demido_fetch::CancellationToken;
use demido_hardware::HardwareReport;
use demido_provision::system::VolumeNeed;
use demido_provision::{AppPayload, InstallPlan, ProvisionEvent, UninstallOptions};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// The app, zipped, compiled into this executable by build.rs (empty in development builds).
static PAYLOAD: &[u8] = include_bytes!(env!("DEMIDO_PAYLOAD_PATH"));

const EVENT: &str = "setup://event";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Install,
    Uninstall,
}

struct SetupState {
    mode: Mode,
    hardware: HardwareReport,
    /// Wizard state handed over by the non-elevated instance.
    resume: Option<serde_json::Value>,
    cancel: Mutex<Option<CancellationToken>>,
    /// Install folder of this uninstaller, in uninstall mode.
    uninstall_dir: Option<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExistingInstall {
    dir: String,
    scope: InstallScope,
    version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Context {
    version: String,
    mode: Mode,
    has_payload: bool,
    hardware: HardwareReport,
    choices: Vec<BackendChoice>,
    default_backend: Backend,
    families: Vec<Family>,
    recommendations: BTreeMap<String, ModelRecommendation>,
    default_dirs: BTreeMap<String, String>,
    elevated: bool,
    existing: Option<ExistingInstall>,
    resume: Option<serde_json::Value>,
    /// Download sizes of the parts that do not depend on the choices.
    fixed_sizes: BTreeMap<String, u64>,
    uninstall_dir: Option<String>,
}

fn find_existing() -> Option<ExistingInstall> {
    let mut candidates: Vec<PathBuf> = vec![
        demido_core::paths::default_install_dir(InstallScope::User),
        demido_core::paths::default_install_dir(InstallScope::Machine),
    ];
    candidates.extend(registered_install_dirs());
    candidates.into_iter().find_map(|dir| {
        let m = demido_provision::folder::installation(&dir)?;
        Some(ExistingInstall {
            dir: dir.to_string_lossy().into_owned(),
            scope: m.scope,
            version: m.version,
        })
    })
}

#[cfg(windows)]
fn registered_install_dirs() -> Vec<PathBuf> {
    let path = format!(
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{}",
        demido_core::brand::UNINSTALL_KEY
    );
    [windows_registry::CURRENT_USER, windows_registry::LOCAL_MACHINE]
        .into_iter()
        .filter_map(|root| root.open(&path).ok()?.get_string("InstallLocation").ok())
        // Some installers quote the path.
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn registered_install_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[tauri::command]
fn setup_context(state: State<'_, Arc<SetupState>>) -> Context {
    let catalog = demido_catalog::catalog();
    let choices = demido_catalog::backend_choices(&state.hardware, catalog);
    let recommendations = choices
        .iter()
        .filter(|c| c.available)
        .map(|c| {
            let key = serde_json::to_value(c.backend)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            (key, demido_catalog::recommend_models(c, catalog))
        })
        .collect();
    let (os, arch) = (demido_core::Os::current(), demido_core::Arch::current());
    let rt = &catalog.runtimes;
    let mut fixed_sizes = BTreeMap::new();
    fixed_sizes.insert("app".into(), PAYLOAD.len() as u64);
    if let Some(a) = rt.node.for_platform(os, arch) {
        fixed_sizes.insert("node".into(), a.size);
    }
    if let Some(a) = rt.uv.for_platform(os, arch) {
        fixed_sizes.insert("uv".into(), a.size);
    }
    fixed_sizes.insert(
        "python".into(),
        rt.python.estimated_size + rt.python.estimated_packages_size,
    );

    let mut default_dirs = BTreeMap::new();
    for (key, scope) in [("user", InstallScope::User), ("machine", InstallScope::Machine)] {
        default_dirs.insert(
            key.to_string(),
            demido_core::paths::default_install_dir(scope)
                .to_string_lossy()
                .into_owned(),
        );
    }
    Context {
        version: demido_core::brand::VERSION.to_string(),
        mode: state.mode,
        has_payload: !PAYLOAD.is_empty(),
        default_backend: demido_catalog::default_backend(&choices),
        choices,
        families: catalog.models.families.clone(),
        recommendations,
        default_dirs,
        elevated: demido_provision::system::is_elevated(),
        existing: find_existing(),
        resume: state.resume.clone(),
        fixed_sizes,
        hardware: state.hardware.clone(),
        uninstall_dir: state.uninstall_dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
    }
}

/// The install folder as typed, without the spaces a paste tends to bring along.
fn install_path(raw: &str) -> PathBuf {
    PathBuf::from(raw.trim())
}

/// How the folder looks as an install folder. Its problem, if any, blocks the installation.
fn inspect_folder(dir: &Path) -> demido_provision::folder::FolderCheck {
    demido_provision::folder::check(dir, &demido_core::paths::user_data_dir())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirInfo {
    writable: bool,
    free_bytes: Option<u64>,
    has_install: bool,
    running: bool,
    /// Holds an `install.json` setup cannot read, which it must not overwrite.
    has_unreadable_install: bool,
    /// Holds files that are not a Demido Studio installation.
    has_other_files: bool,
    /// Is, holds or sits inside the folder with the person's chats and models.
    overlaps_user_data: bool,
    /// Why setup must not install here, ready to show.
    problem: Option<&'static str>,
}

#[tauri::command]
fn check_dir(path: String) -> DirInfo {
    let dir = install_path(&path);
    let folder = inspect_folder(&dir);
    DirInfo {
        writable: demido_provision::system::can_write(&dir),
        free_bytes: demido_provision::system::free_space(&dir),
        has_install: folder.has_install,
        running: !demido_provision::system::running_app_pids(&dir).is_empty(),
        has_unreadable_install: folder.unreadable_install,
        has_other_files: folder.other_files,
        overlaps_user_data: folder.overlaps_user_data,
        problem: folder.problem(),
    }
}

/// Free and needed space on each volume the installation writes to: `install_bytes` go to the
/// install folder, `model_bytes` to the starter models folder of `scope`.
#[tauri::command]
fn check_space(scope: InstallScope, install_dir: String, install_bytes: u64, model_bytes: u64) -> Vec<VolumeNeed> {
    let mut needs = vec![(install_path(&install_dir), install_bytes)];
    if model_bytes > 0 {
        needs.push((demido_core::paths::starter_models_dir(scope), model_bytes));
    }
    demido_provision::system::space_needed(&needs)
}

/// Restarts the wizard with administrator rights, handing over its state. Returns false when
/// the person declined; on success the caller closes this window.
#[tauri::command]
fn relaunch_elevated(app: AppHandle, wizard: serde_json::Value) -> Result<bool, String> {
    let file = std::env::temp_dir().join(format!("demido-setup-{}.json", std::process::id()));
    std::fs::write(&file, wizard.to_string()).map_err(|e| e.to_string())?;
    let accepted = demido_provision::system::relaunch_elevated(&format!("--resume \"{}\"", file.display()))
        .map_err(|e| e.to_string())?;
    if accepted {
        app.exit(0);
    }
    Ok(accepted)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanRequest {
    scope: InstallScope,
    install_dir: String,
    backend: Backend,
    /// Model family to download, or none.
    family: Option<String>,
    shortcuts: bool,
}

#[tauri::command]
fn start_install(app: AppHandle, state: State<'_, Arc<SetupState>>, request: PlanRequest) -> Result<(), String> {
    let catalog = demido_catalog::catalog();
    let choices = demido_catalog::backend_choices(&state.hardware, catalog);
    let choice = choices
        .iter()
        .find(|c| c.backend == request.backend && c.available)
        .ok_or("That runtime cannot run on this computer.")?;
    let rec = demido_catalog::recommend_models(choice, catalog);
    let model = request
        .family
        .as_ref()
        .and_then(|f| rec.picks.iter().find(|(id, _)| id == f).map(|(_, p)| p.clone()));
    let install_dir = install_path(&request.install_dir);
    if install_dir.as_os_str().is_empty() {
        return Err("Choose an install folder.".into());
    }
    if let Some(problem) = inspect_folder(&install_dir).problem() {
        return Err(problem.into());
    }
    let payload = (!PAYLOAD.is_empty()).then(|| AppPayload {
        zip: Arc::from(PAYLOAD),
        uninstaller_source: std::env::current_exe().ok(),
    });
    let plan = InstallPlan {
        scope: request.scope,
        install_dir,
        backend: request.backend,
        variant: choice.variant.clone().ok_or("No runtime build for this choice.")?,
        model,
        model_context: rec.context_length,
        models_dir: demido_core::paths::starter_models_dir(request.scope),
        python: true,
        node: true,
        shortcuts: request.shortcuts,
        register: payload.is_some(),
        payload,
        hardware: serde_json::to_value(&state.hardware).unwrap_or_default(),
    };

    let cancel = CancellationToken::new();
    *state.cancel.lock() = Some(cancel.clone());
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let emit = move |event: ProvisionEvent| {
            let _ = emitter.emit(EVENT, event);
        };
        if let Err(err) = demido_provision::run(&plan, demido_catalog::catalog(), cancel, &emit).await {
            tracing::error!("installation failed: {err:#}");
            let _ = app.emit("setup://fatal", format!("{err:#}"));
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_install(state: State<'_, Arc<SetupState>>) {
    if let Some(c) = state.cancel.lock().take() {
        c.cancel();
    }
}

/// Starts the installed app (as the signed-in user) and closes the installer.
#[tauri::command]
fn launch_app(app: AppHandle, install_dir: String) -> Result<(), String> {
    let exe = install_path(&install_dir).join(demido_core::platform::exe(demido_core::brand::STUDIO_BIN));
    if !exe.is_file() {
        return Err(format!("{} was not found.", exe.display()));
    }
    demido_provision::system::launch_app(&exe).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn uninstall(state: State<'_, Arc<SetupState>>, remove_user_data: bool) -> Result<Vec<String>, String> {
    let dir = state
        .uninstall_dir
        .clone()
        .ok_or("This uninstaller does not belong to an installation.")?;
    tauri::async_runtime::spawn_blocking(move || {
        demido_provision::uninstall(&UninstallOptions {
            install_dir: dir,
            remove_user_data,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn window_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Where this executable lives when it is an installed uninstaller.
fn own_install_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join(InstallManifest::FILE_NAME).is_file().then_some(dir)
}

/// Whether `exe` is the uninstaller setup copies into the install folder, so opening it (a
/// double-click, say) uninstalls instead of starting setup.
fn is_uninstaller(exe: &Path) -> bool {
    exe.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(demido_core::brand::UNINSTALLER_BIN))
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// `--uninstall --quiet`: removes the installation in `dir` without a window. Returns the exit
/// code, non-zero when nothing was removed.
fn uninstall_quietly(dir: Option<&Path>) -> i32 {
    let Some(dir) = dir else {
        tracing::error!("no installation found next to this uninstaller");
        return 1;
    };
    match demido_provision::uninstall(&UninstallOptions {
        install_dir: dir.to_path_buf(),
        remove_user_data: false,
    }) {
        Ok(notes) => {
            for note in notes {
                tracing::info!("{note}");
            }
            0
        }
        Err(err) => {
            tracing::error!("uninstall failed: {err:#}");
            1
        }
    }
}

/// Exit code of a quiet uninstall that had to run again with administrator rights.
fn elevated_exit_code(result: anyhow::Result<Option<u32>>) -> i32 {
    match result {
        Ok(Some(code)) => code as i32,
        Ok(None) => {
            tracing::error!("administrator permission was not given");
            1
        }
        Err(err) => {
            tracing::error!("could not run the uninstaller as administrator: {err:#}");
            1
        }
    }
}

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("DEMIDO_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .try_init();
    let args: Vec<String> = std::env::args().collect();
    let own_dir = own_install_dir();
    let uninstalling = args.iter().any(|a| a == "--uninstall")
        || (own_dir.is_some() && std::env::current_exe().is_ok_and(|exe| is_uninstaller(&exe)));
    let uninstall_dir = if uninstalling { own_dir } else { None };

    if uninstalling {
        let quiet = args.iter().any(|a| a == "--quiet");
        // A machine-wide installation can only be removed with administrator rights.
        let scope = uninstall_dir
            .as_deref()
            .and_then(demido_provision::folder::installation)
            .map(|m| m.scope);
        if scope == Some(InstallScope::Machine) && !demido_provision::system::is_elevated() {
            if quiet {
                // Whoever asked for a quiet uninstall reads the exit code, so wait for the result.
                let result = demido_provision::system::run_elevated("--uninstall --quiet");
                std::process::exit(elevated_exit_code(result));
            }
            let _ = demido_provision::system::relaunch_elevated("--uninstall");
            return;
        }
        if quiet {
            std::process::exit(uninstall_quietly(uninstall_dir.as_deref()));
        }
    }

    let resume = arg_value(&args, "--resume").and_then(|file| {
        let text = std::fs::read_to_string(Path::new(&file)).ok()?;
        let _ = std::fs::remove_file(&file);
        serde_json::from_str(&text).ok()
    });

    let state = Arc::new(SetupState {
        mode: if uninstalling { Mode::Uninstall } else { Mode::Install },
        hardware: demido_hardware::detect(),
        resume,
        cancel: Mutex::new(None),
        uninstall_dir,
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            setup_context,
            check_dir,
            check_space,
            relaunch_elevated,
            start_install,
            cancel_install,
            launch_app,
            uninstall,
            window_ready,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Demido Studio Setup");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_are_trimmed_everywhere() {
        assert_eq!(
            install_path("  C:\\Apps\\Demido Studio \n"),
            Path::new("C:\\Apps\\Demido Studio")
        );
        assert_eq!(install_path("   "), Path::new(""));
    }

    #[test]
    fn the_copied_uninstaller_is_recognised_by_its_name() {
        let dir = Path::new("C:\\Program Files\\Demido Studio");
        assert!(is_uninstaller(&dir.join(demido_core::platform::exe("uninstall"))));
        assert!(is_uninstaller(&dir.join("Uninstall.EXE")));
        assert!(!is_uninstaller(&dir.join("Demido-Studio-Setup-0.1.0.exe")));
        assert!(!is_uninstaller(&dir.join("uninstaller.exe")));
    }

    #[test]
    fn a_quiet_uninstall_without_an_installation_fails() {
        assert_eq!(uninstall_quietly(None), 1);
    }

    #[test]
    fn a_quiet_elevated_uninstall_passes_on_its_result() {
        assert_eq!(elevated_exit_code(Ok(Some(0))), 0);
        assert_eq!(elevated_exit_code(Ok(Some(1))), 1);
        assert_eq!(elevated_exit_code(Ok(None)), 1, "declined prompt");
        assert_eq!(elevated_exit_code(Err(anyhow::anyhow!("no shell"))), 1);
    }
}
