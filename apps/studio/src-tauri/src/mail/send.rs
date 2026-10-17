//! Writing mail: putting a message together, with what a reply or a forward takes from the
//! original, and sending it over SMTP.
//!
//! The message is built twice from the same parts, Message-ID and date: once without the Bcc
//! header for sending (the server still delivers to those addresses, which the envelope names),
//! and once with it for the copy kept in Sent or Drafts, so the person can see who was blind
//! copied and no recipient can.

use std::time::{Duration, SystemTime};

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mail_parser::MessageParser;

use super::accounts::{SMTPS_PORT, SUBMISSION_PORT};
use super::body::Addr;
use super::imap::Login;

/// How long sending may take, the attachments uploaded included.
const SEND_TIMEOUT: Duration = Duration::from_secs(120);
/// How long finding which port the SMTP server answers on may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// A file going out with the message.
#[derive(Clone, Debug)]
pub struct OutFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// A message ready to send or save: the addresses checked, a reply's or forward's subject,
/// quote and attachments already added.
#[derive(Clone, Debug)]
pub struct Draft {
    pub from: Mailbox,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    pub subject: String,
    pub text: String,
    pub attachments: Vec<OutFile>,
    /// The Message-ID answered, with its angle brackets.
    pub in_reply_to: Option<String>,
    /// The thread so far, oldest first, each with its angle brackets.
    pub references: Vec<String>,
    /// Why some of the original's attachments were left out of a forward.
    pub left_out: Option<String>,
}

/// The message as sent, and as kept in Sent or Drafts.
pub struct Built {
    pub message_id: String,
    pub send: Message,
    pub copy: Vec<u8>,
}

impl Draft {
    pub fn build(&self) -> anyhow::Result<Built> {
        let domain = self.from.email.domain().to_string();
        let message_id = format!("<{}@{}>", uuid::Uuid::new_v4().simple(), domain);
        let date = SystemTime::now();
        let make = |keep_bcc: bool| -> anyhow::Result<Message> {
            let mut b = Message::builder()
                .from(self.from.clone())
                .subject(self.subject.clone())
                .message_id(Some(message_id.clone()))
                .date(date)
                .user_agent("Demido Studio".into());
            for m in &self.to {
                b = b.to(m.clone());
            }
            for m in &self.cc {
                b = b.cc(m.clone());
            }
            for m in &self.bcc {
                b = b.bcc(m.clone());
            }
            if let Some(id) = &self.in_reply_to {
                b = b.in_reply_to(id.clone());
            }
            if !self.references.is_empty() {
                b = b.references(self.references.join(" "));
            }
            if keep_bcc {
                b = b.keep_bcc();
            }
            let text = SinglePart::plain(self.text.clone());
            let message = if self.attachments.is_empty() {
                b.singlepart(text)
            } else {
                let mut parts = MultiPart::mixed().singlepart(text);
                for f in &self.attachments {
                    let mime = ContentType::parse(&f.mime).unwrap_or_else(|_| octets());
                    parts = parts.singlepart(Attachment::new(f.name.clone()).body(f.bytes.clone(), mime));
                }
                b.multipart(parts)
            };
            message.map_err(|e| anyhow::anyhow!("The email could not be put together: {e}"))
        };
        Ok(Built {
            send: make(false)?,
            copy: make(true)?.formatted(),
            message_id,
        })
    }
}

fn octets() -> ContentType {
    ContentType::parse("application/octet-stream").expect("a valid content type")
}

/// Addresses as people write them (`ada@example.com`, `Ada <ada@example.com>`), checked.
pub fn mailboxes(list: &[String]) -> anyhow::Result<Vec<Mailbox>> {
    let mut out: Vec<Mailbox> = Vec::new();
    for raw in list.iter().flat_map(|s| split_addresses(s)) {
        let m: Mailbox = raw
            .parse()
            .map_err(|_| anyhow::anyhow!("{raw} is not an email address."))?;
        if !out.iter().any(|o| same_address(o, &m)) {
            out.push(m);
        }
    }
    Ok(out)
}

/// A list written in one string, split at the commas and semicolons between addresses (not
/// those inside a quoted name).
fn split_addresses(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ',' | ';' if !quoted => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out.into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

pub fn same_address(a: &Mailbox, b: &Mailbox) -> bool {
    AsRef::<str>::as_ref(&a.email).eq_ignore_ascii_case(b.email.as_ref())
}

/// An address from a message, as one to write to.
pub fn mailbox(a: &Addr) -> Option<Mailbox> {
    let email = a.email.parse().ok()?;
    let name = (!a.name.is_empty()).then(|| a.name.clone());
    Some(Mailbox::new(name, email))
}

/// An address as people write it, which [`mailboxes`] reads back: `Name <email>`, the name
/// quoted when it needs to be, or the email alone.
pub fn written(m: &Mailbox) -> String {
    m.to_string()
}

/// What a reply takes from the original's header, and what going on with a draft takes from the
/// draft's.
#[derive(Debug, Default, PartialEq)]
pub struct Original {
    pub message_id: Option<String>,
    /// The message it answers, with its angle brackets.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub reply_to: Vec<Addr>,
    /// Kept in the copies in Sent and Drafts only.
    pub bcc: Vec<Addr>,
}

pub fn original(header: &[u8]) -> Original {
    let Some(m) = MessageParser::default().parse_headers(header) else {
        return Original::default();
    };
    let bracket = |id: &str| format!("<{}>", id.trim().trim_start_matches('<').trim_end_matches('>'));
    let in_reply_to = m.in_reply_to().as_text().map(bracket);
    let mut references: Vec<String> = m
        .references()
        .as_text_list()
        .map(|l| l.iter().map(|r| bracket(r)).collect())
        .unwrap_or_default();
    if references.is_empty()
        && let Some(parent) = &in_reply_to
    {
        references.push(parent.clone());
    }
    let addrs = |list: Option<&mail_parser::Address<'_>>| -> Vec<Addr> {
        list.map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Addr {
                        name: x.name().unwrap_or_default().trim().to_string(),
                        email: x.address()?.trim().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    };
    Original {
        message_id: m.message_id().map(bracket),
        in_reply_to,
        references,
        reply_to: addrs(m.reply_to()),
        bcc: addrs(m.bcc()),
    }
}

/// The most a message's own attachments may weigh together; mail servers refuse more.
pub const MAX_ATTACHMENTS: u64 = 25 * 1024 * 1024;

/// Files to attach, read, with a type from their extension. Fails when one cannot be read or
/// they weigh more than [`MAX_ATTACHMENTS`] together.
pub fn read_files(paths: &[std::path::PathBuf]) -> anyhow::Result<Vec<OutFile>> {
    let mut out = Vec::new();
    let mut total = 0u64;
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("Could not read {name}: {e}"))?;
        total += bytes.len() as u64;
        if total > MAX_ATTACHMENTS {
            anyhow::bail!("The attachments come to more than 25 MB, which mail servers refuse.");
        }
        out.push(OutFile {
            mime: mime_of(path).into(),
            name,
            bytes,
        });
    }
    Ok(out)
}

/// A file's type, from its extension.
pub fn mime_of(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xls" => "application/vnd.ms-excel",
        "doc" => "application/msword",
        _ => "application/octet-stream",
    }
}

/// `Re: subject`, unless it already is a reply.
pub fn reply_subject(subject: &str) -> String {
    prefixed("Re:", subject, &["re:", "r:", "aw:", "sv:"])
}

/// `Fwd: subject`, unless it already is a forward.
pub fn forward_subject(subject: &str) -> String {
    prefixed("Fwd:", subject, &["fwd:", "fw:", "i:", "wg:", "tr:"])
}

fn prefixed(prefix: &str, subject: &str, known: &[&str]) -> String {
    let subject = subject.trim();
    let lower = subject.to_lowercase();
    if known.iter().any(|p| lower.starts_with(p)) {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}

/// The original's text under the reply, each line marked with `>`.
pub fn quote(when: &str, from: &str, text: &str) -> String {
    let mut out = format!("On {when}, {from} wrote:\n");
    for line in text.trim_end().lines() {
        out.push('>');
        if !line.is_empty() && !line.starts_with('>') {
            out.push(' ');
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The original's header fields and text under a forward.
pub fn forwarded(from: &str, when: &str, subject: &str, to: &str, text: &str) -> String {
    let mut out = String::from("---------- Forwarded message ----------\n");
    out.push_str(&format!("From: {from}\nDate: {when}\nSubject: {subject}\n"));
    if !to.is_empty() {
        out.push_str(&format!("To: {to}\n"));
    }
    out.push('\n');
    out.push_str(text.trim_end());
    out.push('\n');
    out
}

/// The text the person wrote, then what it answers or passes on.
pub fn with_tail(text: &str, tail: Option<&str>) -> String {
    let text = text.trim_end();
    match tail {
        Some(tail) if text.is_empty() => tail.to_string(),
        Some(tail) => format!("{text}\n\n{tail}"),
        None => format!("{text}\n"),
    }
}

fn transport(
    host: &str,
    port: u16,
    login: &Login,
    timeout: Duration,
) -> anyhow::Result<AsyncSmtpTransport<Tokio1Executor>> {
    let builder = if port == SMTPS_PORT {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
    }
    .map_err(|e| anyhow::anyhow!("The mail server {host} cannot be reached securely: {e}"))?;
    Ok(builder
        .port(port)
        .credentials(Credentials::new(login.username.clone(), login.password.clone()))
        .timeout(Some(timeout))
        .build())
}

/// The port the SMTP server answers on: 465, else 587. None when neither does.
pub async fn probe(host: &str, login: &Login) -> Option<u16> {
    for port in [SMTPS_PORT, SUBMISSION_PORT] {
        let Ok(t) = transport(host, port, login, PROBE_TIMEOUT) else {
            continue;
        };
        let answered = tokio::time::timeout(PROBE_TIMEOUT, t.test_connection()).await;
        if matches!(answered, Ok(Ok(true))) {
            return Some(port);
        }
    }
    None
}

/// Sends the message through `host`, signing in as the IMAP account does.
pub async fn send(host: &str, port: u16, login: &Login, message: &Message) -> anyhow::Result<()> {
    let t = transport(host, port, login, SEND_TIMEOUT)?;
    match tokio::time::timeout(SEND_TIMEOUT, t.send(message.clone())).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) if e.is_permanent() || e.is_transient() => {
            anyhow::bail!("The mail server {host} would not send it: {e}")
        }
        Ok(Err(e)) => anyhow::bail!("Sending through {host}:{port} failed: {e}"),
        Err(_) => anyhow::bail!("The mail server {host} stopped answering while sending."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Draft {
        Draft {
            from: "Ada <ada@example.com>".parse().unwrap(),
            to: mailboxes(&["bob@example.org, \"Lovelace, Ada\" <al@example.net>".into()]).unwrap(),
            cc: Vec::new(),
            bcc: mailboxes(&["secret@example.com".into()]).unwrap(),
            subject: "Hello".into(),
            text: "Hi Bob.\n".into(),
            attachments: Vec::new(),
            in_reply_to: Some("<a@x>".into()),
            references: vec!["<root@x>".into(), "<a@x>".into()],
            left_out: None,
        }
    }

    #[test]
    fn addresses_are_split_and_checked() {
        let d = draft();
        assert_eq!(d.to.len(), 2);
        assert_eq!(d.to[1].name.as_deref(), Some("Lovelace, Ada"));
        assert!(mailboxes(&["not an address".into()]).is_err());
        assert_eq!(mailboxes(&["a@x.com; A@X.com".into()]).unwrap().len(), 1);
    }

    #[test]
    fn bcc_stays_out_of_the_message_sent() {
        let built = draft().build().unwrap();
        let sent = String::from_utf8(built.send.formatted()).unwrap();
        let kept = String::from_utf8(built.copy).unwrap();
        assert!(!sent.contains("secret@example.com"));
        assert!(kept.contains("secret@example.com"));
        assert!(
            built
                .send
                .envelope()
                .to()
                .iter()
                .any(|a| a.to_string() == "secret@example.com")
        );
        for m in [&sent, &kept] {
            assert!(m.contains(&built.message_id));
            assert!(m.contains("In-Reply-To: <a@x>"));
            assert!(m.contains("References: <root@x> <a@x>"));
        }
    }

    #[test]
    fn replies_and_forwards_are_named_once() {
        assert_eq!(reply_subject("Lunch"), "Re: Lunch");
        assert_eq!(reply_subject("RE: Lunch"), "RE: Lunch");
        assert_eq!(forward_subject("Re: Lunch"), "Fwd: Re: Lunch");
        assert_eq!(forward_subject("Fwd: Lunch"), "Fwd: Lunch");
    }

    #[test]
    fn a_reply_quotes_and_threads() {
        let header =
            b"Message-ID: <b@x>\r\nReferences: <root@x> <a@x>\r\nReply-To: List <list@x.org>\r\nSubject: Hi\r\n\r\n";
        let o = original(header);
        assert_eq!(o.message_id.as_deref(), Some("<b@x>"));
        assert_eq!(o.references, ["<root@x>", "<a@x>"]);
        assert_eq!(o.reply_to[0].email, "list@x.org");
        // A draft's header: what it answers, and who it blind copies.
        let draft = original(b"In-Reply-To: <a@x>\r\nBcc: Secret <s@x.org>, t@x.org\r\n\r\n");
        assert_eq!(draft.in_reply_to.as_deref(), Some("<a@x>"));
        assert_eq!(draft.references, ["<a@x>"]);
        assert_eq!(draft.bcc.len(), 2);
        assert_eq!(draft.bcc[0].name, "Secret");
        assert_eq!(
            quote("2026-01-02 10:00", "Bob <bob@x>", "Hello\n\n> earlier\n"),
            "On 2026-01-02 10:00, Bob <bob@x> wrote:\n> Hello\n>\n>> earlier\n"
        );
        assert_eq!(with_tail("Thanks!\n", Some("On…")), "Thanks!\n\nOn…");
    }
}
