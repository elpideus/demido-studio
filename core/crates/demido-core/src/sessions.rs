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
use crate::tools::Step;
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
    /// The tools this answer used before it was written, in order. Empty for most messages.
    pub steps: Vec<Step>,
    pub created_at: i64,
}

/// One task of a piece of work that was split into steps. Demido keeps the list and walks it; the model
/// is only ever asked to do the task in hand.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Task {
    pub id: String,
    pub session_id: String,
    /// The answer this task is part of.
    pub message_id: String,
    /// The task this is a sub-task of. `null` for a task of the first level.
    pub parent_id: Option<String>,
    /// Its place among its siblings, from 1.
    pub position: u32,
    pub title: String,
    /// `pending`, `running`, `done`, `failed` or `stopped`. A task with sub-tasks is `running` while any of them is.
    pub state: String,
    /// What doing it gave, in short. `null` until it is done.
    pub result: Option<String>,
    /// The tools it used.
    pub steps: Vec<Step>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// An account of the older part of a session, which the model is given in place of those messages.
/// The messages themselves stay, for the person to read.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Summary {
    pub id: String,
    pub session_id: String,
    /// The messages up to and including this place in the session are covered by the account.
    pub upto_seq: u32,
    /// The account, as the model wrote it.
    pub content: String,
    /// `asked`: the person asked for it. `auto`: the conversation was getting too long for the model.
    pub why: String,
    /// About how many tokens the conversation was before.
    pub tokens_before: u64,
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
        steps: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or_default(),
    })
}

fn task_from(r: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        session_id: r.get(1)?,
        message_id: r.get(2)?,
        parent_id: r.get(3)?,
        position: r.get(4)?,
        title: r.get(5)?,
        state: r.get(6)?,
        result: r.get(7)?,
        steps: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const SESSION: &str = "SELECT s.id, s.title, s.model, s.created_at, s.updated_at, (SELECT count(*) FROM messages m WHERE m.session_id = s.id), s.thinking FROM sessions s";
const MESSAGE: &str = "SELECT id, session_id, seq, role, content, thought, state, problem, model, timings, created_at, steps FROM messages";
const TASK: &str = "SELECT id, session_id, message_id, parent_id, position, title, state, result, steps, created_at, updated_at FROM tasks";

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

    /// Writes down the tools an answer has used so far.
    pub fn set_steps(&self, message_id: &str, steps: &[Step]) -> Result<()> {
        let steps = serde_json::to_string(steps)?;
        self.db.with(|c| c.execute("UPDATE messages SET steps = ?2 WHERE id = ?1", rusqlite::params![message_id, steps]))?;
        Ok(())
    }

    /// Every task of a session, in the order they were made. Tasks of one answer, and sub-tasks of one task,
    /// follow their `position`.
    pub fn tasks(&self, session_id: &str) -> Result<Vec<Task>> {
        self.db.with(|c| c.prepare(&format!("{TASK} WHERE session_id = ?1 ORDER BY created_at, parent_id IS NOT NULL, position"))?.query_map([session_id], task_from)?.collect())
    }

    /// The tasks of one answer: those of the first level in order, each followed by its sub-tasks.
    pub fn tasks_of(&self, message_id: &str) -> Result<Vec<Task>> {
        let all: Vec<Task> = self.db.with(|c| c.prepare(&format!("{TASK} WHERE message_id = ?1 ORDER BY position"))?.query_map([message_id], task_from)?.collect())?;
        let mut out = Vec::with_capacity(all.len());
        for top in all.iter().filter(|t| t.parent_id.is_none()) {
            out.push(top.clone());
            out.extend(all.iter().filter(|t| t.parent_id.as_deref() == Some(top.id.as_str())).cloned());
        }
        Ok(out)
    }

    /// Splits work into tasks: under an answer, or under one of its tasks.
    pub fn add_tasks(&self, session_id: &str, message_id: &str, parent_id: Option<&str>, titles: &[String]) -> Result<Vec<Task>> {
        let now = now_ms();
        self.db.with(|c| {
            for (i, title) in titles.iter().enumerate() {
                c.execute(
                    "INSERT INTO tasks (id, session_id, message_id, parent_id, position, title, state, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7)",
                    rusqlite::params![uuid::Uuid::now_v7().to_string(), session_id, message_id, parent_id, i as u32 + 1, title.trim(), now],
                )?;
            }
            Ok(())
        })?;
        Ok(self.tasks_of(message_id)?.into_iter().filter(|t| t.parent_id.as_deref() == parent_id).collect())
    }

    /// Writes down how a task stands: its state, and what is given of its result and the tools it used.
    pub fn set_task(&self, id: &str, state: &str, result: Option<&str>, steps: Option<&[Step]>) -> Result<()> {
        let steps = steps.map(serde_json::to_string).transpose()?;
        self.db.with(|c| {
            c.execute("UPDATE tasks SET state = ?2, result = coalesce(?3, result), steps = coalesce(?4, steps), updated_at = ?5 WHERE id = ?1", rusqlite::params![id, state, result, steps, now_ms()])
        })?;
        Ok(())
    }

    /// The tasks of an answer that were being done are not being done any more: its work ended (`stopped`) or broke (`failed`).
    pub fn end_tasks(&self, message_id: &str, state: &str) -> Result<()> {
        self.db.with(|c| c.execute("UPDATE tasks SET state = ?2, updated_at = ?3 WHERE message_id = ?1 AND state = 'running'", rusqlite::params![message_id, state, now_ms()]))?;
        Ok(())
    }

    /// Every time this session was made shorter, oldest first. The last one is the one in force.
    pub fn summaries(&self, session_id: &str) -> Result<Vec<Summary>> {
        self.db.with(|c| {
            c.prepare("SELECT id, session_id, upto_seq, content, why, tokens_before, created_at FROM summaries WHERE session_id = ?1 ORDER BY upto_seq, created_at")?
                .query_map([session_id], |r| {
                    Ok(Summary {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        upto_seq: r.get(2)?,
                        content: r.get(3)?,
                        why: r.get(4)?,
                        tokens_before: r.get::<_, i64>(5)?.max(0) as u64,
                        created_at: r.get(6)?,
                    })
                })?
                .collect()
        })
    }

    /// Writes down an account that stands for the messages up to `upto_seq`.
    pub fn summarise(&self, session_id: &str, upto_seq: u32, content: &str, why: &str, tokens_before: u64) -> Result<Summary> {
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        self.db.with(|c| {
            c.execute(
                "INSERT INTO summaries (id, session_id, upto_seq, content, why, tokens_before, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, session_id, upto_seq, content, why, tokens_before as i64, now],
            )
        })?;
        Ok(Summary { id, session_id: session_id.to_string(), upto_seq, content: content.to_string(), why: why.to_string(), tokens_before, created_at: now })
    }

    /// Moves a session and everything said in it so far back to a moment in the past. For tests about time passing.
    #[cfg(test)]
    pub(crate) fn backdate(&self, id: &str, to_ms: i64) -> Result<()> {
        self.db.with(|c| {
            c.execute("UPDATE sessions SET created_at = ?2 WHERE id = ?1", rusqlite::params![id, to_ms])?;
            c.execute("UPDATE messages SET created_at = ?2 WHERE session_id = ?1", rusqlite::params![id, to_ms])
        })?;
        Ok(())
    }

    /// An answer that was being written when Demido stopped was never finished. Say so, once, when Demido starts.
    pub fn close_unfinished(&self) -> Result<usize> {
        self.db.with(|c| {
            c.execute("UPDATE tasks SET state = 'stopped' WHERE state = 'running'", [])?;
            c.execute("UPDATE messages SET state = 'stopped', problem = 'Demido was stopped while this was being written.' WHERE state = 'answering'", [])
        })
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
    fn an_account_stands_for_the_older_messages_and_goes_with_the_session() {
        let s = fresh();
        let a = s.create(None, "small").unwrap();
        for i in 0..6 {
            s.add(&a.id, if i % 2 == 0 { "user" } else { "assistant" }, &format!("message {i}"), "done", None).unwrap();
        }
        assert!(s.summaries(&a.id).unwrap().is_empty());
        s.summarise(&a.id, 2, "They greeted each other.", "asked", 900).unwrap();
        let later = s.summarise(&a.id, 4, "They greeted each other and agreed on a plan.", "auto", 1400).unwrap();
        let all = s.summaries(&a.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.last(), Some(&later), "the last one is the one in force");
        // The messages themselves are all still there.
        assert_eq!(s.messages(&a.id).unwrap().len(), 6);
        s.delete(&a.id).unwrap();
        assert!(s.summaries(&a.id).unwrap().is_empty());
    }

    #[test]
    fn work_is_split_into_tasks_and_sub_tasks_that_go_with_their_answer() {
        let s = fresh();
        let a = s.create(None, "small").unwrap();
        s.add(&a.id, "user", "Do three things.", "done", None).unwrap();
        let answer = s.add(&a.id, "assistant", "", "answering", Some("small")).unwrap();
        assert!(answer.steps.is_empty());
        let step = Step { tool: "calculate".into(), args: serde_json::json!({ "expression": "1+1" }), result: "2".into(), state: "done".into(), took_ms: 1 };
        s.set_steps(&answer.id, std::slice::from_ref(&step)).unwrap();
        assert_eq!(s.message(&answer.id).unwrap().steps, vec![step.clone()]);

        let tasks = s.add_tasks(&a.id, &answer.id, None, &["Read the file".into(), " Add the hours ".into(), "Tell the total".into()]).unwrap();
        assert_eq!(
            tasks.iter().map(|t| (t.position, t.title.as_str(), t.state.as_str())).collect::<Vec<_>>(),
            [(1, "Read the file", "pending"), (2, "Add the hours", "pending"), (3, "Tell the total", "pending")]
        );
        let subs = s.add_tasks(&a.id, &answer.id, Some(&tasks[1].id), &["Find the numbers".into(), "Add them".into()]).unwrap();
        assert_eq!(subs.len(), 2);
        s.set_task(&tasks[0].id, "done", Some("It lists 3 h, 2 h and 4.5 h."), Some(std::slice::from_ref(&step))).unwrap();
        s.set_task(&tasks[1].id, "running", None, None).unwrap();
        // In order: each task of the first level, followed by its sub-tasks.
        let all = s.tasks_of(&answer.id).unwrap();
        assert_eq!(all.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Read the file", "Add the hours", "Find the numbers", "Add them", "Tell the total"]);
        assert_eq!((all[0].state.as_str(), all[0].result.as_deref(), all[0].steps.len()), ("done", Some("It lists 3 h, 2 h and 4.5 h."), 1));
        assert_eq!(s.tasks(&a.id).unwrap().len(), 5);
        // A task that was running when Demido stopped is marked.
        s.close_unfinished().unwrap();
        assert_eq!(s.tasks_of(&answer.id).unwrap()[1].state, "stopped");
        s.delete(&a.id).unwrap();
        assert!(s.tasks(&a.id).unwrap().is_empty(), "tasks go with their session");
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
