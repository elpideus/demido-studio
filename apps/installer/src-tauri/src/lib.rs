//! The setup wizard's backend. The installation itself is `demido-provision`; this crate adds
//! the window, the choices the wizard offers, elevation for machine-wide installs, launching
//! the app at the end, the uninstaller (the same executable, run with `--uninstall`) and the
//! update the app starts (run with `--update`, see `demido_core::setup_args`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use demido_catalog::{BackendChoice, Family, ModelRecommendation};
use demido_core::setup_args::{self, UpdateArgs};
use demido_core::{Backend, InstallManifest, InstallScope};
use demido_fetch::CancellationToken;
use demido_hardware::HardwareReport;
use demido_provision::system::{SetupLock, VolumeNeed};
use demido_provision::{AppPayload, InstallPlan, ProvisionEvent, StepId, UninstallOptions};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// The app, zipped, compiled into this executable by build.rs (empty in development builds).
static PAYLOAD: &[u8] = include_bytes!(env!("DEMIDO_PAYLOAD_PATH"));

const EVENT: &str = "setup://event";
const FATAL: &str = "setup://fatal";

/// How long an update waits for the app that started it to exit.
const APP_EXIT_WAIT: Duration = Duration::from_secs(60);
/// How long an update then waits for anything else still running from the install folder (the
/// app's helpers exit with it).
const HELPERS_EXIT_WAIT: Duration = Duration::from_secs(15);
/// How long the app gets to close by itself before whatever is left is ended.
const CLOSE_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Install,
    Uninstall,
    /// `--update`: installs the app this setup carries over an installation, in a small window
    /// without the wizard.
    Update,
}

struct SetupState {
    mode: Mode,
    hardware: HardwareReport,
    /// Wizard state handed over by the non-elevated instance.
    resume: Option<serde_json::Value>,
    cancel: Mutex<Option<CancellationToken>>,
    /// Install folder of this uninstaller, in uninstall mode.
    uninstall_dir: Option<PathBuf>,
    /// What `--update` asked for, in update mode.
    update: Option<UpdateRequest>,
    /// The app that started the update (`--wait-pid`). The first update run waits for it; a retry
    /// does not, since by then the number may belong to another process.
    wait_pid: Mutex<Option<u32>>,
    /// Held from the moment this setup starts changing an installation, so a second setup does
    /// not change the same files at the same time and the app does not open halfway through.
    lock: Mutex<Option<SetupLock>>,
    /// Whether a run is under way, and whether the window was closed during it.
    run: Mutex<RunGate>,
}

impl SetupState {
    /// Takes the setup lock for this process, unless it already holds it.
    fn lock(&self) -> Result<(), String> {
        let mut held = self.lock.lock();
        if held.is_none() {
            *held = Some(SetupLock::acquire().ok_or(
                "Another Demido Studio Setup is installing or updating right now. Wait for it to finish, then try again.",
            )?);
        }
        Ok(())
    }

    /// An update the app started: nothing to ask, and the app starts again once it is in place.
    fn automatic(&self) -> bool {
        self.update.as_ref().is_some_and(|u| u.relaunch)
    }
}

/// Whether a run is under way, and whether the window was closed during it. Ending the process
/// in the middle of a run could leave the install folder half swapped, so while one is under way
/// closing the window only hides it, and the process ends with the run.
#[derive(Debug, Default)]
struct RunGate {
    running: bool,
    closed: bool,
}

impl RunGate {
    /// A run starts.
    fn start(&mut self) {
        self.running = true;
        self.closed = false;
    }

    /// The window is being closed. Returns whether a run is under way, in which case the window
    /// must stay open (hidden) until [`RunGate::finish`] says the process may end.
    fn close_requested(&mut self) -> bool {
        if self.running {
            self.closed = true;
        }
        self.running
    }

    /// The run ended. Returns whether the window was closed during it, so the process ends now.
    fn finish(&mut self) -> bool {
        self.running = false;
        std::mem::take(&mut self.closed)
    }
}

/// Whether closing the window during a run stops it. A run the person started stops, as with
/// Cancel: between steps, since the app step always finishes or puts the previous version back.
/// An automatic update, whose window offers no Cancel, finishes out of sight instead, and the
/// app starts again as it would have: stopping it partway would leave the new app without the
/// rest of the update, and the person without the app they were using.
fn close_stops_run(automatic: bool) -> bool {
    !automatic
}

/// What happens once a run whose window was closed has ended.
#[derive(Debug, PartialEq, Eq)]
enum AfterClose {
    /// An automatic update put the new version in place: start it, as the window would have.
    StartApp,
    /// An automatic update failed. Its window had no Cancel, so closing it was not a choice to
    /// stop: the failure page comes back, with Retry and "Open Demido Studio".
    ShowFailure,
    /// A run the person started, and cancelled by closing its window.
    Exit,
}

fn after_close(automatic: bool, succeeded: bool) -> AfterClose {
    match (automatic, succeeded) {
        (true, true) => AfterClose::StartApp,
        (true, false) => AfterClose::ShowFailure,
        (false, _) => AfterClose::Exit,
    }
}

/// An update asked for on the command line, once the installation is found.
#[derive(Clone, Debug, PartialEq, Eq)]
struct UpdateRequest {
    /// The installation to update.
    dir: PathBuf,
    /// Start the app again once the update is in place.
    relaunch: bool,
}

/// How the installed version compares to this setup's.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Relation {
    /// Installed is older: this setup updates it.
    Older,
    Same,
    /// Installed is newer: this setup would go back to an earlier version.
    Newer,
}

/// How `installed` compares to `ours`, both semantic versions. A version that does not parse
/// counts as older: only builds that were never released wrote one, and updating them is right.
fn relation(installed: &str, ours: &str) -> Relation {
    let parse = |v: &str| semver::Version::parse(v.trim().trim_start_matches('v')).ok();
    let (Some(installed), Some(ours)) = (parse(installed), parse(ours)) else {
        return Relation::Older;
    };
    // Build metadata does not make one version newer than another.
    let key = |v: &semver::Version| (v.major, v.minor, v.patch, v.pre.clone());
    match key(&installed).cmp(&key(&ours)) {
        std::cmp::Ordering::Less => Relation::Older,
        std::cmp::Ordering::Equal => Relation::Same,
        std::cmp::Ordering::Greater => Relation::Newer,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExistingInstall {
    dir: String,
    scope: InstallScope,
    version: String,
    relation: Relation,
}

/// What the update window shows, in update mode.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateLaunch {
    dir: String,
    relaunch: bool,
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
    update_launch: Option<UpdateLaunch>,
}

/// The installation in `dir`, if setup can act on it.
fn existing_at(dir: &Path) -> Option<ExistingInstall> {
    let m = demido_provision::folder::installation(dir)?;
    Some(ExistingInstall {
        dir: dir.to_string_lossy().into_owned(),
        scope: m.scope,
        relation: relation(&m.version, demido_core::brand::VERSION),
        version: m.version,
    })
}

fn find_existing() -> Option<ExistingInstall> {
    let mut candidates: Vec<PathBuf> = vec![
        demido_core::paths::default_install_dir(InstallScope::User),
        demido_core::paths::default_install_dir(InstallScope::Machine),
    ];
    candidates.extend(registered_install_dirs());
    candidates.into_iter().find_map(|dir| existing_at(&dir))
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
        // An update is for the installation it was asked for, and only that one.
        existing: match &state.update {
            Some(update) => existing_at(&update.dir),
            None => find_existing(),
        },
        resume: state.resume.clone(),
        fixed_sizes,
        hardware: state.hardware.clone(),
        uninstall_dir: state.uninstall_dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
        update_launch: state.update.as_ref().map(|u| UpdateLaunch {
            dir: u.dir.to_string_lossy().into_owned(),
            relaunch: u.relaunch,
        }),
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
    state.lock()?;
    let payload = app_payload(false);
    let plan = InstallPlan {
        scope: request.scope,
        install_dir,
        backend: request.backend,
        variant: choice.variant.clone().ok_or("No runtime build for this choice.")?,
        model,
        // Search by meaning works with any model, local or online, so it comes either way.
        search_model: Some(demido_catalog::search_model(choice, catalog).clone()),
        model_context: rec.context_length,
        models_dir: demido_core::paths::starter_models_dir(request.scope),
        python: true,
        node: true,
        shortcuts: request.shortcuts,
        register: payload.is_some(),
        payload,
        hardware: serde_json::to_value(&state.hardware).unwrap_or_default(),
    };
    spawn_run(app, state.inner().clone(), plan, None);
    Ok(())
}

/// The app this setup carries, with this executable as the uninstaller to copy next to it;
/// `None` in development builds. `signature_required` is [`AppPayload::signature_required`].
fn app_payload(signature_required: bool) -> Option<AppPayload> {
    (!PAYLOAD.is_empty()).then(|| AppPayload {
        zip: Arc::from(PAYLOAD),
        uninstaller_source: std::env::current_exe().ok(),
        signature_required,
    })
}

/// Whether the uninstaller this setup leaves behind must match the release signature next to
/// this executable: when it runs `elevated` for an update the app started. It then runs from the
/// app's updates folder, which the user can write to, and copies itself where only administrators
/// can. The app writes the signature next to every installer it stages.
fn signature_required(elevated: bool, update: Option<&UpdateRequest>) -> bool {
    elevated && update.is_some_and(|u| u.relaunch)
}

/// Runs `plan` in the background, reporting on `setup://event` and, when it cannot finish, on
/// `setup://fatal`; `cancel_install` stops it. An update first waits for `wait`. While it runs,
/// closing the window only hides it (see [`RunGate`]).
fn spawn_run(app: AppHandle, state: Arc<SetupState>, plan: InstallPlan, wait: Option<AppExit>) {
    let cancel = CancellationToken::new();
    *state.cancel.lock() = Some(cancel.clone());
    // Before the run is under way, so closing the window from now on waits for it.
    state.run.lock().start();
    let studio = plan
        .install_dir
        .join(demido_core::platform::exe(demido_core::brand::STUDIO_BIN));
    tauri::async_runtime::spawn(async move {
        // In a task of its own, so even a panic in the run ends up here, as a failure.
        let result = tauri::async_runtime::spawn(execute(app.clone(), plan, wait, cancel))
            .await
            .unwrap_or_else(|e| Err(format!("Setup stopped unexpectedly: {e}")));
        // Nothing changes the installation any more: the app may open, even while this window
        // still shows how it went.
        drop(state.lock.lock().take());
        let closed = state.run.lock().finish();
        if let Err(message) = &result {
            let _ = app.emit(FATAL, message);
        }
        if closed {
            match after_close(state.automatic(), result.is_ok()) {
                AfterClose::StartApp => {
                    if let Err(err) = demido_provision::system::launch_app(&studio, &[]) {
                        tracing::error!("could not start {}: {err:#}", studio.display());
                    }
                    app.exit(0);
                }
                AfterClose::ShowFailure => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                AfterClose::Exit => app.exit(0),
            }
        }
    });
}

/// Waits for `wait`, then runs `plan`. Fails with what to tell the person.
async fn execute(
    app: AppHandle,
    plan: InstallPlan,
    wait: Option<AppExit>,
    cancel: CancellationToken,
) -> Result<(), String> {
    let emit = move |event: ProvisionEvent| {
        let _ = app.emit(EVENT, event);
    };
    if let Some(wait) = wait {
        let log = emit.clone();
        tauri::async_runtime::spawn_blocking(move || wait.wait(&log))
            .await
            .map_err(|e| e.to_string())
            .and_then(|waited| waited)
            .inspect_err(|message| tracing::error!("update not started: {message}"))?;
    }
    if cancel.is_cancelled() {
        return Err("Cancelled.".into());
    }
    demido_provision::run(&plan, demido_catalog::catalog(), cancel, &emit)
        .await
        .map(|_| ())
        .map_err(|err| {
            tracing::error!("installation failed: {err:#}");
            format!("{err:#}")
        })
}

/// What an update waits for before it touches a file: the app that started it, then anything
/// else still running from the install folder.
#[derive(Debug)]
struct AppExit {
    dir: PathBuf,
    /// The app that started the update, which agreed to exit.
    pid: Option<u32>,
    app_timeout: Duration,
    helpers_timeout: Duration,
    /// How long what is left gets to close by itself before it is ended.
    close_grace: Duration,
}

impl AppExit {
    fn new(dir: &Path, pid: Option<u32>) -> Self {
        Self {
            dir: dir.to_path_buf(),
            pid,
            app_timeout: APP_EXIT_WAIT,
            helpers_timeout: HELPERS_EXIT_WAIT,
            close_grace: CLOSE_GRACE,
        }
    }

    /// Waits, reporting on `log` what it waits for. When the app that started the update agreed
    /// to exit but something still runs from the folder afterwards (an exit that hangs, a helper
    /// left behind, often without a window anyone could close), setup closes it, ending it if it
    /// must. Fails with what to tell the person when something still runs from the folder.
    fn wait(&self, log: &dyn Fn(ProvisionEvent)) -> Result<(), String> {
        let note = |line: &str| {
            log(ProvisionEvent::Log {
                id: StepId::App,
                line: line.to_string(),
            })
        };
        if let Some(pid) = self.pid
            && demido_provision::system::is_running(pid)
        {
            note("Waiting for Demido Studio to close");
            if !demido_provision::system::wait_for_exit(pid, self.app_timeout) {
                note("Demido Studio has not closed yet");
            }
        }
        if !demido_provision::system::running_app_pids(&self.dir).is_empty() {
            note("Waiting for Demido Studio's helper programs to close");
        }
        if demido_provision::system::wait_until_closed(&self.dir, self.helpers_timeout) {
            return Ok(());
        }
        // Without the app's word (a retry), what runs there may be in use: the window offers to
        // close it instead.
        if self.pid.is_some() {
            note("Closing what is still running from the install folder");
            match demido_provision::system::close_app(&self.dir, self.close_grace) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(err) => tracing::error!("could not close what runs from {}: {err:#}", self.dir.display()),
            }
        }
        Err(format!(
            "Demido Studio is still running from {}. Close it and try again.",
            self.dir.display()
        ))
    }
}

/// Updates the installation in `dir` to the app this setup carries, keeping every choice made
/// when it was installed. Returns once the update has started; it reports like `start_install`.
#[tauri::command]
fn start_update(app: AppHandle, state: State<'_, Arc<SetupState>>, dir: String) -> Result<(), String> {
    let dir = install_path(&dir);
    let manifest = demido_provision::folder::installation(&dir)
        .ok_or_else(|| format!("No Demido Studio installation was found in {}.", dir.display()))?;
    let elevated = demido_provision::system::is_elevated();
    let payload =
        app_payload(signature_required(elevated, state.update.as_ref())).ok_or("This setup has no app inside it.")?;
    state.lock()?;
    if manifest.scope == InstallScope::Machine && !elevated {
        return Err("Updating an installation for everyone needs administrator permission.".into());
    }
    let plan = InstallPlan::for_update(
        &dir,
        &manifest,
        &state.hardware,
        demido_catalog::catalog(),
        Some(payload),
    );
    let wait = AppExit::new(&dir, state.wait_pid.lock().take());
    spawn_run(app, state.inner().clone(), plan, Some(wait));
    Ok(())
}

/// Closes Demido Studio running from `dir`, the way closing its window does, and ends what is
/// left after a grace period. Returns whether nothing runs from `dir` any more.
#[tauri::command]
async fn close_app(dir: String) -> Result<bool, String> {
    let dir = install_path(&dir);
    tauri::async_runtime::spawn_blocking(move || demido_provision::system::close_app(&dir, CLOSE_GRACE))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// Starts setup again with administrator rights to update the installation in `dir`, and
/// closes this one. Returns false when the person declined.
#[tauri::command]
fn elevate_update(app: AppHandle, dir: String) -> Result<bool, String> {
    let args = UpdateArgs {
        dir: Some(install_path(&dir)),
        wait_pid: None,
        relaunch: false,
    };
    let accepted = demido_provision::system::relaunch_elevated(&args.to_command_line()).map_err(|e| e.to_string())?;
    if accepted {
        app.exit(0);
    }
    Ok(accepted)
}

#[tauri::command]
fn cancel_install(state: State<'_, Arc<SetupState>>) {
    if let Some(c) = state.cancel.lock().take() {
        c.cancel();
    }
}

/// Starts the installed app (as the signed-in user) and closes the installer. `skip_update`
/// keeps the app from installing a staged update at this launch: after an update that did not
/// finish, opening the app must not start the same update again.
#[tauri::command]
fn launch_app(
    app: AppHandle,
    state: State<'_, Arc<SetupState>>,
    install_dir: String,
    skip_update: Option<bool>,
) -> Result<(), String> {
    let exe = install_path(&install_dir).join(demido_core::platform::exe(demido_core::brand::STUDIO_BIN));
    if !exe.is_file() {
        return Err(format!("{} was not found.", exe.display()));
    }
    // The app does not open while setup holds its lock (released when a run ends; this covers a
    // launch before any run).
    drop(state.lock.lock().take());
    demido_provision::system::launch_app(&exe, app_args(skip_update.unwrap_or(false))).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// The arguments setup starts the app with.
fn app_args(skip_update: bool) -> &'static [&'static str] {
    if skip_update { &[setup_args::SKIP_UPDATE] } else { &[] }
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

    let update_args = if uninstalling { None } else { UpdateArgs::parse(&args) };
    let wait_pid = update_args.as_ref().and_then(|u| u.wait_pid);
    let mut lock = None;
    let update = update_args.map(|args| {
        let dir = update_dir(&args);
        if let Some(outcome) = elevate_for_update(&dir, &args) {
            finish_unelevated(outcome, &dir, &args);
            std::process::exit(0);
        }
        // An update is already running (the app was opened again while it installed, say): that
        // one finishes and starts the app, so this one has nothing to do.
        match SetupLock::acquire() {
            Some(held) => lock = Some(held),
            None => {
                tracing::info!("another setup is already updating Demido Studio");
                std::process::exit(0);
            }
        }
        UpdateRequest {
            dir,
            relaunch: args.relaunch,
        }
    });

    let resume = arg_value(&args, "--resume").and_then(|file| {
        let text = std::fs::read_to_string(Path::new(&file)).ok()?;
        let _ = std::fs::remove_file(&file);
        serde_json::from_str(&text).ok()
    });

    let mode = match (uninstalling, &update) {
        (true, _) => Mode::Uninstall,
        (false, Some(_)) => Mode::Update,
        (false, None) => Mode::Install,
    };
    let state = Arc::new(SetupState {
        mode,
        hardware: demido_hardware::detect(),
        resume,
        cancel: Mutex::new(None),
        uninstall_dir,
        update,
        wait_pid: Mutex::new(wait_pid),
        lock: Mutex::new(lock),
        run: Mutex::new(RunGate::default()),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .on_window_event(|window, event| {
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let state = window.state::<Arc<SetupState>>();
            if !state.run.lock().close_requested() {
                return;
            }
            // Ending the process now could stop the app step halfway through its swap. The
            // window goes; the process ends with the run (`spawn_run`).
            api.prevent_close();
            if close_stops_run(state.automatic())
                && let Some(cancel) = state.cancel.lock().as_ref()
            {
                cancel.cancel();
            }
            let _ = window.hide();
        })
        .setup(move |app| {
            // The window starts hidden; `window_ready` shows it once the page has drawn.
            if mode == Mode::Update
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.set_title("Updating Demido Studio");
                let _ = window.set_size(tauri::LogicalSize::new(520.0, 360.0));
                let _ = window.center();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_context,
            check_dir,
            check_space,
            relaunch_elevated,
            start_install,
            start_update,
            cancel_install,
            close_app,
            elevate_update,
            launch_app,
            uninstall,
            window_ready,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Demido Studio Setup");
}

/// The installation `--update` is for: the one `--dir` names, else the one setup finds, else the
/// default per-user folder (where the window then says there is no installation).
fn update_dir(args: &UpdateArgs) -> PathBuf {
    args.dir
        .clone()
        .or_else(|| find_existing().map(|e| PathBuf::from(e.dir)))
        .unwrap_or_else(|| demido_core::paths::default_install_dir(InstallScope::User))
}

/// What became of asking for administrator rights to update.
#[derive(Debug, PartialEq, Eq)]
enum Elevation {
    /// The elevated copy runs the update.
    Accepted,
    /// The person declined, or Windows could not start the copy.
    Refused,
}

/// A machine-wide installation can only be updated with administrator rights: starts setup again
/// with them, passing the update on. `None` when this process can update `dir` itself.
fn elevate_for_update(dir: &Path, args: &UpdateArgs) -> Option<Elevation> {
    let scope = demido_provision::folder::installation(dir).map(|m| m.scope);
    if scope != Some(InstallScope::Machine) || demido_provision::system::is_elevated() {
        return None;
    }
    let again = elevated_update_args(dir, args);
    Some(
        match demido_provision::system::relaunch_elevated(&again.to_command_line()) {
            Ok(true) => Elevation::Accepted,
            Ok(false) => Elevation::Refused,
            Err(err) => {
                tracing::error!("could not start setup as administrator: {err:#}");
                Elevation::Refused
            }
        },
    )
}

/// The command line the elevated copy gets: the same update, for the folder found here.
fn elevated_update_args(dir: &Path, args: &UpdateArgs) -> UpdateArgs {
    UpdateArgs {
        dir: Some(dir.to_path_buf()),
        wait_pid: args.wait_pid,
        relaunch: args.relaunch,
    }
}

/// After handing the update to an elevated copy, or failing to: when the update was to start the
/// app again and nothing will, starts the version that is installed, so the person is not left
/// without the app they were using.
fn finish_unelevated(outcome: Elevation, dir: &Path, args: &UpdateArgs) {
    if outcome == Elevation::Refused && args.relaunch {
        // The app that asked for the update is still closing; a second copy started now would
        // hand itself over to it and exit with it.
        if let Some(pid) = args.wait_pid {
            demido_provision::system::wait_for_exit(pid, APP_EXIT_WAIT);
        }
        let exe = dir.join(demido_core::platform::exe(demido_core::brand::STUDIO_BIN));
        if let Err(err) = demido_provision::system::launch_app(&exe, &[]) {
            tracing::error!("could not start {}: {err:#}", exe.display());
        }
    }
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
    fn the_installed_version_is_compared_as_a_semantic_version() {
        assert_eq!(relation("0.3.0", "0.4.0"), Relation::Older);
        assert_eq!(relation("0.4.0", "0.4.0"), Relation::Same);
        assert_eq!(relation("0.10.0", "0.4.0"), Relation::Newer);
        // A pre-release comes before its release, and after the release before it.
        assert_eq!(relation("0.4.0-beta.2", "0.4.0"), Relation::Older);
        assert_eq!(relation("0.4.0", "0.4.0-beta.2"), Relation::Newer);
        assert_eq!(relation("0.4.0-beta.2", "0.4.0-beta.10"), Relation::Older);
        assert_eq!(relation("v0.4.0", "0.4.0"), Relation::Same);
        assert_eq!(relation("0.4.0+local", "0.4.0"), Relation::Same);
        // What does not parse is updated.
        assert_eq!(relation("unknown", "0.4.0"), Relation::Older);
        assert_eq!(relation("", "0.4.0"), Relation::Older);
    }

    #[test]
    fn relations_serialize_the_way_the_wizard_reads_them() {
        let names: Vec<serde_json::Value> = [Relation::Older, Relation::Same, Relation::Newer]
            .iter()
            .map(|r| serde_json::to_value(r).unwrap())
            .collect();
        assert_eq!(names, ["older", "same", "newer"]);
        assert_eq!(serde_json::to_value(Mode::Update).unwrap(), "update");
        let launch = serde_json::to_value(UpdateLaunch {
            dir: "D:/Demido Studio".into(),
            relaunch: true,
        })
        .unwrap();
        assert_eq!(
            launch,
            serde_json::json!({ "dir": "D:/Demido Studio", "relaunch": true })
        );
    }

    #[test]
    fn an_update_asked_for_a_folder_updates_that_folder() {
        let args = UpdateArgs {
            dir: Some(PathBuf::from(r"D:\Apps\Demido Studio")),
            wait_pid: Some(7),
            relaunch: true,
        };
        assert_eq!(update_dir(&args), Path::new(r"D:\Apps\Demido Studio"));
    }

    #[test]
    fn the_elevated_copy_gets_the_same_update_for_the_folder_found() {
        let args = UpdateArgs {
            dir: None,
            wait_pid: Some(4242),
            relaunch: true,
        };
        let dir = Path::new(r"C:\Program Files\Demido Studio");
        let again = elevated_update_args(dir, &args);
        assert_eq!(
            again.to_command_line(),
            r#"--update --dir "C:\Program Files\Demido Studio" --wait-pid 4242 --relaunch"#
        );
        let mut argv = vec!["setup.exe".to_string()];
        argv.extend(again.to_args());
        assert_eq!(UpdateArgs::parse(&argv), Some(again));
    }

    /// No installation there, so nothing to elevate for.
    #[test]
    fn only_an_installation_for_everyone_asks_for_administrator_rights() {
        let root = tempfile::tempdir().unwrap();
        let args = UpdateArgs::default();
        assert_eq!(elevate_for_update(root.path(), &args), None);
        let mut manifest = InstallManifest::new(InstallScope::User);
        manifest.created = Some(vec![InstallManifest::FILE_NAME.into()]);
        manifest.save(root.path()).unwrap();
        assert_eq!(elevate_for_update(root.path(), &args), None);
    }

    /// A process that has already exited.
    fn exited_pid() -> u32 {
        let mut done = if cfg!(windows) {
            std::process::Command::new("cmd.exe").args(["/C", "exit 0"]).spawn()
        } else {
            std::process::Command::new("true").spawn()
        }
        .unwrap();
        let pid = done.id();
        done.wait().unwrap();
        pid
    }

    #[test]
    fn an_update_waits_for_nothing_when_nothing_runs() {
        let root = tempfile::tempdir().unwrap();
        let pid = exited_pid();
        let lines = Mutex::new(Vec::new());
        let log = |event: ProvisionEvent| {
            if let ProvisionEvent::Log { line, .. } = event {
                lines.lock().push(line);
            }
        };
        assert_eq!(AppExit::new(root.path(), Some(pid)).wait(&log), Ok(()));
        assert_eq!(AppExit::new(root.path(), None).wait(&log), Ok(()));
        assert_eq!(*lines.lock(), Vec::<String>::new());
    }

    /// This test process runs on, so waiting for it gives up after the timeout, and says so.
    #[test]
    fn an_update_says_what_it_waits_for() {
        let root = tempfile::tempdir().unwrap();
        let lines = Mutex::new(Vec::new());
        let log = |event: ProvisionEvent| {
            if let ProvisionEvent::Log { id, line } = event {
                assert_eq!(id, StepId::App);
                lines.lock().push(line);
            }
        };
        let wait = AppExit {
            app_timeout: Duration::from_millis(200),
            ..AppExit::new(root.path(), Some(std::process::id()))
        };
        assert_eq!(wait.wait(&log), Ok(()));
        assert_eq!(
            *lines.lock(),
            ["Waiting for Demido Studio to close", "Demido Studio has not closed yet"]
        );
    }

    /// Something left running from an install folder: a copy of `cmd.exe` there, waiting for
    /// input that never comes. Ended when dropped, if nothing ended it before.
    #[cfg(windows)]
    struct Leftover(std::process::Child);

    #[cfg(windows)]
    impl Leftover {
        fn start(dir: &Path) -> Self {
            use std::os::windows::process::CommandExt;
            use std::process::Stdio;

            let system = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
            let exe = dir.join("helper.exe");
            std::fs::copy(system.join("System32").join("cmd.exe"), &exe).unwrap();
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let child = std::process::Command::new(&exe)
                .args(["/D", "/K"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .unwrap();
            let leftover = Self(child);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while demido_provision::system::running_app_pids(dir).is_empty() {
                assert!(std::time::Instant::now() < deadline, "the leftover never showed up");
                std::thread::sleep(Duration::from_millis(50));
            }
            leftover
        }

        fn running(&mut self) -> bool {
            self.0.try_wait().unwrap().is_none()
        }
    }

    #[cfg(windows)]
    impl Drop for Leftover {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// An update waiting this briefly, for the app `pid` (which has exited, if given).
    #[cfg(windows)]
    fn brief_wait(dir: &Path, pid: Option<u32>) -> AppExit {
        AppExit {
            app_timeout: Duration::from_millis(200),
            helpers_timeout: Duration::from_millis(300),
            close_grace: Duration::from_millis(300),
            ..AppExit::new(dir, pid)
        }
    }

    /// The app agreed to exit when it started the update, so what outlasts it is closed.
    #[cfg(windows)]
    #[test]
    fn what_outlasts_the_app_that_started_the_update_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let mut leftover = Leftover::start(root.path());
        let lines = Mutex::new(Vec::new());
        let log = |event: ProvisionEvent| {
            if let ProvisionEvent::Log { line, .. } = event {
                lines.lock().push(line);
            }
        };
        assert_eq!(brief_wait(root.path(), Some(exited_pid())).wait(&log), Ok(()));
        assert!(!leftover.running());
        assert_eq!(
            *lines.lock(),
            [
                "Waiting for Demido Studio's helper programs to close",
                "Closing what is still running from the install folder"
            ]
        );
    }

    /// A retry has no word from the app: what runs from the folder may be in use, so the window
    /// offers to close it instead.
    #[cfg(windows)]
    #[test]
    fn a_retry_leaves_running_what_runs_from_the_folder() {
        let root = tempfile::tempdir().unwrap();
        let mut leftover = Leftover::start(root.path());
        let err = brief_wait(root.path(), None).wait(&|_| {}).unwrap_err();
        assert_eq!(
            err,
            format!(
                "Demido Studio is still running from {}. Close it and try again.",
                root.path().display()
            )
        );
        assert!(leftover.running());
    }

    #[test]
    fn only_an_elevated_update_the_app_started_requires_the_signature() {
        let from_app = UpdateRequest {
            dir: PathBuf::from(r"C:\Program Files\Demido Studio"),
            relaunch: true,
        };
        let by_hand = UpdateRequest {
            relaunch: false,
            ..from_app.clone()
        };
        assert!(signature_required(true, Some(&from_app)));
        assert!(
            !signature_required(false, Some(&from_app)),
            "a per-user update runs as the user"
        );
        assert!(
            !signature_required(true, Some(&by_hand)),
            "restarted elevated from the wizard"
        );
        assert!(
            !signature_required(true, None),
            "a downloaded setup run as administrator"
        );
        assert!(!signature_required(false, None));
    }

    #[test]
    fn opening_the_app_after_a_failed_update_skips_the_update() {
        assert_eq!(app_args(true), [setup_args::SKIP_UPDATE]);
        assert!(app_args(false).is_empty());
    }

    #[test]
    fn closing_the_window_during_a_run_waits_for_the_run() {
        let mut gate = RunGate::default();
        // Before any run, the window just closes.
        assert!(!gate.close_requested());
        gate.start();
        assert!(gate.close_requested());
        assert!(gate.close_requested(), "pressed twice");
        assert!(gate.finish(), "the process ends with the run");
        // After the run, the window closes again.
        assert!(!gate.close_requested());
        // A run nobody closed the window during leaves it open.
        gate.start();
        assert!(!gate.finish());
    }

    #[test]
    fn a_closed_automatic_update_finishes_and_starts_the_app() {
        assert!(close_stops_run(false), "a run the person started stops, as with Cancel");
        assert!(!close_stops_run(true));
        assert_eq!(after_close(true, true), AfterClose::StartApp);
        assert_eq!(after_close(true, false), AfterClose::ShowFailure);
        assert_eq!(after_close(false, true), AfterClose::Exit);
        assert_eq!(after_close(false, false), AfterClose::Exit);
    }

    #[test]
    fn a_quiet_elevated_uninstall_passes_on_its_result() {
        assert_eq!(elevated_exit_code(Ok(Some(0))), 0);
        assert_eq!(elevated_exit_code(Ok(Some(1))), 1);
        assert_eq!(elevated_exit_code(Ok(None)), 1, "declined prompt");
        assert_eq!(elevated_exit_code(Err(anyhow::anyhow!("no shell"))), 1);
    }
}
