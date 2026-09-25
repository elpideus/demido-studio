use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use super::St;
use crate::bail_msg;
use crate::error::CmdResult;
use crate::settings::{DISCLAIMER_VERSION, Settings};
use crate::tools::{self, ToolGroup};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInfo {
    pub name: String,
    pub version: Option<String>,
    pub installed: bool,
    pub detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub install_dir: Option<String>,
    pub disclaimer_accepted: bool,
    pub os: String,
    pub cpu: String,
    pub memory_gb: f64,
    pub gpu: Option<String>,
    pub vram_gb: Option<f64>,
    pub backend: Option<String>,
    pub components: Vec<ComponentInfo>,
    pub dev: bool,
}

#[tauri::command]
pub fn app_info(state: St<'_>) -> AppInfo {
    let manifest = state.paths.manifest.as_ref();
    let gpu = state.hardware.primary_gpu();
    let runtime = manifest.and_then(|m| m.runtime.as_ref());
    let tool = |name: &str, info: Option<&demido_core::manifest::ToolInfo>, present: bool| ComponentInfo {
        name: name.into(),
        version: info.map(|t| t.version.clone()),
        installed: present,
        detail: None,
    };
    let settings = state.settings.get();
    AppInfo {
        version: demido_core::brand::VERSION.to_string(),
        data_dir: state.paths.data_dir.to_string_lossy().into_owned(),
        install_dir: state
            .paths
            .install_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        disclaimer_accepted: settings.disclaimer_accepted.as_deref() == Some(DISCLAIMER_VERSION),
        os: state.hardware.os_version.clone(),
        cpu: state.hardware.cpu.name.clone(),
        memory_gb: (state.hardware.total_memory_gb() * 10.0).round() / 10.0,
        gpu: gpu.map(|g| g.name.clone()),
        vram_gb: gpu
            .filter(|g| !g.integrated)
            .map(|g| (g.vram_gb() * 10.0).round() / 10.0),
        backend: runtime.map(|r| r.backend.label().to_string()),
        components: vec![
            ComponentInfo {
                name: "AI runtime".into(),
                version: runtime.map(|r| format!("llama.cpp {} ({})", r.release, r.variant)),
                installed: state.paths.llama_server().is_some(),
                detail: runtime.map(|r| r.backend.label().to_string()),
            },
            tool(
                "Python",
                manifest.and_then(|m| m.python.as_ref()),
                state.paths.python().is_some(),
            ),
            tool(
                "Node.js",
                manifest.and_then(|m| m.node.as_ref()),
                state.paths.node().is_some(),
            ),
            tool(
                "uv",
                manifest.and_then(|m| m.uv.as_ref()),
                manifest.and_then(|m| m.uv.as_ref()).is_some(),
            ),
        ],
        dev: cfg!(debug_assertions),
    }
}

#[tauri::command]
pub fn accept_disclaimer(state: St<'_>) -> CmdResult<()> {
    state
        .settings
        .update(|s| s.disclaimer_accepted = Some(DISCLAIMER_VERSION.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: St<'_>) -> Settings {
    state.settings.get()
}

#[tauri::command]
pub fn update_settings(state: St<'_>, patch: serde_json::Value) -> CmdResult<Settings> {
    state.settings.patch(patch)
}

/// Shows the main window once the UI has painted, so the app never flashes an empty frame.
#[tauri::command]
pub fn window_ready(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Paths the UI may ask to open: Demido's own folders and the model folders.
fn allowed(state: &St<'_>, path: &Path) -> bool {
    let mut roots: Vec<PathBuf> = vec![state.paths.data_dir.clone()];
    roots.extend(state.paths.install_dir.clone());
    roots.extend(state.paths.builtin_model_dirs());
    roots.extend(state.settings.get().extra_model_dirs);
    let Ok(target) = dunce(path) else {
        return false;
    };
    roots
        .iter()
        .filter_map(|r| dunce(r).ok())
        .any(|root| target.starts_with(root))
}

fn dunce(p: &Path) -> std::io::Result<PathBuf> {
    let c = p.canonicalize()?;
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(stripped));
        }
    }
    Ok(c)
}

/// Opens a file with its default app, or a folder in the file manager.
#[tauri::command]
pub fn open_path(app: tauri::AppHandle, state: St<'_>, path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !allowed(&state, &p) {
        bail_msg!("That location is outside Demido Studio's folders.");
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| crate::error::AppError::msg(e.to_string()))
}

/// Shows a file selected in the file manager.
#[tauri::command]
pub fn reveal_path(app: tauri::AppHandle, state: St<'_>, path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !allowed(&state, &p) {
        bail_msg!("That location is outside Demido Studio's folders.");
    }
    app.opener()
        .reveal_item_in_dir(p)
        .map_err(|e| crate::error::AppError::msg(e.to_string()))
}

/// Reads a text file from a chat workspace, for previews (CSV tables, code).
#[tauri::command]
pub fn read_workspace_file(state: St<'_>, path: String, max_bytes: Option<usize>) -> CmdResult<String> {
    let p = PathBuf::from(&path);
    let Ok(target) = dunce(&p) else {
        bail_msg!("That file no longer exists.");
    };
    let Ok(root) = dunce(&state.paths.workspaces_dir) else {
        bail_msg!("The workspaces folder is missing.");
    };
    if !target.starts_with(&root) {
        bail_msg!("Only workspace files can be previewed.");
    }
    let bytes = std::fs::read(&target)?;
    let max = max_bytes.unwrap_or(200_000);
    Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(max)]).into_owned())
}

#[tauri::command]
pub fn list_tool_groups(state: St<'_>) -> Vec<ToolGroup> {
    tools::groups(&state)
}

#[tauri::command]
pub fn set_tool_group(state: St<'_>, id: String, enabled: bool) -> CmdResult<Vec<ToolGroup>> {
    state.settings.update(|s| {
        s.tool_groups.insert(id, enabled);
    })?;
    Ok(tools::groups(&state))
}

#[tauri::command]
pub fn revoke_tool_permission(state: St<'_>, name: String) -> CmdResult<Settings> {
    state.settings.update(|s| {
        s.always_allowed_tools.remove(&name);
    })
}
