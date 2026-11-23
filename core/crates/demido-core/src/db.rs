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
    // 9: models elsewhere the person chose, and what each request to a provider cost
    "CREATE TABLE remote_models (
        id        TEXT PRIMARY KEY,         -- provider/model, as a session names it
        provider  TEXT NOT NULL,
        facts     TEXT NOT NULL,            -- JSON: what the provider said about it when it was chosen
        chosen_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE usage (
        seq           INTEGER PRIMARY KEY AUTOINCREMENT,
        at            INTEGER NOT NULL,     -- milliseconds since 1970
        provider      TEXT NOT NULL,
        model         TEXT NOT NULL,        -- its name at the provider
        session_id    TEXT,                 -- kept when the session is deleted: the money was spent
        message_id    TEXT,                 -- the answer it was asked for
        input         INTEGER NOT NULL,     -- tokens read at the full price
        cached        INTEGER NOT NULL,     -- tokens read from the provider's cache
        cache_written INTEGER NOT NULL,     -- tokens written to the provider's cache
        output        INTEGER NOT NULL,     -- tokens written, thinking included
        cost_usd      REAL NOT NULL,
        exact         INTEGER NOT NULL      -- 1: the provider's own figure. 0: Demido's count from tokens and prices
     ) STRICT;
     CREATE INDEX usage_at ON usage (at);
     CREATE INDEX usage_of_message ON usage (message_id);",
    // 10: a program's own session (Claude Code's, OpenCode's) for a session of Demido's, and how far it has seen it
    "CREATE TABLE program_sessions (
        session_id  TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        provider    TEXT NOT NULL,          -- claude-code or opencode
        external_id TEXT NOT NULL,          -- the program's own name for its session
        seen_seq    INTEGER NOT NULL,       -- the last message of Demido's session the program was given
        cost_so_far REAL NOT NULL DEFAULT 0, -- what the program said its session had cost so far, in dollars
        PRIMARY KEY (session_id, provider)
     ) STRICT;",
    // 11: several accounts at one provider (keys, or Claude Code's sign-ins); the one a session uses; and whose
    // account each request was paid from
    "CREATE TABLE accounts (
        id         TEXT PRIMARY KEY,
        provider   TEXT NOT NULL,
        label      TEXT NOT NULL,           -- the person's own name for it: Personal, Work
        is_default INTEGER NOT NULL,        -- 1 for the one a session uses unless it names another
        created_at INTEGER NOT NULL
     ) STRICT;
     -- null: its provider's default
     ALTER TABLE sessions ADD COLUMN account TEXT;
     ALTER TABLE usage ADD COLUMN account TEXT;
     -- whose profile the program's session is in
     ALTER TABLE program_sessions ADD COLUMN account TEXT NOT NULL DEFAULT '';",
    // 12: artifacts, the files a model makes in a session and gives the person to keep, with every version
    "CREATE TABLE artifacts (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        name       TEXT NOT NULL,           -- as the model named it, with its ending: plan.md, page.html
        created_at INTEGER NOT NULL,
        UNIQUE (session_id, name)
     ) STRICT;
     CREATE TABLE artifact_versions (
        artifact_id TEXT NOT NULL REFERENCES artifacts (id) ON DELETE CASCADE,
        n           INTEGER NOT NULL,       -- 1 for the first
        text        TEXT NOT NULL,
        made_at     INTEGER NOT NULL,
        message_id  TEXT,                   -- the answer that made it
        PRIMARY KEY (artifact_id, n)
     ) STRICT;",
    // 13: schedules and monitors, each a session of its own, and every run of them (DECISIONS section J)
    "CREATE TABLE schedules (
        id              TEXT PRIMARY KEY,
        session_id      TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        what            TEXT NOT NULL,      -- what to do, in the person's words
        said            TEXT NOT NULL,      -- the sentence it was made from
        rule            TEXT NOT NULL,      -- JSON: when it runs (schedules::When)
        watch           TEXT,               -- JSON: what a monitor watches for, or NULL
        state           TEXT NOT NULL,      -- on, paused or done
        paused_why      TEXT,
        next_at         INTEGER,            -- milliseconds since 1970; NULL: not due again
        last_run_at     INTEGER,
        failures        INTEGER NOT NULL DEFAULT 0,   -- runs in a row that failed
        watched         TEXT,               -- JSON: a monitor's last reading
        limit_month_usd REAL,               -- the most a paid provider may cost it in a month
        created_at      INTEGER NOT NULL,
        updated_at      INTEGER NOT NULL
     ) STRICT;
     CREATE INDEX schedules_next ON schedules (state, next_at);
     CREATE TABLE schedule_runs (
        id          TEXT PRIMARY KEY,
        schedule_id TEXT NOT NULL REFERENCES schedules (id) ON DELETE CASCADE,
        due_at      INTEGER NOT NULL,
        started_at  INTEGER NOT NULL,
        ended_at    INTEGER,
        state       TEXT NOT NULL,          -- running, done, failed, told, quiet or skipped
        late        INTEGER NOT NULL,       -- 1: it ran after its time (the computer slept)
        by_hand     INTEGER NOT NULL,       -- 1: the person started it
        message_id  TEXT,                   -- the answer it wrote
        note        TEXT NOT NULL
     ) STRICT;
     CREATE INDEX schedule_runs_of ON schedule_runs (schedule_id, started_at);",
    // Email (DECISIONS Y): the person's accounts (their passwords are in the vault), and what was fetched of their
    // mail, kept so that asking again fetches only what is newer and a message's text once.
    "CREATE TABLE email_accounts (
        id         TEXT PRIMARY KEY,
        address    TEXT NOT NULL,
        label      TEXT NOT NULL,
        preset     TEXT NOT NULL,          -- gmail or other
        imap_host  TEXT NOT NULL,
        imap_port  INTEGER NOT NULL,
        smtp_host  TEXT NOT NULL,
        smtp_port  INTEGER NOT NULL,
        username   TEXT NOT NULL,
        created_at INTEGER NOT NULL
     ) STRICT;
     CREATE TABLE email_messages (
        account      TEXT NOT NULL REFERENCES email_accounts (id) ON DELETE CASCADE,
        folder       TEXT NOT NULL,
        uid_validity INTEGER NOT NULL,
        uid          INTEGER NOT NULL,
        from_who     TEXT NOT NULL,
        to_who       TEXT NOT NULL,
        subject      TEXT NOT NULL,
        date         INTEGER,
        seen         INTEGER NOT NULL,
        size         INTEGER NOT NULL,
        text         TEXT,                 -- once the message was opened
        files        TEXT,                 -- the names of its files, as JSON, once opened
        kept_at      INTEGER NOT NULL,
        PRIMARY KEY (account, folder, uid_validity, uid)
     ) STRICT;",
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
            // On a line of its own: a migration that ends in a comment must not swallow the COMMIT.
            conn.execute_batch(&format!("BEGIN; {sql}\n; PRAGMA user_version = {}; COMMIT;", i + 1))?;
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
