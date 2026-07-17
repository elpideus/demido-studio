//! Tauri commands for the local-models feature: list HF quants, download a model,
//! list/delete downloaded models, and manage the runtime binary.

use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::db::local_models::{self, LocalModel};
use crate::db::model_overrides::{self, ModelOverride};
use crate::db::{settings, LOCAL_PROVIDER_ID};
use crate::local::{engine, graphify, hf, python, searxng};

const MODELS_DIR_KEY: &str = "local_models_dir";
const MODELS_DIRS_KEY: &str = "local_models_dirs";

/// Every folder scanned for models: the user-chosen list, or the app-data default when the
/// user has not chosen any. `local_models_dir` is the pre-multi-folder key, read once so an
/// existing single choice survives the upgrade.
fn models_bases(app: &AppHandle, state: &AppState) -> Vec<std::path::PathBuf> {
    let (list, legacy) = {
        let conn = state.conn.lock().unwrap();
        (
            settings::get(&conn, MODELS_DIRS_KEY).ok().flatten(),
            settings::get(&conn, MODELS_DIR_KEY).ok().flatten(),
        )
    };
    let dirs: Vec<String> = match list {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        None => legacy.into_iter().collect(),
    };
    let dirs: Vec<std::path::PathBuf> = dirs
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(std::path::PathBuf::from)
        .collect();
    if dirs.is_empty() {
        vec![engine::models_dir(app)]
    } else {
        dirs
    }
}

/// Where a newly downloaded model is written: the first configured folder.
fn download_base(app: &AppHandle, state: &AppState) -> std::path::PathBuf {
    models_bases(app, state).remove(0)
}

/// Scan every active folder, sync the DB (add found, prune vanished), auto-enable found
/// models under the local provider, and return the full list.
fn do_scan(app: &AppHandle, state: &AppState) -> Result<Vec<LocalModel>, String> {
    let found: Vec<LocalModel> = models_bases(app, state)
        .iter()
        .flat_map(|base| hf::scan_models_dir(base))
        .collect();
    let conn = state.conn.lock().unwrap();
    // Prune rows whose file no longer exists on disk.
    for existing in local_models::list(&conn).map_err(|e| e.to_string())? {
        if !std::path::Path::new(&existing.file_path).exists() {
            local_models::delete(&conn, &existing.id).map_err(|e| e.to_string())?;
        }
    }
    for m in &found {
        local_models::upsert(&conn, m).map_err(|e| e.to_string())?;
        model_overrides::upsert(
            &conn,
            // Caps columns are ignored by `upsert` — rescanning the models dir must not
            // clear an override the user set.
            &ModelOverride {
                provider_id: LOCAL_PROVIDER_ID.to_string(),
                model_id: m.id.clone(),
                custom_name: None,
                enabled: true,
                caps_vision: None,
                caps_tools: None,
                caps_reasoning: None,
            },
        )
        .map_err(|e| e.to_string())?;
    }
    local_models::list(&conn).map_err(|e| e.to_string())
}

/// Startup detection hook (called from lib.rs setup).
pub fn scan_on_startup(app: &AppHandle, state: &AppState) {
    let _ = do_scan(app, state);
}

/// Every models folder currently scanned (absolute paths). The first one receives downloads.
#[tauri::command]
pub fn get_models_dirs(app: AppHandle, state: State<'_, AppState>) -> Vec<String> {
    models_bases(&app, state.inner())
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// Replace the models folder list and rescan. An empty list falls back to the app-data
/// default. Returns the detected models.
#[tauri::command]
pub fn set_models_dirs(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<LocalModel>, String> {
    let mut clean: Vec<String> = Vec::new();
    for p in paths {
        let p = p.trim().to_string();
        if !p.is_empty() && !clean.contains(&p) {
            clean.push(p);
        }
    }
    let raw = serde_json::to_string(&clean).map_err(|e| e.to_string())?;
    {
        let conn = state.conn.lock().unwrap();
        settings::set(&conn, MODELS_DIRS_KEY, &raw).map_err(|e| e.to_string())?;
    }
    do_scan(&app, state.inner())
}

/// Rescan the current folder for models on disk.
#[tauri::command]
pub fn scan_local_models(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<LocalModel>, String> {
    do_scan(&app, state.inner())
}

/// List the GGUF quants available in a Hugging Face repo (given its URL or owner/name).
#[tauri::command]
pub async fn hf_list_quants(
    state: State<'_, AppState>,
    url: String,
) -> Result<Vec<hf::QuantOption>, String> {
    let repo = hf::parse_repo(&url)?;
    hf::list_quants(&state.http_client, &repo).await
}

/// Download a chosen quant and register it as a local model. Re-resolves the file list
/// from HF rather than trusting the caller.
#[tauri::command]
pub async fn download_local_model(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    quant: String,
) -> Result<LocalModel, String> {
    let repo = hf::parse_repo(&url)?;
    let (quants, mmproj) = hf::quants_and_mmproj(&state.http_client, &repo).await?;
    let opt = quants
        .iter()
        .find(|q| q.quant.eq_ignore_ascii_case(&quant))
        .ok_or_else(|| format!("Quant '{}' not found in {}", quant, repo))?;

    let id = format!("{}::{}", repo, opt.quant);
    // LM-Studio-style layout: <base>/<owner>/<name>/*.gguf
    let (owner, name) = repo.split_once('/').unwrap_or((repo.as_str(), ""));
    let dest = download_base(&app, state.inner()).join(owner).join(name);
    let path = hf::download_quant(
        &app,
        &state.http_client,
        &repo,
        &opt.files,
        opt.size,
        &dest,
        &id,
    )
    .await?;

    // Vision model? Also pull the mmproj projector so --mmproj can enable vision.
    let mmproj_path = match mmproj {
        Some((mpath, msize)) => {
            let p = hf::download_quant(
                &app,
                &state.http_client,
                &repo,
                std::slice::from_ref(&mpath),
                msize,
                &dest,
                &id,
            )
            .await?;
            Some(p.to_string_lossy().to_string())
        }
        None => None,
    };

    let model = LocalModel {
        id,
        repo,
        quant: opt.quant.clone(),
        file_path: path.to_string_lossy().to_string(),
        size: opt.size,
        mmproj_path,
        caps_vision: None,
        caps_tools: None,
        caps_reasoning: None,
    };
    {
        let conn = state.conn.lock().unwrap();
        local_models::upsert(&conn, &model).map_err(|e| e.to_string())?;
    }
    Ok(model)
}

#[tauri::command]
pub fn list_local_models(state: State<'_, AppState>) -> Result<Vec<LocalModel>, String> {
    let conn = state.conn.lock().unwrap();
    local_models::list(&conn).map_err(|e| e.to_string())
}

/// Delete a downloaded model: stop the engine if it's serving it, remove the row, and
/// delete its gguf part files from disk.
#[tauri::command]
pub fn delete_local_model(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    if state.local_engine.current_model().as_deref() == Some(id.as_str()) {
        state.local_engine.stop();
    }
    let model = {
        let conn = state.conn.lock().unwrap();
        let m = local_models::find_by_id(&conn, &id).map_err(|e| e.to_string())?;
        local_models::delete(&conn, &id).map_err(|e| e.to_string())?;
        m
    };
    // Remove the gguf file(s). Delete every file in the model's dir sharing its quant tag
    // (covers multi-part downloads).
    if let Some(m) = model {
        let path = std::path::PathBuf::from(&m.file_path);
        if let Some(dir) = path.parent() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_uppercase();
                    if name.contains(&m.quant.to_uppercase()) && name.ends_with(".GGUF") {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
        // The mmproj name doesn't contain the quant tag, so remove it explicitly.
        if let Some(mp) = &m.mmproj_path {
            let _ = std::fs::remove_file(mp);
        }
    }
    Ok(())
}

/// Whether the llama-server runtime binary is installed.
#[tauri::command]
pub fn local_runtime_ready(app: AppHandle) -> bool {
    engine::runtime_ready(&app)
}

/// Download+install the recommended runtime binary now (progress via `local_runtime_progress`).
#[tauri::command]
pub async fn install_local_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    engine::ensure_runtime(&app, &state.http_client).await.map(|_| ())
}

/// Detected hardware + the four runtime variants (recommended/installed/available + notes).
#[tauri::command]
pub async fn list_runtime_variants(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(engine::Hardware, Vec<engine::VariantInfo>), String> {
    engine::list_variants(&app, &state.http_client).await
}

/// Install a specific runtime variant (nvidia=cuda, amd=hip, apple=metal, cpu).
#[tauri::command]
pub async fn install_runtime_variant(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    engine::install_variant(&app, &state.http_client, &id).await
}

/// Trending GGUF models for the browser's default list.
#[tauri::command]
pub async fn hf_trending_models(
    state: State<'_, AppState>,
) -> Result<Vec<hf::HfModel>, String> {
    hf::trending_models(&state.http_client).await
}

/// Search GGUF models by name (call when query is >= 3 chars).
#[tauri::command]
pub async fn hf_search_models(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<hf::HfModel>, String> {
    hf::search_models(&state.http_client, &query).await
}

/// A repo's model card (README markdown).
#[tauri::command]
pub async fn hf_model_card(
    state: State<'_, AppState>,
    repo: String,
) -> Result<String, String> {
    hf::model_card(&state.http_client, &repo).await
}

/// Model id currently being served by the engine, if any.
#[tauri::command]
pub fn local_running_model(state: State<'_, AppState>) -> Option<String> {
    state.local_engine.current_model()
}

/// Stop the running engine (frees RAM/VRAM).
#[tauri::command]
pub fn stop_local_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.local_engine.stop();
    Ok(())
}

/// Load a local model ahead of the first message, so the wait happens at model-switch time
/// (where the UI shows a spinner) instead of silently inside `send_message`. Also lets the
/// /props caps probe run before the user tries to attach an image.
#[tauri::command]
pub async fn preload_local_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    engine::ensure_model(&app, state.inner(), &model_id).await?;
    Ok(())
}

const SETUP_DONE_KEY: &str = "setup_complete";

/// Whether the first-run setup wizard still needs to be shown. Nothing is downloaded before
/// the user has been through it — that's what keeps the installer itself small.
#[tauri::command]
pub fn setup_needed(state: State<'_, AppState>) -> bool {
    let conn = state.conn.lock().unwrap();
    settings::get(&conn, SETUP_DONE_KEY).ok().flatten().unwrap_or_default() != "true"
}

/// Mark the wizard as done, so it never shows again (installs it kicked off may still be
/// running — the wizard drives those itself).
#[tauri::command]
pub fn complete_setup(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    settings::set(&conn, SETUP_DONE_KEY, "true").map_err(|e| e.to_string())
}

/// Version of the portable Python the wizard would install, read from the release asset
/// name — there is nothing on disk to ask yet.
#[tauri::command]
pub async fn python_available_version(state: State<'_, AppState>) -> Result<String, String> {
    python::available_version(&state.http_client).await
}

/// Whether the portable Python runtime is installed.
#[tauri::command]
pub fn python_ready(app: AppHandle) -> bool {
    python::python_ready(&app)
}

/// Everything the Engine > Python tab renders in one round-trip.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonStatus {
    pub ready: bool,
    pub version: Option<String>,
    pub searxng_installed: bool,
    pub searxng_running: bool,
}

#[tauri::command]
pub fn python_status(app: AppHandle, state: State<'_, AppState>) -> PythonStatus {
    PythonStatus {
        ready: python::python_ready(&app),
        version: python::python_version(&app),
        searxng_installed: searxng::source_ready(&app),
        searxng_running: state.searxng_engine.is_running(),
    }
}

/// Delete the portable Python runtime. Stops SearXNG first — its worker is a child of this
/// interpreter and its pip-installed deps live inside the tree being removed.
#[tauri::command]
pub fn uninstall_python(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.searxng_engine.stop();
    python::uninstall_python(&app)
}

/// Install (or repair) Python + SearXNG without starting the worker.
#[tauri::command]
pub async fn install_searxng(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    searxng::install(&app, &state.http_client).await
}

/// Stop SearXNG and delete its source + settings. The Python runtime is left installed.
#[tauri::command]
pub fn uninstall_searxng(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    searxng::uninstall(&app, &state.searxng_engine)
}

/// Download + extract the portable Python runtime now (progress via `python_install_progress`).
#[tauri::command]
pub async fn install_python(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    python::install_python(&app, &state.http_client).await
}

/// Status of the bundled Python + SearXNG stack, for the Web Browsing settings tab.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearxngStatus {
    pub python_ready: bool,
    pub source_ready: bool,
    pub running: bool,
}

#[tauri::command]
pub fn searxng_status(app: AppHandle, state: State<'_, AppState>) -> SearxngStatus {
    SearxngStatus {
        python_ready: python::python_ready(&app),
        source_ready: searxng::source_ready(&app),
        running: state.searxng_engine.is_running(),
    }
}

/// Install (if needed) and start the bundled SearXNG worker. Progress via
/// `python_install_progress` / `searxng_install_progress` events. Heavy the first time.
#[tauri::command]
pub async fn start_searxng(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    searxng::ensure_running(&app, &state.http_client, &state.searxng_engine).await
}

#[tauri::command]
pub fn stop_searxng(state: State<'_, AppState>) -> Result<(), String> {
    state.searxng_engine.stop();
    Ok(())
}

/// Everything the sidebar's Graphify affordance needs in one round-trip: whether the
/// graphify package is installed into the portable runtime, and whether a graph has been
/// built for `folder`. `folder` is the active conversation's working directory.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphifyStatus {
    pub python_ready: bool,
    pub installed: bool,
    pub graph_built: bool,
    /// Whether automatic graph building is enabled for this folder (default true).
    pub auto_build: bool,
}

#[tauri::command]
pub fn graphify_status(app: AppHandle, folder: String) -> GraphifyStatus {
    GraphifyStatus {
        python_ready: python::python_ready(&app),
        installed: graphify::installed(&app),
        graph_built: graphify::graph_built(&folder),
        auto_build: graphify::auto_build_enabled(&app, &folder),
    }
}

/// Set the per-folder "automatically build graph on new projects" preference.
#[tauri::command]
pub fn graphify_set_auto_build(app: AppHandle, folder: String, enabled: bool) -> Result<(), String> {
    graphify::set_auto_build(&app, &folder, enabled)
}

/// Install (or repair) Python + the graphify package. Progress via
/// `python_install_progress` / `graphify_install_progress`.
#[tauri::command]
pub async fn install_graphify(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    graphify::install(&app, &state.http_client).await
}

/// Remove the graphify install marker + cached renderer (Python runtime left in place).
#[tauri::command]
pub fn uninstall_graphify(app: AppHandle) -> Result<(), String> {
    graphify::uninstall(&app)
}

/// Build (or refresh, with `update`) the graph for `folder`. Installs graphify first if
/// needed. Streams `graphify_build_progress` events; returns when the build finishes.
#[tauri::command]
pub async fn build_graphify(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
    update: bool,
) -> Result<(), String> {
    graphify::build(&app, &state.http_client, folder, update).await
}

/// Run a read query (`query` / `path` / `explain`) against the built graph, returning its
/// stdout text.
#[tauri::command]
pub async fn query_graphify(
    app: AppHandle,
    folder: String,
    kind: String,
    args: Vec<String>,
) -> Result<String, String> {
    graphify::query(&app, folder, kind, args).await
}

/// Cached settled node positions for `folder`, or `null` if none captured yet. Read on graph
/// open so a reopen (or an open after an app restart) skips the physics stabilization.
#[tauri::command]
pub fn graphify_get_positions(app: AppHandle, folder: String) -> Option<serde_json::Value> {
    graphify::get_positions(&app, &folder)
}

/// Persist the settled node positions the graph iframe reported, keyed by `folder`.
#[tauri::command]
pub fn graphify_set_positions(
    app: AppHandle,
    folder: String,
    positions: serde_json::Value,
) -> Result<(), String> {
    graphify::set_positions(&app, &folder, positions)
}

/// The built `graph.html` for `folder`, with the CDN visualisation script inlined so it
/// renders offline. Returned as a string for use as an iframe `srcdoc`.
#[tauri::command]
pub async fn graphify_graph_html(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
) -> Result<String, String> {
    graphify::graph_html(&app, &state.http_client, folder).await
}
