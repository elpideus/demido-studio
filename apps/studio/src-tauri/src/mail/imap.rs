//! One IMAP connection: TLS, login, and the few commands browsing needs.
//!
//! Folders are opened with EXAMINE (read-only) and bodies fetched with BODY.PEEK, so browsing
//! never changes anything on the server: a message opened here stays unread in Gmail.
//! Every command runs under a timeout; a connection that times out or breaks is dropped by its
//! owner and a new one made.

use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use async_imap::imap_proto::{NameAttribute, Response};
use async_imap::types::{Fetch, Flag, Mailbox, Name, UnsolicitedResponse};
use futures_util::TryStreamExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use super::body::{self, Plan};
use super::store::{ANSWERED, DRAFT, FLAGGED, FlagChange, FolderInfo, MessageMeta, SEEN};
use super::utf7;

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);

/// What a message list row needs: flags, dates, size, structure, a few header fields and the
/// first 2 KB of the body for the preview.
const LIST_ITEMS: &str = "UID FLAGS INTERNALDATE RFC822.SIZE BODYSTRUCTURE \
    BODY.PEEK[HEADER.FIELDS (DATE FROM TO CC SUBJECT MESSAGE-ID CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] \
    BODY.PEEK[TEXT]<0.2048>";

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// The server refused the login.
    #[error("{0}")]
    Auth(String),
    /// The server cannot be reached or the connection broke: worth one more try on a new one.
    #[error("{0}")]
    Network(String),
    /// The server answered a command with an error.
    #[error("{0}")]
    Server(String),
}

impl From<async_imap::error::Error> for MailError {
    fn from(e: async_imap::error::Error) -> Self {
        use async_imap::error::Error;
        match e {
            Error::No(text) | Error::Bad(text) => MailError::Server(server_text(&text)),
            Error::Validate(e) => MailError::Server(e.to_string()),
            Error::Io(e) => MailError::Network(format!("The connection to the mail server broke ({e}).")),
            other => MailError::Network(format!("The connection to the mail server broke ({other}).")),
        }
    }
}

/// A server's message without its `[CODE]` prefix.
fn server_text(text: &str) -> String {
    let t = text.trim();
    let t = match t.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((_, rest)) => rest.trim(),
        None => t,
    };
    if t.is_empty() {
        "The mail server refused.".into()
    } else {
        t.to_string()
    }
}

pub struct Login {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Caps {
    /// CONDSTORE: the server says which messages changed since a mod-sequence.
    pub condstore: bool,
    /// Gmail's extensions: labels, message ids, search syntax.
    pub gmail: bool,
    pub idle: bool,
    /// Literals may be sent without waiting (LITERAL+ / LITERAL-): searches outside ASCII.
    pub literal_plus: bool,
}

pub struct Conn {
    session: Option<Session>,
    pub caps: Caps,
    /// The folder EXAMINE last opened.
    selected: Option<String>,
    pub last_used: Instant,
}

fn tls() -> Result<Arc<ClientConfig>, MailError> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if let Some(c) = CONFIG.get() {
        return Ok(c.clone());
    }
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_platform_verifier())
        .map_err(|e| MailError::Network(format!("Secure connections are unavailable: {e}")))?
        .with_no_client_auth();
    Ok(CONFIG.get_or_init(|| Arc::new(config)).clone())
}

async fn timed<T, E: Into<MailError>>(what: Duration, fut: impl Future<Output = Result<T, E>>) -> Result<T, MailError> {
    match tokio::time::timeout(what, fut).await {
        Ok(r) => r.map_err(Into::into),
        Err(_) => Err(MailError::Network("The mail server stopped answering.".into())),
    }
}

pub async fn connect(login: &Login) -> Result<Conn, MailError> {
    let host = login.host.trim();
    let tcp = timed(CONNECT_TIMEOUT, async {
        TcpStream::connect((host, login.port))
            .await
            .map_err(|e| MailError::Network(format!("Cannot reach {host}: {e}")))
    })
    .await?;
    let _ = tcp.set_nodelay(true);
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| MailError::Network(format!("{host} is not a valid server name.")))?;
    let stream = timed(CONNECT_TIMEOUT, async {
        TlsConnector::from(tls()?)
            .connect(name, tcp)
            .await
            .map_err(|e| MailError::Network(format!("A secure connection to {host} failed: {e}")))
    })
    .await?;
    let mut client = async_imap::Client::new(stream);
    let greeting = timed(CONNECT_TIMEOUT, async {
        client.read_response().await.map_err(async_imap::error::Error::from)
    })
    .await?;
    if greeting.is_none() {
        return Err(MailError::Network(format!("{host} closed the connection.")));
    }
    let (mut session, caps) = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        client.login_with_capabilities(&login.username, &login.password),
    )
    .await
    {
        Err(_) => return Err(MailError::Network("The mail server stopped answering.".into())),
        Ok(Err((e, _))) => return Err(login_error(e, host)),
        Ok(Ok(v)) => v,
    };
    let caps = match caps {
        Some(c) => c,
        None => timed(COMMAND_TIMEOUT, session.capabilities()).await?,
    };
    let caps = Caps {
        condstore: caps.has_str("CONDSTORE"),
        gmail: caps.has_str("X-GM-EXT-1"),
        idle: caps.has_str("IDLE"),
        literal_plus: caps.has_str("LITERAL+") || caps.has_str("LITERAL-"),
    }
    .with_enable(caps.has_str("ENABLE"));
    let mut conn = Conn {
        session: Some(session),
        caps,
        selected: None,
        last_used: Instant::now(),
    };
    if conn.caps.condstore {
        // EXAMINE then reports HIGHESTMODSEQ, and fetches can ask for what changed.
        let s = conn.session()?;
        if timed(COMMAND_TIMEOUT, s.run_command_and_check_ok("ENABLE CONDSTORE"))
            .await
            .is_err()
        {
            conn.caps.condstore = false;
        }
    }
    Ok(conn)
}

impl Caps {
    /// CONDSTORE is used only where it can be switched on for the whole connection.
    fn with_enable(mut self, enable: bool) -> Self {
        self.condstore &= enable;
        self
    }
}

fn login_error(e: async_imap::error::Error, host: &str) -> MailError {
    use async_imap::error::Error;
    match e {
        Error::No(text) | Error::Bad(text) => {
            if host.ends_with("gmail.com") || host.ends_with("googlemail.com") {
                MailError::Auth(
                    "Gmail did not accept this email and app password. Use an app password (Google Account → \
                     Security → App passwords), not your Google password; app passwords need 2-Step \
                     Verification turned on."
                        .into(),
                )
            } else {
                MailError::Auth(format!(
                    "The server did not accept the email and password: {}",
                    server_text(&text)
                ))
            }
        }
        other => MailError::from(other),
    }
}

impl Conn {
    fn session(&mut self) -> Result<&mut Session, MailError> {
        self.last_used = Instant::now();
        self.session
            .as_mut()
            .ok_or_else(|| MailError::Network("The connection was closed.".into()))
    }

    /// Messages the server sent on its own (EXISTS, FETCH…) between commands: a sync re-reads
    /// the folder anyway, so they are dropped rather than left to pile up. Returns whether any of
    /// them reported a change to the selected folder.
    fn drain(&mut self) -> bool {
        let mut news = false;
        if let Some(s) = self.session.as_mut() {
            while let Ok(r) = s.unsolicited_responses.try_recv() {
                news |= match r {
                    UnsolicitedResponse::Exists(_)
                    | UnsolicitedResponse::Expunge(_)
                    | UnsolicitedResponse::Recent(_) => true,
                    UnsolicitedResponse::Other(r) => {
                        !matches!(r.parsed(), Response::Data { .. } | Response::Done { .. })
                    }
                    _ => false,
                };
            }
        }
        news
    }

    /// Opens `path` read-only; the answer carries its counts and UIDNEXT/HIGHESTMODSEQ.
    pub async fn examine(&mut self, path: &str) -> Result<Mailbox, MailError> {
        self.drain();
        self.selected = None;
        let s = self.session()?;
        let mbox = timed(COMMAND_TIMEOUT, s.examine(path)).await?;
        self.drain();
        self.selected = Some(path.to_string());
        Ok(mbox)
    }

    pub async fn ensure_selected(&mut self, path: &str) -> Result<(), MailError> {
        if self.selected.as_deref() != Some(path) {
            self.examine(path).await?;
        }
        Ok(())
    }

    pub async fn list_folders(&mut self) -> Result<Vec<FolderInfo>, MailError> {
        let s = self.session()?;
        let names: Vec<Name> = timed(COMMAND_TIMEOUT, async {
            s.list(Some(""), Some("*")).await?.try_collect().await
        })
        .await?;
        let listed: Vec<Listed> = names
            .iter()
            .map(|n| Listed {
                path: n.name().to_string(),
                delimiter: n.delimiter().map(str::to_string),
                role: role(n),
                selectable: selectable(n),
            })
            .collect();
        Ok(folder_infos(&listed))
    }

    /// Message summaries for a sequence set (`uid` false) or a UID set.
    pub async fn fetch_meta(&mut self, set: &str, uid: bool) -> Result<Vec<MessageMeta>, MailError> {
        let gmail = self.caps.gmail;
        let mut items = LIST_ITEMS.to_string();
        if gmail {
            items.push_str(" X-GM-MSGID X-GM-LABELS");
        }
        if self.caps.condstore {
            items.push_str(" MODSEQ");
        }
        let query = format!("({items})");
        let s = self.session()?;
        let fetches: Vec<Fetch> = timed(COMMAND_TIMEOUT, async {
            if uid {
                s.uid_fetch(set, &query).await?.try_collect().await
            } else {
                s.fetch(set, &query).await?.try_collect().await
            }
        })
        .await?;
        Ok(fetches.iter().filter_map(meta).collect())
    }

    /// The flags (and Gmail labels) of a UID range, all of them or only those changed since
    /// `since` (CONDSTORE).
    pub async fn fetch_flags(&mut self, uid_set: &str, since: Option<u64>) -> Result<Vec<FlagChange>, MailError> {
        let mut query = String::from("(UID FLAGS");
        if self.caps.gmail {
            query.push_str(" X-GM-LABELS");
        }
        query.push(')');
        if let Some(since) = since {
            query.push_str(&format!(" (CHANGEDSINCE {since})"));
        }
        let gmail = self.caps.gmail;
        let s = self.session()?;
        let fetches: Vec<Fetch> = timed(COMMAND_TIMEOUT, async {
            s.uid_fetch(uid_set, &query).await?.try_collect().await
        })
        .await?;
        Ok(fetches
            .iter()
            .filter_map(|f| {
                Some(FlagChange {
                    uid: f.uid?,
                    flags: flag_bits(f.flags()),
                    labels: gmail.then(|| f.gmail_labels().map(|l| labels(l)).unwrap_or_default()),
                    modseq: f.modseq,
                })
            })
            .collect())
    }

    /// UIDs matching a search, in ascending order.
    pub async fn uid_search(&mut self, query: &str) -> Result<Vec<u32>, MailError> {
        let s = self.session()?;
        let found = timed(COMMAND_TIMEOUT, s.uid_search(query)).await?;
        let mut uids: Vec<u32> = found.into_iter().collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// A search for what a person typed: Gmail's own search syntax on Gmail, words anywhere in
    /// the message elsewhere. The messages must also match `criteria` (more search keys, such as
    /// dates); with no text, they are the whole search.
    pub async fn search_text(&mut self, text: &str, criteria: &str) -> Result<Vec<u32>, MailError> {
        let text = text.trim();
        if text.is_empty() {
            return self
                .uid_search(if criteria.is_empty() { "ALL" } else { criteria })
                .await;
        }
        let key = if self.caps.gmail { "X-GM-RAW" } else { "TEXT" };
        let keys = if criteria.is_empty() {
            key.to_string()
        } else {
            format!("{criteria} {key}")
        };
        let query = if text.is_ascii() {
            format!("{keys} {}", quote(text))
        } else if self.caps.literal_plus {
            format!("CHARSET UTF-8 {keys} {{{}+}}\r\n{text}", text.len())
        } else {
            format!("CHARSET UTF-8 {keys} {}", quote(text))
        };
        self.uid_search(&query).await
    }

    /// The number of unread messages in a folder.
    pub async fn unseen(&mut self, path: &str) -> Result<Option<u32>, MailError> {
        let s = self.session()?;
        Ok(timed(COMMAND_TIMEOUT, s.status(path, "(UNSEEN)")).await?.unseen)
    }

    /// Fetches `items` of the messages in a UID set of the selected folder, in one request.
    pub async fn fetch_many(&mut self, uid_set: &str, items: &str) -> Result<Vec<Fetch>, MailError> {
        let s = self.session()?;
        timed(COMMAND_TIMEOUT, async {
            s.uid_fetch(uid_set, items).await?.try_collect().await
        })
        .await
    }

    /// Fetches `items` of one message of the selected folder.
    pub async fn fetch_one(&mut self, uid: u32, items: &str) -> Result<Option<Fetch>, MailError> {
        let mut fetches = self.fetch_many(&uid.to_string(), items).await?;
        Ok(fetches
            .iter()
            .position(|f| f.uid == Some(uid))
            .map(|i| fetches.swap_remove(i)))
    }

    /// Waits for news in the selected folder with IDLE, up to `limit`, or until `stop` fires.
    pub async fn idle(
        &mut self,
        limit: Duration,
        stop: &tokio_util::sync::CancellationToken,
    ) -> Result<IdleEnd, MailError> {
        use async_imap::extensions::idle::IdleResponse;
        // A change since the last command is reported with the next command's answer: a NOOP
        // first, so nothing that arrived before IDLE starts goes unnoticed.
        if self.drain() {
            return Ok(IdleEnd::News);
        }
        timed(COMMAND_TIMEOUT, self.session()?.noop()).await?;
        if self.drain() {
            return Ok(IdleEnd::News);
        }
        let session = self
            .session
            .take()
            .ok_or_else(|| MailError::Network("The connection was closed.".into()))?;
        let mut handle = session.idle();
        timed(COMMAND_TIMEOUT, handle.init()).await?;
        let end = {
            let (wait, _interrupt) = handle.wait_with_timeout(limit);
            tokio::select! {
                r = wait => match r? {
                    IdleResponse::NewData(_) => IdleEnd::News,
                    IdleResponse::Timeout | IdleResponse::ManualInterrupt => IdleEnd::Quiet,
                },
                _ = stop.cancelled() => IdleEnd::Stopped,
            }
        };
        let session = timed(COMMAND_TIMEOUT, handle.done()).await?;
        self.session = Some(session);
        self.last_used = Instant::now();
        // What arrived while IDLE was starting or ending.
        Ok(if end == IdleEnd::Quiet && self.drain() {
            IdleEnd::News
        } else {
            end
        })
    }

    pub async fn logout(mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), s.logout()).await;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleEnd {
    News,
    Quiet,
    Stopped,
}

/// An IMAP quoted string.
pub fn quote(s: &str) -> String {
    let clean: String = s.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
}

fn flag_bits<'a>(flags: impl Iterator<Item = Flag<'a>>) -> u8 {
    flags.fold(0, |bits, f| {
        bits | match f {
            Flag::Seen => SEEN,
            Flag::Flagged => FLAGGED,
            Flag::Answered => ANSWERED,
            Flag::Draft => DRAFT,
            _ => 0,
        }
    })
}

/// Gmail labels as people named them; system labels (`\Inbox`, `\Important`…) are left out.
fn labels(raw: &[std::borrow::Cow<'_, str>]) -> Vec<String> {
    raw.iter()
        .filter(|l| !l.starts_with('\\'))
        .map(|l| utf7::decode(l))
        .collect()
}

fn meta(f: &Fetch) -> Option<MessageMeta> {
    let uid = f.uid?;
    let header = body::parse_header(f.header().unwrap_or_default(), f.text());
    let date = f
        .internal_date()
        .map(|d| d.timestamp_millis())
        .or(header.date)
        .unwrap_or_default();
    Some(MessageMeta {
        uid,
        date,
        flags: flag_bits(f.flags()),
        labels: f.gmail_labels().map(|l| labels(l)).unwrap_or_default(),
        size: f.size.unwrap_or_default(),
        plan: f.bodystructure().map(Plan::from_structure).unwrap_or_default(),
        gm_msgid: f.gmail_msg_id().copied(),
        modseq: f.modseq,
        header,
    })
}

/// Where each special folder goes in the list, and what it is called there.
const ROLES: &[(&str, &str)] = &[
    ("inbox", "Inbox"),
    ("flagged", "Starred"),
    ("important", "Important"),
    ("sent", "Sent"),
    ("drafts", "Drafts"),
    ("archive", "Archive"),
    ("all", "All Mail"),
    ("junk", "Spam"),
    ("trash", "Trash"),
];

fn role(name: &Name) -> Option<&'static str> {
    if name.name().eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    name.attributes().iter().find_map(|a| match a {
        NameAttribute::All => Some("all"),
        NameAttribute::Archive => Some("archive"),
        NameAttribute::Drafts => Some("drafts"),
        NameAttribute::Flagged => Some("flagged"),
        NameAttribute::Junk => Some("junk"),
        NameAttribute::Sent => Some("sent"),
        NameAttribute::Trash => Some("trash"),
        NameAttribute::Extension(e) if e.eq_ignore_ascii_case("\\Important") => Some("important"),
        _ => None,
    })
}

fn selectable(name: &Name) -> bool {
    !name.attributes().iter().any(|a| match a {
        NameAttribute::NoSelect => true,
        NameAttribute::Extension(e) => e.eq_ignore_ascii_case("\\NonExistent"),
        _ => false,
    })
}

/// A folder as LIST describes it.
struct Listed {
    path: String,
    delimiter: Option<String>,
    role: Option<&'static str>,
    selectable: bool,
}

/// The folder list in the order shown: special folders first (Inbox, Starred, Sent…), then the
/// person's own folders by name, each nested under its parent. Containers that cannot hold
/// messages (Gmail's `[Gmail]`) are left out and their folders moved up a level.
fn folder_infos(listed: &[Listed]) -> Vec<FolderInfo> {
    let hidden: Vec<&str> = listed
        .iter()
        .filter(|n| !n.selectable)
        .map(|n| n.path.as_str())
        .collect();
    let mut special = Vec::new();
    let mut own = Vec::new();
    let mut seen_roles = Vec::new();
    for n in listed.iter().filter(|n| n.selectable) {
        let path = n.path.clone();
        let delimiter = n.delimiter.clone().filter(|d| !d.is_empty());
        let segments: Vec<&str> = match delimiter.as_deref() {
            Some(d) => path.split(d).collect(),
            None => vec![path.as_str()],
        };
        if let Some(r) = n.role.filter(|r| !seen_roles.contains(r)) {
            seen_roles.push(r);
            let rank = ROLES.iter().position(|(id, _)| *id == r).unwrap_or(ROLES.len());
            special.push((
                rank,
                FolderInfo {
                    path,
                    name: ROLES.get(rank).map_or(r, |(_, label)| label).into(),
                    role: Some(r.into()),
                    delimiter,
                    selectable: true,
                    depth: 0,
                    position: 0,
                },
            ));
            continue;
        }
        // Ancestors that are shown count toward the depth; hidden containers do not.
        let depth = (1..segments.len())
            .filter(|i| {
                let ancestor = segments[..*i].join(delimiter.as_deref().unwrap_or_default());
                !hidden.contains(&ancestor.as_str())
            })
            .count() as u32;
        let display = utf7::decode(segments.last().copied().unwrap_or(&path));
        let sort_key = utf7::decode(&path).to_lowercase();
        own.push((
            sort_key,
            FolderInfo {
                path,
                name: display,
                role: None,
                delimiter,
                selectable: true,
                depth,
                position: 0,
            },
        ));
    }
    special.sort_by_key(|(rank, _)| *rank);
    own.sort_by(|a, b| a.0.cmp(&b.0));
    special
        .into_iter()
        .map(|(_, f)| f)
        .chain(own.into_iter().map(|(_, f)| f))
        .enumerate()
        .map(|(i, mut f)| {
            f.position = i as i64;
            f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_errors_lose_their_codes() {
        assert_eq!(
            server_text("[AUTHENTICATIONFAILED] Invalid credentials (Failure)"),
            "Invalid credentials (Failure)"
        );
        assert_eq!(server_text("Mailbox does not exist"), "Mailbox does not exist");
    }

    #[test]
    fn lists_gmail_folders_in_order() {
        let l = |path: &str, role: Option<&'static str>, selectable: bool| Listed {
            path: path.into(),
            delimiter: Some("/".into()),
            role,
            selectable,
        };
        let folders = folder_infos(&[
            l("Work/Clients", None, true),
            l("[Gmail]", None, false),
            l("[Gmail]/Posta inviata", Some("sent"), true),
            l("INBOX", Some("inbox"), true),
            l("Work", None, true),
            l("[Gmail]/Tutti i messaggi", Some("all"), true),
            l("[Gmail]/Etichetta &AOg-", None, true),
        ]);
        let shown: Vec<(&str, u32)> = folders.iter().map(|f| (f.name.as_str(), f.depth)).collect();
        assert_eq!(
            shown,
            [
                ("Inbox", 0),
                ("Sent", 0),
                ("All Mail", 0),
                ("Etichetta è", 0),
                ("Work", 0),
                ("Clients", 1)
            ]
        );
        assert_eq!(folders[1].path, "[Gmail]/Posta inviata");
        assert_eq!(
            folders.iter().map(|f| f.position).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn quotes_search_text() {
        assert_eq!(quote(r#"from:"Ada" a\b"#), r#""from:\"Ada\" a\\b""#);
        assert_eq!(quote("a\r\nb"), "\"ab\"");
    }
}
