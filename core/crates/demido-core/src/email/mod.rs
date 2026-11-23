//! Email: reading by IMAP and sending by SMTP, in the person's name (DECISIONS Y, research note 26).
//!
//! - **An account is the person's own**: an address, its servers, and its password (for Gmail an app password,
//!   Y1), kept in the vault as `email.<account>` like a key (U), never shown again and never seen by a model.
//! - **Reading only reads.** A mailbox is opened with `EXAMINE`, read-only, and a message fetched with
//!   `BODY.PEEK`, so nothing is marked as read, moved or deleted. What was fetched is kept in Demido's database
//!   (the caching rule, P): asking again fetches only what is newer, and a message's text once.
//! - **What other people write is text from outside** (T10, T11, N4): cleaned, fenced and followed by the
//!   person's request when a model is given it. A mail's pictures are never loaded (X5c): it is given as text.
//! - **Sending cannot be undone**, so it asks the person at Cautious and Balanced, and the model at Smart (T6), as
//!   every reach out of Demido does; never "yes for the session". A schedule sends without asking (J2), and says
//!   what it sent, to whom.
//!
//! Only TLS: IMAP on its own port (993), SMTP on 465, or 587 with STARTTLS. Certificates are checked against the
//! system's own. A test's stand-in on this computer may be spoken to in the clear (`stand_in`).

#[cfg(any(test, debug_assertions))]
pub mod stand_in;

use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::rustls;
use utoipa::ToSchema;

use crate::db::Db;
use crate::events::EventLog;
use crate::vault::Vault;
use crate::{Error, Result, now_ms};

/// How many of the newest messages in a mailbox are listed.
pub const MOST_LISTED: usize = 50;
/// How much of a message's text is kept and given.
const MOST_TEXT: usize = 200_000;
const TAKES_AT_MOST: Duration = Duration::from_secs(30);

/// An email account, as the person and the window see it. Never its password.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Account {
    pub id: String,
    pub address: String,
    /// The person's own name for it.
    pub label: String,
    /// `gmail` or `other`.
    pub preset: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    /// Where its password is kept, and its last four characters.
    pub password: Option<crate::vault::KeyFacts>,
    pub created_at: i64,
}

/// An account as the person gives it. For Gmail, the address and the app password are enough.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct NewAccount {
    pub address: String,
    #[serde(default)]
    pub label: Option<String>,
    /// `gmail` or `other`.
    #[serde(default = "gmail")]
    pub preset: String,
    #[serde(default)]
    pub imap_host: Option<String>,
    #[serde(default)]
    pub imap_port: Option<u16>,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    /// The name to sign in with, when it is not the address.
    #[serde(default)]
    pub username: Option<String>,
    /// The password, or for Gmail the app password. Kept in the vault, never given back.
    pub password: String,
}

fn gmail() -> String {
    "gmail".into()
}

/// One message in a list: who, what about, when. Its text only once it was opened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Summary {
    pub account: String,
    pub folder: String,
    pub uid: u32,
    pub from: String,
    pub to: String,
    pub subject: String,
    /// Milliseconds since 1970, from its `Date`.
    pub date: Option<i64>,
    pub seen: bool,
    pub size: u32,
}

/// A message opened: its summary, its text, the names of its files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Opened {
    #[serde(flatten)]
    pub summary: Summary,
    pub text: String,
    pub files: Vec<String>,
}

/// A message to send, as the person or the model wrote it.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct Outgoing {
    pub to: String,
    pub subject: String,
    pub text: String,
}

/// How a server is spoken to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Wire {
    Tls,
    /// SMTP on 587: in the clear until STARTTLS, which is required.
    StartTls,
    /// Only a stand-in on this computer, in a test or a development build's trial.
    #[cfg(any(test, debug_assertions))]
    Plain,
}

pub struct Mail {
    db: Arc<Db>,
    vault: Arc<Vault>,
    events: Arc<EventLog>,
    /// A test's or a trial's stand-ins may be spoken to in the clear.
    #[cfg(any(test, debug_assertions))]
    plain: bool,
}

fn vault_name(account: &str) -> String {
    format!("email.{account}")
}

/// The TLS settings for every mail server: the system's own certificates, Rust's own crypto.
fn tls() -> Result<Arc<rustls::ClientConfig>> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_platform_verifier())
        .map_err(|e| Error::Other(format!("TLS could not be set up ({e})")))?
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Who a message is from or to, in a line: "Ada Lovelace <ada@example.org>".
fn people(addresses: Option<mail_parser::AddressList<'_>>) -> String {
    let Some(addresses) = addresses else { return String::new() };
    addresses
        .mailboxes()
        .map(|a| match (a.name(), a.address()) {
            (Some(name), Some(address)) => format!("{name} <{address}>"),
            (None, Some(address)) => address.to_string(),
            (Some(name), None) => name.to_string(),
            (None, None) => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A message's readable text: its plain part, or its HTML part read as a page is read; and its files' names.
fn text_of(raw: &[u8]) -> (String, Vec<String>) {
    let Some(message) = mail_parser::MessageParser::default().parse(raw) else { return ("(The message could not be read.)".into(), Vec::new()) };
    let text = match message.body_text(0) {
        Some(text) if !text.trim().is_empty() => text.into_owned(),
        _ => match message.body_html(0) {
            Some(html) => crate::web::page::readable(&html, "").text,
            None => String::new(),
        },
    };
    let mut text = text.replace('\r', "");
    if let Some((at, _)) = text.char_indices().nth(MOST_TEXT) {
        text.truncate(at);
    }
    let files = message.attachments().filter_map(|a| a.attachment_name().map(str::to_string)).collect();
    (text, files)
}

/// What a fetched header says, as a summary.
fn summary_of(account: &str, folder: &str, uid: u32, seen: bool, size: u32, header: &[u8]) -> Summary {
    let parsed = mail_parser::MessageParser::default().parse_headers(header);
    let (from, to, subject, date) = match &parsed {
        Some(m) => (people(m.from()), people(m.to()), m.subject().unwrap_or_default().to_string(), m.date().map(|d| d.to_timestamp() * 1000)),
        None => (String::new(), String::new(), String::new(), None),
    };
    Summary { account: account.into(), folder: folder.into(), uid, from, to, subject: if subject.trim().is_empty() { "(no subject)".into() } else { subject }, date, seen, size }
}

impl Mail {
    pub fn new(db: Arc<Db>, vault: Arc<Vault>, events: Arc<EventLog>) -> Self {
        Self {
            db,
            vault,
            events,
            #[cfg(any(test, debug_assertions))]
            plain: std::env::var("DEMIDO_DEV_MAIL_PLAIN").is_ok_and(|v| v == "1"),
        }
    }

    /// The same, speaking to stand-ins on this computer in the clear. For tests.
    #[cfg(test)]
    pub(crate) fn in_the_clear(mut self) -> Self {
        self.plain = true;
        self
    }

    fn wire(&self, host: &str, port: u16, smtp: bool) -> Result<Wire> {
        #[cfg(any(test, debug_assertions))]
        if self.plain && (host == "127.0.0.1" || host == "localhost") {
            return Ok(Wire::Plain);
        }
        let _ = host;
        Ok(match (smtp, port) {
            (true, 587) => Wire::StartTls,
            (_, 993 | 465) => Wire::Tls,
            _ => return Err(Error::Invalid(format!("Demido speaks to mail servers only over TLS: IMAP on 993, SMTP on 465 or 587. Port {port} is not one of them."))),
        })
    }

    fn row(&self, r: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
        let id: String = r.get(0)?;
        Ok(Account {
            password: self.vault.facts(&vault_name(&id)),
            id,
            address: r.get(1)?,
            label: r.get(2)?,
            preset: r.get(3)?,
            imap_host: r.get(4)?,
            imap_port: r.get::<_, i64>(5)? as u16,
            smtp_host: r.get(6)?,
            smtp_port: r.get::<_, i64>(7)? as u16,
            username: r.get(8)?,
            created_at: r.get(9)?,
        })
    }

    /// The accounts, the oldest first: the first is the one models use.
    pub fn accounts(&self) -> Result<Vec<Account>> {
        self.db.with(|c| {
            c.prepare("SELECT id, address, label, preset, imap_host, imap_port, smtp_host, smtp_port, username, created_at FROM email_accounts ORDER BY created_at, rowid")?
                .query_map([], |r| self.row(r))?
                .collect()
        })
    }

    pub fn account(&self, id: &str) -> Result<Account> {
        self.accounts()?.into_iter().find(|a| a.id == id).ok_or_else(|| Error::NotFound(format!("There is no email account \"{id}\" in Demido.")))
    }

    fn password(&self, account: &Account) -> Result<String> {
        self.vault.get(&vault_name(&account.id))?.ok_or_else(|| Error::Invalid(format!("The account {} has no password any more. Add it again on the Email screen.", account.address)))
    }

    /// Adds an account, once it has signed in to its IMAP server with the password given.
    pub async fn add(&self, new: NewAccount) -> Result<Account> {
        let address = new.address.trim().to_string();
        if !address.contains('@') || address.len() > 254 {
            return Err(Error::Invalid("That is not an email address.".into()));
        }
        let gmail = new.preset == "gmail";
        let (imap_host, imap_port, smtp_host, smtp_port) = if gmail {
            ("imap.gmail.com".to_string(), 993, "smtp.gmail.com".to_string(), 465)
        } else {
            let host = |h: Option<String>, what: &str| h.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).ok_or_else(|| Error::Invalid(format!("Say the {what} server's name.")));
            (host(new.imap_host, "IMAP")?, new.imap_port.unwrap_or(993), host(new.smtp_host, "SMTP")?, new.smtp_port.unwrap_or(465))
        };
        // Gmail's app password is shown with spaces; it is the letters that count.
        let password = if gmail { new.password.split_whitespace().collect::<String>() } else { new.password.clone() };
        if password.is_empty() {
            return Err(Error::Invalid("Give the password (for Gmail, an app password).".into()));
        }
        let username = new.username.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| address.clone());
        self.wire(&imap_host, imap_port, false)?;
        self.wire(&smtp_host, smtp_port, true)?;
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let label = new.label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).unwrap_or_else(|| address.clone());
        let account = Account {
            id: id.clone(),
            address,
            label,
            preset: if gmail { "gmail".into() } else { "other".into() },
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            username,
            password: None,
            created_at: now_ms(),
        };
        // Signed in first: an account that cannot sign in is not kept.
        let mut session = self.open(&account, &password).await.map_err(|e| match e {
            Error::Invalid(why) if gmail => Error::Invalid(format!("{why} For Gmail, the password is an app password, made in your Google account with 2-Step Verification on; it does not work for work or school accounts, accounts with security keys only, or Advanced Protection.")),
            other => other,
        })?;
        let _ = session.logout().await;
        let vault = self.vault.clone();
        let (name, value) = (vault_name(&id), password);
        tokio::task::spawn_blocking(move || vault.put(&name, &value)).await.map_err(|e| Error::Other(e.to_string()))??;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO email_accounts (id, address, label, preset, imap_host, imap_port, smtp_host, smtp_port, username, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    account.id,
                    account.address,
                    account.label,
                    account.preset,
                    account.imap_host,
                    account.imap_port as i64,
                    account.smtp_host,
                    account.smtp_port as i64,
                    account.username,
                    account.created_at
                ],
            )
        })?;
        self.events.append("email.account.added", Some(&account.id), json!({ "address": account.address, "preset": account.preset }))?;
        self.account(&id)
    }

    /// Deletes an account, its password and what was kept of its mail. The mailbox itself is not touched.
    pub fn remove(&self, id: &str) -> Result<Account> {
        let account = self.account(id)?;
        let _ = self.vault.remove(&vault_name(id));
        self.db.with(|c| c.execute("DELETE FROM email_accounts WHERE id = ?1", [id]))?;
        self.events.append("email.account.removed", Some(id), json!({ "address": account.address }))?;
        Ok(account)
    }

    /// Signs in to an account's IMAP server.
    async fn open(&self, account: &Account, password: &str) -> Result<async_imap::Session<Box<dyn Stream>>> {
        let wire = self.wire(&account.imap_host, account.imap_port, false)?;
        let unreachable = |e: std::io::Error| Error::Net(format!("{}:{} could not be reached ({e}).", account.imap_host, account.imap_port));
        let tcp = tokio::time::timeout(TAKES_AT_MOST, tokio::net::TcpStream::connect((account.imap_host.as_str(), account.imap_port)))
            .await
            .map_err(|_| Error::Net(format!("{} did not answer in time.", account.imap_host)))?
            .map_err(unreachable)?;
        let stream: Box<dyn Stream> = match wire {
            #[cfg(any(test, debug_assertions))]
            Wire::Plain => Box::new(tcp),
            _ => {
                let name = rustls::pki_types::ServerName::try_from(account.imap_host.clone()).map_err(|_| Error::Invalid(format!("\"{}\" is not a server's name.", account.imap_host)))?;
                Box::new(tokio_rustls::TlsConnector::from(tls()?).connect(name, tcp).await.map_err(|e| Error::Net(format!("{} could not be reached over TLS ({e}).", account.imap_host)))?)
            }
        };
        let mut client = async_imap::Client::new(stream);
        // The server's greeting comes first.
        let _ = tokio::time::timeout(TAKES_AT_MOST, client.read_response()).await.map_err(|_| Error::Net(format!("{} said nothing.", account.imap_host)))?;
        match tokio::time::timeout(TAKES_AT_MOST, client.login(&account.username, password)).await {
            Err(_) => Err(Error::Net(format!("{} did not answer in time.", account.imap_host))),
            Ok(Err((e, _))) => Err(Error::Invalid(format!("{} refused the sign-in ({}).", account.imap_host, e.to_string().chars().take(160).collect::<String>()))),
            Ok(Ok(session)) => Ok(session),
        }
    }

    /// The messages kept of a mailbox, the newest first. Nothing is asked of the server.
    pub fn kept(&self, account: &str, folder: &str) -> Result<Vec<Summary>> {
        self.db.with(|c| {
            c.prepare(
                "SELECT account, folder, uid, from_who, to_who, subject, date, seen, size FROM email_messages WHERE account = ?1 AND folder = ?2 ORDER BY coalesce(date, 0) DESC, uid DESC LIMIT ?3",
            )?
            .query_map(rusqlite::params![account, folder, MOST_LISTED as i64], |r| {
                Ok(Summary {
                    account: r.get(0)?,
                    folder: r.get(1)?,
                    uid: r.get::<_, i64>(2)? as u32,
                    from: r.get(3)?,
                    to: r.get(4)?,
                    subject: r.get(5)?,
                    date: r.get(6)?,
                    seen: r.get::<_, i64>(7)? == 1,
                    size: r.get::<_, i64>(8)? as u32,
                })
            })?
            .collect()
        })
    }

    /// Asks the server for what is new in a mailbox, keeps it, and gives the newest. Read-only: `EXAMINE`, and
    /// only the headers; nothing is marked as read.
    pub async fn refresh(&self, account_id: &str, folder: &str) -> Result<Vec<Summary>> {
        let account = self.account(account_id)?;
        let password = self.password(&account)?;
        let mut session = self.open(&account, &password).await?;
        let mailbox = session.examine(folder).await.map_err(|e| Error::Invalid(format!("The mailbox \"{folder}\" could not be opened ({e}).")))?;
        let validity = mailbox.uid_validity.unwrap_or(0) as i64;
        // A mailbox whose numbering started again: what was kept of it is about other messages now.
        self.db.with(|c| c.execute("DELETE FROM email_messages WHERE account = ?1 AND folder = ?2 AND uid_validity != ?3", rusqlite::params![account.id, folder, validity]))?;
        let known: i64 = self.db.with(|c| c.query_row("SELECT coalesce(max(uid), 0) FROM email_messages WHERE account = ?1 AND folder = ?2", rusqlite::params![account.id, folder], |r| r.get(0)))?;
        if mailbox.exists > 0 {
            // The newest by their numbers in the mailbox; only those not kept yet are taken.
            let from = mailbox.exists.saturating_sub(MOST_LISTED as u32 - 1).max(1);
            let fetched: Vec<async_imap::types::Fetch> = session
                .fetch(format!("{from}:{}", mailbox.exists), "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER])")
                .await
                .map_err(|e| Error::Net(e.to_string()))?
                .try_collect()
                .await
                .map_err(|e| Error::Net(e.to_string()))?;
            let now = now_ms();
            for f in fetched {
                let Some(uid) = f.uid else { continue };
                if (uid as i64) <= known {
                    continue;
                }
                let seen = f.flags().any(|fl| matches!(fl, async_imap::types::Flag::Seen));
                let s = summary_of(&account.id, folder, uid, seen, f.size.unwrap_or(0), f.header().unwrap_or_default());
                self.db.with(|c| {
                    c.execute(
                        "INSERT OR IGNORE INTO email_messages (account, folder, uid_validity, uid, from_who, to_who, subject, date, seen, size, kept_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        rusqlite::params![s.account, s.folder, validity, s.uid as i64, s.from, s.to, s.subject, s.date, s.seen as i64, s.size as i64, now],
                    )
                })?;
            }
        }
        let _ = session.logout().await;
        self.kept(&account.id, folder)
    }

    /// Opens a message: its text kept from before, or fetched once with `BODY.PEEK[]`, which does not mark it read.
    pub async fn open_message(&self, account_id: &str, folder: &str, uid: u32) -> Result<Opened> {
        let summary = self.kept(account_id, folder)?.into_iter().find(|s| s.uid == uid);
        let kept: Option<(String, String)> = self.db.with(|c| {
            c.query_row("SELECT text, files FROM email_messages WHERE account = ?1 AND folder = ?2 AND uid = ?3 AND text IS NOT NULL", rusqlite::params![account_id, folder, uid as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
        })?;
        let summary = summary.ok_or_else(|| Error::NotFound("That message is not among those Demido has listed. List the mailbox again.".into()))?;
        if let Some((text, files)) = kept {
            return Ok(Opened { summary, text, files: serde_json::from_str(&files).unwrap_or_default() });
        }
        let account = self.account(account_id)?;
        let password = self.password(&account)?;
        let mut session = self.open(&account, &password).await?;
        session.examine(folder).await.map_err(|e| Error::Invalid(format!("The mailbox \"{folder}\" could not be opened ({e}).")))?;
        let fetched: Vec<async_imap::types::Fetch> =
            session.uid_fetch(uid.to_string(), "BODY.PEEK[]").await.map_err(|e| Error::Net(e.to_string()))?.try_collect().await.map_err(|e| Error::Net(e.to_string()))?;
        let _ = session.logout().await;
        let raw = fetched.iter().find_map(|f| f.body().map(<[u8]>::to_vec)).ok_or_else(|| Error::NotFound("The server has no such message any more.".into()))?;
        let (text, files) = tokio::task::spawn_blocking(move || text_of(&raw)).await.map_err(|e| Error::Other(e.to_string()))?;
        self.db.with(|c| {
            c.execute(
                "UPDATE email_messages SET text = ?4, files = ?5 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                rusqlite::params![account_id, folder, uid as i64, text, serde_json::to_string(&files).unwrap_or_default()],
            )
        })?;
        Ok(Opened { summary, text, files })
    }

    /// Sends a message from an account. It cannot be taken back: whether it may go was decided before.
    pub async fn send(&self, account_id: &str, out: &Outgoing) -> Result<()> {
        use lettre::AsyncTransport;
        use lettre::transport::smtp::client::{Tls, TlsParameters};
        let account = self.account(account_id)?;
        let password = self.password(&account)?;
        let from: lettre::message::Mailbox = account.address.parse().map_err(|_| Error::Invalid(format!("{} is not an address mail can be sent from.", account.address)))?;
        let mut message = lettre::Message::builder().from(from).subject(out.subject.trim());
        let mut to_count = 0;
        for to in out.to.split([',', ';']).map(str::trim).filter(|t| !t.is_empty()) {
            message = message.to(to.parse().map_err(|_| Error::Invalid(format!("\"{to}\" is not an email address.")))?);
            to_count += 1;
        }
        if to_count == 0 {
            return Err(Error::Invalid("A message needs someone to go to.".into()));
        }
        let message = message.header(lettre::message::header::ContentType::TEXT_PLAIN).body(out.text.clone()).map_err(|e| Error::Invalid(format!("The message could not be made ({e}).")))?;
        let wire = self.wire(&account.smtp_host, account.smtp_port, true)?;
        let tls = || TlsParameters::builder(account.smtp_host.clone()).build_rustls().map_err(|e| Error::Other(format!("TLS could not be set up ({e})")));
        let tls = match wire {
            Wire::Tls => Tls::Wrapper(tls()?),
            Wire::StartTls => Tls::Required(tls()?),
            #[cfg(any(test, debug_assertions))]
            Wire::Plain => Tls::None,
        };
        let transport = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&account.smtp_host)
            .port(account.smtp_port)
            .tls(tls)
            .credentials(lettre::transport::smtp::authentication::Credentials::new(account.username.clone(), password))
            .timeout(Some(TAKES_AT_MOST))
            .build();
        transport.send(message).await.map_err(|e| Error::Net(format!("The message was not sent: {} said no ({e}).", account.smtp_host)))?;
        self.events.append("email.sent", Some(&account.id), json!({ "to": out.to, "subject": out.subject, "characters": out.text.chars().count() }))?;
        Ok(())
    }
}

/// The inbox as a model is given it: numbered, the newest first.
pub fn listed(account: &Account, list: &[Summary]) -> String {
    if list.is_empty() {
        return format!("The inbox of {} is empty.", account.address);
    }
    let mut out = format!("The newest messages in the inbox of {}, the newest first (read_mail with a number):\n", account.address);
    for (n, s) in list.iter().enumerate() {
        let when = s.date.and_then(|d| jiff::Timestamp::from_millisecond(d).ok()).map(|t| t.to_zoned_datetime(jiff::tz::TimeZone::system()).strftime("%-d %b %H:%M").to_string()).unwrap_or_default();
        out.push_str(&format!("{}. {} — {} ({when}){}\n", n + 1, s.from, s.subject, if s.seen { "" } else { ", unread" }));
    }
    out.trim_end().to_string()
}

/// A message as a model is given it: who, what about, and its text. Said to be someone else's words.
pub fn told(m: &Opened) -> String {
    let mut out = format!("A message to the person, written by someone else:\nFrom: {}\nTo: {}\nSubject: {}\n", m.summary.from, m.summary.to, m.summary.subject);
    if !m.files.is_empty() {
        out.push_str(&format!("Files with it (not opened): {}\n", m.files.join(", ")));
    }
    out.push('\n');
    out.push_str(if m.text.trim().is_empty() { "(The message has no text.)" } else { &m.text });
    out
}

/// What an IMAP connection runs over: TLS, or for a stand-in, plain TCP.
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + std::fmt::Debug> Stream for T {}

#[cfg(test)]
mod tests;
