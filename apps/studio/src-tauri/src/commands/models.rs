use std::path::PathBuf;

use serde::Serialize;
use tauri::Emitter;

use super::St;
use crate::attachments::meaning::SearchStatus;
use crate::bail_msg;
use crate::error::{AppError, CmdResult};
use crate::models::downloads::{DownloadJob, DownloadSpec};
use crate::models::hf::{self, HfRepo, HfRepoFiles};
use crate::models::{CHANGED_EVENT, ModelEntry, ModelSettings, ModelSource};
use crate::runtime::RuntimeStatus;
use crate::secrets::HF_TOKEN;

fn changed(state: &St<'_>) -> Vec<ModelEntry> {
    let list = state.models.list();
    let _ = state.app.emit(CHANGED_EVENT, &list);
    list
}

#[tauri::command]
pub fn list_models(state: St<'_>) -> Vec<ModelEntry> {
    state.models.list()
}

#[tauri::command]
pub async fn rescan_models(state: St<'_>) -> CmdResult<Vec<ModelEntry>> {
    let models = state.models.clone();
    tokio::task::spawn_blocking(move || models.rescan())
        .await
        .map_err(|e| AppError::msg(e.to_string()))?;
    Ok(changed(&state))
}

#[tauri::command]
pub fn update_model(state: St<'_>, id: String, settings: ModelSettings) -> CmdResult<ModelEntry> {
    state.models.set_settings(&id, settings)?;
    changed(&state);
    state
        .models
        .get(&id)
        .ok_or_else(|| AppError::msg("That model is gone."))
}

/// Turns every listed model on or off, for the Models tab's "Activate all" and "Deactivate all".
#[tauri::command]
pub fn set_models_enabled(state: St<'_>, ids: Vec<String>, enabled: bool) -> CmdResult<Vec<ModelEntry>> {
    state.models.set_enabled(&ids, enabled)?;
    Ok(changed(&state))
}

#[tauri::command]
pub fn import_model_avatar(state: St<'_>, id: String, source: String) -> CmdResult<ModelEntry> {
    let name = state.models.import_avatar(&PathBuf::from(source))?;
    let mut settings = state.models.settings_of(&id);
    settings.avatar = Some(name);
    state.models.set_settings(&id, settings)?;
    changed(&state);
    state
        .models
        .get(&id)
        .ok_or_else(|| AppError::msg("That model is gone."))
}

#[tauri::command]
pub fn set_default_model(state: St<'_>, id: String) -> CmdResult<Vec<ModelEntry>> {
    state.settings.update(|s| s.default_model = Some(id))?;
    Ok(changed(&state))
}

#[tauri::command]
pub async fn delete_model(state: St<'_>, id: String) -> CmdResult<Vec<ModelEntry>> {
    if state.runtime.loaded_model().await.as_deref() == Some(id.as_str()) {
        state.runtime.stop().await;
    }
    let paused = state.models.pause_checks().await;
    state.models.delete_local(&id)?;
    drop(paused);
    Ok(changed(&state))
}

#[tauri::command]
pub fn runtime_status(state: St<'_>) -> RuntimeStatus {
    state.runtime.status()
}

#[tauri::command]
pub fn runtime_logs(state: St<'_>) -> Vec<String> {
    state.runtime.recent_logs()
}

/// Loads a local model ahead of the first message.
#[tauri::command]
pub async fn load_model(state: St<'_>, id: String) -> CmdResult<RuntimeStatus> {
    let entry = state
        .models
        .get(&id)
        .ok_or_else(|| AppError::msg("That model is gone."))?;
    if entry.source != ModelSource::Local {
        return Ok(state.runtime.status());
    }
    let spec = state.models.launch_spec(&id)?;
    state.runtime.ensure(&spec).await.map_err(AppError::msg)?;
    Ok(state.runtime.status())
}

#[tauri::command]
pub async fn unload_model(state: St<'_>) -> CmdResult<RuntimeStatus> {
    state.runtime.stop().await;
    Ok(state.runtime.status())
}

#[tauri::command]
pub async fn hf_search(state: St<'_>, query: String) -> CmdResult<Vec<HfRepo>> {
    let token = state.secrets.get(HF_TOKEN);
    hf::search(&state.http, query.trim(), token.as_deref())
        .await
        .map_err(|e| AppError::msg(format!("Hugging Face search failed: {e}")))
}

#[tauri::command]
pub async fn hf_repo_files(state: St<'_>, repo: String) -> CmdResult<HfRepoFiles> {
    let token = state.secrets.get(HF_TOKEN);
    let vram = state::budget(&state);
    hf::repo_files(&state.http, repo.trim(), token.as_deref(), Some(vram))
        .await
        .map_err(|e| AppError::msg(format!("Could not list the files of {repo}: {e}")))
}

mod state {
    use super::St;

    pub fn budget(state: &St<'_>) -> f64 {
        crate::state::AppState::memory_budget_gb(&state.hardware)
    }
}

/// A curated starter model for this machine, as the Download view lists it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub family: String,
    pub family_label: String,
    pub description: String,
    pub name: String,
    pub repo: String,
    pub file: String,
    pub quant: String,
    pub size: u64,
    pub sha256: String,
    pub installed: bool,
}

#[tauri::command]
pub fn recommended_models(state: St<'_>) -> Vec<Recommendation> {
    let catalog = demido_catalog::catalog();
    let choices = demido_catalog::backend_choices(&state.hardware, catalog);
    let backend = state
        .paths
        .manifest
        .as_ref()
        .and_then(|m| m.runtime.as_ref())
        .map(|r| r.backend)
        .unwrap_or_else(|| demido_catalog::default_backend(&choices));
    let Some(choice) = choices.iter().find(|c| c.backend == backend) else {
        return Vec::new();
    };
    let rec = demido_catalog::recommend_models(choice, catalog);
    let installed: Vec<String> = state.models.list().into_iter().filter_map(|m| m.path).collect();
    rec.picks
        .into_iter()
        .filter_map(|(family, pick)| {
            let f = catalog.models.families.iter().find(|f| f.id == family)?;
            Some(Recommendation {
                installed: installed.iter().any(|p| p.ends_with(&pick.file)),
                family: family.clone(),
                family_label: f.label.clone(),
                description: f.description.clone(),
                name: pick.name,
                repo: pick.repo,
                file: pick.file,
                quant: pick.quant,
                size: pick.size,
                sha256: pick.sha256,
            })
        })
        .collect()
}

#[tauri::command]
pub fn download_model(state: St<'_>, spec: DownloadSpec) -> CmdResult<DownloadJob> {
    state.downloads.enqueue(spec).map_err(|e| AppError::msg(e.to_string()))
}

/// How far the search model has indexed attached files, and which model it is.
#[tauri::command]
pub fn search_status(state: St<'_>) -> SearchStatus {
    state.embedder.status()
}

/// Downloads the search model this computer should use (installations from before it came
/// with setup have none).
#[tauri::command]
pub fn download_search_model(state: St<'_>) -> CmdResult<DownloadJob> {
    let m = state.embedder.preferred();
    state
        .downloads
        .enqueue(DownloadSpec {
            repo: m.repo.clone(),
            name: m.name.clone(),
            quant: Some(m.quant.clone()),
            paths: vec![m.file.clone()],
            sizes: vec![m.size],
            sha256: vec![Some(m.sha256.clone())],
        })
        .map_err(|e| AppError::msg(e.to_string()))
}

#[tauri::command]
pub fn list_downloads(state: St<'_>) -> Vec<DownloadJob> {
    state.downloads.list()
}

#[tauri::command]
pub fn pause_download(state: St<'_>, id: String) {
    state.downloads.pause(&id);
}

#[tauri::command]
pub fn resume_download(state: St<'_>, id: String) {
    state.downloads.resume(&id);
}

#[tauri::command]
pub fn cancel_download(state: St<'_>, id: String) {
    state.downloads.cancel(&id);
}

#[tauri::command]
pub fn clear_downloads(state: St<'_>) -> Vec<DownloadJob> {
    state.downloads.clear_finished();
    state.downloads.list()
}

#[tauri::command]
pub fn has_hf_token(state: St<'_>) -> bool {
    state.secrets.get(HF_TOKEN).is_some()
}

#[tauri::command]
pub fn set_hf_token(state: St<'_>, token: Option<String>) -> bool {
    match token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        Some(t) => state.secrets.set(HF_TOKEN, &t),
        None => {
            state.secrets.delete(HF_TOKEN);
            true
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFolder {
    pub path: String,
    pub managed: bool,
    pub exists: bool,
}

#[tauri::command]
pub fn model_folders(state: St<'_>) -> Vec<ModelFolder> {
    let mut out: Vec<ModelFolder> = state
        .paths
        .builtin_model_dirs()
        .into_iter()
        .map(|p| ModelFolder {
            exists: p.is_dir(),
            path: p.to_string_lossy().into_owned(),
            managed: true,
        })
        .collect();
    out.extend(state.settings.get().extra_model_dirs.into_iter().map(|p| ModelFolder {
        exists: p.is_dir(),
        path: p.to_string_lossy().into_owned(),
        managed: false,
    }));
    out
}

/// Folders other apps keep GGUF models in, when they exist and are not added yet.
#[tauri::command]
pub fn suggested_model_folders(state: St<'_>) -> Vec<String> {
    let Some(home) = dirs_home() else {
        return Vec::new();
    };
    let current = state.settings.get().extra_model_dirs;
    [
        home.join(".lmstudio").join("models"),
        home.join(".cache").join("lm-studio").join("models"),
    ]
    .into_iter()
    .filter(|p| p.is_dir() && !current.contains(p))
    .map(|p| p.to_string_lossy().into_owned())
    .collect()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

#[tauri::command]
pub async fn add_model_folder(state: St<'_>, path: String) -> CmdResult<Vec<ModelEntry>> {
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        bail_msg!("That folder does not exist.");
    }
    state.settings.update(|s| {
        if !s.extra_model_dirs.contains(&p) {
            s.extra_model_dirs.push(p);
        }
    })?;
    rescan_models(state).await
}

#[tauri::command]
pub async fn remove_model_folder(state: St<'_>, path: String) -> CmdResult<Vec<ModelEntry>> {
    let p = PathBuf::from(path);
    state.settings.update(|s| s.extra_model_dirs.retain(|d| d != &p))?;
    rescan_models(state).await
}
