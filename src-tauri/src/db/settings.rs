use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub default_provider_id: String,
    pub default_model_id: String,
    pub system_prompt: String,
    pub auth_enabled: bool,
    pub context_window_limit: i64,
    pub task_provider_id: String,
    pub task_model_id: String,
    pub title_every_n_messages: i64,
}

fn get_val(conn: &Connection, key: &str, default: &str) -> Result<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .or_else(|_| Ok(default.to_string()))
}

pub fn get_all(conn: &Connection) -> Result<AppSettings> {
    Ok(AppSettings {
        default_provider_id: get_val(conn, "default_provider_id", "")?,
        default_model_id: get_val(conn, "default_model_id", "")?,
        system_prompt: get_val(conn, "system_prompt", "")?,
        auth_enabled: get_val(conn, "auth_enabled", "false")? == "true",
        context_window_limit: get_val(conn, "context_window_limit", "8192")?
            .parse()
            .unwrap_or(8192),
        task_provider_id: get_val(conn, "task_provider_id", "")?,
        task_model_id: get_val(conn, "task_model_id", "")?,
        title_every_n_messages: get_val(conn, "title_every_n_messages", "5")?
            .parse()
            .unwrap_or(5),
    })
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [key, value],
    )?;
    Ok(())
}
