//! Discord as the person (DECISIONS G18; research note 37): the account's own token, on the same REST API and
//! Gateway a bot uses (`bots::discord`), without `Bot ` before it. Discord forbids this ("self-bots"); the person
//! linked it after a warning that says so (G2).
//!
//! What is used: `GET /users/@me` (who the token is), `GET /channels/{id}/messages` (a chat's newest), `POST
//! /channels/{id}/messages` (sending), and the Gateway's Identify with no capabilities asked, so that READY gives
//! the direct messages with their people and the servers with their channels (Read (unofficial):
//! discord-userdoccers), and `MESSAGE_CREATE` for each new message, the person's own too.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{Heard, Line, NewChat};
use crate::{Error, Result};

pub const GATEWAY: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

fn said(e: reqwest::Error) -> Error {
    Error::Net(format!("Discord could not be reached ({}).", e.without_url()))
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(30)).build().expect("a client can be made")
}

/// A person's name as Discord shows it: their display name, or their username.
fn name_of(user: &Value) -> String {
    user["global_name"].as_str().filter(|s| !s.is_empty()).or_else(|| user["username"].as_str()).unwrap_or("someone").to_string()
}

/// A message as Discord gives it, as a line Demido keeps. A file or an embed is named, not fetched.
pub(crate) fn line_of(m: &Value, me: &str) -> Line {
    let author = &m["author"];
    let from_id = author["id"].as_str().unwrap_or_default().to_string();
    let mut text = m["content"].as_str().unwrap_or_default().to_string();
    for file in m["attachments"].as_array().into_iter().flatten() {
        let name = file["filename"].as_str().unwrap_or("a file");
        text.push_str(&format!("{}[a file: {name}]", if text.is_empty() { "" } else { "\n" }));
    }
    let at = m["timestamp"].as_str().and_then(|t| t.parse::<jiff::Timestamp>().ok()).map_or_else(crate::now_ms, |t| t.as_millisecond());
    Line { chat: m["channel_id"].as_str().unwrap_or_default().into(), id: m["id"].as_str().unwrap_or_default().into(), mine: from_id == me, from_name: name_of(author), from_id, text, at }
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Client {
    pub fn new(api: &str, token: &str) -> Self {
        Self { http: http(), base: api.trim_end_matches('/').to_string(), token: token.trim().to_string() }
    }

    async fn ask(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let got = request.header("authorization", &self.token).send().await.map_err(said)?;
        let status = got.status();
        if status == 401 {
            return Err(Error::Invalid(
                "Discord says this token is not valid any more (401): it changes when the password does, or when the account signs out everywhere. Link the account again.".into(),
            ));
        }
        if status == 429 {
            let wait: Value = got.json().await.unwrap_or(Value::Null);
            tokio::time::sleep(Duration::from_secs_f64(wait["retry_after"].as_f64().unwrap_or(2.0).min(60.0))).await;
            return Err(Error::Net("Discord says this is too fast (429). Try again in a moment.".into()));
        }
        let body: Value = got.json().await.map_err(said)?;
        if !status.is_success() {
            return Err(Error::Net(format!("Discord said no ({status}): {}", body["message"].as_str().unwrap_or("no reason given"))));
        }
        Ok(body)
    }

    /// Who the token is: its id, the name Discord shows, its username. A bot's token is refused: bots are added
    /// as bots (G12), where Discord allows them.
    pub async fn me(&self) -> Result<(String, String, String)> {
        let me = self.ask(self.http.get(format!("{}/users/@me", self.base))).await?;
        if me["bot"] == true {
            return Err(Error::Invalid("That token is a bot's. Add it under Bots, where Discord allows it.".into()));
        }
        Ok((me["id"].as_str().unwrap_or_default().into(), name_of(&me), me["username"].as_str().unwrap_or_default().into()))
    }

    /// The newest messages of a chat, the oldest first.
    pub async fn history(&self, channel: &str, me: &str) -> Result<Vec<Line>> {
        let got = self.ask(self.http.get(format!("{}/channels/{channel}/messages?limit=50", self.base))).await?;
        let mut lines: Vec<Line> = got.as_array().into_iter().flatten().map(|m| line_of(m, me)).collect();
        lines.sort_by_key(|l| l.at);
        Ok(lines)
    }

    /// Sends a message as the person, mentioning nobody it does not name.
    pub async fn send(&self, channel: &str, text: &str, me: &str) -> Result<Line> {
        let sent = self.ask(self.http.post(format!("{}/channels/{channel}/messages", self.base)).json(&json!({ "content": text, "allowed_mentions": { "parse": [] } }))).await?;
        Ok(line_of(&sent, me))
    }
}

/// The chats READY gives: direct messages and groups with their people, and each server's text channels. Both
/// shapes are read: people on the chat (`recipients`) or by their ids beside a list of `users`.
pub(crate) fn chats_of(ready: &Value) -> Vec<NewChat> {
    let users: HashMap<&str, String> = ready["users"].as_array().into_iter().flatten().filter_map(|u| Some((u["id"].as_str()?, name_of(u)))).collect();
    let mut out = Vec::new();
    for c in ready["private_channels"].as_array().into_iter().flatten() {
        let Some(id) = c["id"].as_str() else { continue };
        let people: Vec<String> = match c["recipients"].as_array() {
            Some(r) => r.iter().map(name_of).collect(),
            None => c["recipient_ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|i| users.get(i).cloned().unwrap_or_else(|| i.to_string())).collect(),
        };
        let named = c["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string);
        let group = c["type"].as_u64() == Some(3);
        out.push(NewChat { id: id.into(), name: named.unwrap_or_else(|| people.join(", ")), kind: if group { "group" } else { "direct" }.into(), ..NewChat::default() });
    }
    for g in ready["guilds"].as_array().into_iter().flatten() {
        let place = g["name"].as_str().or_else(|| g.pointer("/properties/name").and_then(Value::as_str)).unwrap_or("A server").to_string();
        let mut channels: Vec<&Value> = g["channels"].as_array().into_iter().flatten().filter(|c| matches!(c["type"].as_u64(), Some(0 | 5))).collect();
        channels.sort_by_key(|c| c["position"].as_i64().unwrap_or(0));
        for c in channels {
            let Some(id) = c["id"].as_str() else { continue };
            out.push(NewChat { id: id.into(), name: format!("#{}", c["name"].as_str().unwrap_or("channel")), kind: "channel".into(), place: Some(place.clone()), ..NewChat::default() });
        }
    }
    out
}

/// Listens on the Gateway as the person and passes on their chats and every new message, until it ends.
pub async fn listen(gateway: String, token: String, me: String, to: tokio::sync::mpsc::Sender<Heard>) -> Result<()> {
    let (mut out, mut inn) = crate::bots::discord::open_gateway(&gateway).await?.split();
    let hello: Value = match inn.next().await {
        Some(Ok(Message::Text(t))) => serde_json::from_str(&t).unwrap_or(Value::Null),
        _ => return Err(Error::Net("Discord's Gateway said no hello.".into())),
    };
    let every = Duration::from_millis(hello.pointer("/d/heartbeat_interval").and_then(Value::as_u64).unwrap_or(41_250));
    // A person's account needs no intents; no capabilities, so READY comes in its plain shape. It says what it is.
    let identify = json!({ "op": 2, "d": { "token": token.trim(), "capabilities": 0, "properties": { "os": std::env::consts::OS, "browser": "Demido Studio", "device": "" }, "presence": { "status": "unknown", "since": 0, "activities": [], "afk": false }, "compress": false } });
    out.send(Message::Text(identify.to_string().into())).await.map_err(|e| Error::Net(e.to_string()))?;
    let mut beat = tokio::time::interval(every);
    beat.tick().await;
    let mut last_seq = Value::Null;
    loop {
        tokio::select! {
            _ = beat.tick() => {
                if out.send(Message::Text(json!({ "op": 1, "d": last_seq }).to_string().into())).await.is_err() {
                    return Err(Error::Net("Discord's Gateway closed.".into()));
                }
            }
            got = inn.next() => {
                let Some(Ok(message)) = got else { return Err(Error::Net("Discord's Gateway closed.".into())) };
                let text = match message {
                    Message::Text(t) => t.to_string(),
                    // 4004: the token is not valid. Not tried again until the person links it again.
                    Message::Close(Some(frame)) if u16::from(frame.code) == 4004 => return Err(Error::Invalid("Discord says the token is not valid any more. Link the account again.".into())),
                    Message::Close(_) => return Err(Error::Net("Discord's Gateway closed.".into())),
                    _ => continue,
                };
                let e: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if !e["s"].is_null() {
                    last_seq = e["s"].clone();
                }
                let heard = match (e["op"].as_u64(), e["t"].as_str()) {
                    (Some(0), Some("READY")) => Some(Heard::Chats(chats_of(&e["d"]))),
                    (Some(0), Some("MESSAGE_CREATE")) => {
                        let d = &e["d"];
                        let line = line_of(d, &me);
                        // A new direct message from someone not in the list yet: named after them.
                        let chat = NewChat { id: line.chat.clone(), name: if line.mine { String::new() } else { line.from_name.clone() }, kind: if d["guild_id"].is_null() { "direct" } else { "channel" }.into(), weak: true, ..NewChat::default() };
                        Some(Heard::Said { chat, line, count: true })
                    }
                    (Some(0), Some("CHANNEL_CREATE")) if e.pointer("/d/guild_id").is_none_or(Value::is_null) => {
                        Some(Heard::Chats(chats_of(&json!({ "private_channels": [e["d"].clone()] }))))
                    }
                    (Some(1), _) => { let _ = out.send(Message::Text(json!({ "op": 1, "d": last_seq }).to_string().into())).await; None }
                    (Some(7), _) | (Some(9), _) => return Err(Error::Net("Discord asked to connect again.".into())),
                    _ => None,
                };
                if let Some(heard) = heard && to.send(heard).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}
