//! The mail cache: folders, message summaries and opened bodies, in `cache/mail.db`.
//!
//! Each folder keeps what the last sync learned from the server (UIDVALIDITY, UIDNEXT,
//! HIGHESTMODSEQ, the message count) so the next sync asks only for what changed. Messages are
//! mirrored from the newest down to `low_uid`: every message on the server with a UID at or above
//! it is here, older ones arrive page by page as the list scrolls. Bodies are kept for the
//! messages opened, the least recently opened dropped first once they pass a size limit.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::Context;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::body::{Addr, Body, Header, Plan};

pub const SEEN: u8 = 1;
pub const FLAGGED: u8 = 2;
pub const ANSWERED: u8 = 4;
pub const DRAFT: u8 = 8;

/// Opened bodies kept, in bytes.
const MAX_BODY_BYTES: i64 = 256 * 1024 * 1024;

const MIGRATIONS: &[&str] = &[
    // 1: folders, message summaries, bodies
    r#"
    CREATE TABLE folders (
        account      TEXT NOT NULL,
        path         TEXT NOT NULL,
        name         TEXT NOT NULL,
        role         TEXT,
        delimiter    TEXT,
        selectable   INTEGER NOT NULL DEFAULT 1,
        depth        INTEGER NOT NULL DEFAULT 0,
        position     INTEGER NOT NULL DEFAULT 0,
        uidvalidity  INTEGER,
        uidnext      INTEGER,
        modseq       INTEGER,
        exists_count INTEGER,
        unseen       INTEGER,
        low_uid      INTEGER,
        complete     INTEGER NOT NULL DEFAULT 0,
        checked_at   INTEGER,
        PRIMARY KEY (account, path)
    );
    CREATE TABLE messages (
        id           INTEGER PRIMARY KEY,
        account      TEXT NOT NULL,
        folder       TEXT NOT NULL,
        uid          INTEGER NOT NULL,
        date         INTEGER NOT NULL,
        from_name    TEXT NOT NULL DEFAULT '',
        from_addr    TEXT NOT NULL DEFAULT '',
        recipients   TEXT NOT NULL DEFAULT '{}',
        subject      TEXT NOT NULL DEFAULT '',
        message_id   TEXT,
        flags        INTEGER NOT NULL DEFAULT 0,
        labels       TEXT NOT NULL DEFAULT '[]',
        size         INTEGER NOT NULL DEFAULT 0,
        attachments  INTEGER NOT NULL DEFAULT 0,
        snippet      TEXT NOT NULL DEFAULT '',
        plan         TEXT NOT NULL DEFAULT '{}',
        gm_msgid     INTEGER,
        modseq       INTEGER,
        UNIQUE (account, folder, uid)
    );
    CREATE INDEX messages_by_date ON messages(account, folder, date DESC, uid DESC);
    CREATE TABLE bodies (
        message    INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
        html       TEXT,
        text       TEXT NOT NULL,
        bytes      INTEGER NOT NULL,
        opened_at  INTEGER NOT NULL
    );
    CREATE INDEX bodies_by_age ON bodies(opened_at);
    "#,
    // 2: Gmail's copies of one message, in each of its labels' folders
    r#"
    CREATE INDEX messages_by_gm_msgid ON messages(account, gm_msgid);
    "#,
    // 3: message ids are never handed out twice. A model keeps the ids it was shown (in exports,
    // say), and after a move a reused id would name another message. The ids before this start
    // well past the highest, so ones already gone are not reused either.
    r#"
    CREATE TABLE messages_new (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        account      TEXT NOT NULL,
        folder       TEXT NOT NULL,
        uid          INTEGER NOT NULL,
        date         INTEGER NOT NULL,
        from_name    TEXT NOT NULL DEFAULT '',
        from_addr    TEXT NOT NULL DEFAULT '',
        recipients   TEXT NOT NULL DEFAULT '{}',
        subject      TEXT NOT NULL DEFAULT '',
        message_id   TEXT,
        flags        INTEGER NOT NULL DEFAULT 0,
        labels       TEXT NOT NULL DEFAULT '[]',
        size         INTEGER NOT NULL DEFAULT 0,
        attachments  INTEGER NOT NULL DEFAULT 0,
        snippet      TEXT NOT NULL DEFAULT '',
        plan         TEXT NOT NULL DEFAULT '{}',
        gm_msgid     INTEGER,
        modseq       INTEGER,
        UNIQUE (account, folder, uid)
    );
    INSERT INTO messages_new (id, account, folder, uid, date, from_name, from_addr, recipients, subject,
        message_id, flags, labels, size, attachments, snippet, plan, gm_msgid, modseq)
    SELECT id, account, folder, uid, date, from_name, from_addr, recipients, subject,
        message_id, flags, labels, size, attachments, snippet, plan, gm_msgid, modseq
    FROM messages;
    DROP TABLE messages;
    ALTER TABLE messages_new RENAME TO messages;
    CREATE INDEX messages_by_date ON messages(account, folder, date DESC, uid DESC);
    CREATE INDEX messages_by_gm_msgid ON messages(account, gm_msgid);
    DELETE FROM sqlite_sequence WHERE name = 'messages';
    INSERT INTO sqlite_sequence (name, seq) SELECT 'messages', COALESCE(MAX(id), 0) + 100000 FROM messages;
    "#,
];

/// A folder as the server lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
    pub role: Option<String>,
    pub delimiter: Option<String>,
    pub selectable: bool,
    pub depth: u32,
    pub position: i64,
}

/// What the last sync learned about a folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FolderState {
    pub uidvalidity: Option<u32>,
    pub uidnext: Option<u32>,
    pub modseq: Option<u64>,
    pub exists: Option<u32>,
    pub unseen: Option<u32>,
    /// Every message from this UID up is cached. None until the folder is first loaded.
    pub low_uid: Option<u32>,
    /// No messages older than `low_uid` remain on the server.
    pub complete: bool,
    pub checked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub path: String,
    pub name: String,
    pub role: Option<String>,
    pub selectable: bool,
    pub depth: u32,
    pub total: Option<u32>,
    pub unseen: Option<u32>,
}

/// A message's summary as a fetch returns it.
#[derive(Clone, Debug, Default)]
pub struct MessageMeta {
    pub uid: u32,
    pub date: i64,
    pub header: Header,
    pub flags: u8,
    pub labels: Vec<String>,
    pub size: u32,
    pub plan: Plan,
    pub gm_msgid: Option<u64>,
    pub modseq: Option<u64>,
}

/// A change to a cached message's flags or labels.
#[derive(Clone, Debug)]
pub struct FlagChange {
    pub uid: u32,
    pub flags: u8,
    pub labels: Option<Vec<String>>,
    pub modseq: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Recipients {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    to: Vec<Addr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cc: Vec<Addr>,
}

/// One row of the message list.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: i64,
    pub account: String,
    pub folder: String,
    pub uid: u32,
    pub date: i64,
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub snippet: String,
    pub unread: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
    pub attachments: u32,
    pub labels: Vec<String>,
    pub size: u32,
    pub message_id: Option<String>,
    #[serde(skip)]
    pub plan: Plan,
}

const SUMMARY_COLUMNS: &str = "id, account, folder, uid, date, from_name, from_addr, recipients, subject, snippet, \
                               flags, attachments, labels, size, message_id, plan";

fn summary(r: &rusqlite::Row<'_>) -> rusqlite::Result<Summary> {
    let recipients: Recipients = serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default();
    let flags: u8 = r.get(10)?;
    Ok(Summary {
        id: r.get(0)?,
        account: r.get(1)?,
        folder: r.get(2)?,
        uid: r.get(3)?,
        date: r.get(4)?,
        from: Addr {
            name: r.get(5)?,
            email: r.get(6)?,
        },
        to: recipients.to,
        cc: recipients.cc,
        subject: r.get(8)?,
        snippet: r.get(9)?,
        unread: flags & SEEN == 0,
        flagged: flags & FLAGGED != 0,
        answered: flags & ANSWERED != 0,
        draft: flags & DRAFT != 0,
        attachments: r.get(11)?,
        labels: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
        size: r.get(13)?,
        message_id: r.get(14)?,
        plan: serde_json::from_str(&r.get::<_, String>(15)?).unwrap_or_default(),
    })
}

/// A span of time in ms since the epoch, both ends included; an end left out is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct DateRange {
    pub since: Option<i64>,
    pub until: Option<i64>,
}

impl DateRange {
    pub fn contains(&self, ms: i64) -> bool {
        self.since.is_none_or(|s| ms >= s) && self.until.is_none_or(|u| ms <= u)
    }
}

/// Which messages a page shows.
#[derive(Clone, Debug, Default)]
pub struct PageQuery<'a> {
    pub limit: u32,
    /// Words that must all appear in the sender, subject or preview.
    pub filter: Option<&'a str>,
    pub unread_only: bool,
    /// When they arrived.
    pub dates: DateRange,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
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

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> anyhow::Result<T> {
        Ok(f(&self.conn.lock())?)
    }

    pub fn folders(&self, account: &str) -> anyhow::Result<Vec<FolderView>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT path, name, role, selectable, depth, exists_count, unseen FROM folders
                 WHERE account = ?1 ORDER BY position",
            )?;
            stmt.query_map([account], |r| {
                Ok(FolderView {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    role: r.get(2)?,
                    selectable: r.get(3)?,
                    depth: r.get(4)?,
                    total: r.get(5)?,
                    unseen: r.get(6)?,
                })
            })?
            .collect()
        })
    }

    /// Replaces the account's folder list, keeping what was synced of the folders that remain.
    /// Returns whether a folder was added or removed.
    pub fn set_folders(&self, account: &str, folders: &[FolderInfo]) -> anyhow::Result<bool> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let existing: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM folders WHERE account = ?1")?;
            stmt.query_map([account], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        let keep: HashSet<&str> = folders.iter().map(|f| f.path.as_str()).collect();
        let changed = existing.len() != keep.len() || existing.iter().any(|p| !keep.contains(p.as_str()));
        for gone in existing.iter().filter(|p| !keep.contains(p.as_str())) {
            tx.execute(
                "DELETE FROM folders WHERE account = ?1 AND path = ?2",
                params![account, gone],
            )?;
            tx.execute(
                "DELETE FROM messages WHERE account = ?1 AND folder = ?2",
                params![account, gone],
            )?;
        }
        for f in folders {
            tx.execute(
                "INSERT INTO folders (account, path, name, role, delimiter, selectable, depth, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(account, path) DO UPDATE SET name = excluded.name, role = excluded.role,
                   delimiter = excluded.delimiter, selectable = excluded.selectable, depth = excluded.depth,
                   position = excluded.position",
                params![
                    account,
                    f.path,
                    f.name,
                    f.role,
                    f.delimiter,
                    f.selectable,
                    f.depth,
                    f.position
                ],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The folder with `role` ("inbox", "all", "sent"…), or one whose path or name is `name`.
    pub fn find_folder(&self, account: &str, name: &str) -> anyhow::Result<Option<String>> {
        let lower = name.trim().to_lowercase();
        self.with(|c| {
            c.query_row(
                "SELECT path FROM folders WHERE account = ?1 AND selectable = 1 AND
                   (role = ?2 OR lower(path) = ?2 OR lower(name) = ?2)
                 ORDER BY (role = ?2) DESC, position LIMIT 1",
                params![account, lower],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn folder_state(&self, account: &str, path: &str) -> anyhow::Result<FolderState> {
        let state = self.with(|c| {
            c.query_row(
                "SELECT uidvalidity, uidnext, modseq, exists_count, unseen, low_uid, complete, checked_at
                 FROM folders WHERE account = ?1 AND path = ?2",
                params![account, path],
                |r| {
                    Ok(FolderState {
                        uidvalidity: r.get(0)?,
                        uidnext: r.get(1)?,
                        modseq: r.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                        exists: r.get(3)?,
                        unseen: r.get(4)?,
                        low_uid: r.get(5)?,
                        complete: r.get(6)?,
                        checked_at: r.get(7)?,
                    })
                },
            )
            .optional()
        })?;
        Ok(state.unwrap_or_default())
    }

    pub fn save_folder_state(&self, account: &str, path: &str, s: &FolderState) -> anyhow::Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO folders (account, path, name, uidvalidity, uidnext, modseq, exists_count, unseen,
                   low_uid, complete, checked_at)
                 VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(account, path) DO UPDATE SET uidvalidity = excluded.uidvalidity,
                   uidnext = excluded.uidnext, modseq = excluded.modseq, exists_count = excluded.exists_count,
                   unseen = excluded.unseen, low_uid = excluded.low_uid, complete = excluded.complete,
                   checked_at = excluded.checked_at",
                params![
                    account,
                    path,
                    s.uidvalidity,
                    s.uidnext,
                    s.modseq.map(|v| v as i64),
                    s.exists,
                    s.unseen,
                    s.low_uid,
                    s.complete,
                    s.checked_at
                ],
            )
            .map(|_| ())
        })
    }

    /// Forgets a folder's messages (its UIDVALIDITY changed, so its UIDs mean other messages now).
    pub fn clear_folder(&self, account: &str, path: &str) -> anyhow::Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM messages WHERE account = ?1 AND folder = ?2",
                params![account, path],
            )?;
            c.execute(
                "UPDATE folders SET uidvalidity = NULL, uidnext = NULL, modseq = NULL, exists_count = NULL,
                   unseen = NULL, low_uid = NULL, complete = 0, checked_at = NULL
                 WHERE account = ?1 AND path = ?2",
                params![account, path],
            )
            .map(|_| ())
        })
    }

    pub fn remove_account(&self, account: &str) -> anyhow::Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM messages WHERE account = ?1", [account])?;
            c.execute("DELETE FROM folders WHERE account = ?1", [account])
                .map(|_| ())
        })
    }

    /// Adds fetched messages, or updates the flags, labels and preview of ones already cached.
    pub fn upsert(&self, account: &str, folder: &str, list: &[MessageMeta]) -> anyhow::Result<()> {
        if list.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages (account, folder, uid, date, from_name, from_addr, recipients, subject,
                   message_id, flags, labels, size, attachments, snippet, plan, gm_msgid, modseq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT(account, folder, uid) DO UPDATE SET flags = excluded.flags, labels = excluded.labels,
                   modseq = excluded.modseq,
                   snippet = CASE WHEN messages.snippet = '' THEN excluded.snippet ELSE messages.snippet END",
            )?;
            for m in list {
                let from = m.header.from.clone().unwrap_or_default();
                let recipients = Recipients {
                    to: m.header.to.clone(),
                    cc: m.header.cc.clone(),
                };
                stmt.execute(params![
                    account,
                    folder,
                    m.uid,
                    m.date,
                    from.name,
                    from.email,
                    serde_json::to_string(&recipients).unwrap_or_default(),
                    m.header.subject,
                    m.header.message_id,
                    m.flags,
                    serde_json::to_string(&m.labels).unwrap_or_default(),
                    m.size,
                    m.plan.attachments.len() as u32,
                    m.header.snippet,
                    serde_json::to_string(&m.plan).unwrap_or_default(),
                    m.gm_msgid.map(|v| v as i64),
                    m.modseq.map(|v| v as i64),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Applies flag changes to cached messages. Returns whether any cached message changed.
    pub fn update_flags(&self, account: &str, folder: &str, changes: &[FlagChange]) -> anyhow::Result<bool> {
        if changes.is_empty() {
            return Ok(false);
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut changed = 0;
        {
            let mut stmt = tx.prepare(
                "UPDATE messages SET flags = ?4, labels = COALESCE(?5, labels), modseq = COALESCE(?6, modseq)
                 WHERE account = ?1 AND folder = ?2 AND uid = ?3
                   AND (flags != ?4 OR (?5 IS NOT NULL AND labels != ?5))",
            )?;
            for ch in changes {
                let labels = ch.labels.as_ref().map(|l| serde_json::to_string(l).unwrap_or_default());
                changed += stmt.execute(params![
                    account,
                    folder,
                    ch.uid,
                    ch.flags,
                    labels,
                    ch.modseq.map(|v| v as i64)
                ])?;
            }
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Sets (`on`) or clears a flag bit ([`SEEN`], [`FLAGGED`]…) on cached messages of `folder`,
    /// with their copies in other folders (on Gmail, the folders of the message's other labels).
    /// Marking read or unread moves each folder's unread count to match. Returns the folders whose
    /// messages changed, with how many changed in each.
    pub fn set_flag(
        &self,
        account: &str,
        folder: &str,
        uids: &[u32],
        bit: u8,
        on: bool,
    ) -> anyhow::Result<Vec<(String, u32)>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut turned: BTreeMap<String, u32> = BTreeMap::new();
        {
            let ids = with_copies(&tx, account, folder, uids)?;
            let sql = if on {
                format!(
                    "UPDATE messages SET flags = flags | {bit} WHERE id = ?1 AND (flags & {bit}) = 0 RETURNING folder"
                )
            } else {
                format!(
                    "UPDATE messages SET flags = flags & ~{bit} WHERE id = ?1 AND (flags & {bit}) != 0 RETURNING folder"
                )
            };
            let mut set = tx.prepare(&sql)?;
            for id in ids {
                if let Some(path) = set.query_row([id], |r| r.get::<_, String>(0)).optional()? {
                    *turned.entry(path).or_default() += 1;
                }
            }
            if bit == SEEN {
                // Read lowers the unread count, unread raises it.
                let sign = if on { -1 } else { 1 };
                let mut count = tx.prepare(
                    "UPDATE folders SET unseen = MAX(unseen + ?3, 0) WHERE account = ?1 AND path = ?2 AND unseen IS NOT NULL",
                )?;
                for (path, n) in &turned {
                    count.execute(params![account, path, sign * i64::from(*n)])?;
                }
            }
        }
        tx.commit()?;
        Ok(turned.into_iter().collect())
    }

    /// Marks cached messages read; see [`Store::set_flag`].
    #[cfg(test)]
    pub fn mark_seen(&self, account: &str, folder: &str, uids: &[u32]) -> anyhow::Result<Vec<(String, u32)>> {
        self.set_flag(account, folder, uids, SEEN, true)
    }

    /// Takes messages moved or deleted on the server out of the cache, and out of their folders'
    /// counts, so the next sync finds the cache matching the server. `copies` takes their copies
    /// in other folders too (on Gmail, a message trashed or deleted leaves every label). Returns
    /// the folders that changed.
    pub fn remove_messages(
        &self,
        account: &str,
        folder: &str,
        uids: &[u32],
        copies: bool,
    ) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut removed: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        {
            let ids = if copies {
                with_copies(&tx, account, folder, uids)?
            } else {
                let mut find = tx.prepare("SELECT id FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3")?;
                let mut ids = Vec::new();
                for uid in uids {
                    if let Some(id) = find.query_row(params![account, folder, uid], |r| r.get(0)).optional()? {
                        ids.push(id);
                    }
                }
                ids
            };
            let mut delete = tx.prepare("DELETE FROM messages WHERE id = ?1 RETURNING folder, flags")?;
            for id in ids {
                let gone = delete
                    .query_row([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u8>(1)?)))
                    .optional()?;
                if let Some((path, flags)) = gone {
                    let entry = removed.entry(path).or_default();
                    entry.0 += 1;
                    entry.1 += i64::from(flags & SEEN == 0);
                }
            }
            let mut count = tx.prepare(
                "UPDATE folders SET exists_count = MAX(exists_count - ?3, 0), unseen = MAX(unseen - ?4, 0)
                 WHERE account = ?1 AND path = ?2",
            )?;
            for (path, (all, unread)) in &removed {
                count.execute(params![account, path, all, unread])?;
            }
        }
        tx.commit()?;
        let mut folders: Vec<String> = removed.into_keys().collect();
        if !folders.iter().any(|f| f == folder) {
            folders.push(folder.to_string());
        }
        Ok(folders)
    }

    /// The folder is asked again the next time it is shown: messages were moved or copied into
    /// it, which only the server can number.
    pub fn mark_stale(&self, account: &str, path: &str) -> anyhow::Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE folders SET checked_at = NULL WHERE account = ?1 AND path = ?2",
                params![account, path],
            )
            .map(|_| ())
        })
    }

    /// The character the account's server puts between a folder and its subfolders.
    pub fn delimiter(&self, account: &str) -> anyhow::Result<Option<String>> {
        self.with(|c| {
            c.query_row(
                "SELECT delimiter FROM folders WHERE account = ?1 AND delimiter IS NOT NULL AND delimiter != '' LIMIT 1",
                [account],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// The folders holding the cached copies of a Gmail message (one for each of its labels).
    pub fn gmail_folders(&self, account: &str, gm_msgid: u64) -> anyhow::Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT folder FROM messages WHERE account = ?1 AND gm_msgid = ?2")?;
            let folders = stmt.query_map(params![account, gm_msgid as i64], |r| r.get(0))?;
            folders.collect()
        })
    }

    /// Gmail's ids of cached messages, for finding them again in another folder.
    pub fn gmail_ids(&self, ids: &[i64]) -> anyhow::Result<Vec<u64>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT gm_msgid FROM messages WHERE id = ?1 AND gm_msgid IS NOT NULL")?;
            let mut out = Vec::new();
            for id in ids {
                if let Some(gm) = stmt.query_row([id], |r| r.get::<_, i64>(0)).optional()? {
                    out.push(gm as u64);
                }
            }
            Ok(out)
        })
    }

    /// Cached UIDs at or above `from_uid`.
    pub fn uids_from(&self, account: &str, folder: &str, from_uid: u32) -> anyhow::Result<Vec<u32>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT uid FROM messages WHERE account = ?1 AND folder = ?2 AND uid >= ?3 ORDER BY uid")?;
            stmt.query_map(params![account, folder, from_uid], |r| r.get(0))?
                .collect()
        })
    }

    pub fn max_uid(&self, account: &str, folder: &str) -> anyhow::Result<Option<u32>> {
        self.with(|c| {
            c.query_row(
                "SELECT MAX(uid) FROM messages WHERE account = ?1 AND folder = ?2",
                params![account, folder],
                |r| r.get(0),
            )
        })
    }

    pub fn delete_uids(&self, account: &str, folder: &str, uids: &[u32]) -> anyhow::Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3")?;
            for uid in uids {
                stmt.execute(params![account, folder, uid])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The newest messages of a folder.
    pub fn page(&self, account: &str, folder: &str, q: &PageQuery<'_>) -> anyhow::Result<Vec<Summary>> {
        let mut sql = format!("SELECT {SUMMARY_COLUMNS} FROM messages WHERE account = ?1 AND folder = ?2");
        let mut args: Vec<rusqlite::types::Value> = vec![account.to_string().into(), folder.to_string().into()];
        for word in q.filter.unwrap_or_default().split_whitespace().take(8) {
            args.push(format!("%{}%", like_escape(word)).into());
            let n = args.len();
            sql.push_str(&format!(
                " AND (subject LIKE ?{n} ESCAPE '\\' OR from_name LIKE ?{n} ESCAPE '\\' OR from_addr LIKE ?{n} ESCAPE '\\'
                  OR snippet LIKE ?{n} ESCAPE '\\' OR recipients LIKE ?{n} ESCAPE '\\')"
            ));
        }
        if q.unread_only {
            sql.push_str(&format!(" AND (flags & {SEEN}) = 0"));
        }
        if let Some(since) = q.dates.since {
            sql.push_str(&format!(" AND date >= {since}"));
        }
        if let Some(until) = q.dates.until {
            sql.push_str(&format!(" AND date <= {until}"));
        }
        sql.push_str(&format!(" ORDER BY date DESC, uid DESC LIMIT {}", q.limit.max(1)));
        self.with(|c| {
            let mut stmt = c.prepare(&sql)?;
            stmt.query_map(rusqlite::params_from_iter(args), summary)?.collect()
        })
    }

    /// Cached messages of a folder, all of them or those at or above `from_uid`.
    pub fn count(&self, account: &str, folder: &str, from_uid: u32) -> anyhow::Result<u32> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM messages WHERE account = ?1 AND folder = ?2 AND uid >= ?3",
                params![account, folder, from_uid],
                |r| r.get(0),
            )
        })
    }

    pub fn summaries_by_uid(&self, account: &str, folder: &str, uids: &[u32]) -> anyhow::Result<Vec<Summary>> {
        let mut out = Vec::with_capacity(uids.len());
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {SUMMARY_COLUMNS} FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3"
            ))?;
            for uid in uids {
                if let Some(s) = stmt.query_row(params![account, folder, uid], summary).optional()? {
                    out.push(s);
                }
            }
            Ok(())
        })?;
        out.sort_by(|a, b| b.date.cmp(&a.date).then(b.uid.cmp(&a.uid)));
        Ok(out)
    }

    pub fn message(&self, id: i64) -> anyhow::Result<Option<Summary>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {SUMMARY_COLUMNS} FROM messages WHERE id = ?1"),
                [id],
                summary,
            )
            .optional()
        })
    }

    pub fn delete_message(&self, id: i64) -> anyhow::Result<()> {
        self.with(|c| c.execute("DELETE FROM messages WHERE id = ?1", [id]).map(|_| ()))
    }

    /// A cached body, marked as just opened. On Gmail a message is in the folder of each of its
    /// labels, and its body cached under any of them serves them all.
    pub fn body(&self, id: i64) -> anyhow::Result<Option<Body>> {
        let row = |r: &rusqlite::Row<'_>| {
            Ok((
                r.get::<_, i64>(0)?,
                Body {
                    html: r.get(1)?,
                    text: r.get(2)?,
                },
            ))
        };
        self.with(|c| {
            let mut found = c
                .query_row("SELECT message, html, text FROM bodies WHERE message = ?1", [id], row)
                .optional()?;
            if found.is_none() {
                found = c
                    .query_row(
                        "SELECT b.message, b.html, b.text FROM messages m
                         JOIN messages o ON o.account = m.account AND o.gm_msgid = m.gm_msgid AND o.id != m.id
                         JOIN bodies b ON b.message = o.id
                         WHERE m.id = ?1 AND m.gm_msgid IS NOT NULL
                         LIMIT 1",
                        [id],
                        row,
                    )
                    .optional()?;
            }
            if let Some((message, _)) = &found {
                c.execute(
                    "UPDATE bodies SET opened_at = ?2 WHERE message = ?1",
                    params![message, crate::db::now_ms()],
                )?;
            }
            Ok(found.map(|(_, body)| body))
        })
    }

    pub fn save_body(&self, id: i64, body: &Body) -> anyhow::Result<()> {
        self.save_bodies(&[(id, body)])
    }

    /// Caches the bodies of many messages at once, in one transaction.
    pub fn save_bodies(&self, bodies: &[(i64, &Body)]) -> anyhow::Result<()> {
        if bodies.is_empty() {
            return Ok(());
        }
        let now = crate::db::now_ms();
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            {
                let mut save = tx.prepare(
                    "INSERT OR REPLACE INTO bodies (message, html, text, bytes, opened_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                // A message first listed without a preview gets one from its body.
                let mut preview = tx.prepare("UPDATE messages SET snippet = ?2 WHERE id = ?1 AND snippet = ''")?;
                for (id, body) in bodies {
                    let bytes = body.html.as_ref().map_or(0, String::len) + body.text.len();
                    save.execute(params![id, body.html, body.text, bytes as i64, now])?;
                    preview.execute(params![id, super::body::snippet(&body.text)])?;
                }
            }
            tx.commit()
        })?;
        self.prune_bodies(MAX_BODY_BYTES)
    }

    fn prune_bodies(&self, max_bytes: i64) -> anyhow::Result<()> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM bodies", [], |r| r.get(0))?;
            if total <= max_bytes {
                return Ok(());
            }
            let mut excess = total - max_bytes * 9 / 10;
            let mut drop = Vec::new();
            {
                let mut stmt = c.prepare("SELECT message, bytes FROM bodies ORDER BY opened_at")?;
                let mut rows = stmt.query([])?;
                while excess > 0
                    && let Some(r) = rows.next()?
                {
                    drop.push(r.get::<_, i64>(0)?);
                    excess -= r.get::<_, i64>(1)?;
                }
            }
            for id in drop {
                c.execute("DELETE FROM bodies WHERE message = ?1", [id])?;
            }
            Ok(())
        })
    }
}

/// The cache ids of the messages of `folder` with these UIDs, and of their copies in other
/// folders (Gmail's labels, by X-GM-MSGID).
fn with_copies(tx: &Connection, account: &str, folder: &str, uids: &[u32]) -> rusqlite::Result<Vec<i64>> {
    let mut find = tx.prepare("SELECT id, gm_msgid FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3")?;
    let mut copies = tx.prepare("SELECT id FROM messages WHERE account = ?1 AND gm_msgid = ?2")?;
    let mut ids = Vec::new();
    for uid in uids {
        let found = find
            .query_row(params![account, folder, uid], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .optional()?;
        let Some((id, gm_msgid)) = found else { continue };
        ids.push(id);
        if let Some(gm) = gm_msgid {
            for copy in copies.query_map(params![account, gm], |r| r.get::<_, i64>(0))? {
                ids.push(copy?);
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let version: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= MIGRATIONS.len() {
        return Ok(());
    }
    // A migration that rebuilds a table drops the old one, which with foreign keys on would take
    // every opened body with it. The setting only changes outside a transaction.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let applied = (|| -> anyhow::Result<()> {
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)
                .with_context(|| format!("applying mail cache migration {}", i + 1))?;
            let broken = tx.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_some();
            anyhow::ensure!(!broken, "mail cache migration {} broke a reference", i + 1);
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", "ON")?;
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(uid: u32, date: i64, subject: &str) -> MessageMeta {
        MessageMeta {
            uid,
            date,
            header: Header {
                subject: subject.into(),
                from: Some(Addr {
                    name: "Ada".into(),
                    email: "ada@example.com".into(),
                }),
                snippet: format!("about {subject}"),
                ..Header::default()
            },
            ..MessageMeta::default()
        }
    }

    #[test]
    fn ids_are_never_handed_out_twice() {
        // A cache from before ids were kept unique, with an opened body.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..2] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 2).unwrap();
        let s = Store { conn: Mutex::new(conn) };
        s.upsert("a", "INBOX", &[meta(1, 10, "old"), meta(2, 20, "new")])
            .unwrap();
        let id = |s: &Store, folder, uid| s.summaries_by_uid("a", folder, &[uid]).unwrap()[0].id;
        let old = id(&s, "INBOX", 1);
        let body = Body {
            html: None,
            text: "hello".into(),
        };
        s.save_body(old, &body).unwrap();

        let conn = s.conn.into_inner();
        migrate(&conn).unwrap();
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        let s = Store { conn: Mutex::new(conn) };
        assert_eq!(id(&s, "INBOX", 1), old);
        assert_eq!(s.body(old).unwrap().map(|b| b.text).as_deref(), Some("hello"));

        // The newest message moves: its copy in the Trash does not get its id back.
        let newest = id(&s, "INBOX", 2);
        s.delete_uids("a", "INBOX", &[2]).unwrap();
        s.upsert("a", "[Gmail]/Trash", &[meta(7, 20, "new")]).unwrap();
        assert!(id(&s, "[Gmail]/Trash", 7) > newest + 1000);
        let moved = id(&s, "[Gmail]/Trash", 7);
        s.delete_uids("a", "[Gmail]/Trash", &[7]).unwrap();
        s.upsert("a", "INBOX", &[meta(3, 30, "next")]).unwrap();
        assert!(id(&s, "INBOX", 3) > moved);
    }

    #[test]
    fn pages_newest_first_and_filters_by_words() {
        let s = Store::in_memory();
        s.upsert(
            "a",
            "INBOX",
            &[
                meta(1, 10, "Invoice March"),
                meta(2, 30, "Lunch?"),
                meta(3, 20, "Invoice April"),
            ],
        )
        .unwrap();
        let all = s
            .page(
                "a",
                "INBOX",
                &PageQuery {
                    limit: 10,
                    ..PageQuery::default()
                },
            )
            .unwrap();
        assert_eq!(all.iter().map(|m| m.uid).collect::<Vec<_>>(), [2, 3, 1]);
        assert!(all[0].unread);
        let found = s
            .page(
                "a",
                "INBOX",
                &PageQuery {
                    limit: 10,
                    filter: Some("invoice apr"),
                    ..PageQuery::default()
                },
            )
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subject, "Invoice April");
        let dated = |since, until| {
            let q = PageQuery {
                limit: 10,
                dates: DateRange { since, until },
                ..PageQuery::default()
            };
            s.page("a", "INBOX", &q)
                .unwrap()
                .iter()
                .map(|m| m.uid)
                .collect::<Vec<_>>()
        };
        assert_eq!(dated(Some(20), None), [2, 3]);
        assert_eq!(dated(None, Some(20)), [3, 1]);
        assert_eq!(dated(Some(11), Some(29)), [3]);
        assert_eq!(s.count("a", "INBOX", 2).unwrap(), 2);
        assert_eq!(s.max_uid("a", "INBOX").unwrap(), Some(3));
    }

    #[test]
    fn flag_changes_report_only_real_changes() {
        let s = Store::in_memory();
        s.upsert("a", "INBOX", &[meta(1, 10, "x")]).unwrap();
        let seen = FlagChange {
            uid: 1,
            flags: SEEN,
            labels: None,
            modseq: Some(5),
        };
        assert!(s.update_flags("a", "INBOX", std::slice::from_ref(&seen)).unwrap());
        assert!(!s.update_flags("a", "INBOX", &[seen]).unwrap());
        let unread_only = PageQuery {
            limit: 10,
            unread_only: true,
            ..PageQuery::default()
        };
        assert!(s.page("a", "INBOX", &unread_only).unwrap().is_empty());
    }

    #[test]
    fn marking_read_reaches_the_copies_in_other_labels() {
        let s = Store::in_memory();
        let copy = |uid, gm_msgid| MessageMeta {
            gm_msgid: Some(gm_msgid),
            ..meta(uid, 10, "x")
        };
        s.upsert("a", "INBOX", &[copy(1, 100), copy(2, 200), meta(3, 10, "y")])
            .unwrap();
        s.upsert("a", "[Gmail]/All Mail", &[copy(51, 100), copy(52, 200)])
            .unwrap();
        for (path, unseen) in [("INBOX", 3), ("[Gmail]/All Mail", 9)] {
            let state = FolderState {
                unseen: Some(unseen),
                low_uid: Some(1),
                ..FolderState::default()
            };
            s.save_folder_state("a", path, &state).unwrap();
        }
        // UID 4 is not cached; 1 and 3 are, and 1 is also in All Mail.
        let turned = s.mark_seen("a", "INBOX", &[1, 3, 4]).unwrap();
        assert_eq!(turned, [("INBOX".to_string(), 2), ("[Gmail]/All Mail".to_string(), 1)]);
        assert_eq!(s.folder_state("a", "INBOX").unwrap().unseen, Some(1));
        assert_eq!(s.folder_state("a", "[Gmail]/All Mail").unwrap().unseen, Some(8));
        let unread = |path| {
            let q = PageQuery {
                limit: 10,
                unread_only: true,
                ..PageQuery::default()
            };
            s.page("a", path, &q).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>()
        };
        assert_eq!(unread("INBOX"), [2]);
        assert_eq!(unread("[Gmail]/All Mail"), [52]);
        // Read already: nothing changes, and the counts stay.
        assert!(s.mark_seen("a", "INBOX", &[1]).unwrap().is_empty());
        assert_eq!(s.folder_state("a", "INBOX").unwrap().unseen, Some(1));
        // Unread again, in both labels, and the counts go back up.
        let turned = s.set_flag("a", "INBOX", &[1], SEEN, false).unwrap();
        assert_eq!(turned, [("INBOX".to_string(), 1), ("[Gmail]/All Mail".to_string(), 1)]);
        assert_eq!(s.folder_state("a", "INBOX").unwrap().unseen, Some(2));
        assert_eq!(s.folder_state("a", "[Gmail]/All Mail").unwrap().unseen, Some(9));
    }

    #[test]
    fn starring_leaves_the_unread_counts_alone() {
        let s = Store::in_memory();
        s.upsert("a", "INBOX", &[meta(1, 10, "x")]).unwrap();
        let state = FolderState {
            unseen: Some(1),
            low_uid: Some(1),
            ..FolderState::default()
        };
        s.save_folder_state("a", "INBOX", &state).unwrap();
        assert_eq!(
            s.set_flag("a", "INBOX", &[1], FLAGGED, true).unwrap(),
            [("INBOX".to_string(), 1)]
        );
        let m = &s
            .page(
                "a",
                "INBOX",
                &PageQuery {
                    limit: 1,
                    ..PageQuery::default()
                },
            )
            .unwrap()[0];
        assert!(m.flagged && m.unread);
        assert_eq!(s.folder_state("a", "INBOX").unwrap().unseen, Some(1));
    }

    #[test]
    fn removed_messages_leave_the_cache_and_the_counts() {
        let s = Store::in_memory();
        let copy = |uid, gm_msgid| MessageMeta {
            gm_msgid: Some(gm_msgid),
            ..meta(uid, 10, "x")
        };
        s.upsert("a", "INBOX", &[copy(1, 100), copy(2, 200)]).unwrap();
        s.upsert("a", "Work", &[copy(7, 100)]).unwrap();
        for (path, exists) in [("INBOX", 2), ("Work", 1)] {
            let state = FolderState {
                exists: Some(exists),
                unseen: Some(exists),
                low_uid: Some(1),
                ..FolderState::default()
            };
            s.save_folder_state("a", path, &state).unwrap();
        }
        // Moved out of the inbox: its copy in Work stays.
        assert_eq!(s.remove_messages("a", "INBOX", &[1], false).unwrap(), ["INBOX"]);
        assert_eq!(s.folder_state("a", "INBOX").unwrap().exists, Some(1));
        assert_eq!(s.folder_state("a", "INBOX").unwrap().unseen, Some(1));
        assert_eq!(s.count("a", "Work", 0).unwrap(), 1);
        // Deleted: every copy goes.
        s.upsert("a", "INBOX", &[copy(1, 100)]).unwrap();
        let mut folders = s.remove_messages("a", "INBOX", &[1], true).unwrap();
        folders.sort();
        assert_eq!(folders, ["INBOX", "Work"]);
        assert_eq!(s.count("a", "Work", 0).unwrap(), 0);
        assert_eq!(s.folder_state("a", "Work").unwrap().exists, Some(0));
    }

    #[test]
    fn a_new_uidvalidity_forgets_the_folder() {
        let s = Store::in_memory();
        s.upsert("a", "INBOX", &[meta(1, 10, "x")]).unwrap();
        s.save_folder_state(
            "a",
            "INBOX",
            &FolderState {
                uidvalidity: Some(7),
                low_uid: Some(1),
                ..FolderState::default()
            },
        )
        .unwrap();
        s.clear_folder("a", "INBOX").unwrap();
        assert_eq!(s.count("a", "INBOX", 0).unwrap(), 0);
        assert_eq!(s.folder_state("a", "INBOX").unwrap(), FolderState::default());
    }

    #[test]
    fn bodies_go_with_their_message_and_fill_a_missing_preview() {
        let s = Store::in_memory();
        let mut m = meta(1, 10, "x");
        m.header.snippet.clear();
        s.upsert("a", "INBOX", &[m]).unwrap();
        let id = s
            .page(
                "a",
                "INBOX",
                &PageQuery {
                    limit: 1,
                    ..PageQuery::default()
                },
            )
            .unwrap()[0]
            .id;
        let body = Body {
            html: None,
            text: "Hello\nthere".into(),
        };
        s.save_body(id, &body).unwrap();
        assert_eq!(s.body(id).unwrap(), Some(body));
        assert_eq!(s.message(id).unwrap().unwrap().snippet, "Hello there");
        s.delete_uids("a", "INBOX", &[1]).unwrap();
        assert_eq!(s.body(id).unwrap(), None);
    }

    #[test]
    fn old_bodies_are_dropped_past_the_limit() {
        let s = Store::in_memory();
        s.upsert("a", "INBOX", &[meta(1, 10, "x"), meta(2, 20, "y")]).unwrap();
        let ids: Vec<i64> = s
            .page(
                "a",
                "INBOX",
                &PageQuery {
                    limit: 5,
                    ..PageQuery::default()
                },
            )
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        let big = Body {
            html: None,
            text: "z".repeat(600),
        };
        s.save_body(ids[1], &big).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.save_body(ids[0], &big).unwrap();
        s.prune_bodies(1000).unwrap();
        assert!(s.body(ids[1]).unwrap().is_none());
        assert!(s.body(ids[0]).unwrap().is_some());
    }

    #[test]
    fn folders_are_found_by_role_or_name() {
        let s = Store::in_memory();
        let f = |path: &str, role: Option<&str>, position| FolderInfo {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            role: role.map(Into::into),
            delimiter: Some("/".into()),
            selectable: true,
            depth: 0,
            position,
        };
        let list = [
            f("INBOX", Some("inbox"), 0),
            f("[Gmail]/Posta inviata", Some("sent"), 1),
            f("Work", None, 2),
        ];
        assert!(s.set_folders("a", &list).unwrap());
        assert!(!s.set_folders("a", &list).unwrap());
        assert_eq!(
            s.find_folder("a", "sent").unwrap().as_deref(),
            Some("[Gmail]/Posta inviata")
        );
        assert_eq!(s.find_folder("a", "work").unwrap().as_deref(), Some("Work"));
        assert_eq!(s.find_folder("a", "nope").unwrap(), None);
        s.upsert("a", "Work", &[meta(1, 1, "x")]).unwrap();
        // A label deleted on the server goes, with its messages.
        assert!(s.set_folders("a", &[f("INBOX", Some("inbox"), 0)]).unwrap());
        assert_eq!(s.count("a", "Work", 0).unwrap(), 0);
        assert_eq!(s.folders("a").unwrap().len(), 1);
    }
}
