//! Bots for Telegram and Discord: the person talks to Demido from the chat app (DECISIONS G4, G12 to G16;
//! research note 32).
//!
//! A bot is the services' own allowed way for a program, and it is its own account: nothing of the person's
//! account is touched. The person makes the bot (@BotFather on Telegram, the Developer Portal on Discord) and
//! gives Demido its token, which is kept in the vault (`bot.<id>`). Then:
//!
//! - **Only the owner gives it orders.** Demido shows a code; the person sends it to the bot (`/start <code>` on
//!   Telegram, the code in a direct message on Discord), and that account alone is answered from then on. Anyone
//!   else is told the bot answers only its owner, and nothing they write reaches a model (G6).
//! - **What the owner writes goes into a session of its own**, as typed words would, with a short note of where
//!   it came from and what is missing there (G5); the answer goes back as plain text, cut into the service's
//!   lengths, at a human pace (one message a second in a chat).
//! - **A use of a tool that needs a yes is asked there too**: the bot sends what would happen and why, and the
//!   owner's next "yes" or "no" answers it.
//! - **Every message the bot got and sent is kept** (`bot_messages`), for the chat screens.
//!
//! Each service is a thin client of Demido's own over its documented calls (`telegram`, `discord`), so that a
//! test can stand a service in on this computer.

pub mod discord;
pub mod telegram;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::chat::Chat;
use crate::db::Db;
use crate::events::EventLog;
use crate::vault::Vault;
use crate::{Error, Result, now_ms};

/// A bot, as the window sees it. Never its token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Bot {
    pub id: String,
    /// `telegram` or `discord`.
    pub service: String,
    /// Its name at the service (`@heron_watch_bot`).
    pub name: String,
    /// The code the owner sends it to pair, until they have.
    pub code: Option<String>,
    /// The owner's account at the service, once paired, and their name there.
    pub owner: Option<String>,
    pub owner_name: Option<String>,
    /// The session the owner's messages go into.
    pub session_id: Option<String>,
    pub token: Option<crate::vault::KeyFacts>,
    /// Whether its connection to the service is running now.
    pub running: bool,
    /// Why it is not, when it stopped.
    pub problem: Option<String>,
    pub created_at: i64,
}

/// One message the bot got or sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Said {
    pub bot: String,
    pub chat: String,
    /// `in` or `out`.
    pub way: String,
    /// Who wrote it, at the service.
    pub from: String,
    pub text: String,
    /// `answered` (the owner's, given to a model), `ignored` (someone else's), `paired`, `sent`.
    pub state: String,
    pub at: i64,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct NewBot {
    /// `telegram` or `discord`.
    pub service: String,
    /// The token the service gave for the bot. Kept in the vault, never given back.
    pub token: String,
}

/// A message that came to a bot, as either client gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
    pub chat: String,
    pub from: String,
    pub from_name: String,
    pub text: String,
    /// A direct, one-to-one conversation with the bot.
    pub private: bool,
}

/// The longest message each service takes.
fn longest(service: &str) -> usize {
    if service == "discord" { 2_000 } else { 4_096 }
}

/// What a model is told with each message that comes through a bot (G5).
pub fn where_from(service: &str) -> String {
    let name = if service == "discord" { "Discord" } else { "Telegram" };
    format!(
        "(Demido's note: this came through {name}, from the bot's owner, who alone may give it orders. The answer goes back there as plain text, at most {} characters a message; there is no task list, no card to ask them on and no files there: a use that needs their yes is asked there in words, and anything else that needs the window, say so.)\n\n",
        longest(service)
    )
}

/// Text cut into pieces a service takes, at a line or a space where it can.
pub fn pieces(text: &str, most: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while rest.chars().count() > most {
        let cut = rest.char_indices().nth(most).map_or(rest.len(), |(i, _)| i);
        let head = &rest[..cut];
        let at = head.rfind('\n').or_else(|| head.rfind(' ')).filter(|i| *i > most / 2).unwrap_or(cut);
        out.push(rest[..at].trim_end().to_string());
        rest = rest[at..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

fn vault_name(id: &str) -> String {
    format!("bot.{id}")
}

/// What runs each bot, and what it last said about itself.
#[derive(Default)]
struct Running {
    tasks: HashMap<String, tokio::task::JoinHandle<()>>,
    problems: HashMap<String, String>,
}

pub struct Bots {
    db: Arc<Db>,
    vault: Arc<Vault>,
    events: Arc<EventLog>,
    chat: Arc<Chat>,
    running: Mutex<Running>,
    /// Where each service is: a test points them at stand-ins.
    pub(crate) telegram_at: String,
    pub(crate) discord_at: String,
    pub(crate) discord_gateway: Option<String>,
    /// The pause after each message sent: one a second, as Telegram asks. A test makes it short.
    pub(crate) pace: Duration,
}

impl Bots {
    pub fn new(db: Arc<Db>, vault: Arc<Vault>, events: Arc<EventLog>, chat: Arc<Chat>) -> Self {
        Self { db, vault, events, chat, running: Mutex::default(), telegram_at: telegram::API.into(), discord_at: discord::API.into(), discord_gateway: None, pace: Duration::from_millis(1_000) }
    }

    fn row(&self, r: &rusqlite::Row<'_>) -> rusqlite::Result<Bot> {
        let id: String = r.get(0)?;
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        Ok(Bot {
            token: self.vault.facts(&vault_name(&id)),
            running: running.tasks.get(&id).is_some_and(|t| !t.is_finished()),
            problem: running.problems.get(&id).cloned(),
            service: r.get(1)?,
            name: r.get(2)?,
            code: r.get(3)?,
            owner: r.get(4)?,
            owner_name: r.get(5)?,
            session_id: r.get(6)?,
            created_at: r.get(7)?,
            id,
        })
    }

    pub fn all(&self) -> Result<Vec<Bot>> {
        self.db.with(|c| c.prepare("SELECT id, service, name, code, owner, owner_name, session_id, created_at FROM bots ORDER BY created_at, rowid")?.query_map([], |r| self.row(r))?.collect())
    }

    pub fn get(&self, id: &str) -> Result<Bot> {
        self.all()?.into_iter().find(|b| b.id == id).ok_or_else(|| Error::NotFound(format!("There is no bot \"{id}\" in Demido.")))
    }

    fn token(&self, id: &str) -> Result<String> {
        self.vault.get(&vault_name(id))?.ok_or_else(|| Error::Invalid("This bot's token is gone. Add the bot again.".into()))
    }

    /// Adds a bot, once its service has said the token is a bot's, and starts it.
    pub async fn add(self: &Arc<Self>, new: NewBot) -> Result<Bot> {
        let token = new.token.trim().to_string();
        if token.is_empty() {
            return Err(Error::Invalid("Give the bot's token.".into()));
        }
        let name = match new.service.as_str() {
            "telegram" => telegram::Client::new(&self.telegram_at, &token).me().await?,
            "discord" => discord::Client::new(&self.discord_at, &token).me().await?,
            other => return Err(Error::Invalid(format!("There are bots for telegram and discord, not \"{other}\"."))),
        };
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        // A code nobody can guess, short enough to type: six digits and letters.
        let code = uuid::Uuid::new_v4().simple().to_string()[..6].to_uppercase();
        let vault = self.vault.clone();
        let (key, value) = (vault_name(&id), token);
        tokio::task::spawn_blocking(move || vault.put(&key, &value)).await.map_err(|e| Error::Other(e.to_string()))??;
        self.db.with(|c| c.execute("INSERT INTO bots (id, service, name, code, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, new.service, name, code, now_ms()]))?;
        self.events.append("bot.added", Some(&id), json!({ "service": new.service, "name": name }))?;
        self.start(&id);
        self.get(&id)
    }

    /// Stops a bot and forgets it: its token, and what it kept of its chats. Its session stays.
    pub fn remove(&self, id: &str) -> Result<Bot> {
        let bot = self.get(id)?;
        if let Some(task) = self.running.lock().unwrap_or_else(|e| e.into_inner()).tasks.remove(id) {
            task.abort();
        }
        let _ = self.vault.remove(&vault_name(id));
        self.db.with(|c| c.execute("DELETE FROM bots WHERE id = ?1", [id]))?;
        self.events.append("bot.removed", Some(id), json!({ "service": bot.service }))?;
        Ok(bot)
    }

    /// What a bot got and sent, the newest last.
    pub fn said(&self, id: &str) -> Result<Vec<Said>> {
        self.db.with(|c| {
            c.prepare("SELECT bot, chat, way, from_who, text, state, at FROM bot_messages WHERE bot = ?1 ORDER BY at, rowid")?
                .query_map([id], |r| Ok(Said { bot: r.get(0)?, chat: r.get(1)?, way: r.get(2)?, from: r.get(3)?, text: r.get(4)?, state: r.get(5)?, at: r.get(6)? }))?
                .collect()
        })
    }

    fn note(&self, bot: &str, chat: &str, way: &str, from: &str, text: &str, state: &str) {
        let _ = self.db.with(|c| {
            c.execute("INSERT INTO bot_messages (bot, chat, way, from_who, text, state, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", rusqlite::params![bot, chat, way, from, text, state, now_ms()])
        });
        self.events.tell("bot.message", Some(bot), json!({ "way": way, "state": state }));
    }

    /// Starts every bot: when the server starts.
    pub fn start_all(self: &Arc<Self>) {
        for bot in self.all().unwrap_or_default() {
            self.start(&bot.id);
        }
    }

    /// Starts a bot's connection to its service, on a task of its own.
    pub fn start(self: &Arc<Self>, id: &str) {
        let me = self.clone();
        let id = id.to_string();
        let task = tokio::spawn({
            let id = id.clone();
            async move {
                // A connection that breaks is made again after a while; a token the service refuses is not.
                let mut wait = Duration::from_secs(5);
                loop {
                    let ended = me.run(&id).await;
                    let (problem, again) = match ended {
                        Err(Error::Invalid(why)) => (why, false),
                        Err(e) => (e.to_string(), true),
                        Ok(()) => ("it stopped".to_string(), true),
                    };
                    me.running.lock().unwrap_or_else(|e| e.into_inner()).problems.insert(id.clone(), problem.clone());
                    let _ = me.events.append("bot.stopped", Some(&id), json!({ "problem": problem, "again": again }));
                    if !again || me.get(&id).is_err() {
                        return;
                    }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(300));
                }
            }
        });
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        running.problems.remove(&id);
        if let Some(old) = running.tasks.insert(id, task) {
            old.abort();
        }
    }

    /// Ends every bot: when the server stops.
    pub fn end(&self) {
        for (_, task) in self.running.lock().unwrap_or_else(|e| e.into_inner()).tasks.drain() {
            task.abort();
        }
    }

    async fn run(self: &Arc<Self>, id: &str) -> Result<()> {
        let bot = self.get(id)?;
        let token = self.token(id)?;
        match bot.service.as_str() {
            "telegram" => {
                let client = telegram::Client::new(&self.telegram_at, &token);
                let mut offset = 0;
                loop {
                    let (incoming, next) = client.updates(offset).await?;
                    offset = next;
                    for message in incoming {
                        self.heard(id, &message, &|chat, text| {
                            let client = client.clone();
                            async move { client.send(&chat, &text).await }
                        })
                        .await;
                    }
                }
            }
            "discord" => {
                let client = discord::Client::new(&self.discord_at, &token);
                let gateway = match &self.discord_gateway {
                    Some(at) => at.clone(),
                    None => client.gateway().await?,
                };
                let (tx, mut rx) = tokio::sync::mpsc::channel::<Incoming>(64);
                let listening = tokio::spawn(discord::listen(gateway, token.clone(), tx));
                while let Some(message) = rx.recv().await {
                    self.heard(id, &message, &|chat, text| {
                        let client = client.clone();
                        async move { client.send(&chat, &text).await }
                    })
                    .await;
                }
                listening.await.map_err(|e| Error::Other(e.to_string()))?
            }
            other => Err(Error::Invalid(format!("unknown service {other}"))),
        }
    }

    /// One message that came to a bot: pairing, someone else's, a yes or a no, or the owner's words for a model.
    async fn heard<F, Fut>(self: &Arc<Self>, id: &str, m: &Incoming, send: &F)
    where
        F: Fn(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let Ok(bot) = self.get(id) else { return };
        let words = m.text.trim();
        let (service, bot_name) = (bot.service.clone(), bot.name.clone());
        let (service, bot_name) = (&service, &bot_name);
        let say = |text: String| async move {
            for piece in pieces(&text, longest(service)) {
                if send(m.chat.clone(), piece.clone()).await.is_ok() {
                    self.note(id, &m.chat, "out", bot_name, &piece, "sent");
                }
                // A human pace: one message a second in a chat (Telegram's own advice).
                tokio::time::sleep(self.pace).await;
            }
        };
        // Pairing: the code Demido showed, from a direct conversation, while nobody owns the bot.
        if bot.owner.is_none() {
            let given = words.trim_start_matches("/start").trim().to_uppercase();
            if m.private && bot.code.as_deref().is_some_and(|code| given == code) {
                let session = match self.chat.start(Some(&format!("{} bot", if bot.service == "discord" { "Discord" } else { "Telegram" })), None).await {
                    Ok(s) => s,
                    Err(e) => {
                        say(format!("Demido could not start a session: {e}")).await;
                        return;
                    }
                };
                let _ = self.db.with(|c| c.execute("UPDATE bots SET owner = ?2, owner_name = ?3, code = NULL, session_id = ?4 WHERE id = ?1", rusqlite::params![id, m.from, m.from_name, session.id]));
                self.note(id, &m.chat, "in", &m.from_name, "(the pairing code)", "paired");
                let _ = self.events.append("bot.paired", Some(id), json!({ "owner_name": m.from_name }));
                say("Paired. This bot answers you, and only you, from Demido now.".into()).await;
            } else {
                self.note(id, &m.chat, "in", &m.from_name, words, "ignored");
                if m.private {
                    say("This bot is not paired yet. Send it the code Demido shows for it.".into()).await;
                }
            }
            return;
        }
        // Someone else: what they write never reaches a model (G6).
        if bot.owner.as_deref() != Some(m.from.as_str()) {
            self.note(id, &m.chat, "in", &m.from_name, words, "ignored");
            if m.private {
                say("This bot answers only its owner.".into()).await;
            }
            return;
        }
        let Some(session) = bot.session_id.clone() else { return };
        // A use of a tool waiting for a yes: the owner's yes or no answers it.
        if let Some(request) = self.chat.waiting_for(&session) {
            let said = match words.to_lowercase().trim_end_matches(['.', '!']) {
                "yes" | "y" | "ok" | "go" => Some(crate::permissions::Said::Once),
                "no" | "n" | "stop" => Some(crate::permissions::Said::No),
                _ => None,
            };
            self.note(id, &m.chat, "in", &m.from_name, words, "answered");
            match said {
                Some(said) => {
                    let _ = self.chat.answer_request(&session, &request.id, said);
                }
                None => {
                    say(format!("Demido is waiting for your yes or no first: {}. Reply yes or no.", request.what)).await;
                    return;
                }
            }
            self.follow(id, &session, &say).await;
            return;
        }
        self.note(id, &m.chat, "in", &m.from_name, words, "answered");
        if let Err(e) = self.chat.say(&session, &format!("{}{words}", where_from(&bot.service))) {
            say(format!("Demido could not take that: {e}")).await;
            return;
        }
        self.follow(id, &session, &say).await;
    }

    /// Waits for the session's answer and sends it; or, when a use of a tool waits for a yes, asks for it there.
    async fn follow<S, Fut>(&self, id: &str, session: &str, say: &S)
    where
        S: Fn(String) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let _ = id;
        for _ in 0..(30 * 60 * 4) {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(request) = self.chat.waiting_for(session) {
                say(format!("Demido asks first: {}.\n{}\nReply yes or no.", request.what, request.why)).await;
                return;
            }
            if self.chat.answering(session) {
                continue;
            }
            let Ok(messages) = self.chat.sessions().messages(session) else { return };
            if let Some(last) = messages.iter().rev().find(|m| m.role == "assistant") {
                let text = if last.content.trim().is_empty() { "(Demido gave no answer.)".to_string() } else { last.content.clone() };
                say(text).await;
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_answer_is_cut_where_a_line_or_a_word_ends() {
        let text = format!("{}\n{}", "word ".repeat(30).trim(), "next ".repeat(30).trim());
        let cut = pieces(&text, 100);
        assert!(cut.iter().all(|p| p.chars().count() <= 100), "{cut:?}");
        assert_eq!(cut.join(" ").split_whitespace().count(), 60, "nothing lost: {cut:?}");
        assert!(cut.iter().all(|p| !p.starts_with(' ') && !p.ends_with(' ')));
        assert_eq!(pieces("short", 100), ["short"]);
        assert!(pieces("   ", 100).is_empty());
        assert!(where_from("telegram").contains("Telegram") && where_from("telegram").contains("4096 characters"));
    }
}
