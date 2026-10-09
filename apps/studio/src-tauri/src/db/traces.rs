use rusqlite::{OptionalExtension, Row, params};

use super::{Db, Trace};

fn row_to_trace(r: &Row<'_>) -> rusqlite::Result<Trace> {
    let request: String = r.get("request")?;
    let response: Option<String> = r.get("response")?;
    Ok(Trace {
        id: r.get("id")?,
        chat_id: r.get("chat_id")?,
        message_id: r.get("message_id")?,
        created_at: r.get("created_at")?,
        model_id: r.get("model_id")?,
        request: serde_json::from_str(&request).unwrap_or(serde_json::Value::Null),
        response: response.and_then(|s| serde_json::from_str(&s).ok()),
        duration_ms: r.get("duration_ms")?,
        error: r.get("error")?,
        voice: r
            .get::<_, Option<String>>("voice")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    })
}

impl Db {
    pub fn save_trace(&self, t: &Trace) -> rusqlite::Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO traces
                   (id, chat_id, message_id, created_at, model_id, request, response, duration_ms, error, voice)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    t.id,
                    t.chat_id,
                    t.message_id,
                    t.created_at,
                    t.model_id,
                    t.request.to_string(),
                    t.response.as_ref().map(|r| r.to_string()),
                    t.duration_ms,
                    t.error,
                    (!t.voice.is_empty()).then(|| serde_json::to_string(&t.voice).unwrap_or_default()),
                ],
            )
        })?;
        Ok(())
    }

    pub fn trace_for_message(&self, message_id: &str) -> rusqlite::Result<Option<Trace>> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM traces WHERE message_id = ?1 ORDER BY created_at DESC LIMIT 1",
                [message_id],
                row_to_trace,
            )
            .optional()
        })
    }

    pub fn traces_for_chat(&self, chat_id: &str) -> rusqlite::Result<Vec<Trace>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM traces WHERE chat_id = ?1 ORDER BY created_at")?;
            let rows = stmt.query_map([chat_id], row_to_trace)?;
            rows.collect()
        })
    }
}
