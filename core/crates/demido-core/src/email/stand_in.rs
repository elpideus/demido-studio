//! A mail server stand-in on this computer: the part of IMAP and of SMTP that Demido speaks, in the clear, to try
//! email with no real mailbox (DECISIONS Y). Used by the tests, and by a development build's trial
//! (`demido mail-stand-in`, debug builds only). It answers as RFC 3501 and RFC 5321 say, and writes down every
//! command it was sent, so that a test can see that nothing marked, moved or deleted a message.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// One message in the stand-in's inbox.
#[derive(Clone, Debug)]
pub struct Held {
    pub uid: u32,
    pub seen: bool,
    pub raw: Vec<u8>,
}

/// What the stand-in holds, and what it was sent.
#[derive(Debug, Default)]
pub struct Mailbox {
    pub user: String,
    pub password: String,
    pub inbox: Vec<Held>,
    pub uid_validity: u32,
    /// Every IMAP command, as it came (the password in LOGIN taken out).
    pub commands: Vec<String>,
    /// Every message sent through its SMTP side, with the sign-in it came with and its envelope.
    pub sent: Vec<Sent>,
}

#[derive(Clone, Debug)]
pub struct Sent {
    pub signed_in_as: String,
    pub from: String,
    pub to: Vec<String>,
    pub data: String,
}

pub type Shared = Arc<Mutex<Mailbox>>;

/// A message as a mail program would write it.
pub fn message(from: &str, to: &str, subject: &str, date: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{}@stand-in.example>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        uuid::Uuid::new_v4().simple(),
        body.replace('\n', "\r\n")
    )
    .into_bytes()
}

/// The words of a command line: quoted strings as one word each.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted, mut escaped) = (Vec::new(), String::new(), false, false);
    for c in line.chars() {
        match c {
            '\\' if quoted && !escaped => escaped = true,
            '"' if !escaped => quoted = !quoted,
            ' ' if !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => {
                word.push(c);
                escaped = false;
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn header_of(raw: &[u8]) -> &[u8] {
    raw.windows(4).position(|w| w == b"\r\n\r\n").map_or(raw, |at| &raw[..at + 4])
}

/// The sequence numbers a set names, `3:5`, `7`, `2:*`, within 1 to `last`.
fn numbers(set: &str, last: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for part in set.split(',') {
        let (a, b) = part.split_once(':').unwrap_or((part, part));
        let at = |s: &str| if s == "*" { last } else { s.parse().unwrap_or(0) };
        let (a, b) = (at(a), at(b));
        out.extend(a.min(b).max(1)..=a.max(b).min(last));
    }
    out
}

async fn imap(stream: TcpStream, box_: Shared) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let _ = write.write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Demido's stand-in is ready\r\n").await;
    while let Ok(Some(line)) = lines.next_line().await {
        let w = words(&line);
        let (Some(tag), Some(command)) = (w.first(), w.get(1)) else { continue };
        let command = command.to_ascii_uppercase();
        box_.lock().unwrap().commands.push(if command == "LOGIN" { format!("{tag} LOGIN {}", w.get(2).cloned().unwrap_or_default()) } else { line.clone() });
        let mut out: Vec<u8> = Vec::new();
        match command.as_str() {
            "CAPABILITY" => out.extend_from_slice(format!("* CAPABILITY IMAP4rev1\r\n{tag} OK CAPABILITY completed\r\n").as_bytes()),
            "LOGIN" => {
                let b = box_.lock().unwrap();
                let ok = w.get(2) == Some(&b.user) && w.get(3) == Some(&b.password);
                out.extend_from_slice(if ok { format!("{tag} OK LOGIN completed\r\n") } else { format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n") }.as_bytes());
            }
            "EXAMINE" | "SELECT" => {
                let b = box_.lock().unwrap();
                out.extend_from_slice(
                    format!(
                        "* FLAGS (\\Seen \\Answered)\r\n* {} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n{tag} OK [READ-ONLY] {command} completed\r\n",
                        b.inbox.len(),
                        b.uid_validity
                    )
                    .as_bytes(),
                );
            }
            "FETCH" | "UID" => {
                let by_uid = command == "UID";
                let (set, what) = if by_uid { (w.get(3).cloned().unwrap_or_default(), w[4..].join(" ")) } else { (w.get(2).cloned().unwrap_or_default(), w[3..].join(" ")) };
                let b = box_.lock().unwrap();
                let last = b.inbox.len() as u32;
                let chosen: Vec<(u32, &Held)> = if by_uid {
                    let wanted: Vec<u32> = numbers(&set, u32::MAX).into_iter().take(10_000).collect();
                    b.inbox.iter().enumerate().filter(|(_, h)| wanted.contains(&h.uid)).map(|(i, h)| (i as u32 + 1, h)).collect()
                } else {
                    numbers(&set, last).into_iter().map(|n| (n, &b.inbox[n as usize - 1])).collect()
                };
                let upper = what.to_ascii_uppercase();
                for (n, held) in chosen {
                    let mut parts = format!("* {n} FETCH (UID {}", held.uid).into_bytes();
                    if upper.contains("FLAGS") {
                        parts.extend_from_slice(if held.seen { b" FLAGS (\\Seen)" } else { b" FLAGS ()" });
                    }
                    if upper.contains("RFC822.SIZE") {
                        parts.extend_from_slice(format!(" RFC822.SIZE {}", held.raw.len()).as_bytes());
                    }
                    if upper.contains("BODY.PEEK[HEADER]") || upper.contains("BODY[HEADER]") {
                        let h = header_of(&held.raw);
                        parts.extend_from_slice(format!(" BODY[HEADER] {{{}}}\r\n", h.len()).as_bytes());
                        parts.extend_from_slice(h);
                    } else if upper.contains("BODY.PEEK[]") || upper.contains("BODY[]") {
                        parts.extend_from_slice(format!(" BODY[] {{{}}}\r\n", held.raw.len()).as_bytes());
                        parts.extend_from_slice(&held.raw);
                    }
                    parts.extend_from_slice(b")\r\n");
                    out.extend_from_slice(&parts);
                }
                out.extend_from_slice(format!("{tag} OK FETCH completed\r\n").as_bytes());
            }
            "LIST" => out.extend_from_slice(format!("* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n{tag} OK LIST completed\r\n").as_bytes()),
            "NOOP" => out.extend_from_slice(format!("{tag} OK NOOP completed\r\n").as_bytes()),
            "LOGOUT" => {
                let _ = write.write_all(format!("* BYE Demido's stand-in says goodbye\r\n{tag} OK LOGOUT completed\r\n").as_bytes()).await;
                return;
            }
            // STORE, COPY, MOVE, EXPUNGE and the rest: this stand-in does none of them, and says so.
            _ => out.extend_from_slice(format!("{tag} BAD the stand-in does not do {command}\r\n").as_bytes()),
        }
        if write.write_all(&out).await.is_err() {
            return;
        }
    }
}

async fn smtp(stream: TcpStream, box_: Shared) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let _ = write.write_all(b"220 stand-in.example ESMTP Demido's stand-in\r\n").await;
    let (mut signed_in_as, mut from, mut to) = (String::new(), String::new(), Vec::new());
    let mut waiting_for_login = 0;
    while let Ok(Some(line)) = lines.next_line().await {
        let upper = line.to_ascii_uppercase();
        let reply: String = if waiting_for_login == 1 {
            signed_in_as = String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(line.trim()).unwrap_or_default()).into_owned();
            waiting_for_login = 2;
            "334 UGFzc3dvcmQ6\r\n".into()
        } else if waiting_for_login == 2 {
            let password = String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(line.trim()).unwrap_or_default()).into_owned();
            waiting_for_login = 0;
            let b = box_.lock().unwrap();
            if signed_in_as == b.user && password == b.password { "235 2.7.0 Accepted\r\n".into() } else { "535 5.7.8 Bad credentials\r\n".into() }
        } else if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            "250-stand-in.example\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n".into()
        } else if let Some(rest) = upper.strip_prefix("AUTH PLAIN") {
            let given = line[line.len() - rest.len()..].trim();
            let plain = base64::engine::general_purpose::STANDARD.decode(given).unwrap_or_default();
            let parts: Vec<String> = plain.split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
            let b = box_.lock().unwrap();
            signed_in_as = parts.get(1).cloned().unwrap_or_default();
            if parts.get(1) == Some(&b.user) && parts.get(2) == Some(&b.password) { "235 2.7.0 Accepted\r\n".into() } else { "535 5.7.8 Bad credentials\r\n".into() }
        } else if upper.starts_with("AUTH LOGIN") {
            waiting_for_login = 1;
            "334 VXNlcm5hbWU6\r\n".into()
        } else if upper.starts_with("MAIL FROM:") {
            from = line[10..].trim().trim_matches(['<', '>']).to_string();
            "250 2.1.0 OK\r\n".into()
        } else if upper.starts_with("RCPT TO:") {
            to.push(line[8..].trim().trim_matches(['<', '>']).to_string());
            "250 2.1.5 OK\r\n".into()
        } else if upper == "DATA" {
            let _ = write.write_all(b"354 Go ahead\r\n").await;
            let mut data = String::new();
            while let Ok(Some(l)) = lines.next_line().await {
                if l == "." {
                    break;
                }
                data.push_str(&l);
                data.push('\n');
            }
            box_.lock().unwrap().sent.push(Sent { signed_in_as: signed_in_as.clone(), from: from.clone(), to: std::mem::take(&mut to), data });
            "250 2.0.0 Queued\r\n".into()
        } else if upper == "QUIT" {
            let _ = write.write_all(b"221 2.0.0 Bye\r\n").await;
            return;
        } else if upper == "RSET" || upper == "NOOP" {
            "250 OK\r\n".into()
        } else {
            "502 5.5.1 Not done by the stand-in\r\n".into()
        };
        if write.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
    }
}

/// Starts the stand-in: an IMAP port and an SMTP port on 127.0.0.1, sharing one mailbox.
pub async fn start(user: &str, password: &str, inbox: Vec<Held>) -> std::io::Result<(u16, u16, Shared)> {
    let box_: Shared = Arc::new(Mutex::new(Mailbox { user: user.into(), password: password.into(), inbox, uid_validity: 7, ..Mailbox::default() }));
    let imap_at = TcpListener::bind("127.0.0.1:0").await?;
    let smtp_at = TcpListener::bind("127.0.0.1:0").await?;
    let (imap_port, smtp_port) = (imap_at.local_addr()?.port(), smtp_at.local_addr()?.port());
    let (a, b) = (box_.clone(), box_.clone());
    tokio::spawn(async move {
        while let Ok((s, _)) = imap_at.accept().await {
            tokio::spawn(imap(s, a.clone()));
        }
    });
    tokio::spawn(async move {
        while let Ok((s, _)) = smtp_at.accept().await {
            tokio::spawn(smtp(s, b.clone()));
        }
    });
    Ok((imap_port, smtp_port, box_))
}
