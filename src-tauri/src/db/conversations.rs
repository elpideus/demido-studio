use chrono::Utc;
use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model_id: String,
    pub agent_mode: String,
    pub working_directory: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, provider_id, model_id, agent_mode, working_directory, created_at, updated_at
         FROM conversations WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map([id], |r| {
        Ok(Conversation {
            id: r.get(0)?,
            title: r.get(1)?,
            provider_id: r.get(2)?,
            model_id: r.get(3)?,
            agent_mode: r.get(4)?,
            working_directory: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
        })
    })?;
    rows.next().transpose()
}

pub fn list(conn: &Connection) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, provider_id, model_id, agent_mode, working_directory, created_at, updated_at
         FROM conversations ORDER BY updated_at DESC"
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Conversation {
            id: r.get(0)?,
            title: r.get(1)?,
            provider_id: r.get(2)?,
            model_id: r.get(3)?,
            agent_mode: r.get(4)?,
            working_directory: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn create(conn: &Connection, provider_id: &str, model_id: &str) -> Result<Conversation> {
    let now = Utc::now().timestamp_millis();
    let conv = Conversation {
        id: Uuid::new_v4().to_string(),
        title: "New conversation".into(),
        provider_id: provider_id.into(),
        model_id: model_id.into(),
        agent_mode: "off".into(),
        working_directory: None,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO conversations (id, title, provider_id, model_id, agent_mode, working_directory, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            conv.id, conv.title, conv.provider_id, conv.model_id,
            conv.agent_mode, conv.working_directory, conv.created_at, conv.updated_at
        ],
    )?;
    Ok(conv)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    Ok(())
}

pub fn update_title(conn: &Connection, id: &str, title: &str) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![title, now, id],
    )?;
    Ok(())
}

pub fn touch(conn: &Connection, id: &str) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, id],
    )?;
    Ok(())
}

pub fn set_agent_mode(conn: &Connection, id: &str, mode: &str) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET agent_mode = ?1 WHERE id = ?2",
        rusqlite::params![mode, id],
    )?;
    Ok(())
}

pub fn set_working_directory(conn: &Connection, id: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET working_directory = ?1 WHERE id = ?2",
        rusqlite::params![path, id],
    )?;
    Ok(())
}
