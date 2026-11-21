//! Sessions and what was said in them.
//!
//! A session is a conversation with one model. It lives in the core's own storage, so the window, the
//! command line and any program see the same conversation, and closing a window loses nothing.
//!
//! This file only keeps and returns. Running a turn (asking the model, streaming its answer) is in `chat`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::db::Db;
use crate::{Error, Result, now_ms};

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Session {
    pub id: String,
    pub title: String,
    /// The model this session talks to, by its name.
    pub model: String,
    /// Whether the model thinks before it answers: `off`, `on`, or one of the model's own levels.
    pub thinking: String,
    pub created_at: i64,
    /// When something was last said in it.
    pub updated_at: i64,
    /// How many messages it holds.
    pub messages: u32,
}

/// What the runtime reported about one answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Timings {
    /// Tokens of the prompt that did not have to be read again, because the start of the prompt had not changed.
    pub reused: u64,
    /// Tokens of the prompt that were read.
    pub read: u64,
    /// Tokens written.
    pub written: u64,
    /// Tokens written per second.
    pub per_second: f64,
    /// Milliseconds from asking to the last token.
    pub took_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    /// Its place in the session, from 1.
    pub seq: u32,
    /// `user` or `assistant`.
    pub role: String,
    pub content: String,
    /// The model's thinking, kept apart from its answer. `null` when it did not think.
    pub thought: Option<String>,
    /// `done`, `answering`, `stopped` (by the person) or `failed`.
    pub state: String,
    /// Why it failed, in words.
    pub problem: Option<String>,
    /// The model that wrote it. `null` for what the person wrote.
    pub model: Option<String>,
    pub timings: Option<Timings>,
    pub created_at: i64,
}

pub struct Sessions {
    db: Arc<Db>,
}

fn session_from(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session { id: r.get(0)?, title: r.get(1)?, model: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)?, messages: r.get(5)?, thinking: r.get(6)? })
}

fn message_from(r: &rusqlite::Row) -> rusqlite::Result<Message> {
    let timings: Option<String> = r.get(9)?;
    Ok(Message {
        id: r.get(0)?,
        session_id: r.get(1)?,
        seq: r.get(2)?,
        role: r.get(3)?,
        content: r.get(4)?,
        thought: r.get(5)?,
        state: r.get(6)?,
        problem: r.get(7)?,
        model: r.get(8)?,
        timings: timings.and_then(|t| serde_json::from_str(&t).ok()),
        created_at: r.get(10)?,
    })
}

const SESSION: &str = "SELECT s.id, s.title, s.model, s.created_at, s.updated_at, (SELECT count(*) FROM messages m WHERE m.session_id = s.id), s.thinking FROM sessions s";
const MESSAGE: &str = "SELECT id, session_id, seq, role, content, thought, state, problem, model, timings, created_at FROM messages";

/// A title from the first thing said: its first line, cut at a word.
pub fn title_from(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New session");
    if line.chars().count() <= 60 {
        return line.to_string();
    }
    let cut: String = line.chars().take(60).collect();
    format!("{}…", cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head).trim_end_matches([',', '.', ';', ':']))
}

impl Sessions {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    fn missing(id: &str) -> Error {
        Error::NotFound(format!("There is no session \"{id}\". `demido sessions` lists the ones there are."))
    }

    pub fn create(&self, title: Option<&str>, model: &str) -> Result<Session> {
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        let title = title.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("New session");
        self.db.with(|c| c.execute("INSERT INTO sessions (id, title, model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", rusqlite::params![id, title, model, now]))?;
        self.get(&id)
    }

    /// Every session, the one spoken in most recently first.
    pub fn list(&self) -> Result<Vec<Session>> {
        self.db.with(|c| c.prepare(&format!("{SESSION} ORDER BY s.updated_at DESC, s.id DESC"))?.query_map([], session_from)?.collect())
    }

    pub fn get(&self, id: &str) -> Result<Session> {
        self.db.with(|c| c.prepare(&format!("{SESSION} WHERE s.id = ?1"))?.query_map([id], session_from)?.next().transpose())?.ok_or_else(|| Self::missing(id))
    }

    /// Changes what is given: the title, the model, whether the model thinks first.
    pub fn change(&self, id: &str, title: Option<&str>, model: Option<&str>, thinking: Option<&str>) -> Result<Session> {
        self.get(id)?;
        self.db.with(|c| {
            if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
                c.execute("UPDATE sessions SET title = ?2 WHERE id = ?1", rusqlite::params![id, title])?;
            }
            if let Some(model) = model {
                c.execute("UPDATE sessions SET model = ?2 WHERE id = ?1", rusqlite::params![id, model])?;
            }
            if let Some(thinking) = thinking {
                c.execute("UPDATE sessions SET thinking = ?2 WHERE id = ?1", rusqlite::params![id, thinking])?;
            }
            Ok(())
        })?;
        self.get(id)
    }

    /// Deletes a session and everything said in it.
    pub fn delete(&self, id: &str) -> Result<Session> {
        let gone = self.get(id)?;
        self.db.with(|c| c.execute("DELETE FROM sessions WHERE id = ?1", [id]))?;
        Ok(gone)
    }

    /// What was said, in order.
    pub fn messages(&self, session_id: &str) -> Result<Vec<Message>> {
        self.db.with(|c| c.prepare(&format!("{MESSAGE} WHERE session_id = ?1 ORDER BY seq"))?.query_map([session_id], message_from)?.collect())
    }

    pub fn message(&self, id: &str) -> Result<Message> {
        self.db.with(|c| c.prepare(&format!("{MESSAGE} WHERE id = ?1"))?.query_map([id], message_from)?.next().transpose())?.ok_or_else(|| Error::NotFound(format!("There is no message \"{id}\".")))
    }

    /// Adds a message at the end of a session.
    pub fn add(&self, session_id: &str, role: &str, content: &str, state: &str, model: Option<&str>) -> Result<Message> {
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        self.db.with(|c| {
            c.execute(
                "INSERT INTO messages (id, session_id, seq, role, content, state, model, created_at)
                 VALUES (?1, ?2, (SELECT coalesce(max(seq), 0) + 1 FROM messages WHERE session_id = ?2), ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, session_id, role, content, state, model, now],
            )?;
            c.execute("UPDATE sessions SET updated_at = ?2 WHERE id = ?1", rusqlite::params![session_id, now])
        })?;
        self.message(&id)
    }

    /// Writes down how an answer ended: what it says, what the model thought, and how it went.
    pub fn finish(&self, id: &str, content: &str, thought: Option<&str>, state: &str, problem: Option<&str>, timings: Option<&Timings>) -> Result<Message> {
        let timings: Option<Value> = timings.map(serde_json::to_value).transpose()?;
        self.db.with(|c| {
            c.execute(
                "UPDATE messages SET content = ?2, thought = ?3, state = ?4, problem = ?5, timings = ?6 WHERE id = ?1",
                rusqlite::params![id, content, thought.filter(|t| !t.trim().is_empty()), state, problem, timings.map(|t| t.to_string())],
            )
        })?;
        self.message(id)
    }

    /// An answer that was being written when Demido stopped was never finished. Say so, once, when Demido starts.
    pub fn close_unfinished(&self) -> Result<usize> {
        self.db.with(|c| c.execute("UPDATE messages SET state = 'stopped', problem = 'Demido was stopped while this was being written.' WHERE state = 'answering'", []))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Sessions {
        Sessions::new(Arc::new(Db::memory().unwrap()))
    }

    #[test]
    fn a_session_keeps_what_was_said_in_order() {
        let s = fresh();
        let a = s.create(None, "small").unwrap();
        assert_eq!((a.title.as_str(), a.model.as_str(), a.messages), ("New session", "small", 0));
        let q = s.add(&a.id, "user", "Name three colours.", "done", None).unwrap();
        let r = s.add(&a.id, "assistant", "", "answering", Some("small")).unwrap();
        assert_eq!((q.seq, r.seq, r.state.as_str()), (1, 2, "answering"));
        let t = Timings { reused: 27, read: 25, written: 11, per_second: 235.5, took_ms: 62 };
        let done = s.finish(&r.id, "Red, blue, yellow.", Some("  "), "done", None, Some(&t)).unwrap();
        assert_eq!((done.content.as_str(), done.thought, done.state.as_str(), done.timings), ("Red, blue, yellow.", None, "done", Some(t)));
        let said = s.messages(&a.id).unwrap();
        assert_eq!(said.iter().map(|m| (m.role.as_str(), m.seq)).collect::<Vec<_>>(), [("user", 1), ("assistant", 2)]);
        assert_eq!(s.get(&a.id).unwrap().messages, 2);
    }

    #[test]
    fn sessions_are_listed_by_who_spoke_last_and_deleted_whole() {
        let s = fresh();
        let a = s.create(Some("First"), "small").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let b = s.create(Some("  Second  "), "small").unwrap();
        assert_eq!(s.list().unwrap().iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), ["Second", "First"]);
        std::thread::sleep(std::time::Duration::from_millis(3));
        s.add(&a.id, "user", "hello", "done", None).unwrap();
        assert_eq!(s.list().unwrap()[0].id, a.id, "speaking in a session brings it to the top");

        assert_eq!(s.change(&b.id, Some("Renamed"), Some("big"), None).unwrap().model, "big");
        assert_eq!(b.thinking, "off", "a session starts with thinking off");
        assert_eq!(s.change(&b.id, None, None, Some("on")).unwrap().thinking, "on");
        s.delete(&a.id).unwrap();
        assert!(s.messages(&a.id).unwrap().is_empty(), "its messages go with it");
        assert!(matches!(s.get(&a.id), Err(Error::NotFound(_))));
        assert!(matches!(s.delete("nope"), Err(Error::NotFound(_))));
    }

    #[test]
    fn an_answer_cut_off_by_a_stop_of_demido_is_marked() {
        let s = fresh();
        let a = s.create(None, "small").unwrap();
        let r = s.add(&a.id, "assistant", "Half an ans", "answering", Some("small")).unwrap();
        assert_eq!(s.close_unfinished().unwrap(), 1);
        let after = s.message(&r.id).unwrap();
        assert_eq!((after.state.as_str(), after.content.as_str()), ("stopped", "Half an ans"));
        assert!(after.problem.unwrap().contains("Demido was stopped"));
    }

    #[test]
    fn a_title_is_the_first_line_cut_at_a_word() {
        assert_eq!(title_from("Fix the login bug"), "Fix the login bug");
        assert_eq!(title_from("\n\n  Plan my week  \nwith details"), "Plan my week");
        let long = title_from("Explain in great detail how a transformer model works, including attention and embeddings");
        assert!(long.chars().count() <= 61 && long.ends_with('…') && !long.contains("  "), "{long}");
        assert_eq!(title_from("   "), "New session");
    }
}
