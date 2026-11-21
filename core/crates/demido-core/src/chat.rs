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
//! Thinking (brief 4.9) is a choice of the session: off, on, or one of the model's own levels, for the models
//! the runtime says can think. It is off unless the person turns it on (L6): measured, it costs seconds and
//! on easy requests buys nothing. A thought that runs long can be ended from outside ("Answer now"), and
//! the model then goes on to its answer.
//!
//! A conversation that nears what the model can hold is made shorter (brief 4.15): the model writes a short
//! account of the older part, and from then on it is given that account in place of those messages. The
//! messages stay in the session for the person to read. It happens by itself before a turn that would not
//! fit comfortably, and by hand whenever the person asks. How long a conversation is, is not asked of
//! anyone: the runtime said how many tokens the last turn was, and that is counted on from.
//!
//! Before an answer is written, the model is asked whether it needs a tool, and work can be split into
//! tasks that Demido walks one by one. That part is in `turn.rs`, with what was measured to shape it.
//!
//! Not built yet: tools that reach outside Demido (files, commands, the network), settings per model.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::engine::{Answer, Engine, Piece};
use crate::events::EventLog;
use crate::sessions::{Message, Session, Sessions, Summary, title_from};
use crate::settings::Settings;
use crate::tools;
use crate::{Error, Result};

mod turn;

/// What every model is told first, in every session. The answer style of the brief (4.14): the answer
/// first, short, like a manual. Kept short itself, and it never changes within a session.
pub const INSTRUCTIONS: &str = "Answer first. Then numbered steps, only if steps are needed. One idea per sentence. Exact values with units. No filler, no praise. Do not repeat the question.";

/// What the model is told when it writes the account that will stand for the older part of a conversation.
///
/// The order is the point. The conversation comes first, fenced off as material; what to do with it comes
/// after it. Measured on the two test models (`core/dev/account-trial.py`, research note 13): told what to do
/// first and then shown the conversation, they went on with the conversation and kept 1 and 3 of 15 facts.
/// Told last, they kept 13 and 14.
pub const ACCOUNT_ROLE: &str = "You write notes on conversations. You never continue or answer them.";
pub const ACCOUNT_INSTRUCTIONS: &str = "Above is a conversation between a person and a model. Do not continue it and do not answer it. Write notes on it for someone who takes over: who the person is, what they want, every fact and number they gave, what was decided, and what is still open. Short lines.";

/// An account may be this long, in tokens.
const ACCOUNT_AT_MOST: u64 = 400;
/// When a conversation is made shorter, the newest messages stay word for word: about this share of the
/// size at which it is made shorter.
const KEPT_WORD_FOR_WORD: f64 = 0.4;

/// How much of what the model can hold a session is using.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ContextUse {
    /// About how many tokens the next prompt will be. From the runtime's own count of the last turn, plus
    /// an estimate for what was said since.
    pub used: u64,
    /// How many tokens the model is loaded with room for.
    pub limit: u64,
    /// At this many, the older part of the conversation is replaced by an account.
    pub compact_at: u64,
    /// Whether that happens by itself.
    pub auto: bool,
}

/// About how many tokens a text is, when nothing has counted it. On the generous side.
fn roughly(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(3)
}

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
    settings: Arc<Settings>,
    /// What time and what day it is. A test gives its own.
    clock: tools::Clock,
    /// The sessions that are being answered right now.
    turns: Mutex<HashMap<String, Running>>,
}

/// An answer that is being written.
#[derive(Clone)]
struct Running {
    /// Set to stop the answer.
    stop: Arc<AtomicBool>,
    /// The runtime's own name for the answer, once it has said it. A thought is ended by it.
    named: Arc<Mutex<Option<String>>>,
}

impl Chat {
    pub fn new(sessions: Arc<Sessions>, engine: Arc<Engine>, events: Arc<EventLog>, settings: Arc<Settings>) -> Self {
        Self { sessions, engine, events, settings, clock: tools::here, turns: Mutex::default() }
    }

    /// The same, telling the time and the day from `clock`: for tests.
    pub fn with_clock(mut self, clock: tools::Clock) -> Self {
        self.clock = clock;
        self
    }

    /// What the model is loaded with room for, and the size at which a conversation is made shorter.
    fn limits(&self) -> (u64, u64) {
        let limit = self.settings.value::<String>("models.context").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(8192);
        // Three quarters, so there is always room for the answer. A size chosen by hand never goes above that.
        let most = limit * 3 / 4;
        let chosen = self.settings.value::<String>("context.compactAt").ok().and_then(|v| v.parse::<u64>().ok());
        (limit, chosen.map_or(most, |c| c.min(most)))
    }

    /// The messages the model is still given word for word: the ones after the last account.
    fn in_view(&self, session_id: &str) -> Result<(Option<Summary>, Vec<Message>)> {
        let account = self.sessions.summaries(session_id)?.pop();
        let after = account.as_ref().map_or(0, |a| a.upto_seq);
        Ok((account, self.sessions.messages(session_id)?.into_iter().filter(|m| m.seq > after).collect()))
    }

    /// About how many tokens the next prompt of a session will be.
    fn tokens_in_view(account: &Option<Summary>, messages: &[Message]) -> u64 {
        // The last answer the runtime timed says exactly how long the conversation was up to and including it,
        // as long as nothing was made shorter since. From there on it is an estimate.
        let counted = messages.iter().rposition(|m| m.timings.is_some());
        match counted {
            Some(at) => {
                let t = messages[at].timings.as_ref().expect("just found");
                t.reused + t.read + t.written + messages[at + 1..].iter().map(|m| roughly(&m.content) + 4).sum::<u64>()
            }
            None => roughly(INSTRUCTIONS) + account.as_ref().map_or(0, |a| roughly(&a.content)) + messages.iter().map(|m| roughly(&m.content) + 4).sum::<u64>(),
        }
    }

    /// How much of what the model can hold this session is using.
    pub fn context_use(&self, session_id: &str) -> Result<ContextUse> {
        let (account, messages) = self.in_view(session_id)?;
        let (limit, compact_at) = self.limits();
        // An account made after the last timed answer makes that count too high: estimate from scratch then.
        let stale = account.as_ref().is_some_and(|a| messages.iter().all(|m| m.timings.is_none() || m.created_at < a.created_at));
        let used = if stale {
            Self::tokens_in_view(
                &account,
                &messages
                    .iter()
                    .cloned()
                    .map(|mut m| {
                        m.timings = None;
                        m
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            Self::tokens_in_view(&account, &messages)
        };
        Ok(ContextUse { used, limit, compact_at, auto: self.settings.value::<bool>("context.autoCompact").unwrap_or(true) })
    }

    /// Makes a session shorter for the model: writes an account of its older part and gives the model that
    /// account from now on, in place of those messages. The messages stay. `why` is `asked` or `auto`.
    pub async fn compact(&self, session_id: &str, why: &str) -> Result<Summary> {
        if self.answering(session_id) {
            return Err(Error::Busy("This session is writing an answer. Wait for it, or stop it, and then make it shorter.".into()));
        }
        let session = self.sessions.get(session_id)?;
        self.account_of(&session, why, None).await
    }

    /// Writes the account. `still_to_answer` is the answer about to be written, when this happens inside a turn.
    async fn account_of(&self, session: &Session, why: &str, still_to_answer: Option<&str>) -> Result<Summary> {
        let (before, messages) = self.in_view(&session.id)?;
        let said: Vec<&Message> = messages.iter().filter(|m| Some(m.id.as_str()) != still_to_answer && !(m.role == "assistant" && m.content.trim().is_empty())).collect();
        let (_, compact_at) = self.limits();
        let tokens_before = self.context_use(&session.id)?.used;
        // The newest messages stay word for word, and never fewer than the last thing the person said and, if
        // it was answered, its answer. When it happens by itself, as many more as fit in a share of the
        // size. When the person asks, only those two: they asked for it to be as short as it gets.
        let room = if why == "asked" { 0 } else { (compact_at as f64 * KEPT_WORD_FOR_WORD) as u64 };
        let (mut keep, mut kept_tokens) = (0, 0);
        for m in said.iter().rev() {
            let size = roughly(&m.content) + 4;
            if keep >= 2 && kept_tokens + size > room {
                break;
            }
            keep += 1;
            kept_tokens += size;
        }
        let older = &said[..said.len().saturating_sub(keep.max(1))];
        let Some(last_covered) = older.last() else {
            return Err(Error::Invalid("There is nothing to make shorter yet: the conversation is only a few messages long.".into()));
        };
        self.events.tell("session.compacting", Some(&session.id), json!({ "why": why, "messages": older.len() }));
        let mut transcript = String::new();
        if let Some(earlier) = &before {
            transcript.push_str(&format!("Notes on what came before:\n{}\n\n", earlier.content.trim()));
        }
        for m in older {
            transcript.push_str(&format!("{}: {}\n\n", if m.role == "user" { "Person" } else { "Model" }, m.content.trim()));
        }
        // The material first, fenced. What to do with it, last.
        let asked = format!("<conversation>\n{}\n</conversation>\n\n{ACCOUNT_INSTRUCTIONS}", transcript.trim_end());
        self.engine.load(&session.model).await?;
        let body = json!({
            "model": session.model,
            "messages": [{ "role": "system", "content": ACCOUNT_ROLE }, { "role": "user", "content": asked }],
            "stream": true,
            "max_tokens": ACCOUNT_AT_MOST,
            "temperature": 0.2,
            "chat_template_kwargs": { "enable_thinking": false },
            "reasoning_effort": "none",
        });
        let written = self.engine.answer(&session.model, body, &AtomicBool::new(false), &mut |_| {}).await?;
        let account = written.content.trim();
        if account.is_empty() {
            return Err(Error::Other("The model wrote nothing when asked for an account of the conversation, so nothing was made shorter.".into()));
        }
        let summary = self.sessions.summarise(&session.id, last_covered.seq, account, why, tokens_before)?;
        self.events.append(
            "session.compacted",
            Some(&session.id),
            json!({ "summary": summary.id, "why": why, "upto_seq": summary.upto_seq, "messages": older.len(), "tokens_before": tokens_before }),
        )?;
        Ok(summary)
    }

    fn turns(&self) -> std::sync::MutexGuard<'_, HashMap<String, Running>> {
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
        self.say_with(session_id, text, false)
    }

    /// The same. With `plan`, the model is first asked for the steps of the request, and Demido then has it
    /// do them one at a time.
    pub fn say_with(self: &Arc<Self>, session_id: &str, text: &str, plan: bool) -> Result<Turn> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("Nothing was written.".into()));
        }
        let session = self.sessions.get(session_id)?;
        let running = Running { stop: Arc::new(AtomicBool::new(false)), named: Arc::default() };
        {
            let mut turns = self.turns();
            if turns.contains_key(session_id) {
                return Err(Error::Busy("This session is still writing its last answer. Wait for it, or stop it.".into()));
            }
            turns.insert(session_id.to_string(), running.clone());
        }
        let written = (|| -> Result<Turn> {
            let said = self.sessions.add(session_id, "user", text, "done", None)?;
            if session.messages == 0 && session.title == "New session" {
                self.sessions.change(session_id, Some(&title_from(text)), None, None)?;
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
        tokio::spawn(async move { me.write_answer(session, answer_id, running, plan).await });
        Ok(turn)
    }

    /// What the model of a session is told first, word for word: what day it is, and how to answer. A model
    /// does not know the date, and does not know that it does not: asked how long until Christmas, one made
    /// a date up and counted from that. The day is the day the session started, so these words never
    /// change, and the runtime reads them once. The date comes first and the way to answer last: with the
    /// date last, one model began its answers with it.
    pub fn told_first(&self, session: &Session) -> String {
        format!("Today's date: {}.\n\n{INSTRUCTIONS}", (self.clock)(session.created_at).day)
    }

    /// What a model is shown before each answer when it may use tools, word for word. `None` when tools are off.
    pub fn tools_told(&self) -> Option<String> {
        self.settings.value::<bool>("tools.use").unwrap_or(true).then(turn::told_about_tools)
    }

    /// Changes a session: its title, its model, whether the model thinks first. A model must be one that is
    /// on this computer; a way of thinking must be one the model has.
    pub async fn change(&self, session_id: &str, title: Option<&str>, model: Option<&str>, thinking: Option<&str>) -> Result<Session> {
        let before = self.sessions.get(session_id)?;
        let models = self.engine.list().await?.models;
        if let Some(named) = model
            && !models.iter().any(|m| m.id == named)
        {
            return Err(Error::NotFound(format!("There is no model called \"{named}\" on this computer.")));
        }
        if let Some(wanted) = thinking.filter(|t| *t != "off") {
            // What the session's model (the new one, if it is being changed) can be set to. Not known until
            // the model has been loaded once; then "on" is taken on trust and the runtime decides.
            let choices = models.iter().find(|m| m.id == model.unwrap_or(&before.model)).and_then(|m| m.thinking.clone());
            match choices {
                Some(can) if can.is_empty() => return Err(Error::Invalid(format!("{} does not think before it answers, so there is nothing to turn on.", model.unwrap_or(&before.model)))),
                Some(can) if !can.iter().any(|c| c == wanted) => return Err(Error::Invalid(format!("Thinking can be: off, {}. \"{wanted}\" is not one of them.", can.join(", ")))),
                None if wanted != "on" => return Err(Error::Invalid("Until this model has been loaded once, thinking can only be off or on.".into())),
                _ => {}
            }
        }
        let changed = self.sessions.change(session_id, title, model, thinking)?;
        self.events.append("session.changed", Some(session_id), json!({ "title": changed.title, "model": changed.model, "thinking": changed.thinking }))?;
        Ok(changed)
    }

    /// Ends the thought the model is in, so that it goes on to its answer. False when it was not thinking.
    pub async fn answer_now(&self, session_id: &str) -> Result<bool> {
        let named = self.turns().get(session_id).and_then(|r| r.named.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let Some(named) = named else { return Ok(false) };
        let session = self.sessions.get(session_id)?;
        self.engine.end_thought(&session.model, &named).await
    }

    /// Stops the answer being written in a session. What was written so far is kept. False if none was.
    pub fn stop(&self, session_id: &str) -> bool {
        match self.turns().get(session_id) {
            Some(running) => {
                running.stop.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// What comes before the request of a turn: what the model is told first, then what was said, oldest
    /// first. And the request itself: the last thing the person said. Nothing that was in an earlier prompt
    /// is changed or left out later, so the runtime never reads it twice. The one exception is when the
    /// conversation is made shorter: then the older messages give way to an account of them, once, and the
    /// prompt is stable again from there.
    fn before(&self, session: &Session, answer_id: &str) -> Result<(Vec<Value>, String)> {
        let (account, in_view) = self.in_view(&session.id)?;
        let told = match &account {
            Some(account) => format!("{}\n\nEarlier in this conversation, in short:\n{}", self.told_first(session), account.content),
            None => self.told_first(session),
        };
        let mut messages = vec![json!({ "role": "system", "content": told })];
        // The day is said once, at the start. When the person speaks again on a later day, that is said too.
        // From what is stored, so it reads the same in every later prompt.
        let mut day = (self.clock)(session.created_at).date;
        let mut dated = |m: &Message| {
            let at = (self.clock)(m.created_at);
            if at.date == day {
                return m.content.clone();
            }
            day = at.date;
            format!("Today's date is now {}.\n\n{}", at.day, m.content)
        };
        let words = turn::Words::request();
        // The answer about to be written is not part of its own question.
        let upto = in_view.iter().position(|m| m.id == answer_id).unwrap_or(in_view.len());
        let request_at = in_view[..upto].iter().rposition(|m| m.role == "user");
        for (i, m) in in_view[..upto].iter().enumerate() {
            // An answer that failed said nothing.
            if Some(i) == request_at || (m.role == "assistant" && m.content.trim().is_empty()) {
                continue;
            }
            if m.role == "assistant" {
                // The tools an answer used are part of what led to it: each call and its result, as they were.
                messages.extend(turn::after_request(&m.steps, &turn::End::Answer, &words));
                messages.push(json!({ "role": m.role, "content": m.content }));
            } else {
                messages.push(json!({ "role": m.role, "content": dated(m) }));
            }
        }
        Ok((messages, request_at.map(|i| dated(&in_view[i])).unwrap_or_default()))
    }

    /// The prompt for the answer itself: everything before, the request, and the tools used for it.
    fn prompt(&self, session: &Session, answer_id: &str) -> Result<Vec<Value>> {
        let (mut messages, request) = self.before(session, answer_id)?;
        let steps = self.sessions.message(answer_id)?.steps;
        messages.extend(turn::request_messages(&request, &steps, &turn::End::Answer, &turn::Words::request()));
        Ok(messages)
    }

    async fn write_answer(self: Arc<Self>, session: Session, answer_id: String, running: Running, plan: bool) {
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
        let outcome: Result<Answer> = async {
            // The model is loaded if it is not: its loading is told by the engine's own news.
            self.engine.load(&session.model).await?;
            // A conversation that has grown past the set size is made shorter first, so that this answer has room.
            let room = self.context_use(&session.id)?;
            if room.auto && room.used > room.compact_at {
                match self.account_of(&session, "auto", Some(&answer_id)).await {
                    Ok(_) | Err(Error::Invalid(_)) => {}
                    // If the account cannot be written, the answer is still tried: the runtime says no if it truly does not fit.
                    Err(e) => tracing::warn!("A session could not be made shorter: {e}"),
                }
            }
            // What the model needs before it can answer: a tool, or the work split into tasks and done.
            if self.settings.value::<bool>("tools.use").unwrap_or(true) {
                self.work(&session, &answer_id, plan, &running.stop).await?;
            }
            if running.stop.load(Ordering::Relaxed) {
                return Ok(Answer { stopped: true, ..Answer::default() });
            }
            let thinks = session.thinking != "off";
            let mut body = json!({
                "model": session.model,
                "messages": self.prompt(&session, &answer_id)?,
                "stream": true,
                "stream_options": { "include_usage": true },
                // Said both ways: the first is how this family of templates is told, the second how the runtime is.
                "chat_template_kwargs": { "enable_thinking": thinks },
            });
            if !thinks {
                body["reasoning_effort"] = json!("none");
            } else {
                // So that a thought that runs long can be ended from outside.
                body["reasoning_control"] = json!(true);
                if session.thinking != "on" {
                    body["reasoning_effort"] = json!(session.thinking);
                }
            }
            // The answer so far, looked at now and then: a model that goes round is stopped (`turn::going_round`).
            let (mut written, mut look_at, mut ends_at) = (String::new(), 400usize, None);
            let mut heard = |piece: Piece| {
                match piece {
                    Piece::Started(name) => *running.named.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string()),
                    Piece::Text(text) => {
                        waiting.0.push_str(text);
                        written.push_str(text);
                        if ends_at.is_none() && written.len() >= look_at {
                            look_at = written.len() + 200;
                            ends_at = turn::going_round(&written);
                            if ends_at.is_some() {
                                running.stop.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    Piece::Thought(text) => waiting.1.push_str(text),
                }
                if last_told.elapsed() >= TELL_EVERY {
                    last_told = Instant::now();
                    tell(&mut waiting);
                }
            };
            let mut answer = self.engine.answer(&session.model, body, &running.stop, &mut heard).await?;
            if let Some(end) = ends_at {
                // Kept up to the first of the repeats. The rest said nothing new.
                answer.content.truncate(end.min(answer.content.len()));
                answer.went_round = true;
            }
            Ok(answer)
        }
        .await;
        tell(&mut waiting);
        self.turns().remove(&session.id);
        let ended = match outcome {
            Ok(answer) => {
                let state = if answer.stopped { "stopped" } else { "done" };
                let why = answer.went_round.then_some("The model began to repeat itself, so Demido stopped it here.");
                self.sessions.finish(&answer_id, &answer.content, Some(&answer.thought), state, why, Some(&answer.timings)).map(|_| {
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
