//! Global system prompt, stored as `system_prompt.md` in app-data (sibling of `skills/`).
//!
//! The file is the only source of truth — there is no DB copy to disagree with it. `migrate_from_db`
//! moves a pre-file install's `settings.system_prompt` row into the file once, then drops the row so
//! a later run can't resurrect stale text. Read fresh on every message, so an external editor's save
//! applies to the next message with no restart.

use std::path::PathBuf;
use tauri::{command, AppHandle, Manager};

pub fn prompt_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("system_prompt.md")
}

/// Missing file reads as empty prompt — same as an install that never set one.
pub fn read(app: &AppHandle) -> String {
    std::fs::read_to_string(prompt_path(app)).unwrap_or_default()
}

#[command]
pub fn get_system_prompt(app: AppHandle) -> String {
    read(&app)
}

#[command]
pub fn set_system_prompt(app: AppHandle, content: String) -> Result<(), String> {
    let path = prompt_path(&app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// Shown in Settings so the user can open the file in their own editor.
#[command]
pub fn get_system_prompt_path(app: AppHandle) -> String {
    prompt_path(&app).to_string_lossy().to_string()
}

/// Names the prompt may interpolate, for the Settings hint list.
#[command]
pub fn list_prompt_vars() -> Vec<String> {
    crate::vars::KNOWN_VARS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// One-time move of the old `settings.system_prompt` row into the file. No-op once the file exists,
/// so a user who empties the file on purpose doesn't get the old text written back.
pub fn migrate_from_db(app: &AppHandle, conn: &rusqlite::Connection) {
    let path = prompt_path(app);
    if path.exists() {
        return;
    }
    let existing = crate::db::settings::get(conn, "system_prompt")
        .ok()
        .flatten()
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&path, &existing).is_ok() {
        let _ = conn.execute("DELETE FROM settings WHERE key = 'system_prompt'", []);
    }
}
