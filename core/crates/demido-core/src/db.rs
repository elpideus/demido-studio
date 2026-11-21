//! The one SQLite file.
//!
//! WAL mode, so the command line can read while the server writes. Migrations are numbered and only ever
//! added to: the file remembers how many it has had (`user_version`).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;

use crate::Result;

/// Each entry runs once, in order. Never edit one that has shipped: add a new one.
const MIGRATIONS: &[&str] = &[
    // 1: settings that differ from their default, and the log of what happened
    "CREATE TABLE settings (
        key        TEXT PRIMARY KEY,
        value      TEXT NOT NULL,           -- JSON
        updated_at INTEGER NOT NULL         -- milliseconds since 1970
     ) STRICT;
     CREATE TABLE events (
        seq     INTEGER PRIMARY KEY AUTOINCREMENT,
        id      TEXT NOT NULL,
        at      INTEGER NOT NULL,           -- milliseconds since 1970
        kind    TEXT NOT NULL,              -- for example setting.changed
        subject TEXT,                       -- what it is about, for example a setting's id
        data    TEXT NOT NULL               -- JSON
     ) STRICT;
     CREATE INDEX events_at ON events (at);",
    // 2: sessions and what was said in them
    "CREATE TABLE sessions (
        id         TEXT PRIMARY KEY,
        title      TEXT NOT NULL,
        model      TEXT NOT NULL,           -- the model this session talks to, by its name
        created_at INTEGER NOT NULL,        -- milliseconds since 1970
        updated_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE messages (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        seq        INTEGER NOT NULL,        -- its place in the session, from 1
        role       TEXT NOT NULL,           -- user or assistant
        content    TEXT NOT NULL,
        thought    TEXT,                    -- the model's thinking, kept apart from its answer
        state      TEXT NOT NULL,           -- done, answering, stopped or failed
        problem    TEXT,                    -- why it failed, in words
        model      TEXT,                    -- the model that wrote it
        timings    TEXT,                    -- JSON: tokens and speed, as the runtime reported them
        created_at INTEGER NOT NULL,
        UNIQUE (session_id, seq)
     ) STRICT;",
    // 3: each session says whether its model thinks first: off, on, or one of the model's levels
    "ALTER TABLE sessions ADD COLUMN thinking TEXT NOT NULL DEFAULT 'off';",
    // 4: a long conversation made shorter. The messages stay; the model is given the account in their place.
    "CREATE TABLE summaries (
        id            TEXT PRIMARY KEY,
        session_id    TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        upto_seq      INTEGER NOT NULL,     -- the messages up to and including this one are covered
        content       TEXT NOT NULL,        -- the account of them, as the model wrote it
        why           TEXT NOT NULL,        -- asked (by the person) or auto (it was getting too long)
        tokens_before INTEGER NOT NULL,     -- about how long the conversation was
        created_at    INTEGER NOT NULL
     ) STRICT;",
    // 5: the tools an answer used, kept with the answer; and the tasks a piece of work was split into
    "ALTER TABLE messages ADD COLUMN steps TEXT NOT NULL DEFAULT '[]';   -- JSON: each use of a tool, in order
     CREATE TABLE tasks (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        message_id TEXT NOT NULL REFERENCES messages (id) ON DELETE CASCADE,  -- the answer it is part of
        parent_id  TEXT REFERENCES tasks (id) ON DELETE CASCADE,             -- set for a sub-task
        position   INTEGER NOT NULL,        -- its place among its siblings, from 1
        title      TEXT NOT NULL,
        state      TEXT NOT NULL,           -- pending, running, done, failed or stopped
        result     TEXT,                    -- what doing it gave, in short
        steps      TEXT NOT NULL DEFAULT '[]', -- JSON: the tools it used
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
     ) STRICT;
     CREATE INDEX tasks_of_message ON tasks (message_id, position);",
    // 6: a session's folder and how much it may do without asking; and what each change to a file replaced
    "ALTER TABLE sessions ADD COLUMN folder TEXT;                               -- the one place its tools may read and change files
     ALTER TABLE sessions ADD COLUMN permission TEXT NOT NULL DEFAULT 'balanced'; -- cautious, balanced, smart or autonomous
     CREATE TABLE changes (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        message_id TEXT NOT NULL,           -- the answer that made the change
        path       TEXT NOT NULL,           -- the file, inside the session's folder
        existed    INTEGER NOT NULL,        -- 0 when the change created the file
        before     BLOB,                    -- what the file held before
        undone_at  INTEGER,                 -- set when the change was undone
        created_at INTEGER NOT NULL
     ) STRICT;",
    // 7: a change made by a program: the whole folder was saved first, and this says where
    "ALTER TABLE changes ADD COLUMN checkpoint TEXT;",
    // 8: what a change changed, as a diff made at the moment of the change, and its size in a few words
    "ALTER TABLE changes ADD COLUMN diff TEXT; ALTER TABLE changes ADD COLUMN diff_stat TEXT;",
];

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::prepare(Connection::open(path)?)
    }

    /// A database that lives in memory and vanishes with the process. For tests.
    pub fn memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
        let done: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(done as usize) {
            conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Runs `f` with the connection. Keep it short: one connection serves everyone.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        Ok(f(&conn)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_file_gets_every_migration_once() {
        let db = Db::memory().unwrap();
        let version: i64 = db.with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0))).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let tables: i64 =
            db.with(|c| c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'events', 'sessions', 'messages', 'summaries')", [], |r| r.get(0))).unwrap();
        assert_eq!(tables, 5);
    }
}
