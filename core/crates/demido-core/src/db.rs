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
        let tables: i64 = db.with(|c| c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'events')", [], |r| r.get(0))).unwrap();
        assert_eq!(tables, 2);
    }
}
