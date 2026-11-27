//! Telegram as the person (DECISIONS G19; research note 37), by MTProto through `grammers` 0.12.
//!
//! Linking: the person's own api_id and api_hash (my.telegram.org, API terms 2.1), their phone number, the code
//! Telegram sends to their Telegram app, and their password if the account has one. Telegram's session (the
//! data centre and its authorization key) is kept in the vault with the api_id, never in a file of its own.
//! Running: the chats (the newest 200 dialogs), each new message, a chat's newest 50 when the window opens it,
//! and sending as the person when they say so. Nothing is marked as read (API terms 1.4).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grammers_client::client::{LoginToken, PasswordToken};
use grammers_client::peer::Peer;
use grammers_client::sender::{ConnectionParams, UpdatesConfiguration};
use grammers_client::update::Update;
use grammers_client::{Client, SenderPool, SignInError};
use grammers_session::storages::{MemorySession, MemorySessionError};
use grammers_session::types::{DcOption, PeerId, PeerInfo, UpdateState, UpdatesState};
use grammers_session::{BoxFuture, Session, SessionData};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::{Heard, Line, NewChat, Order, Step};
use crate::{Error, Result};

/// What is kept in the vault for a Telegram account: the person's api_id and api_hash, and the session.
#[derive(Clone, Serialize, Deserialize)]
struct Kept {
    api_id: i32,
    api_hash: String,
    home_dc: i32,
    dc_options: Vec<DcOption>,
}

/// Telegram's session in memory, with a copy of what must outlive Demido (the data centres and their keys).
struct KeptSession {
    inner: MemorySession,
    copy: Mutex<Kept>,
    changed: std::sync::atomic::AtomicBool,
}

impl KeptSession {
    fn new(kept: Kept) -> Self {
        let mut data = SessionData { home_dc: kept.home_dc, ..SessionData::default() };
        for dc in &kept.dc_options {
            data.dc_options.insert(dc.id, dc.clone());
        }
        let copy = Kept { dc_options: data.dc_options.values().cloned().collect(), ..kept };
        Self { inner: MemorySession::from(data), copy: Mutex::new(copy), changed: false.into() }
    }

    fn secret(&self) -> String {
        serde_json::to_string(&*self.copy.lock().unwrap_or_else(|e| e.into_inner())).unwrap_or_default()
    }

    /// What changed since it was last kept, if anything.
    fn take_changed(&self) -> Option<String> {
        self.changed.swap(false, std::sync::atomic::Ordering::Relaxed).then(|| self.secret())
    }
}

impl Session for KeptSession {
    type Error = MemorySessionError;

    fn home_dc_id(&self) -> std::result::Result<i32, Self::Error> {
        self.inner.home_dc_id()
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, std::result::Result<(), Self::Error>> {
        self.copy.lock().unwrap_or_else(|e| e.into_inner()).home_dc = dc_id;
        self.changed.store(true, std::sync::atomic::Ordering::Relaxed);
        self.inner.set_home_dc_id(dc_id)
    }

    fn dc_option(&self, dc_id: i32) -> std::result::Result<Option<DcOption>, Self::Error> {
        self.inner.dc_option(dc_id)
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, std::result::Result<(), Self::Error>> {
        {
            let mut copy = self.copy.lock().unwrap_or_else(|e| e.into_inner());
            copy.dc_options.retain(|d| d.id != dc_option.id);
            copy.dc_options.push(dc_option.clone());
        }
        self.changed.store(true, std::sync::atomic::Ordering::Relaxed);
        self.inner.set_dc_option(dc_option)
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, std::result::Result<Option<PeerInfo>, Self::Error>> {
        self.inner.peer(peer)
    }

    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, std::result::Result<(), Self::Error>> {
        self.inner.cache_peer(peer)
    }

    fn updates_state(&self) -> BoxFuture<'_, std::result::Result<UpdatesState, Self::Error>> {
        self.inner.updates_state()
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, std::result::Result<(), Self::Error>> {
        self.inner.set_update_state(update)
    }
}

/// How Demido shows itself to Telegram, in the person's list of active sessions.
fn params() -> ConnectionParams {
    ConnectionParams { app_version: format!("Demido Studio {}", crate::VERSION), ..ConnectionParams::default() }
}

fn net(e: impl std::fmt::Display) -> Error {
    Error::Net(format!("Telegram said no ({e})."))
}

/// A connection to Telegram on a session, with what drives it.
pub struct Connected {
    client: Client,
    session: Arc<KeptSession>,
    pool: tokio::task::JoinHandle<()>,
    updates: Option<tokio::sync::mpsc::Receiver<grammers_session::updates::UpdatesLike>>,
}

fn connect(kept: Kept) -> Connected {
    let session = Arc::new(KeptSession::new(kept.clone()));
    let SenderPool { runner, handle, updates } = SenderPool::with_configuration(session.clone(), kept.api_id, params());
    let client = Client::new(handle);
    let pool = tokio::spawn(async move {
        runner.run().await;
    });
    Connected { client, session, pool, updates: Some(updates) }
}

pub enum Login {
    Code(LoginToken),
    Password(PasswordToken),
    /// Telegram refused in a way that cannot go on: the person removes the account and links it again.
    Spent,
}

/// A sign-in under way, waiting for the person's code or password.
pub enum Pending {
    Real {
        connected: Box<Connected>,
        login: Login,
        phone: String,
    },
    #[cfg(any(test, debug_assertions))]
    StandIn {
        asked: &'static str,
        phone: String,
    },
}

/// What came of an answer.
pub enum Went {
    Done { me: String, name: String, handle: String, secret: String },
    Again { pending: Pending, step: Step },
}

fn step(kind: &str, hint: Option<String>, problem: Option<String>) -> Step {
    Step { kind: kind.into(), hint, show: None, problem }
}

fn chat_id(peer: PeerId) -> String {
    peer.bot_api_dialog_id_unchecked().to_string()
}

fn chat_of(peer: &Peer) -> NewChat {
    let kind = match peer {
        Peer::User(_) => "direct",
        Peer::Group(_) => "group",
        _ => "channel",
    };
    NewChat { id: chat_id(peer.id()), name: peer.name().filter(|n| !n.is_empty()).unwrap_or("A conversation").to_string(), kind: kind.into(), place: None }
}

fn line_of(m: &grammers_client::message::Message) -> Line {
    let mine = m.outgoing();
    let (from_id, from_name) = match m.sender() {
        Some(p) => (chat_id(p.id()), p.name().unwrap_or("someone").to_string()),
        None => (chat_id(m.peer_id()), m.peer().and_then(Peer::name).unwrap_or("someone").to_string()),
    };
    let mut text = m.text().to_string();
    if m.media().is_some() {
        text.push_str(if text.is_empty() { "[a picture or a file]" } else { "\n[a picture or a file]" });
    }
    Line { chat: chat_id(m.peer_id()), id: m.id().to_string(), from_id, from_name, mine, text, at: m.date().as_millisecond() }
}

/// Telegram's stand-in for the tests: a sign-in with a known code and password, two chats, what was sent.
#[cfg(any(test, debug_assertions))]
#[derive(Default)]
pub struct StandIn {
    pub code: String,
    pub password: Option<String>,
    pub sent: Mutex<Vec<(String, String)>>,
    pub signed_out: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub enum Way {
    Real,
    #[cfg(any(test, debug_assertions))]
    StandIn(Arc<StandIn>),
}

impl Way {
    /// Asks Telegram to send a code to the person's Telegram app.
    pub async fn begin(&self, api_id: i32, api_hash: &str, phone: &str) -> Result<Pending> {
        match self {
            Way::Real => {
                let connected = connect(Kept { api_id, api_hash: api_hash.into(), home_dc: 2, dc_options: Vec::new() });
                let token = connected
                    .client
                    .request_login_code(phone, api_hash)
                    .await
                    .map_err(|e| Error::Invalid(format!("Telegram did not send a code ({e}). Check the api_id, the api_hash and the number, with its country code.")))?;
                Ok(Pending::Real { connected: Box::new(connected), login: Login::Code(token), phone: phone.into() })
            }
            #[cfg(any(test, debug_assertions))]
            Way::StandIn(_) => Ok(Pending::StandIn { asked: "code", phone: phone.into() }),
        }
    }

    pub async fn code(&self, pending: Pending, code: &str) -> Went {
        match pending {
            Pending::Real { connected, login: Login::Code(token), phone } => match connected.client.sign_in(&token, code).await {
                Ok(user) => done(&connected, &user, &phone),
                Err(SignInError::PasswordRequired(t)) => {
                    let hint = t.hint().map(str::to_string);
                    Went::Again { pending: Pending::Real { connected, login: Login::Password(t), phone }, step: step("password", hint, None) }
                }
                Err(SignInError::InvalidCode) => {
                    Went::Again { pending: Pending::Real { connected, login: Login::Code(token), phone }, step: step("code", None, Some("That code is not the one Telegram sent.".into())) }
                }
                Err(SignInError::SignUpRequired) => Went::Again {
                    pending: Pending::Real { connected, login: Login::Code(token), phone },
                    step: step("code", None, Some("This number has no Telegram account. Make one in Telegram's own app first.".into())),
                },
                Err(e) => Went::Again { pending: Pending::Real { connected, login: Login::Code(token), phone }, step: step("code", None, Some(format!("Telegram said no ({e})."))) },
            },
            #[cfg(any(test, debug_assertions))]
            Pending::StandIn { asked: "code", phone } => {
                let Way::StandIn(s) = self else { unreachable!() };
                if code != s.code {
                    return Went::Again { pending: Pending::StandIn { asked: "code", phone }, step: step("code", None, Some("That code is not the one Telegram sent.".into())) };
                }
                if s.password.is_some() {
                    return Went::Again { pending: Pending::StandIn { asked: "password", phone }, step: step("password", Some("the heron".into()), None) };
                }
                Went::Done { me: "42".into(), name: "Ada".into(), handle: "@ada".into(), secret: "{\"stand_in\":true}".into() }
            }
            other => Went::Again { pending: other, step: step("password", None, Some("Telegram asks for the account's password now.".into())) },
        }
    }

    pub async fn password(&self, pending: Pending, password: &str) -> Went {
        match pending {
            Pending::Real { connected, login: Login::Password(token), phone } => match connected.client.check_password(token, password).await {
                Ok(user) => done(&connected, &user, &phone),
                Err(SignInError::InvalidPassword(t)) => {
                    let hint = t.hint().map(str::to_string);
                    Went::Again { pending: Pending::Real { connected, login: Login::Password(t), phone }, step: step("password", hint, Some("That password is not the account's.".into())) }
                }
                Err(e) => Went::Again {
                    pending: Pending::Real { connected, login: Login::Spent, phone },
                    step: step("password", None, Some(format!("Telegram said no ({e}). Remove the account and link it again."))),
                },
            },
            #[cfg(any(test, debug_assertions))]
            Pending::StandIn { asked: "password", phone } => {
                let Way::StandIn(s) = self else { unreachable!() };
                if s.password.as_deref() != Some(password) {
                    return Went::Again {
                        pending: Pending::StandIn { asked: "password", phone },
                        step: step("password", Some("the heron".into()), Some("That password is not the account's.".into())),
                    };
                }
                Went::Done { me: "42".into(), name: "Ada".into(), handle: "@ada".into(), secret: "{\"stand_in\":true}".into() }
            }
            other => Went::Again { pending: other, step: step("code", None, Some("Give the code Telegram sent first.".into())) },
        }
    }

    /// Runs a linked account: its chats, each new message, and the window's orders, until it ends.
    pub async fn run(self, secret: String, _me: String, tx: mpsc::Sender<Heard>, mut orders: mpsc::Receiver<Order>) -> Result<()> {
        match self {
            Way::Real => {
                let kept: Kept = serde_json::from_str(&secret).map_err(|_| Error::Invalid("This account's sign-in could not be read. Link it again.".into()))?;
                let mut c = connect(kept);
                let ended = run_real(&mut c, &tx, &mut orders).await;
                c.client.disconnect();
                c.pool.abort();
                if let Some(secret) = c.session.take_changed() {
                    let _ = tx.send(Heard::Keep(secret)).await;
                }
                ended
            }
            #[cfg(any(test, debug_assertions))]
            Way::StandIn(s) => {
                let chats = vec![
                    NewChat { id: "100".into(), name: "Charles".into(), kind: "direct".into(), place: None },
                    NewChat { id: "-200".into(), name: "Heron Watch".into(), kind: "group".into(), place: None },
                ];
                let _ = tx.send(Heard::Chats(chats)).await;
                let line = Line { chat: "100".into(), id: "1".into(), from_id: "100".into(), from_name: "Charles".into(), mine: false, text: "The engine is ready.".into(), at: 1_000 };
                let _ = tx.send(Heard::Said { chat: NewChat { id: "100".into(), name: "Charles".into(), kind: "direct".into(), place: None }, line }).await;
                while let Some(order) = orders.recv().await {
                    match order {
                        Order::Send { chat, text, done } => {
                            s.sent.lock().unwrap().push((chat.clone(), text.clone()));
                            let n = s.sent.lock().unwrap().len();
                            let _ = done.send(Ok(Line { chat, id: format!("s{n}"), from_id: "42".into(), from_name: "Ada".into(), mine: true, text, at: crate::now_ms() }));
                        }
                        Order::History { chat, done } => {
                            let _ =
                                done.send(Ok(vec![Line { chat: chat.clone(), id: "0".into(), from_id: chat, from_name: "Charles".into(), mine: false, text: "An older message.".into(), at: 500 }]));
                        }
                        Order::SignOut { done } => {
                            s.signed_out.store(true, std::sync::atomic::Ordering::Relaxed);
                            let _ = done.send(());
                            return Ok(());
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// A sign-in done: who the account is, and what to keep of it.
fn done(connected: &Connected, user: &grammers_client::peer::User, phone: &str) -> Went {
    let secret = connected.session.secret();
    connected.client.disconnect();
    connected.pool.abort();
    let handle = user.username().map(|u| format!("@{u}")).unwrap_or_else(|| phone.to_string());
    Went::Done { me: chat_id(user.id()), name: user.full_name(), handle, secret }
}

async fn run_real(c: &mut Connected, tx: &mpsc::Sender<Heard>, orders: &mut mpsc::Receiver<Order>) -> Result<()> {
    if !c.client.is_authorized().await.map_err(net)? {
        let _ = tx.send(Heard::Gone("Telegram ended this sign-in (on the phone, under Devices, or because it was not used). Link the account again.".into())).await;
        return Ok(());
    }
    // The chats, newest first, with each one's last message.
    let mut dialogs = c.client.iter_dialogs();
    let (mut chats, mut lasts, mut known) = (Vec::new(), Vec::new(), HashMap::new());
    while let Some(dialog) = dialogs.next().await.map_err(net)? {
        let chat = chat_of(dialog.peer());
        known.insert(chat.id.clone(), dialog.peer_ref());
        if let Some(m) = &dialog.last_message {
            lasts.push((chat.clone(), line_of(m)));
        }
        chats.push(chat);
        if chats.len() >= 200 {
            break;
        }
    }
    let _ = tx.send(Heard::Chats(chats)).await;
    for (chat, line) in lasts {
        let _ = tx.send(Heard::Said { chat, line }).await;
    }
    let updates = c.updates.take().ok_or_else(|| Error::Other("no updates".into()))?;
    let mut stream = c.client.stream_updates(updates, UpdatesConfiguration { catch_up: false }).await.map_err(net)?;
    let mut keep = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            update = stream.next() => {
                if let Update::NewMessage(m) = update.map_err(net)? {
                    let chat = m.peer().map(chat_of).unwrap_or(NewChat { id: chat_id(m.peer_id()), name: "A conversation".into(), kind: "direct".into(), place: None });
                    if let Ok(Some(r)) = m.peer_ref().await { known.insert(chat.id.clone(), r); }
                    let _ = tx.send(Heard::Said { chat, line: line_of(&m) }).await;
                }
            },
            order = orders.recv() => match order {
                Some(Order::Send { chat, text, done }) => {
                    let went = match known.get(&chat) {
                        Some(peer) => c.client.send_message(*peer, text.as_str()).await.map(|m| line_of(&m)).map_err(net),
                        None => Err(Error::Invalid("Demido does not know this chat at Telegram yet.".into())),
                    };
                    let _ = done.send(went);
                }
                Some(Order::History { chat, done }) => {
                    let went = match known.get(&chat) {
                        Some(peer) => {
                            let mut messages = c.client.iter_messages(*peer).limit(50);
                            let mut lines = Vec::new();
                            loop {
                                match messages.next().await {
                                    Ok(Some(m)) => lines.push(line_of(&m)),
                                    Ok(None) => break Ok(lines.into_iter().rev().collect()),
                                    Err(e) => break Err(net(e)),
                                }
                            }
                        }
                        None => Err(Error::Invalid("Demido does not know this chat at Telegram yet.".into())),
                    };
                    let _ = done.send(went);
                }
                Some(Order::SignOut { done }) => {
                    let _ = c.client.sign_out().await;
                    let _ = done.send(());
                    return Ok(());
                }
                None => return Ok(()),
            },
            _ = keep.tick() => {
                if let Some(secret) = c.session.take_changed() {
                    let _ = tx.send(Heard::Keep(secret)).await;
                }
            }
        }
    }
}
