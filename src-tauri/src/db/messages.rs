use chrono::Utc;
use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub created_at: i64,
    pub token_count: Option<i64>,
    pub thinking: Option<String>,
}

pub fn list(conn: &Connection, conversation_id: &str) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, role, content, tool_call_id, created_at, token_count, thinking
         FROM messages WHERE conversation_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([conversation_id], |r| {
        Ok(Message {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            role: r.get(2)?,
            content: r.get(3)?,
            tool_call_id: r.get(4)?,
            created_at: r.get(5)?,
            token_count: r.get(6)?,
            thinking: r.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn insert(
    conn: &Connection,
    conversation_id: &str,
    role: &str,
    content: &str,
    tool_call_id: Option<&str>,
    thinking: Option<&str>,
) -> Result<Message> {
    let msg = Message {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.into(),
        role: role.into(),
        content: content.into(),
        tool_call_id: tool_call_id.map(|s| s.into()),
        created_at: Utc::now().timestamp_millis(),
        token_count: None,
        thinking: thinking.map(|s| s.into()),
    };
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, tool_call_id, created_at, thinking)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![msg.id, msg.conversation_id, msg.role, msg.content, msg.tool_call_id, msg.created_at, msg.thinking],
    )?;
    Ok(msg)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub conversation_id: String,
    pub snippet: String,
}

pub fn search(conn: &Connection, query: &str) -> Result<Vec<SearchResult>> {
    let mut stmt = conn.prepare(
        "SELECT conversation_id, snippet(messages_fts, 0, '<mark>', '</mark>', '...', 20)
         FROM messages_fts WHERE messages_fts MATCH ?1
         ORDER BY rank LIMIT 50",
    )?;
    let rows = stmt.query_map([query], |r| {
        Ok(SearchResult {
            conversation_id: r.get(0)?,
            snippet: r.get(1)?,
        })
    })?;
    rows.collect()
}

/// Deletes all messages in the same conversation that were created after `message_id`.
/// Uses rowid (always monotonic) rather than created_at (millisecond precision, may collide).
pub fn delete_after(conn: &Connection, message_id: &str) -> Result<()> {
    let (rowid, conversation_id): (i64, String) = conn.query_row(
        "SELECT rowid, conversation_id FROM messages WHERE id = ?1",
        [message_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    conn.execute(
        "DELETE FROM messages WHERE conversation_id = ?1 AND rowid > ?2",
        rusqlite::params![conversation_id, rowid],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use tempfile::NamedTempFile;

    fn setup() -> (rusqlite::Connection, NamedTempFile) {
        let f = NamedTempFile::new().unwrap();
        let conn = init(f.path()).unwrap();
        (conn, f)
    }

    #[test]
    fn delete_after_keeps_pivot() {
        let (conn, _f) = setup();
        conn.execute_batch(
            "
            INSERT INTO conversations (id, title, provider_id, model_id, created_at, updated_at)
            VALUES ('c1', 'test', 'p', 'm', 0, 0);
        ",
        )
        .unwrap();
        insert(&conn, "c1", "user", "msg1", None, None).unwrap();
        let pivot = insert(&conn, "c1", "user", "msg2", None, None).unwrap();
        insert(&conn, "c1", "user", "msg3", None, None).unwrap();

        delete_after(&conn, &pivot.id).unwrap();

        let remaining = list(&conn, "c1").unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[1].id, pivot.id);
    }
}

/// Deletes `message_id` itself AND all later messages in the same conversation.
/// Uses rowid (monotonic) for correct ordering.
pub fn delete_from(conn: &Connection, message_id: &str) -> Result<()> {
    let (rowid, conversation_id): (i64, String) = conn.query_row(
        "SELECT rowid, conversation_id FROM messages WHERE id = ?1",
        [message_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    conn.execute(
        "DELETE FROM messages WHERE conversation_id = ?1 AND rowid >= ?2",
        rusqlite::params![conversation_id, rowid],
    )?;
    Ok(())
}

/// Deletes a single message by id.
pub fn delete_one(conn: &Connection, message_id: &str) -> Result<()> {
    conn.execute("DELETE FROM messages WHERE id = ?1", [message_id])?;
    Ok(())
}

/// Updates the content of a single message.
pub fn update_content(conn: &Connection, message_id: &str, content: &str) -> Result<()> {
    conn.execute(
        "UPDATE messages SET content = ?1 WHERE id = ?2",
        rusqlite::params![content, message_id],
    )?;
    Ok(())
}
