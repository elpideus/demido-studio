//! Running a turn: what the person wrote goes to the model, and its answer comes back piece by piece.
//!
//! The core runs the turn, not the window (DECISIONS L2). `say` writes down what was said, starts the answer
//! and returns at once. The answer is then written in the background: told piece by piece as news that is
//! not kept (`message.delta`), and stored whole when it is complete (`message.done`). A window that closes
//! mid-answer loses nothing, and one that opens late reads the stored message.
//!
//! The prompt always begins with the same instructions and then has the conversation, oldest first, only
//! ever added to (L4). The runtime keeps what it has read of a prompt, so an unchanged beginning is not read
//! again: each turn costs the new words, not the whole conversation.
//!
//! Not built yet: tools, the task list, making a long conversation shorter (compaction), a switch for
//! thinking (it is off: L6), settings per model.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::engine::{Engine, Piece};
use crate::events::EventLog;
use crate::sessions::{Message, Session, Sessions, title_from};
use crate::{Error, Result};

/// What every model is told first, in every session. The answer style of the brief (4.14): the answer
/// first, short, like a manual. Kept short itself, and it never changes within a session.
pub const INSTRUCTIONS: &str = "Answer first. Then numbered steps, only if steps are needed. One idea per sentence. Exact values with units. No filler, no praise. Do not repeat the question.";

/// Pieces of an answer are gathered and passed on about this often: often enough to read as writing,
/// not so often that the window does nothing else.
const TELL_EVERY: Duration = Duration::from_millis(33);

/// What `say` gives back at once: what the person said, and the answer that is now being written.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Turn {
    pub said: Message,
    /// Empty and `answering` at this moment. Its text arrives as `message.delta` news and is stored at `message.done`.
    pub answer: Message,
}

pub struct Chat {
    sessions: Arc<Sessions>,
    engine: Arc<Engine>,
    events: Arc<EventLog>,
    /// The sessions that are being answered right now, each with the switch that stops its answer.
    turns: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Chat {
    pub fn new(sessions: Arc<Sessions>, engine: Arc<Engine>, events: Arc<EventLog>) -> Self {
        Self { sessions, engine, events, turns: Mutex::default() }
    }

    fn turns(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.turns.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether this session is being answered right now.
    pub fn answering(&self, session_id: &str) -> bool {
        self.turns().contains_key(session_id)
    }

    /// Starts a session. With no model named, it takes one that is loaded; with none loaded, the first on disk.
    pub async fn start(&self, title: Option<&str>, model: Option<&str>) -> Result<Session> {
        let on_disk = self.engine.list().await?;
        let model = match model {
            Some(named) => on_disk
                .models
                .iter()
                .find(|m| m.id == named)
                .ok_or_else(|| Error::NotFound(format!("There is no model called \"{named}\" in {}. `demido models` lists what is there.", on_disk.folder)))?,
            None => on_disk
                .models
                .iter()
                .find(|m| m.state == "loaded")
                .or(on_disk.models.first())
                .ok_or_else(|| Error::Invalid(format!("There is no model to talk to yet. Put a GGUF file in {}.", on_disk.folder)))?,
        };
        let session = self.sessions.create(title, &model.id)?;
        self.events.append("session.created", Some(&session.id), json!({ "title": session.title, "model": session.model }))?;
        Ok(session)
    }

    /// Deletes a session and everything said in it. An answer being written in it is stopped first.
    pub fn delete(&self, session_id: &str) -> Result<Session> {
        self.stop(session_id);
        let gone = self.sessions.delete(session_id)?;
        self.events.append("session.deleted", Some(&gone.id), json!({ "title": gone.title }))?;
        Ok(gone)
    }

    /// Writes down what the person said and starts the answer. Returns at once; the answer follows as news.
    pub fn say(self: &Arc<Self>, session_id: &str, text: &str) -> Result<Turn> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("Nothing was written.".into()));
        }
        let session = self.sessions.get(session_id)?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut turns = self.turns();
            if turns.contains_key(session_id) {
                return Err(Error::Busy("This session is still writing its last answer. Wait for it, or stop it.".into()));
            }
            turns.insert(session_id.to_string(), stop.clone());
        }
        let written = (|| -> Result<Turn> {
            let said = self.sessions.add(session_id, "user", text, "done", None)?;
            if session.messages == 0 && session.title == "New session" {
                self.sessions.change(session_id, Some(&title_from(text)), None)?;
            }
            let answer = self.sessions.add(session_id, "assistant", "", "answering", Some(&session.model))?;
            self.events.append("message.added", Some(session_id), json!({ "message": said.id, "answer": answer.id }))?;
            Ok(Turn { said, answer })
        })();
        let turn = match written {
            Ok(turn) => turn,
            Err(e) => {
                self.turns().remove(session_id);
                return Err(e);
            }
        };
        let (me, answer_id) = (self.clone(), turn.answer.id.clone());
        tokio::spawn(async move { me.write_answer(session, answer_id, stop).await });
        Ok(turn)
    }

    /// Stops the answer being written in a session. What was written so far is kept. False if none was.
    pub fn stop(&self, session_id: &str) -> bool {
        match self.turns().get(session_id) {
            Some(switch) => {
                switch.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// The prompt for the next answer: the instructions, then what was said, oldest first. Nothing that was
    /// in an earlier prompt is ever changed or left out later, so the runtime never reads it twice.
    fn prompt(&self, session: &Session, answer_id: &str) -> Result<Vec<Value>> {
        let mut messages = vec![json!({ "role": "system", "content": INSTRUCTIONS })];
        for m in self.sessions.messages(&session.id)? {
            // The answer about to be written is not part of its own question. An answer that failed said nothing.
            if m.id == answer_id || (m.role == "assistant" && m.content.trim().is_empty()) {
                continue;
            }
            messages.push(json!({ "role": m.role, "content": m.content }));
        }
        Ok(messages)
    }

    async fn write_answer(self: Arc<Self>, session: Session, answer_id: String, stop: Arc<AtomicBool>) {
        // Gathered as they arrive, told every few tens of milliseconds.
        let mut waiting = (String::new(), String::new());
        let mut last_told = Instant::now();
        let tell = |waiting: &mut (String, String)| {
            if !waiting.0.is_empty() || !waiting.1.is_empty() {
                self.events.tell("message.delta", Some(&session.id), json!({ "message": answer_id, "text": waiting.0, "thought": waiting.1 }));
                waiting.0.clear();
                waiting.1.clear();
            }
        };
        let outcome = async {
            // The model is loaded if it is not: its loading is told by the engine's own news.
            self.engine.load(&session.model).await?;
            let body = json!({
                "model": session.model,
                "messages": self.prompt(&session, &answer_id)?,
                "stream": true,
                "stream_options": { "include_usage": true },
                // Off until there is a switch for it (DECISIONS L6).
                "chat_template_kwargs": { "enable_thinking": false },
            });
            let mut heard = |piece: Piece| {
                match piece {
                    Piece::Text(text) => waiting.0.push_str(text),
                    Piece::Thought(text) => waiting.1.push_str(text),
                }
                if last_told.elapsed() >= TELL_EVERY {
                    last_told = Instant::now();
                    tell(&mut waiting);
                }
            };
            self.engine.answer(&session.model, body, &stop, &mut heard).await
        }
        .await;
        tell(&mut waiting);
        self.turns().remove(&session.id);
        let ended = match outcome {
            Ok(answer) => {
                let state = if answer.stopped { "stopped" } else { "done" };
                self.sessions.finish(&answer_id, &answer.content, Some(&answer.thought), state, None, Some(&answer.timings)).map(|_| {
                    (
                        if answer.stopped { "message.stopped" } else { "message.done" },
                        json!({ "message": answer_id, "written": answer.timings.written, "per_second": answer.timings.per_second, "reused": answer.timings.reused, "read": answer.timings.read }),
                    )
                })
            }
            Err(why) => self.sessions.finish(&answer_id, "", None, "failed", Some(&why.to_string()), None).map(|_| ("message.failed", json!({ "message": answer_id, "problem": why.to_string() }))),
        };
        match ended {
            Ok((kind, data)) => {
                let _ = self.events.append(kind, Some(&session.id), data);
            }
            // The session was deleted while its answer was being written: there is nowhere to write it.
            Err(e) => tracing::debug!("An answer could not be stored: {e}"),
        }
    }
}

#[cfg(test)]
mod tests;
