//! Selective reset.
//!
//! The wipe cannot happen while the app is running — SQLite holds `demido.db` open (and on
//! Windows an open file simply can't be removed), and the engines hold child processes. So
//! `request_reset` only drops a marker next to the data and restarts; `apply_pending` runs on
//! the next boot, *before* anything is opened, and does the deleting. `db::init` reseeds and
//! the setup wizard reappears on its own, since both key off state that just got deleted.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MARKER: &str = "reset-pending.json";

/// What the user chose to wipe. Everything is opt-in per scope; the frontend decides the
/// defaults. The user's own models folder, if they pointed Demido at one outside app-data, is
/// deliberately never touched — we delete what we installed, never a directory they chose and
/// may keep other things in.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ResetRequest {
    /// Conversations and their messages.
    pub conversations: bool,
    /// App settings, including the global system prompt.
    pub settings: bool,
    /// Cloud providers, model overrides and the stored API keys.
    pub providers: bool,
    /// Configured MCP servers.
    pub mcp_servers: bool,
    /// Connected Google accounts and their tokens.
    pub google_accounts: bool,
    /// Installed skills.
    pub skills: bool,
    /// The one-shot first-run dialogs: the disclaimer and the setup wizard show again.
    pub first_run_dialogs: bool,
    /// The downloaded llama.cpp runtime.
    pub inference_runtime: bool,
    /// The portable Python runtime.
    pub python_runtime: bool,
    /// Python-based tools (SearXNG).
    pub python_tools: bool,
    /// Downloaded GGUF models (those inside app-data).
    pub models: bool,
}

/// Record the request and let the caller restart. Nothing is deleted here.
pub fn request_reset(app_dir: &Path, req: &ResetRequest) -> Result<(), String> {
    let json = serde_json::to_string(req).map_err(|e| e.to_string())?;
    std::fs::write(app_dir.join(MARKER), json)
        .map_err(|e| format!("Could not schedule reset: {}", e))
}

/// Run any pending reset. Call at startup before opening the DB or reading secrets.
pub fn apply_pending(app_dir: &Path) {
    let marker = app_dir.join(MARKER);
    let Ok(raw) = std::fs::read_to_string(&marker) else {
        return;
    };
    let req: ResetRequest = serde_json::from_str(&raw).unwrap_or_default();

    wipe_rows(app_dir, &req);

    if req.providers {
        let _ = std::fs::remove_file(app_dir.join("secrets.json"));
    }
    for (wanted, dir) in [
        (req.skills, "skills"),
        (req.inference_runtime, "runtime"),
        (req.python_runtime, "python"),
        (req.python_tools, "searxng"),
        (req.models, "models"),
    ] {
        if wanted {
            let _ = std::fs::remove_dir_all(app_dir.join(dir));
        }
    }
    let _ = std::fs::remove_file(&marker);
}

/// Delete only the selected tables' rows. The DB file itself survives, so the schema and its
/// version stay intact; `db::init` reseeds whatever it finds empty (default providers, default
/// settings) exactly as it does on a first run.
fn wipe_rows(app_dir: &Path, req: &ResetRequest) {
    let db = app_dir.join("demido.db");
    if !db.exists() {
        return;
    }
    let Ok(conn) = Connection::open(&db) else {
        return;
    };
    let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");

    let mut sql = String::new();
    if req.conversations {
        // Messages cascade; the FTS index follows via the delete trigger.
        sql.push_str("DELETE FROM conversations;");
    }
    if req.settings {
        sql.push_str("DELETE FROM settings;");
    } else if req.first_run_dialogs {
        // The wizard keys off this one row; the disclaimer's flag lives in localStorage and is
        // cleared by the caller, since only the webview can reach it.
        sql.push_str("DELETE FROM settings WHERE key = 'setup_complete';");
    }
    if req.providers {
        sql.push_str("DELETE FROM providers; DELETE FROM model_overrides;");
    }
    if req.mcp_servers {
        sql.push_str("DELETE FROM mcp_servers;");
    }
    if req.google_accounts {
        sql.push_str("DELETE FROM accounts;");
    }
    if req.models {
        // The GGUF files are gone; leaving the rows would list models that can't load.
        sql.push_str("DELETE FROM local_models;");
    }
    if !sql.is_empty() {
        let _ = conn.execute_batch(&sql);
    }
}

pub fn app_data_dir(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path().app_data_dir().expect("app data dir")
}
