//! The person's own WhatsApp, Telegram and Discord accounts (DECISIONS G17 to G25; research note 37).
//!
//! Bots (`bots`) are the services' allowed way and their own accounts. Here is the other way, which the owner
//! decided on behind a warning (G2): the person links their own account, and Demido reads their chats and sends
//! as them when they say so in the window. Each service is spoken to by the core alone:
//!
//! - **Discord**, with the account's token, on Discord's REST API and Gateway (`discord`). Discord forbids it.
//! - **Telegram**, by MTProto through `grammers`, with the person's own api_id, their phone number, the code
//!   Telegram sends them and their password if they have one (`telegram`).
//! - **WhatsApp**, as a linked device, through a helper program beside the core (`whatsapp`), because its library
//!   cannot be built into the core (note 37: one SQLite per program).
//!
//! Every account, its chats and what was said in them are kept in the database; each account's secret (a token,
//! a session) is kept in the vault (`linked.<id>`), never given back. What other people write is data (G6).
//! Nothing here marks a message as read at the service: a chat's unread count is Demido's own.

pub mod discord;
pub mod telegram;
pub mod whatsapp;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use utoipa::ToSchema;

use crate::db::Db;
use crate::events::EventLog;
use crate::vault::Vault;
use crate::{Error, Result, now_ms};

/// A linked account, as the window sees it. Never its secret.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    /// `whatsapp`, `telegram` or `discord`.
    pub service: String,
    /// The name the service shows for it.
    pub name: String,
    /// Its username or phone number there.
    pub handle: String,
    /// `linking` (a step is waiting for the person) or `linked`.
    pub state: String,
    /// What linking waits for, while it does.
    pub step: Option<Step>,
    /// Whether its connection to the service is running now.
    pub running: bool,
    /// Why it is not, when it stopped.
    pub problem: Option<String>,
    pub secret: Option<crate::vault::KeyFacts>,
    pub created_at: i64,
}

/// What linking an account waits for: a code the service sent the person, their password, or the phone to scan
/// or type a code Demido shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Step {
    /// `code` (Telegram's, sent to the person's app), `password` (the account's own), `qr` (to scan with the
    /// phone) or `pair_code` (to type on the phone).
    pub kind: String,
    /// The password's hint, where the account has one.
    pub hint: Option<String>,
    /// The QR code as an SVG picture, or the code to type on the phone.
    pub show: Option<String>,
    /// What went wrong with the last answer, if anything.
    pub problem: Option<String>,
}

/// One chat of an account: a direct conversation, a group, or a server's channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Chat {
    pub account: String,
    pub id: String,
    pub name: String,
    /// `direct`, `group` or `channel`.
    pub kind: String,
    /// The server a channel is in (Discord).
    pub place: Option<String>,
    pub last_at: Option<i64>,
    pub last_text: String,
    /// New messages since the person last opened it in Demido. Demido's own count: nothing is marked at the service.
    pub unread: i64,
}

/// One message of a chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Line {
    pub chat: String,
    pub id: String,
    pub from_id: String,
    pub from_name: String,
    /// Written by the person, here or anywhere else they use the account.
    pub mine: bool,
    pub text: String,
    pub at: i64,
}

/// A chat as a service names it, before it is kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewChat {
    pub id: String,
    /// Its name; empty, or a placeholder, when the service gave none (`is_placeholder`).
    pub name: String,
    pub kind: String,
    pub place: Option<String>,
    /// How many are unread, as the service counts it, where it says.
    pub unread: Option<i64>,
    /// A name that stands in for a better one (a sender's own name for a direct chat): it replaces only a
    /// placeholder, never a chat's own name.
    pub weak: bool,
}

/// What stands for a name the service did not give: nothing, "A conversation", "someone", or the id's number.
pub fn is_placeholder(name: &str, id: &str) -> bool {
    let name = name.trim();
    name.is_empty() || name == "A conversation" || name == "someone" || name == id.split('@').next().unwrap_or(id)
}

/// The same, in SQL, for a column of names beside a column of ids.
const PLACEHOLDER: &str = "(TRIM({n}) IN ('', 'A conversation', 'someone') OR {n} = CASE WHEN INSTR({i}, '@') > 0 THEN SUBSTR({i}, 1, INSTR({i}, '@') - 1) ELSE {i} END)";

fn placeholder_sql(name: &str, id: &str) -> String {
    PLACEHOLDER.replace("{n}", name).replace("{i}", id)
}

/// An account to link.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct NewLink {
    /// `whatsapp`, `telegram` or `discord`.
    pub service: String,
    /// The person ticked that they read what the service's rules say and what it risks (G2).
    #[serde(default)]
    pub understood: bool,
    /// Discord: the account's token.
    #[serde(default)]
    pub token: Option<String>,
    /// Telegram: the person's own api_id and api_hash (my.telegram.org).
    #[serde(default)]
    pub api_id: Option<i32>,
    #[serde(default)]
    pub api_hash: Option<String>,
    /// Telegram: the phone number to sign in with. WhatsApp: given, a code to type on the phone instead of a QR code.
    #[serde(default)]
    pub phone: Option<String>,
}

/// The person's answer to a step.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct Answer {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// What a service's connection tells the store.
#[derive(Debug)]
pub enum Heard {
    /// Chats it knows: added, or renamed.
    Chats(Vec<NewChat>),
    /// A message, in a chat that may be new. `count`: a new one, counted as unread when it is someone else's;
    /// not one the service sent from before (a history).
    Said { chat: NewChat, line: Line, count: bool },
    /// Who someone is, by any of their ids: from the person's address book, or (`weak`) the name they gave
    /// themselves, which replaces only a placeholder.
    Name { ids: Vec<String>, name: String, weak: bool },
    /// Linking waits for the person (a QR code, a code).
    Step(Step),
    /// Linking is done: who the account is.
    Linked { me: String, name: String, handle: String },
    /// The service ended the link (signed out on the phone, or the account is gone).
    Gone(String),
    /// A new secret to keep in place of the old one (Telegram's session, after it changed).
    Keep(String),
}

/// What the window asks of a running connection.
#[derive(Debug)]
pub enum Order {
    Send {
        chat: String,
        text: String,
        done: oneshot::Sender<Result<Line>>,
    },
    History {
        chat: String,
        done: oneshot::Sender<Result<Vec<Line>>>,
    },
    /// The person removes the account: sign out at the service where it can be done.
    SignOut {
        done: oneshot::Sender<()>,
    },
}

fn vault_name(id: &str) -> String {
    format!("linked.{id}")
}

/// The longest message each service takes, in characters.
pub fn longest(service: &str) -> usize {
    match service {
        "discord" => 2_000,
        "telegram" => 4_096,
        _ => 65_536,
    }
}

#[derive(Default)]
struct Running {
    tasks: HashMap<String, tokio::task::JoinHandle<()>>,
    problems: HashMap<String, String>,
    steps: HashMap<String, Step>,
    orders: HashMap<String, mpsc::Sender<Order>>,
    /// Telegram sign-ins under way: the connection, waiting for a code or a password.
    telegram: HashMap<String, telegram::Pending>,
}

pub struct Linked {
    db: Arc<Db>,
    vault: Arc<Vault>,
    events: Arc<EventLog>,
    /// Where a helper keeps what it must (WhatsApp's keys, `linked/<id>/`).
    home: PathBuf,
    running: Mutex<Running>,
    /// Where each service is: a test points them at stand-ins.
    pub(crate) discord_at: String,
    pub(crate) discord_gateway: String,
    pub(crate) telegram: telegram::Way,
    pub(crate) whatsapp: whatsapp::Way,
}

const COLUMNS: &str = "id, service, name, handle, me, state, created_at";

impl Linked {
    pub fn new(db: Arc<Db>, vault: Arc<Vault>, events: Arc<EventLog>, home: PathBuf) -> Self {
        let helper = whatsapp::Way::Helper(whatsapp::helper_beside(std::env::current_exe().ok()));
        #[allow(unused_mut)]
        let mut me = Self {
            db,
            vault,
            events,
            home,
            running: Mutex::default(),
            discord_at: crate::bots::discord::API.into(),
            discord_gateway: discord::GATEWAY.into(),
            telegram: telegram::Way::Real,
            whatsapp: helper,
        };
        // A development build's trial of the window, with no real account: Telegram and WhatsApp stood in for.
        #[cfg(debug_assertions)]
        if std::env::var("DEMIDO_DEV_LINKED").as_deref() == Ok("stand-in") {
            tracing::warn!("Development build: Telegram and WhatsApp are stand-ins (code 12345, password heron).");
            me.telegram = telegram::Way::StandIn(Arc::new(telegram::StandIn { code: "12345".into(), password: Some("heron".into()), ..telegram::StandIn::default() }));
            me.whatsapp = whatsapp::Way::StandIn(Arc::default());
        }
        me
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn row(&self, r: &rusqlite::Row<'_>) -> rusqlite::Result<(Account, String)> {
        let id: String = r.get(0)?;
        let running = self.lock();
        let me: String = r.get(4)?;
        Ok((
            Account {
                secret: self.vault.facts(&vault_name(&id)),
                running: running.tasks.get(&id).is_some_and(|t| !t.is_finished()),
                problem: running.problems.get(&id).cloned(),
                step: running.steps.get(&id).cloned(),
                service: r.get(1)?,
                name: r.get(2)?,
                handle: r.get(3)?,
                state: r.get(5)?,
                created_at: r.get(6)?,
                id,
            },
            me,
        ))
    }

    fn rows(&self) -> Result<Vec<(Account, String)>> {
        self.db.with(|c| c.prepare(&format!("SELECT {COLUMNS} FROM linked_accounts ORDER BY created_at, rowid"))?.query_map([], |r| self.row(r))?.collect())
    }

    pub fn all(&self) -> Result<Vec<Account>> {
        Ok(self.rows()?.into_iter().map(|(a, _)| a).collect())
    }

    pub fn get(&self, id: &str) -> Result<Account> {
        Ok(self.with_me(id)?.0)
    }

    /// An account and its own id at the service (to tell the person's messages from others').
    fn with_me(&self, id: &str) -> Result<(Account, String)> {
        self.rows()?.into_iter().find(|(a, _)| a.id == id).ok_or_else(|| Error::NotFound(format!("There is no linked account \"{id}\".")))
    }

    fn secret(&self, id: &str) -> Result<String> {
        self.vault.get(&vault_name(id))?.ok_or_else(|| Error::Invalid("This account's sign-in is gone. Link it again.".into()))
    }

    pub(crate) fn keep_secret(&self, id: &str, secret: &str) -> Result<()> {
        let vault = self.vault.clone();
        vault.put(&vault_name(id), secret).map(|_| ())
    }

    /// The folder a helper keeps an account's files in.
    pub(crate) fn folder(&self, id: &str) -> PathBuf {
        self.home.join("linked").join(id)
    }

    fn insert(&self, id: &str, service: &str, name: &str, handle: &str, me: &str, state: &str) -> Result<()> {
        self.db.with(|c| c.execute(&format!("INSERT INTO linked_accounts ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"), rusqlite::params![id, service, name, handle, me, state, now_ms()]))?;
        Ok(())
    }

    pub(crate) fn set_step(&self, id: &str, step: Option<Step>) {
        {
            let mut running = self.lock();
            match step {
                Some(s) => running.steps.insert(id.into(), s),
                None => running.steps.remove(id),
            };
        }
        self.events.tell("linked.changed", Some(id), json!({}));
    }

    /// Links an account, once the person ticked that they understood what it means (G2).
    pub async fn link(self: &Arc<Self>, new: NewLink) -> Result<Account> {
        if !new.understood {
            return Err(Error::Invalid("First read what linking means for this account, and tick that you understood.".into()));
        }
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        match new.service.as_str() {
            "discord" => {
                let token = new.token.as_deref().map(str::trim).unwrap_or_default().trim_matches('"').to_string();
                if token.is_empty() {
                    return Err(Error::Invalid("Give the account's token.".into()));
                }
                let (me, name, handle) = discord::Client::new(&self.discord_at, &token).me().await?;
                if self.rows()?.iter().any(|(a, m)| a.service == "discord" && *m == me) {
                    return Err(Error::Invalid(format!("{name} is linked already.")));
                }
                self.keep_secret(&id, &token)?;
                self.insert(&id, "discord", &name, &handle, &me, "linked")?;
                self.events.append("linked.added", Some(&id), json!({ "service": "discord" }))?;
                self.start(&id);
            }
            "telegram" => {
                let (api_id, api_hash, phone) = match (new.api_id, new.api_hash.as_deref().map(str::trim), new.phone.as_deref().map(str::trim)) {
                    (Some(i), Some(h), Some(p)) if i > 0 && !h.is_empty() && !p.is_empty() => (i, h.to_string(), p.to_string()),
                    _ => return Err(Error::Invalid("Give the api_id and api_hash made at my.telegram.org, and the phone number of the account.".into())),
                };
                let pending = self.telegram.begin(api_id, &api_hash, &phone).await?;
                self.insert(&id, "telegram", &phone, &phone, "", "linking")?;
                self.lock().telegram.insert(id.clone(), pending);
                self.set_step(&id, Some(Step { kind: "code".into(), hint: None, show: None, problem: None }));
                self.events.append("linked.added", Some(&id), json!({ "service": "telegram" }))?;
            }
            "whatsapp" => {
                let phone = new.phone.as_deref().map(|p| p.chars().filter(char::is_ascii_digit).collect::<String>()).filter(|p| !p.is_empty());
                self.insert(&id, "whatsapp", "WhatsApp", "", "", "linking")?;
                self.events.append("linked.added", Some(&id), json!({ "service": "whatsapp", "by_code": phone.is_some() }))?;
                self.set_step(&id, Some(Step { kind: if phone.is_some() { "pair_code" } else { "qr" }.into(), hint: None, show: None, problem: None }));
                self.start_with(&id, phone);
            }
            other => return Err(Error::Invalid(format!("Accounts can be linked on whatsapp, telegram and discord, not \"{other}\"."))),
        }
        self.get(&id)
    }

    /// The person's answer to a step of linking: Telegram's code, then the password if the account has one.
    pub async fn answer(self: &Arc<Self>, id: &str, answer: Answer) -> Result<Account> {
        let account = self.get(id)?;
        if account.service != "telegram" || account.state != "linking" {
            return Err(Error::Invalid("This account is not waiting for a code or a password.".into()));
        }
        let pending =
            self.lock().telegram.remove(id).ok_or_else(|| Error::Invalid("This sign-in is not under way any more (Demido was restarted, or it took too long). Remove it and link it again.".into()))?;
        let step = self.lock().steps.get(id).cloned();
        let went = match (step.as_ref().map(|s| s.kind.as_str()), answer.code.as_deref().map(str::trim), answer.password.as_deref()) {
            (Some("code"), Some(code), _) if !code.is_empty() => self.telegram.code(pending, code).await,
            (Some("password"), _, Some(password)) if !password.is_empty() => self.telegram.password(pending, password).await,
            (_, _, _) => {
                self.lock().telegram.insert(id.into(), pending);
                return Err(Error::Invalid("Give the code Telegram sent, or the account's password where it asks for it.".into()));
            }
        };
        match went {
            telegram::Went::Done { me, name, handle, secret } => {
                self.keep_secret(id, &secret)?;
                self.db.with(|c| c.execute("UPDATE linked_accounts SET name = ?2, handle = ?3, me = ?4, state = 'linked' WHERE id = ?1", rusqlite::params![id, name, handle, me]))?;
                self.set_step(id, None);
                self.events.append("linked.linked", Some(id), json!({ "service": "telegram" }))?;
                self.start(id);
            }
            telegram::Went::Again { pending, step } => {
                self.lock().telegram.insert(id.into(), pending);
                self.set_step(id, Some(step));
            }
        }
        self.get(id)
    }

    /// Removes an account from Demido: signed out at the service where Demido can do it (Telegram, WhatsApp),
    /// then its sign-in, its chats and what was kept of them are deleted. A Discord token cannot be signed out by
    /// another program: changing the password does it.
    pub async fn unlink(&self, id: &str) -> Result<Account> {
        let account = self.get(id)?;
        let orders = self.lock().orders.get(id).cloned();
        if let Some(orders) = orders {
            let (done, wait) = oneshot::channel();
            if orders.send(Order::SignOut { done }).await.is_ok() {
                let _ = tokio::time::timeout(Duration::from_secs(10), wait).await;
            }
        }
        {
            let mut running = self.lock();
            if let Some(task) = running.tasks.remove(id) {
                task.abort();
            }
            running.orders.remove(id);
            running.steps.remove(id);
            running.problems.remove(id);
            running.telegram.remove(id);
        }
        let _ = self.vault.remove(&vault_name(id));
        let _ = std::fs::remove_dir_all(self.folder(id));
        self.db.with(|c| c.execute("DELETE FROM linked_accounts WHERE id = ?1", [id]))?;
        self.events.append("linked.removed", Some(id), json!({ "service": account.service }))?;
        Ok(account)
    }

    pub fn chats(&self, id: &str) -> Result<Vec<Chat>> {
        self.get(id)?;
        self.db.with(|c| {
            c.prepare("SELECT account, id, name, kind, place, last_at, last_text, unread FROM linked_chats WHERE account = ?1 ORDER BY last_at IS NULL, last_at DESC, place, name COLLATE NOCASE")?
                .query_map([id], |r| Ok(Chat { account: r.get(0)?, id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, place: r.get(4)?, last_at: r.get(5)?, last_text: r.get(6)?, unread: r.get(7)? }))?
                .collect()
        })
    }

    fn kept(&self, id: &str, chat: &str) -> Result<Vec<Line>> {
        self.db.with(|c| {
            c.prepare("SELECT chat, id, from_id, from_name, mine, text, at FROM (SELECT * FROM linked_messages WHERE account = ?1 AND chat = ?2 ORDER BY at DESC LIMIT 200) ORDER BY at")?
                .query_map([id, chat], |r| Ok(Line { chat: r.get(0)?, id: r.get(1)?, from_id: r.get(2)?, from_name: r.get(3)?, mine: r.get::<_, i64>(4)? != 0, text: r.get(5)?, at: r.get(6)? }))?
                .collect()
        })
    }

    /// A chat's messages, the oldest first: what was kept, or with `fresh` the newest from the service too.
    /// Opening a chat sets Demido's own unread count to none; nothing is marked at the service.
    pub async fn lines(&self, id: &str, chat: &str, fresh: bool) -> Result<Vec<Line>> {
        let (account, me) = self.with_me(id)?;
        if fresh {
            let got = match account.service.as_str() {
                "discord" => discord::Client::new(&self.discord_at, &self.secret(id)?).history(chat, &me).await?,
                _ => self.order(id, |done| Order::History { chat: chat.into(), done }).await?,
            };
            for line in &got {
                self.keep_line(id, line)?;
            }
        }
        self.db.with(|c| c.execute("UPDATE linked_chats SET unread = 0 WHERE account = ?1 AND id = ?2", [id, chat]))?;
        self.kept(id, chat)
    }

    /// Sends a message as the person, as they wrote it in the window.
    pub async fn send(&self, id: &str, chat: &str, text: &str) -> Result<Line> {
        let (account, me) = self.with_me(id)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("Write something to send.".into()));
        }
        if text.chars().count() > longest(&account.service) {
            return Err(Error::Invalid(format!("A message there is {} characters at most.", longest(&account.service))));
        }
        let line = match account.service.as_str() {
            "discord" => discord::Client::new(&self.discord_at, &self.secret(id)?).send(chat, text, &me).await?,
            _ => self.order(id, |done| Order::Send { chat: chat.into(), text: text.into(), done }).await?,
        };
        self.keep_line(id, &line)?;
        self.touch(id, chat, &line, false)?;
        self.events.append("linked.sent", Some(id), json!({ "chat": chat, "characters": text.chars().count() }))?;
        Ok(line)
    }

    async fn order<T>(&self, id: &str, make: impl FnOnce(oneshot::Sender<Result<T>>) -> Order) -> Result<T> {
        let orders = self.lock().orders.get(id).cloned().ok_or_else(|| Error::Invalid("The account is not connected now.".into()))?;
        let (done, wait) = oneshot::channel();
        orders.send(make(done)).await.map_err(|_| Error::Invalid("The account is not connected now.".into()))?;
        tokio::time::timeout(Duration::from_secs(60), wait)
            .await
            .map_err(|_| Error::Net("The service did not answer in time.".into()))?
            .map_err(|_| Error::Net("The connection ended before it answered.".into()))?
    }

    /// Keeps a chat. A placeholder never replaces a name; a weak name replaces only a placeholder.
    fn keep_chat(&self, id: &str, chat: &NewChat) -> Result<()> {
        let placeholder = is_placeholder(&chat.name, &chat.id);
        let name = if chat.name.trim().is_empty() { chat.id.split('@').next().unwrap_or(&chat.id).to_string() } else { chat.name.clone() };
        let old = placeholder_sql("linked_chats.name", "linked_chats.id");
        self.db.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO linked_chats (account, id, name, kind, place, unread) VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, 0))
                     ON CONFLICT (account, id) DO UPDATE SET
                        name = CASE WHEN ?7 THEN linked_chats.name WHEN ?8 AND NOT {old} THEN linked_chats.name ELSE excluded.name END,
                        kind = excluded.kind, place = COALESCE(excluded.place, linked_chats.place), unread = COALESCE(?6, linked_chats.unread)"
                ),
                rusqlite::params![id, chat.id, name, chat.kind, chat.place, chat.unread, placeholder, chat.weak],
            )
        })?;
        Ok(())
    }

    /// Who someone is, wherever they are named: their direct chat, and their messages in groups.
    fn keep_name(&self, id: &str, ids: &[String], name: &str, weak: bool) -> Result<()> {
        let (chats, lines) = (placeholder_sql("name", "id"), placeholder_sql("from_name", "from_id"));
        self.db.with(|c| {
            for who in ids {
                c.execute(&format!("UPDATE linked_chats SET name = ?3 WHERE account = ?1 AND id = ?2 AND kind = 'direct' AND (NOT ?4 OR {chats})"), rusqlite::params![id, who, name, weak])?;
                c.execute(&format!("UPDATE linked_messages SET from_name = ?3 WHERE account = ?1 AND from_id = ?2 AND mine = 0 AND (NOT ?4 OR {lines})"), rusqlite::params![id, who, name, weak])?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Sets every chat of an account as read in Demido: its own counts only, nothing at the service.
    pub fn all_read(&self, id: &str) -> Result<()> {
        self.get(id)?;
        self.db.with(|c| c.execute("UPDATE linked_chats SET unread = 0 WHERE account = ?1", [id]))?;
        self.events.tell("linked.chats", Some(id), json!({}));
        Ok(())
    }

    fn keep_line(&self, id: &str, line: &Line) -> Result<bool> {
        Ok(self.db.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO linked_messages (account, chat, id, from_id, from_name, mine, text, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![id, line.chat, line.id, line.from_id, line.from_name, line.mine as i64, line.text, line.at],
            )
        })? > 0)
    }

    /// A chat's newest message, and one more unread when it is someone else's.
    fn touch(&self, id: &str, chat: &str, line: &Line, count: bool) -> Result<()> {
        let short: String = line.text.chars().take(160).collect();
        self.db.with(|c| {
            c.execute(
                "UPDATE linked_chats SET last_at = MAX(COALESCE(last_at, 0), ?3), last_text = CASE WHEN ?3 >= COALESCE(last_at, 0) THEN ?4 ELSE last_text END, unread = unread + ?5 WHERE account = ?1 AND id = ?2",
                rusqlite::params![id, chat, line.at, short, (count && !line.mine) as i64],
            )
        })?;
        Ok(())
    }

    /// What a connection heard.
    pub(crate) fn heard(&self, id: &str, heard: Heard) -> Result<()> {
        match heard {
            Heard::Chats(chats) => {
                for chat in &chats {
                    self.keep_chat(id, chat)?;
                }
                self.events.tell("linked.chats", Some(id), json!({ "count": chats.len() }));
            }
            Heard::Said { chat, line, count } => {
                self.keep_chat(id, &chat)?;
                if self.keep_line(id, &line)? {
                    self.touch(id, &line.chat, &line, count)?;
                    self.events.tell("linked.message", Some(id), json!({ "chat": line.chat, "mine": line.mine }));
                }
            }
            Heard::Name { ids, name, weak } => {
                self.keep_name(id, &ids, &name, weak)?;
                self.events.tell("linked.chats", Some(id), json!({}));
            }
            Heard::Step(step) => self.set_step(id, Some(step)),
            Heard::Linked { me, name, handle } => {
                self.db.with(|c| c.execute("UPDATE linked_accounts SET name = ?2, handle = ?3, me = ?4, state = 'linked' WHERE id = ?1", rusqlite::params![id, name, handle, me]))?;
                self.set_step(id, None);
                self.events.append("linked.linked", Some(id), json!({}))?;
            }
            Heard::Keep(secret) => self.keep_secret(id, &secret)?,
            Heard::Gone(why) => {
                self.db.with(|c| c.execute("UPDATE linked_accounts SET state = 'gone' WHERE id = ?1", [id]))?;
                self.lock().problems.insert(id.into(), why.clone());
                self.events.append("linked.gone", Some(id), json!({ "why": why }))?;
            }
        }
        Ok(())
    }

    /// Starts every linked account's connection: when the server starts.
    pub fn start_all(self: &Arc<Self>) {
        for account in self.all().unwrap_or_default() {
            if account.state == "linked" {
                self.start(&account.id);
            }
        }
    }

    pub fn start(self: &Arc<Self>, id: &str) {
        self.start_with(id, None);
    }

    /// Starts an account's connection on a task of its own; one that breaks is made again, waiting longer each
    /// time, and one the service refuses is not.
    fn start_with(self: &Arc<Self>, id: &str, phone: Option<String>) {
        let me = self.clone();
        let id = id.to_string();
        let (orders, mut inbox) = mpsc::channel::<Order>(16);
        let task = tokio::spawn({
            let id = id.clone();
            async move {
                let mut wait = Duration::from_secs(5);
                loop {
                    let ended = me.run(&id, &mut inbox, phone.clone()).await;
                    let (problem, again) = match ended {
                        Err(Error::Invalid(why)) => (why, false),
                        Err(e) => (e.to_string(), true),
                        Ok(()) => ("it stopped".to_string(), false),
                    };
                    me.lock().problems.insert(id.clone(), problem.clone());
                    let _ = me.events.append("linked.stopped", Some(&id), json!({ "problem": problem, "again": again }));
                    if !again || me.get(&id).is_err() {
                        return;
                    }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(300));
                }
            }
        });
        let mut running = self.lock();
        running.problems.remove(&id);
        running.orders.insert(id.clone(), orders);
        if let Some(old) = running.tasks.insert(id, task) {
            old.abort();
        }
    }

    /// Ends every connection: when the server stops.
    pub fn end(&self) {
        let mut running = self.lock();
        running.orders.clear();
        for (_, task) in running.tasks.drain() {
            task.abort();
        }
    }

    async fn run(self: &Arc<Self>, id: &str, inbox: &mut mpsc::Receiver<Order>, phone: Option<String>) -> Result<()> {
        let (account, me) = self.with_me(id)?;
        let (tx, mut rx) = mpsc::channel::<Heard>(256);
        let listening = match account.service.as_str() {
            "discord" => tokio::spawn(discord::listen(self.discord_gateway.clone(), self.secret(id)?, me, tx)),
            "telegram" | "whatsapp" => {
                let (to, from_window) = mpsc::channel::<Order>(16);
                let task = if account.service == "telegram" {
                    tokio::spawn(self.telegram.clone().run(self.secret(id)?, me, tx, from_window))
                } else {
                    tokio::spawn(self.whatsapp.clone().run(self.folder(id), phone, tx, from_window))
                };
                if let Some(why) = self.pass_orders(id, inbox, to, &mut rx).await {
                    task.abort();
                    return Err(Error::Invalid(why));
                }
                return task.await.map_err(|e| Error::Other(e.to_string()))?;
            }
            other => return Err(Error::Invalid(format!("unknown service {other}"))),
        };
        // Discord: everything heard is kept; the window's orders go by REST, not through here.
        loop {
            tokio::select! {
                heard = rx.recv() => match heard {
                    Some(h) => { let _ = self.heard(id, h); }
                    None => break,
                },
                order = inbox.recv() => match order {
                    Some(Order::SignOut { done }) => { let _ = done.send(()); }
                    Some(Order::Send { done, .. }) => { let _ = done.send(Err(Error::Other("sent by REST".into()))); }
                    Some(Order::History { done, .. }) => { let _ = done.send(Err(Error::Other("read by REST".into()))); }
                    None => break,
                },
            }
        }
        listening.await.map_err(|e| Error::Other(e.to_string()))?
    }

    /// Keeps what a connection hears, and passes the window's orders on to it, until it ends (`None`: look at how
    /// it ended) or the service ended the link (why).
    async fn pass_orders(&self, id: &str, inbox: &mut mpsc::Receiver<Order>, to: mpsc::Sender<Order>, rx: &mut mpsc::Receiver<Heard>) -> Option<String> {
        loop {
            tokio::select! {
                heard = rx.recv() => match heard {
                    Some(Heard::Gone(why)) => { let _ = self.heard(id, Heard::Gone(why.clone())); return Some(why); }
                    Some(h) => { let _ = self.heard(id, h); }
                    None => return None,
                },
                order = inbox.recv() => match order {
                    Some(order) => { if to.send(order).await.is_err() { return None; } }
                    None => return Some("The account was removed.".into()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests;
