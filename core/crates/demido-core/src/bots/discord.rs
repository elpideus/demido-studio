//! Discord's API, the part a bot of Demido's needs (https://discord.com/developers/docs, read 2026-11-24): its
//! own name (`GET /users/@me`), where its Gateway is (`GET /gateway/bot`), sending (`POST
//! /channels/{id}/messages`), and the Gateway, a WebSocket that tells it of messages: Hello, then a heartbeat
//! at the interval it gives, Identify, and `MESSAGE_CREATE`. Only direct messages are asked for (the
//! `DIRECT_MESSAGES` intent): that is where the owner talks to it, and it needs no privileged intent.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_rustls::rustls;
use tokio_tungstenite::tungstenite::Message;

use super::Incoming;
use crate::{Error, Result};

pub const API: &str = "https://discord.com/api/v10";
/// Direct messages only (1 << 12).
const INTENTS: u64 = 1 << 12;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

fn said(e: reqwest::Error) -> Error {
    Error::Net(format!("Discord could not be reached ({}).", e.without_url()))
}

impl Client {
    pub fn new(api: &str, token: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("DiscordBot (https://github.com/elpideus/demido-studio, {})", crate::VERSION))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("a client can be made");
        Self { http, base: api.trim_end_matches('/').to_string(), token: token.into() }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let got = self.http.get(format!("{}{path}", self.base)).header("authorization", format!("Bot {}", self.token)).send().await.map_err(said)?;
        if got.status() == 401 {
            return Err(Error::Invalid("Discord says this is not a bot's token (401). Copy it again from the Developer Portal, under Bot.".into()));
        }
        let status = got.status();
        let body: Value = got.json().await.map_err(said)?;
        if !status.is_success() {
            return Err(Error::Net(format!("Discord said no ({status}): {}", body["message"].as_str().unwrap_or("no reason given"))));
        }
        Ok(body)
    }

    /// The bot's own name: which also proves the token is a bot's.
    pub async fn me(&self) -> Result<String> {
        let me = self.get("/users/@me").await?;
        if me["bot"] != true {
            return Err(Error::Invalid("That token is a person's, not a bot's. Demido never acts as a Discord account (Discord forbids it): make a bot in the Developer Portal.".into()));
        }
        Ok(me["username"].as_str().unwrap_or("bot").to_string())
    }

    /// Where the Gateway is, with the version and the encoding Demido speaks.
    pub async fn gateway(&self) -> Result<String> {
        let at = self.get("/gateway/bot").await?;
        Ok(format!("{}/?v=10&encoding=json", at["url"].as_str().unwrap_or("wss://gateway.discord.gg").trim_end_matches('/')))
    }

    pub async fn send(&self, channel: &str, text: &str) -> Result<()> {
        let got = self
            .http
            .post(format!("{}/channels/{channel}/messages", self.base))
            .header("authorization", format!("Bot {}", self.token))
            .json(&json!({ "content": text, "allowed_mentions": { "parse": [] } }))
            .send()
            .await
            .map_err(said)?;
        if got.status() == 429 {
            let wait: Value = got.json().await.unwrap_or(Value::Null);
            tokio::time::sleep(Duration::from_secs_f64(wait["retry_after"].as_f64().unwrap_or(2.0).min(60.0))).await;
            return Err(Error::Net("Discord says the bot is sending too fast (429).".into()));
        }
        if !got.status().is_success() {
            return Err(Error::Net(format!("Discord did not take the message ({}).", got.status())));
        }
        Ok(())
    }
}

/// Listens on the Gateway and passes on every direct message, until the connection ends.
pub async fn listen(gateway: String, token: String, to: tokio::sync::mpsc::Sender<Incoming>) -> Result<()> {
    let url = url::Url::parse(&gateway).map_err(|_| Error::Invalid("Discord's Gateway address is wrong.".into()))?;
    let host = url.host_str().unwrap_or_default().to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await.map_err(|e| Error::Net(format!("Discord's Gateway could not be reached ({e}).")))?;
    let stream: Box<dyn crate::email::Stream> = if url.scheme() == "wss" {
        use rustls_platform_verifier::BuilderVerifierExt;
        let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_platform_verifier())
            .map_err(|e| Error::Other(format!("TLS could not be set up ({e})")))?
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.clone()).map_err(|_| Error::Invalid("Discord's Gateway has no proper name.".into()))?;
        Box::new(tokio_rustls::TlsConnector::from(std::sync::Arc::new(config)).connect(name, tcp).await.map_err(|e| Error::Net(format!("Discord's Gateway could not be reached over TLS ({e}).")))?)
    } else {
        Box::new(tcp)
    };
    let (ws, _) = tokio_tungstenite::client_async(gateway.as_str(), stream).await.map_err(|e| Error::Net(format!("Discord's Gateway refused the connection ({e}).")))?;
    let (mut out, mut inn) = ws.split();
    // Hello first: how often to beat.
    let hello: Value = match inn.next().await {
        Some(Ok(Message::Text(t))) => serde_json::from_str(&t).unwrap_or(Value::Null),
        _ => return Err(Error::Net("Discord's Gateway said no hello.".into())),
    };
    let every = Duration::from_millis(hello.pointer("/d/heartbeat_interval").and_then(Value::as_u64).unwrap_or(41_250));
    let identify = json!({ "op": 2, "d": { "token": token, "intents": INTENTS, "properties": { "os": std::env::consts::OS, "browser": "DemidoStudio", "device": "DemidoStudio" } } });
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
                let Message::Text(text) = message else { continue };
                let e: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if !e["s"].is_null() {
                    last_seq = e["s"].clone();
                }
                match (e["op"].as_u64(), e["t"].as_str()) {
                    (Some(0), Some("MESSAGE_CREATE")) => {
                        let d = &e["d"];
                        if d.pointer("/author/bot") == Some(&Value::Bool(true)) {
                            continue;
                        }
                        let incoming = Incoming {
                            chat: d["channel_id"].as_str().unwrap_or_default().into(),
                            from: d.pointer("/author/id").and_then(Value::as_str).unwrap_or_default().into(),
                            from_name: d.pointer("/author/username").and_then(Value::as_str).unwrap_or_default().into(),
                            text: d["content"].as_str().unwrap_or_default().into(),
                            private: d["guild_id"].is_null(),
                        };
                        if to.send(incoming).await.is_err() { return Ok(()) }
                    }
                    // Asked to beat now.
                    (Some(1), _) => { let _ = out.send(Message::Text(json!({ "op": 1, "d": last_seq }).to_string().into())).await; }
                    // Asked to connect again, or the session is not valid: the caller starts over.
                    (Some(7), _) | (Some(9), _) => return Err(Error::Net("Discord asked the bot to connect again.".into())),
                    _ => {}
                }
            }
        }
    }
}
