use rusqlite::{OptionalExtension, Row, params};

use super::{Db, Message, MessageStatus, Role, ToolCall};

fn json_col<T: serde::de::DeserializeOwned>(r: &Row<'_>, col: &str) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = r.get(col)?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

fn row_to_message(r: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get("id")?,
        chat_id: r.get("chat_id")?,
        seq: r.get("seq")?,
        role: Role::parse(&r.get::<_, String>("role")?),
        content: r.get("content")?,
        reasoning: r.get("reasoning")?,
        tool_calls: json_col::<Vec<ToolCall>>(r, "tool_calls")?.unwrap_or_default(),
        tool_call_id: r.get("tool_call_id")?,
        tool_name: r.get("tool_name")?,
        tool_result: json_col(r, "tool_result")?,
        model_id: r.get("model_id")?,
        status: MessageStatus::parse(&r.get::<_, String>("status")?),
        error: r.get("error")?,
        stats: json_col(r, "stats")?,
        provider_meta: json_col(r, "provider_meta")?,
        created_at: r.get("created_at")?,
    })
}

fn to_json<T: serde::Serialize>(v: &Option<T>) -> Option<String> {
    v.as_ref().and_then(|v| serde_json::to_string(v).ok())
}

impl Db {
    pub fn list_messages(&self, chat_id: &str) -> rusqlite::Result<Vec<Message>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM messages WHERE chat_id = ?1 ORDER BY seq")?;
            let rows = stmt.query_map([chat_id], row_to_message)?;
            rows.collect()
        })
    }

    pub fn get_message(&self, id: &str) -> rusqlite::Result<Option<Message>> {
        self.with(|c| {
            c.query_row("SELECT * FROM messages WHERE id = ?1", [id], row_to_message)
                .optional()
        })
    }

    /// The sequence number the next message in a chat should take.
    pub fn next_seq(&self, chat_id: &str) -> rusqlite::Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE chat_id = ?1",
                [chat_id],
                |r| r.get(0),
            )
        })
    }

    /// Inserts or replaces a message by id.
    pub fn save_message(&self, m: &Message) -> rusqlite::Result<()> {
        let tool_calls = if m.tool_calls.is_empty() {
            None
        } else {
            serde_json::to_string(&m.tool_calls).ok()
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO messages (id, chat_id, seq, role, content, reasoning, tool_calls,
                    tool_call_id, tool_name, tool_result, model_id, status, error, stats,
                    provider_meta, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content, reasoning = excluded.reasoning,
                    tool_calls = excluded.tool_calls, tool_result = excluded.tool_result,
                    model_id = excluded.model_id, status = excluded.status, error = excluded.error,
                    stats = excluded.stats, provider_meta = excluded.provider_meta",
                params![
                    m.id,
                    m.chat_id,
                    m.seq,
                    m.role.as_str(),
                    m.content,
                    m.reasoning,
                    tool_calls,
                    m.tool_call_id,
                    m.tool_name,
                    to_json(&m.tool_result),
                    m.model_id,
                    m.status.as_str(),
                    m.error,
                    to_json(&m.stats),
                    to_json(&m.provider_meta),
                    m.created_at,
                ],
            )
        })?;
        Ok(())
    }

    /// Deletes every message from `seq` onwards (used to regenerate or edit a turn).
    pub fn truncate_messages(&self, chat_id: &str, from_seq: i64) -> rusqlite::Result<usize> {
        self.with(|c| {
            c.execute(
                "DELETE FROM messages WHERE chat_id = ?1 AND seq >= ?2",
                params![chat_id, from_seq],
            )
        })
    }

    /// Marks messages left mid-flight by a crash or forced exit as cancelled.
    pub fn close_dangling_messages(&self) -> rusqlite::Result<usize> {
        self.with(|c| {
            c.execute(
                "UPDATE messages SET status = 'cancelled'
                 WHERE status IN ('streaming', 'running', 'awaitingApproval')",
                [],
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_order_and_update_in_place() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        let seq = db.next_seq(&chat.id).unwrap();
        assert_eq!(seq, 1);
        let user = Message::new(&chat.id, seq, Role::User, "hello");
        db.save_message(&user).unwrap();
        let mut reply = Message::new(&chat.id, db.next_seq(&chat.id).unwrap(), Role::Assistant, "");
        reply.status = MessageStatus::Streaming;
        db.save_message(&reply).unwrap();
        reply.content = "hi there".into();
        reply.status = MessageStatus::Done;
        reply.tool_calls.push(ToolCall {
            id: "c1".into(),
            name: "market_quote".into(),
            arguments: "{}".into(),
        });
        db.save_message(&reply).unwrap();
        let all = db.list_messages(&chat.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].content, "hi there");
        assert_eq!(all[1].tool_calls.len(), 1);
        assert_eq!(db.truncate_messages(&chat.id, 2).unwrap(), 1);
        db.delete_chat(&chat.id).unwrap();
        assert!(db.list_messages(&chat.id).unwrap().is_empty());
    }
}
