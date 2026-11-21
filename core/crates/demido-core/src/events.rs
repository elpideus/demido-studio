//! The log of what happened.
//!
//! Every change the core makes is appended here and sent to whoever is listening. Nothing is ever edited.
//! The window uses it to stay current, the command line to follow along, and you to see what was done.
//! Each event has a number that only goes up, so a listener that reconnects can ask for "everything after".

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use utoipa::ToSchema;

use crate::db::Db;
use crate::{Result, now_ms};

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Event {
    /// Its place in the log. Only goes up.
    pub seq: i64,
    pub id: String,
    /// When, in milliseconds since 1970.
    pub at: i64,
    /// What happened, for example `setting.changed`.
    pub kind: String,
    /// What it is about, for example a setting's id.
    pub subject: Option<String>,
    #[schema(value_type = Object)]
    pub data: Value,
}

pub struct EventLog {
    db: Arc<Db>,
    live: broadcast::Sender<Event>,
}

impl EventLog {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db, live: broadcast::channel(256).0 }
    }

    /// Writes one event down and tells the listeners.
    pub fn append(&self, kind: &str, subject: Option<&str>, data: Value) -> Result<Event> {
        let (id, at, text) = (uuid::Uuid::now_v7().to_string(), now_ms(), data.to_string());
        let seq = self.db.with(|c| {
            c.execute("INSERT INTO events (id, at, kind, subject, data) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, at, kind, subject, text])?;
            Ok(c.last_insert_rowid())
        })?;
        let event = Event { seq, id, at, kind: kind.to_string(), subject: subject.map(str::to_string), data };
        let _ = self.live.send(event.clone()); // nobody listening is not an error
        Ok(event)
    }

    /// Up to `limit` events with a number greater than `seq`, oldest first.
    /// Says something to whoever is listening now, and keeps nothing. For progress: a download reports
    /// several times a second, and that is news, not history. Such an event has no number (`seq` 0) and no
    /// id, and a client that reconnects does not get it again.
    pub fn tell(&self, kind: &str, subject: Option<&str>, data: Value) {
        let _ = self.live.send(Event { seq: 0, id: String::new(), at: now_ms(), kind: kind.to_string(), subject: subject.map(str::to_string), data });
    }

    pub fn after(&self, seq: i64, limit: u32) -> Result<Vec<Event>> {
        self.db.with(|c| {
            let mut q = c.prepare("SELECT seq, id, at, kind, subject, data FROM events WHERE seq > ?1 ORDER BY seq LIMIT ?2")?;
            let rows = q.query_map(rusqlite::params![seq, limit], |r| {
                let data: String = r.get(5)?;
                Ok(Event { seq: r.get(0)?, id: r.get(1)?, at: r.get(2)?, kind: r.get(3)?, subject: r.get(4)?, data: serde_json::from_str(&data).unwrap_or(Value::Null) })
            })?;
            rows.collect()
        })
    }

    /// The newest `limit` events, oldest first.
    pub fn latest(&self, limit: u32) -> Result<Vec<Event>> {
        let top: i64 = self.db.with(|c| c.query_row("SELECT coalesce(max(seq), 0) FROM events", [], |r| r.get(0)))?;
        self.after((top - i64::from(limit)).max(0), limit)
    }

    pub fn listen(&self) -> broadcast::Receiver<Event> {
        self.live.subscribe()
    }

    /// Forgets events older than `days`. Returns how many went.
    pub fn forget_older_than(&self, days: u32) -> Result<usize> {
        let before = now_ms() - i64::from(days) * 86_400_000;
        self.db.with(|c| c.execute("DELETE FROM events WHERE at < ?1", [before]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_keep_their_order_and_can_be_read_from_any_point() {
        let log = EventLog::new(Arc::new(Db::memory().unwrap()));
        let a = log.append("test.one", None, json!({ "n": 1 })).unwrap();
        let b = log.append("test.two", Some("thing"), json!({ "n": 2 })).unwrap();
        assert!(b.seq > a.seq);
        let all = log.after(0, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].subject.as_deref(), Some("thing"));
        let later = log.after(a.seq, 10).unwrap();
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].data["n"], 2);
    }

    #[test]
    fn a_listener_hears_what_is_appended() {
        let log = EventLog::new(Arc::new(Db::memory().unwrap()));
        let mut ear = log.listen();
        log.append("test.heard", None, json!({})).unwrap();
        assert_eq!(ear.try_recv().unwrap().kind, "test.heard");
    }

    #[test]
    fn what_is_only_told_is_heard_and_not_kept() {
        let log = EventLog::new(Arc::new(Db::memory().unwrap()));
        let mut ear = log.listen();
        log.tell("test.progress", Some("thing"), json!({ "done": 5 }));
        let heard = ear.try_recv().unwrap();
        assert_eq!((heard.kind.as_str(), heard.seq), ("test.progress", 0));
        assert!(log.after(0, 10).unwrap().is_empty());
    }
}
