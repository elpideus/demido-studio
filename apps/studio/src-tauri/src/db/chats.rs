use rusqlite::{OptionalExtension, Row, params};

use super::{Chat, Db, new_id, now_ms};

fn row_to_chat(r: &Row<'_>) -> rusqlite::Result<Chat> {
    Ok(Chat {
        id: r.get("id")?,
        title: r.get("title")?,
        model_id: r.get("model_id")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        pinned: r.get::<_, i64>("pinned")? != 0,
    })
}

impl Db {
    pub fn list_chats(&self) -> rusqlite::Result<Vec<Chat>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM chats ORDER BY pinned DESC, updated_at DESC LIMIT 500",
            )?;
            let rows = stmt.query_map([], row_to_chat)?;
            rows.collect()
        })
    }

    pub fn get_chat(&self, id: &str) -> rusqlite::Result<Option<Chat>> {
        self.with(|c| {
            c.query_row("SELECT * FROM chats WHERE id = ?1", [id], row_to_chat)
                .optional()
        })
    }

    pub fn create_chat(&self, title: &str, model_id: Option<&str>) -> rusqlite::Result<Chat> {
        let now = now_ms();
        let chat = Chat {
            id: new_id(),
            title: title.to_string(),
            model_id: model_id.map(str::to_string),
            created_at: now,
            updated_at: now,
            pinned: false,
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO chats (id, title, model_id, created_at, updated_at, pinned)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![chat.id, chat.title, chat.model_id, chat.created_at, chat.updated_at],
            )
        })?;
        Ok(chat)
    }

    pub fn rename_chat(&self, id: &str, title: &str) -> rusqlite::Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE chats SET title = ?2 WHERE id = ?1",
                params![id, title],
            )
        })?;
        Ok(())
    }

    pub fn set_chat_pinned(&self, id: &str, pinned: bool) -> rusqlite::Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE chats SET pinned = ?2 WHERE id = ?1",
                params![id, pinned as i64],
            )
        })?;
        Ok(())
    }

    /// Marks a chat as used now, optionally recording the model it last used.
    pub fn touch_chat(&self, id: &str, model_id: Option<&str>) -> rusqlite::Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE chats SET updated_at = ?2, model_id = COALESCE(?3, model_id) WHERE id = ?1",
                params![id, now_ms(), model_id],
            )
        })?;
        Ok(())
    }

    pub fn delete_chat(&self, id: &str) -> rusqlite::Result<()> {
        self.with(|c| c.execute("DELETE FROM chats WHERE id = ?1", [id]))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chats_round_trip_and_order_by_recency() {
        let db = Db::in_memory();
        let a = db.create_chat("First", None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = db.create_chat("Second", Some("local:x")).unwrap();
        assert_eq!(db.list_chats().unwrap()[0].id, b.id);
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.touch_chat(&a.id, None).unwrap();
        assert_eq!(db.list_chats().unwrap()[0].id, a.id);
        db.rename_chat(&a.id, "Renamed").unwrap();
        assert_eq!(db.get_chat(&a.id).unwrap().unwrap().title, "Renamed");
        db.delete_chat(&a.id).unwrap();
        assert!(db.get_chat(&a.id).unwrap().is_none());
        let _ = new_id();
    }
}
