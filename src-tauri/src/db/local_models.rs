use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

/// A GGUF model file downloaded from Hugging Face and available for local inference.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// User-facing model id, unique. Form: "<repo>::<quant>" (e.g. "bartowski/Foo-GGUF::Q4_K_M").
    pub id: String,
    pub repo: String,
    pub quant: String,
    pub file_path: String,
    pub size: i64,
    /// Optional vision projector (mmproj-*.gguf), passed to llama-server via --mmproj.
    pub mmproj_path: Option<String>,
    /// Probed from llama-server `/props` on first load. None = never loaded yet.
    pub caps_vision: Option<bool>,
    pub caps_tools: Option<bool>,
    pub caps_reasoning: Option<bool>,
}

impl LocalModel {
    /// What llama.cpp told us about this model, if it has ever been loaded.
    pub fn caps(&self) -> crate::caps::PartialCaps {
        crate::caps::PartialCaps {
            vision: self.caps_vision,
            tools: self.caps_tools,
            reasoning: self.caps_reasoning,
        }
    }
}

const COLS: &str = "id, repo, quant, file_path, size, mmproj_path, caps_vision, caps_tools, caps_reasoning";

fn from_row(r: &rusqlite::Row) -> Result<LocalModel> {
    Ok(LocalModel {
        id: r.get(0)?,
        repo: r.get(1)?,
        quant: r.get(2)?,
        file_path: r.get(3)?,
        size: r.get(4)?,
        mmproj_path: r.get(5)?,
        caps_vision: r.get(6)?,
        caps_tools: r.get(7)?,
        caps_reasoning: r.get(8)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<LocalModel>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM local_models ORDER BY created_at DESC"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    rows.collect()
}

pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<LocalModel>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM local_models WHERE id = ?1"))?;
    let mut rows = stmt.query_map([id], from_row)?;
    rows.next().transpose()
}

/// Store what llama-server reported for a model. Left alone by `upsert`, so re-downloading
/// a model keeps its probe.
pub fn set_caps(conn: &Connection, id: &str, caps: &crate::caps::PartialCaps) -> Result<()> {
    conn.execute(
        "UPDATE local_models SET caps_vision = ?1, caps_tools = ?2, caps_reasoning = ?3
         WHERE id = ?4",
        rusqlite::params![caps.vision, caps.tools, caps.reasoning, id],
    )?;
    Ok(())
}

pub fn upsert(conn: &Connection, m: &LocalModel) -> Result<()> {
    conn.execute(
        "INSERT INTO local_models (id, repo, quant, file_path, size, mmproj_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           repo=excluded.repo, quant=excluded.quant,
           file_path=excluded.file_path, size=excluded.size, mmproj_path=excluded.mmproj_path",
        rusqlite::params![m.id, m.repo, m.quant, m.file_path, m.size, m.mmproj_path],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM local_models WHERE id = ?1", [id])?;
    Ok(())
}
