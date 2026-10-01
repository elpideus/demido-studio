//! Attached files: their rows, the text and media stored with them, their passages (an indexed
//! table, searched with BM25 through the FTS5 index `passages_fts` over it), and the passages each
//! message was shown.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use super::{Attachment, Db, Message};

/// A passage of an attached file.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub attachment_id: String,
    /// 0-based position in the file.
    pub seq: u32,
    /// Page (slide, sheet) it starts on.
    pub page: Option<u32>,
    pub text: String,
    /// Its row in `attachment_passages` (and in the index).
    pub rowid: i64,
}

/// What is stored with a new attachment besides its row.
pub struct AttachmentContent<'a> {
    /// The text a model reads.
    pub text: Option<&'a str>,
    /// What a model is given of an image or a sound, and its MIME type.
    pub media: Option<(&'a [u8], &'a str)>,
    pub chunks: &'a [demido_extract::Chunk],
}

const COLUMNS: &str = "a.id, a.chat_id, a.message_id, a.name, a.file, a.mime, a.kind, a.size, a.pages, a.width, a.height, a.tokens, a.note, a.created_at";

fn kind_str(kind: demido_extract::Kind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "other".into())
}

fn parse_kind(s: &str) -> demido_extract::Kind {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(demido_extract::Kind::Other)
}

fn row_to_attachment(r: &Row<'_>) -> rusqlite::Result<Attachment> {
    let chat_id: Option<String> = r.get("chat_id")?;
    let stored: String = r.get("file")?;
    Ok(Attachment {
        id: r.get("id")?,
        file: chat_id.as_ref().map(|_| stored.clone()),
        chat_id,
        message_id: r.get("message_id")?,
        name: r.get("name")?,
        stored,
        path: String::new(),
        mime: r.get("mime")?,
        kind: parse_kind(&r.get::<_, String>("kind")?),
        size: r.get::<_, i64>("size")?.max(0) as u64,
        pages: r.get::<_, Option<i64>>("pages")?.map(|v| v as u32),
        tokens: r.get::<_, Option<i64>>("tokens")?.map(|v| v.max(0) as u64),
        width: r.get::<_, Option<i64>>("width")?.map(|v| v as u32),
        height: r.get::<_, Option<i64>>("height")?.map(|v| v as u32),
        note: r.get("note")?,
        created_at: r.get("created_at")?,
    })
}

fn row_to_passage(r: &Row<'_>) -> rusqlite::Result<Passage> {
    Ok(Passage {
        attachment_id: r.get(0)?,
        seq: r.get::<_, i64>(1)?.max(0) as u32,
        page: r.get::<_, Option<i64>>(2)?.map(|p| p as u32),
        text: r.get(3)?,
        rowid: r.get(4)?,
    })
}

impl Db {
    /// Stores a new (staged) attachment with its text, media and passages.
    pub fn insert_attachment(&self, a: &Attachment, content: AttachmentContent<'_>) -> rusqlite::Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO attachments (id, chat_id, message_id, name, file, mime, kind, size, pages,
                    width, height, text, tokens, note, media, media_mime, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    a.id,
                    a.chat_id,
                    a.message_id,
                    a.name,
                    a.stored,
                    a.mime,
                    kind_str(a.kind),
                    a.size as i64,
                    a.pages,
                    a.width,
                    a.height,
                    content.text,
                    a.tokens.map(|t| t as i64),
                    a.note,
                    content.media.map(|m| m.0),
                    content.media.map(|m| m.1),
                    a.created_at,
                ],
            )?;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO attachment_passages (attachment_id, seq, page, text) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for chunk in content.chunks {
                    insert.execute(params![a.id, chunk.seq, chunk.page, chunk.text])?;
                }
            }
            tx.commit()
        })
    }

    pub fn get_attachment(&self, id: &str) -> rusqlite::Result<Option<Attachment>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM attachments a WHERE a.id = ?1"),
                [id],
                row_to_attachment,
            )
            .optional()
        })
    }

    /// The files sent in a chat, in the order of their messages and, within one, as attached.
    /// Files of messages that no longer exist are left out.
    pub fn chat_attachments(&self, chat_id: &str) -> rusqlite::Result<Vec<Attachment>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM attachments a JOIN messages m ON m.id = a.message_id
                 WHERE a.chat_id = ?1 ORDER BY m.seq, a.position"
            ))?;
            let rows = stmt.query_map([chat_id], row_to_attachment)?;
            rows.collect()
        })
    }

    /// The sent file of a chat stored at `file` (a workspace path such as `uploads/a.pdf`), the
    /// newest when the name was used again.
    pub fn attachment_at(&self, chat_id: &str, file: &str) -> rusqlite::Result<Option<Attachment>> {
        self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {COLUMNS} FROM attachments a WHERE a.chat_id = ?1 AND a.file = ?2
                     ORDER BY a.created_at DESC LIMIT 1"
                ),
                params![chat_id, file],
                row_to_attachment,
            )
            .optional()
        })
    }

    /// Puts each message's files on it.
    pub(crate) fn fill_attachments(&self, chat_id: &str, messages: &mut [Message]) -> rusqlite::Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let mut by_message: HashMap<String, Vec<Attachment>> = HashMap::new();
        for a in self.chat_attachments(chat_id)? {
            if let Some(id) = a.message_id.clone() {
                by_message.entry(id).or_default().push(a);
            }
        }
        for m in messages {
            m.attachments = by_message.remove(&m.id).unwrap_or_default();
        }
        Ok(())
    }

    /// Links a staged attachment to the message it was sent with, as its `position`th file.
    /// False when it is not staged.
    pub fn link_attachment(
        &self,
        id: &str,
        chat_id: &str,
        message_id: &str,
        file: &str,
        position: usize,
    ) -> rusqlite::Result<bool> {
        self.with(|c| {
            c.execute(
                "UPDATE attachments SET chat_id = ?2, message_id = ?3, file = ?4, position = ?5
                 WHERE id = ?1 AND chat_id IS NULL",
                params![id, chat_id, message_id, file, position as i64],
            )
        })
        .map(|n| n == 1)
    }

    /// Puts a sent attachment back in the composer (a send that failed halfway).
    pub fn unlink_attachment(&self, id: &str, stored: &str) -> rusqlite::Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE attachments SET chat_id = NULL, message_id = NULL, file = ?2, position = 0 WHERE id = ?1",
                params![id, stored],
            )
        })?;
        Ok(())
    }

    /// Moves a message's files to the message that replaces it (an edit).
    pub fn relink_attachments(&self, chat_id: &str, from: &str, to: &str) -> rusqlite::Result<usize> {
        self.with(|c| {
            c.execute(
                "UPDATE attachments SET message_id = ?3 WHERE chat_id = ?1 AND message_id = ?2",
                params![chat_id, from, to],
            )
        })
    }

    /// Forgets what belonged to messages that no longer exist (removed by an edit further up):
    /// their files' rows and the passages they were shown. The files stay in the workspace, where
    /// later answers may have used them.
    pub fn forget_removed_messages(&self, chat_id: &str) -> rusqlite::Result<usize> {
        self.with(|c| {
            c.execute(
                "DELETE FROM message_passages WHERE chat_id = ?1
                 AND message_id NOT IN (SELECT id FROM messages WHERE chat_id = ?1)",
                [chat_id],
            )?;
            c.execute(
                "DELETE FROM attachments WHERE chat_id = ?1 AND message_id IS NOT NULL
                 AND message_id NOT IN (SELECT id FROM messages WHERE chat_id = ?1)",
                [chat_id],
            )
        })
    }

    /// Deletes a staged attachment; sent ones are kept. True when one was deleted.
    pub fn delete_staged_attachment(&self, id: &str) -> rusqlite::Result<bool> {
        self.with(|c| c.execute("DELETE FROM attachments WHERE id = ?1 AND chat_id IS NULL", [id]))
            .map(|n| n == 1)
    }

    /// Deletes every staged attachment (files left in the composer when the app closed).
    pub fn delete_staged_attachments(&self) -> rusqlite::Result<usize> {
        self.with(|c| c.execute("DELETE FROM attachments WHERE chat_id IS NULL", []))
    }

    pub fn attachment_text(&self, id: &str) -> rusqlite::Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT text FROM attachments WHERE id = ?1", [id], |r| r.get(0))
                .optional()
                .map(Option::flatten)
        })
    }

    /// The image or sound stored with an attachment for a model, and its MIME type.
    pub fn attachment_media(&self, id: &str) -> rusqlite::Result<Option<(Vec<u8>, String)>> {
        self.with(|c| {
            c.query_row(
                "SELECT media, media_mime FROM attachments WHERE id = ?1 AND media IS NOT NULL",
                [id],
                |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())),
            )
            .optional()
        })
    }

    /// Bytes of the image or sound stored with an attachment for a model.
    pub fn attachment_media_len(&self, id: &str) -> rusqlite::Result<Option<usize>> {
        self.with(|c| {
            c.query_row("SELECT length(media) FROM attachments WHERE id = ?1", [id], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()
            .map(|v| v.flatten().map(|n| n.max(0) as usize))
        })
    }

    /// Passages of the given files matching an FTS5 query, best first (BM25).
    pub fn search_passages(&self, ids: &[String], query: &str, limit: usize) -> rusqlite::Result<Vec<Passage>> {
        if ids.is_empty() || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (0..ids.len())
            .map(|i| format!("?{}", i + 3))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT p.attachment_id, p.seq, p.page, p.text, p.id
             FROM passages_fts JOIN attachment_passages p ON p.id = passages_fts.rowid
             WHERE passages_fts MATCH ?1 AND p.attachment_id IN ({placeholders})
             ORDER BY rank LIMIT ?2"
        );
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            let mut values: Vec<rusqlite::types::Value> = vec![query.to_string().into(), (limit as i64).into()];
            values.extend(ids.iter().map(|id| rusqlite::types::Value::from(id.clone())));
            let rows = stmt.query_map(params_from_iter(values), row_to_passage)?;
            rows.collect()
        })
    }

    /// How many of `terms` (FTS5 queries of one word each) each passage in `rowids` matches, as
    /// FTS5 matches them: "terminated" and "termination" are one term, not two.
    pub fn matching_terms(&self, rowids: &[i64], terms: &[String]) -> rusqlite::Result<HashMap<i64, u32>> {
        let mut counts: HashMap<i64, u32> = HashMap::new();
        if rowids.is_empty() {
            return Ok(counts);
        }
        let list = rowids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let sql = format!("SELECT rowid FROM passages_fts WHERE passages_fts MATCH ?1 AND rowid IN ({list})");
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            for term in terms {
                let hits: HashSet<i64> = stmt.query_map([term], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                for rowid in hits {
                    *counts.entry(rowid).or_default() += 1;
                }
            }
            Ok(counts)
        })
    }

    /// The first `count` passages of a file, in order.
    pub fn first_passages(&self, id: &str, count: usize) -> rusqlite::Result<Vec<Passage>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT attachment_id, seq, page, text, id FROM attachment_passages
                 WHERE attachment_id = ?1 ORDER BY seq LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![id, count as i64], row_to_passage)?;
            rows.collect()
        })
    }

    /// One passage of a file.
    pub fn passage(&self, id: &str, seq: u32) -> rusqlite::Result<Option<Passage>> {
        self.with(|c| {
            c.query_row(
                "SELECT attachment_id, seq, page, text, id FROM attachment_passages
                 WHERE attachment_id = ?1 AND seq = ?2",
                params![id, seq],
                row_to_passage,
            )
            .optional()
        })
    }

    /// The passages a message was shown the first time its prompt was built, as (file, seq).
    pub fn shown_passages(&self, message_id: &str) -> rusqlite::Result<Option<Vec<(String, u32)>>> {
        self.with(|c| {
            c.query_row(
                "SELECT passages FROM message_passages WHERE message_id = ?1",
                [message_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
        .map(|raw| raw.map(|json| serde_json::from_str(&json).unwrap_or_default()))
    }

    /// Remembers the passages a message is shown, so it is shown the same ones every time
    /// (replacing what was remembered: a choice is only ever extended).
    pub fn remember_passages(
        &self,
        chat_id: &str,
        message_id: &str,
        passages: &[(String, u32)],
    ) -> rusqlite::Result<()> {
        let json = serde_json::to_string(passages).unwrap_or_else(|_| "[]".into());
        self.with(|c| {
            c.execute(
                "INSERT INTO message_passages (message_id, chat_id, passages) VALUES (?1, ?2, ?3)
                 ON CONFLICT(message_id) DO UPDATE SET passages = excluded.passages",
                params![message_id, chat_id, json],
            )
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Role, new_id, now_ms};
    use demido_extract::{Chunk, Kind};

    fn staged(name: &str) -> Attachment {
        Attachment {
            id: new_id(),
            chat_id: None,
            message_id: None,
            name: name.into(),
            stored: format!("x/{name}"),
            file: None,
            path: String::new(),
            mime: "application/pdf".into(),
            kind: Kind::Document,
            size: 10,
            pages: Some(2),
            tokens: Some(40),
            width: None,
            height: None,
            note: None,
            created_at: now_ms(),
        }
    }

    fn chunks(texts: &[&str]) -> Vec<Chunk> {
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| Chunk {
                seq: i as u32,
                page: Some(i as u32 + 1),
                text: t.to_string(),
            })
            .collect()
    }

    fn insert(db: &Db, a: &Attachment, texts: &[&str]) {
        db.insert_attachment(
            a,
            AttachmentContent {
                text: Some("whole text"),
                media: None,
                chunks: &chunks(texts),
            },
        )
        .unwrap();
    }

    #[test]
    fn attachments_move_from_staging_to_a_message_and_are_searchable() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        let a = staged("contract.pdf");
        insert(
            &db,
            &a,
            &[
                "This agreement starts on the first of May.",
                "Either party may terminate the agreement with thirty days of written notice.",
                "Payments are due monthly.",
            ],
        );
        assert_eq!(
            db.get_attachment(&a.id).unwrap().unwrap().file,
            None,
            "staged has no workspace path"
        );

        let m = Message::new(&chat.id, 1, Role::User, "When can it end?");
        db.save_message(&m).unwrap();
        assert!(
            db.link_attachment(&a.id, &chat.id, &m.id, "uploads/contract.pdf", 0)
                .unwrap()
        );
        assert!(
            !db.link_attachment(&a.id, &chat.id, &m.id, "uploads/again.pdf", 0)
                .unwrap(),
            "a sent attachment is never linked twice"
        );
        let messages = db.list_messages(&chat.id).unwrap();
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(messages[0].attachments[0].file.as_deref(), Some("uploads/contract.pdf"));
        assert_eq!(db.attachment_text(&a.id).unwrap().as_deref(), Some("whole text"));

        // Porter stemming: "terminating" finds "terminate".
        let hits = db
            .search_passages(std::slice::from_ref(&a.id), "\"terminating\" OR \"notice\"", 5)
            .unwrap();
        assert_eq!(hits[0].seq, 1);
        assert_eq!(hits[0].page, Some(2));
        let terms: Vec<String> = ["\"terminating\"", "\"terminated\"", "\"notice\"", "\"payments\""]
            .map(String::from)
            .into();
        let counts = db.matching_terms(&[hits[0].rowid], &terms).unwrap();
        assert_eq!(
            counts[&hits[0].rowid], 3,
            "each query word counts once, whatever form it takes in the passage"
        );
        assert_eq!(db.first_passages(&a.id, 2).unwrap().len(), 2);
        assert_eq!(db.passage(&a.id, 2).unwrap().unwrap().text, "Payments are due monthly.");
        assert!(
            !db.delete_staged_attachment(&a.id).unwrap(),
            "sent files are not discarded"
        );

        // Deleting the chat removes the rows, the passages and their index entries.
        db.delete_chat(&chat.id).unwrap();
        assert!(db.get_attachment(&a.id).unwrap().is_none());
        assert!(db.first_passages(&a.id, 5).unwrap().is_empty());
        let indexed: i64 = db
            .with(|c| {
                c.query_row(
                    "SELECT count(*) FROM passages_fts WHERE passages_fts MATCH 'payments'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(indexed, 0);
    }

    #[test]
    fn files_keep_the_order_they_were_attached_in() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        let m = Message::new(&chat.id, 1, Role::User, "compare");
        db.save_message(&m).unwrap();
        // "after" finished reading first, but was attached second.
        let after = staged("after.png");
        insert(&db, &after, &[]);
        let before = staged("before.png");
        insert(&db, &before, &[]);
        db.link_attachment(&before.id, &chat.id, &m.id, "uploads/before.png", 0)
            .unwrap();
        db.link_attachment(&after.id, &chat.id, &m.id, "uploads/after.png", 1)
            .unwrap();
        let names: Vec<String> = db.list_messages(&chat.id).unwrap()[0]
            .attachments
            .iter()
            .map(|a| a.name.clone())
            .collect();
        assert_eq!(names, ["before.png", "after.png"]);
    }

    #[test]
    fn edits_keep_files_and_forget_the_rest() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        let first = Message::new(&chat.id, 1, Role::User, "one");
        let later = Message::new(&chat.id, 3, Role::User, "two");
        db.save_message(&first).unwrap();
        db.save_message(&later).unwrap();
        for (m, name) in [(&first, "a.txt"), (&later, "b.txt")] {
            let a = staged(name);
            insert(&db, &a, &[]);
            db.link_attachment(&a.id, &chat.id, &m.id, &format!("uploads/{name}"), 0)
                .unwrap();
        }
        db.remember_passages(&chat.id, &later.id, &[("x".into(), 1)]).unwrap();
        // Edit the first message: everything from it on goes, the new one takes its files.
        db.truncate_messages(&chat.id, 1).unwrap();
        let edited = Message::new(&chat.id, 1, Role::User, "one, edited");
        db.save_message(&edited).unwrap();
        assert_eq!(db.relink_attachments(&chat.id, &first.id, &edited.id).unwrap(), 1);
        assert_eq!(db.forget_removed_messages(&chat.id).unwrap(), 1);
        let files = db.chat_attachments(&chat.id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "a.txt");
        assert_eq!(files[0].message_id.as_deref(), Some(edited.id.as_str()));
        assert_eq!(db.shown_passages(&later.id).unwrap(), None);
    }

    #[test]
    fn a_message_keeps_the_passages_it_was_shown() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        assert_eq!(db.shown_passages("m1").unwrap(), None);
        db.remember_passages(&chat.id, "m1", &[("a".into(), 3), ("b".into(), 0)])
            .unwrap();
        assert_eq!(
            db.shown_passages("m1").unwrap(),
            Some(vec![("a".to_string(), 3), ("b".to_string(), 0)])
        );
        db.remember_passages(&chat.id, "m2", &[]).unwrap();
        assert_eq!(
            db.shown_passages("m2").unwrap(),
            Some(vec![]),
            "an empty choice is remembered too"
        );
    }

    #[test]
    fn staged_attachments_are_cleared() {
        let db = Db::in_memory();
        let a = staged("photo.png");
        db.insert_attachment(
            &a,
            AttachmentContent {
                text: None,
                media: Some((&[1, 2, 3], "image/jpeg")),
                chunks: &[],
            },
        )
        .unwrap();
        assert_eq!(
            db.attachment_media(&a.id).unwrap(),
            Some((vec![1, 2, 3], "image/jpeg".to_string()))
        );
        assert_eq!(db.delete_staged_attachments().unwrap(), 1);
        assert!(db.get_attachment(&a.id).unwrap().is_none());
    }

    #[test]
    fn a_failed_send_puts_files_back() {
        let db = Db::in_memory();
        let chat = db.create_chat("t", None).unwrap();
        let a = staged("a.txt");
        insert(&db, &a, &[]);
        db.link_attachment(&a.id, &chat.id, "m", "uploads/a.txt", 0).unwrap();
        db.unlink_attachment(&a.id, &a.stored).unwrap();
        let back = db.get_attachment(&a.id).unwrap().unwrap();
        assert_eq!((back.chat_id, back.file, back.stored), (None, None, a.stored));
    }
}
