//! Telegram's Bot API, the part a bot of Demido's needs: `getMe`, `getUpdates` with long polling, and
//! `sendMessage` (https://core.telegram.org/bots/api, read 2026-11-24). The token is in the address Telegram
//! asks for, so no error here ever carries the address.

use std::time::Duration;

use serde_json::{Value, json};

use super::Incoming;
use crate::{Error, Result};

pub const API: &str = "https://api.telegram.org";
/// How long one `getUpdates` waits on Telegram's side for something to come.
const WAIT_S: u64 = 50;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    /// `<api>/bot<token>`: never shown.
    base: String,
}

fn said(e: reqwest::Error) -> Error {
    Error::Net(format!("Telegram could not be reached ({}).", e.without_url()))
}

impl Client {
    pub fn new(api: &str, token: &str) -> Self {
        let http = reqwest::Client::builder().user_agent(crate::web::name()).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(WAIT_S + 20)).build().expect("a client can be made");
        Self { http, base: format!("{}/bot{token}", api.trim_end_matches('/')) }
    }

    async fn call(&self, method: &str, body: Value) -> Result<Value> {
        let got: Value = self.http.post(format!("{}/{method}", self.base)).json(&body).send().await.map_err(said)?.json().await.map_err(said)?;
        if got["ok"] == true {
            return Ok(got["result"].clone());
        }
        let code = got["error_code"].as_i64().unwrap_or(0);
        if code == 429 {
            // It says how long to wait: waited, and the caller tries again.
            let wait = got.pointer("/parameters/retry_after").and_then(Value::as_u64).unwrap_or(5);
            tokio::time::sleep(Duration::from_secs(wait.min(120))).await;
        }
        Err(match code {
            401 | 404 => Error::Invalid("Telegram says this is not a bot's token (401). Copy it again from @BotFather.".into()),
            _ => Error::Net(format!("Telegram said no ({code}): {}", got["description"].as_str().unwrap_or("no reason given"))),
        })
    }

    /// The bot's own name, `@…`: which also proves the token is a bot's.
    pub async fn me(&self) -> Result<String> {
        let me = self.call("getMe", json!({})).await?;
        Ok(format!("@{}", me["username"].as_str().unwrap_or("bot")))
    }

    /// The messages that came since `offset`, waiting up to 50 s for one; and the offset to ask from next.
    pub async fn updates(&self, offset: i64) -> Result<(Vec<Incoming>, i64)> {
        let updates = self.call("getUpdates", json!({ "offset": offset, "timeout": WAIT_S, "allowed_updates": ["message"] })).await?;
        let mut next = offset;
        let mut out = Vec::new();
        for u in updates.as_array().into_iter().flatten() {
            next = next.max(u["update_id"].as_i64().unwrap_or(0) + 1);
            let m = &u["message"];
            let (Some(text), Some(chat), Some(from)) = (m["text"].as_str(), m.pointer("/chat/id").and_then(Value::as_i64), m.pointer("/from/id").and_then(Value::as_i64)) else { continue };
            let name = m
                .pointer("/from/username")
                .and_then(Value::as_str)
                .map(|u| format!("@{u}"))
                .or_else(|| m.pointer("/from/first_name").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| from.to_string());
            out.push(Incoming { chat: chat.to_string(), from: from.to_string(), from_name: name, text: text.to_string(), private: m.pointer("/chat/type").and_then(Value::as_str) == Some("private") });
        }
        Ok((out, next))
    }

    pub async fn send(&self, chat: &str, text: &str) -> Result<()> {
        self.call("sendMessage", json!({ "chat_id": chat, "text": text })).await.map(|_| ())
    }
}
