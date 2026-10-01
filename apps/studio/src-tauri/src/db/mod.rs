//! SQLite storage for chats, messages, attached files and turn traces.
//!
//! One connection behind a mutex: every statement here is small, and SQLite in WAL mode
//! serializes writers anyway. Schema changes are numbered migrations keyed on `user_version`.

mod attachments;
mod chats;
mod messages;
mod traces;
pub mod types;
mod vectors;

use std::path::Path;

use anyhow::Context;
use parking_lot::Mutex;
use rusqlite::Connection;

pub use attachments::{AttachmentContent, Passage};
pub use types::*;

pub struct Db {
    conn: Mutex<Connection>,
}

const MIGRATIONS: &[&str] = &[
    // 1: chats, messages, traces
    r#"
    CREATE TABLE chats (
        id          TEXT PRIMARY KEY,
        title       TEXT NOT NULL,
        model_id    TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        pinned      INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE messages (
        id             TEXT PRIMARY KEY,
        chat_id        TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        seq            INTEGER NOT NULL,
        role           TEXT NOT NULL,
        content        TEXT NOT NULL DEFAULT '',
        reasoning      TEXT,
        tool_calls     TEXT,
        tool_call_id   TEXT,
        tool_name      TEXT,
        tool_result    TEXT,
        model_id       TEXT,
        status         TEXT NOT NULL,
        error          TEXT,
        stats          TEXT,
        provider_meta  TEXT,
        created_at     INTEGER NOT NULL
    );
    CREATE INDEX messages_by_chat ON messages(chat_id, seq);
    CREATE TABLE traces (
        id           TEXT PRIMARY KEY,
        chat_id      TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        message_id   TEXT,
        created_at   INTEGER NOT NULL,
        model_id     TEXT,
        request      TEXT NOT NULL,
        response     TEXT,
        duration_ms  INTEGER,
        error        TEXT
    );
    CREATE INDEX traces_by_chat ON traces(chat_id, created_at);
    CREATE INDEX traces_by_message ON traces(message_id);
    "#,
    // 2: attached files, their passages (searched with BM25 through an FTS5 index over them),
    // and the passages each message was shown
    r#"
    CREATE TABLE attachments (
        id          TEXT PRIMARY KEY,
        chat_id     TEXT REFERENCES chats(id) ON DELETE CASCADE,
        message_id  TEXT,
        position    INTEGER NOT NULL DEFAULT 0,
        name        TEXT NOT NULL,
        file        TEXT NOT NULL,
        mime        TEXT NOT NULL,
        kind        TEXT NOT NULL,
        size        INTEGER NOT NULL,
        pages       INTEGER,
        width       INTEGER,
        height      INTEGER,
        tokens      INTEGER,
        note        TEXT,
        created_at  INTEGER NOT NULL,
        -- Large values last: reading a row's other columns never walks through them.
        media_mime  TEXT,
        text        TEXT,
        media       BLOB
    );
    CREATE INDEX attachments_by_message ON attachments(message_id);
    CREATE INDEX attachments_by_chat ON attachments(chat_id);
    CREATE TABLE attachment_passages (
        id             INTEGER PRIMARY KEY,
        attachment_id  TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
        seq            INTEGER NOT NULL,
        page           INTEGER,
        text           TEXT NOT NULL
    );
    CREATE INDEX passages_by_attachment ON attachment_passages(attachment_id, seq);
    CREATE VIRTUAL TABLE passages_fts USING fts5(
        text, content = 'attachment_passages', content_rowid = 'id',
        tokenize = 'porter unicode61 remove_diacritics 2'
    );
    CREATE TRIGGER passages_added AFTER INSERT ON attachment_passages BEGIN
        INSERT INTO passages_fts(rowid, text) VALUES (new.id, new.text);
    END;
    CREATE TRIGGER passages_removed AFTER DELETE ON attachment_passages BEGIN
        INSERT INTO passages_fts(passages_fts, rowid, text) VALUES ('delete', old.id, old.text);
    END;
    CREATE TABLE message_passages (
        message_id  TEXT PRIMARY KEY,
        chat_id     TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        passages    TEXT NOT NULL
    );
    "#,
    // 3: what each passage means, as a search model put it (little-endian f32s), one row per
    // model: a vector of one model cannot be compared with another's.
    r#"
    CREATE TABLE passage_vectors (
        passage_id  INTEGER NOT NULL REFERENCES attachment_passages(id) ON DELETE CASCADE,
        model       TEXT NOT NULL,
        vector      BLOB NOT NULL,
        PRIMARY KEY (passage_id, model)
    ) WITHOUT ROWID;
    "#,
];

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&conn).unwrap();
        Self { conn: Mutex::new(conn) }
    }

    pub(crate) fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        f(&self.conn.lock())
    }
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)
            .with_context(|| format!("applying migration {}", i + 1))?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A new time-ordered id.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
