//! A mail server stand-in on this computer: the part of IMAP and of SMTP that Demido speaks, in the clear, to try
//! email with no real mailbox (DECISIONS Y). Used by the tests, and by a development build's trial
//! (`demido mail-stand-in`, debug builds only). It answers as RFC 3501 and RFC 5321 say, and writes down every
//! command it was sent, so that a test can see that nothing marked, moved or deleted a message.

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// One message in one of the stand-in's mailboxes.
#[derive(Clone, Debug, Default)]
pub struct Held {
    pub uid: u32,
    pub seen: bool,
    pub raw: Vec<u8>,
    pub flagged: bool,
    /// Marked to go at the next EXPUNGE.
    pub deleted: bool,
}

/// A mailbox besides the inbox, with what it is for (RFC 6154): `\Sent`, `\Trash`, `\Drafts`, `\Archive`.
#[derive(Clone, Debug)]
pub struct Folder {
    pub name: String,
    pub role: Option<&'static str>,
    pub held: Vec<Held>,
}

/// What the stand-in holds, and what it was sent.
#[derive(Debug, Default)]
pub struct Mailbox {
    pub user: String,
    pub password: String,
    pub inbox: Vec<Held>,
    /// The mailboxes besides the inbox: Sent, Drafts, Trash and Archive to start with.
    pub folders: Vec<Folder>,
    /// The UID the next message put in a mailbox gets.
    pub next_uid: u32,
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

impl Mailbox {
    /// A mailbox by its name; INBOX in any case.
    pub fn place(&mut self, name: &str) -> Option<&mut Vec<Held>> {
        if name.eq_ignore_ascii_case("INBOX") {
            return Some(&mut self.inbox);
        }
        self.folders.iter_mut().find(|f| f.name == name).map(|f| &mut f.held)
    }

    /// The same, to look at.
    pub fn held_in(&self, name: &str) -> Option<&Vec<Held>> {
        if name.eq_ignore_ascii_case("INBOX") {
            return Some(&self.inbox);
        }
        self.folders.iter().find(|f| f.name == name).map(|f| &f.held)
    }
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

/// Whether a message answers a search: each `TEXT`, `SUBJECT`, `FROM` or `BODY` word found in it, any case.
fn answers(held: &Held, criteria: &[String]) -> bool {
    let all = String::from_utf8_lossy(&held.raw).to_lowercase();
    let head = String::from_utf8_lossy(header_of(&held.raw)).to_lowercase();
    let mut rest = criteria.iter();
    while let Some(key) = rest.next() {
        let key = key.to_ascii_uppercase();
        let ok = match key.as_str() {
            "ALL" => true,
            "TEXT" | "BODY" => rest.next().is_some_and(|w| all.contains(&w.to_lowercase())),
            "SUBJECT" => rest.next().is_some_and(|w| head.lines().any(|l| l.starts_with("subject:") && l.contains(&w.to_lowercase()))),
            "FROM" => rest.next().is_some_and(|w| head.lines().any(|l| l.starts_with("from:") && l.contains(&w.to_lowercase()))),
            "UNSEEN" => !held.seen,
            "FLAGGED" => held.flagged,
            "CHARSET" => {
                rest.next();
                true
            }
            _ => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn flags_of(held: &Held) -> String {
    let mut f = Vec::new();
    if held.seen {
        f.push("\\Seen");
    }
    if held.flagged {
        f.push("\\Flagged");
    }
    if held.deleted {
        f.push("\\Deleted");
    }
    format!("FLAGS ({})", f.join(" "))
}

async fn imap(stream: TcpStream, box_: Shared) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let _ = write.write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN MOVE UIDPLUS SPECIAL-USE] Demido's stand-in is ready\r\n").await;
    // The mailbox this connection has open, and whether it may change it (SELECT) or only look (EXAMINE).
    let (mut open, mut may_change) = (String::from("INBOX"), false);
    while let Ok(Some(line)) = lines.next_line().await {
        let w = words(&line);
        let (Some(tag), Some(command)) = (w.first(), w.get(1)) else { continue };
        let command = command.to_ascii_uppercase();
        box_.lock().unwrap().commands.push(if command == "LOGIN" { format!("{tag} LOGIN {}", w.get(2).cloned().unwrap_or_default()) } else { line.clone() });
        let mut out: Vec<u8> = Vec::new();
        // UID FETCH, UID SEARCH and the rest: the command is the word after UID.
        let (by_uid, verb, args): (bool, String, Vec<String>) =
            if command == "UID" { (true, w.get(2).map(|v| v.to_ascii_uppercase()).unwrap_or_default(), w[3.min(w.len())..].to_vec()) } else { (false, command.clone(), w[2.min(w.len())..].to_vec()) };
        match verb.as_str() {
            "CAPABILITY" => out.extend_from_slice(format!("* CAPABILITY IMAP4rev1 MOVE UIDPLUS SPECIAL-USE\r\n{tag} OK CAPABILITY completed\r\n").as_bytes()),
            "LOGIN" => {
                let b = box_.lock().unwrap();
                let ok = w.get(2) == Some(&b.user) && w.get(3) == Some(&b.password);
                out.extend_from_slice(if ok { format!("{tag} OK LOGIN completed\r\n") } else { format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n") }.as_bytes());
            }
            "EXAMINE" | "SELECT" => {
                let name = args.first().cloned().unwrap_or_default();
                let b = box_.lock().unwrap();
                match b.held_in(&name) {
                    None => out.extend_from_slice(format!("{tag} NO [NONEXISTENT] no mailbox {name}\r\n").as_bytes()),
                    Some(held) => {
                        (open, may_change) = (name, verb == "SELECT");
                        out.extend_from_slice(
                            format!(
                                "* FLAGS (\\Seen \\Answered \\Flagged \\Deleted)\r\n* {} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n{tag} OK [{}] {verb} completed\r\n",
                                held.len(),
                                b.uid_validity,
                                if may_change { "READ-WRITE" } else { "READ-ONLY" }
                            )
                            .as_bytes(),
                        );
                    }
                }
            }
            "FETCH" => {
                let (set, what) = (args.first().cloned().unwrap_or_default(), args[1.min(args.len())..].join(" "));
                let b = box_.lock().unwrap();
                let held = b.held_in(&open).cloned().unwrap_or_default();
                let last = held.len() as u32;
                let chosen: Vec<(u32, &Held)> = if by_uid {
                    let wanted: Vec<u32> = numbers(&set, u32::MAX).into_iter().take(10_000).collect();
                    held.iter().enumerate().filter(|(_, h)| wanted.contains(&h.uid)).map(|(i, h)| (i as u32 + 1, h)).collect()
                } else {
                    numbers(&set, last).into_iter().map(|n| (n, &held[n as usize - 1])).collect()
                };
                let upper = what.to_ascii_uppercase();
                for (n, h) in chosen {
                    let mut parts = format!("* {n} FETCH (UID {}", h.uid).into_bytes();
                    if upper.contains("FLAGS") {
                        parts.extend_from_slice(format!(" {}", flags_of(h)).as_bytes());
                    }
                    if upper.contains("RFC822.SIZE") {
                        parts.extend_from_slice(format!(" RFC822.SIZE {}", h.raw.len()).as_bytes());
                    }
                    if upper.contains("BODY.PEEK[HEADER]") || upper.contains("BODY[HEADER]") {
                        let head = header_of(&h.raw);
                        parts.extend_from_slice(format!(" BODY[HEADER] {{{}}}\r\n", head.len()).as_bytes());
                        parts.extend_from_slice(head);
                    } else if upper.contains("BODY.PEEK[]") || upper.contains("BODY[]") {
                        parts.extend_from_slice(format!(" BODY[] {{{}}}\r\n", h.raw.len()).as_bytes());
                        parts.extend_from_slice(&h.raw);
                    }
                    parts.extend_from_slice(b")\r\n");
                    out.extend_from_slice(&parts);
                }
                out.extend_from_slice(format!("{tag} OK FETCH completed\r\n").as_bytes());
            }
            "SEARCH" => {
                let b = box_.lock().unwrap();
                let held = b.held_in(&open).cloned().unwrap_or_default();
                let found: Vec<String> = held.iter().enumerate().filter(|(_, h)| !h.deleted && answers(h, &args)).map(|(i, h)| if by_uid { h.uid.to_string() } else { (i + 1).to_string() }).collect();
                out.extend_from_slice(format!("* SEARCH {}\r\n{tag} OK SEARCH completed\r\n", found.join(" ")).trim_start().as_bytes());
                if found.is_empty() {
                    out = format!("* SEARCH\r\n{tag} OK SEARCH completed\r\n").into_bytes();
                }
            }
            "STORE" if !may_change => out.extend_from_slice(format!("{tag} NO [READ-ONLY] the mailbox was opened with EXAMINE\r\n").as_bytes()),
            "STORE" => {
                let (set, how) = (args.first().cloned().unwrap_or_default(), args.get(1).map(|h| h.to_ascii_uppercase()).unwrap_or_default());
                let named = args[2.min(args.len())..].join(" ").to_ascii_uppercase();
                let on = !how.starts_with('-');
                let mut b = box_.lock().unwrap();
                let held = b.place(&open).expect("it was opened");
                let wanted = numbers(&set, u32::MAX);
                for (i, h) in held.iter_mut().enumerate() {
                    if !wanted.contains(&if by_uid { h.uid } else { i as u32 + 1 }) {
                        continue;
                    }
                    if named.contains("\\SEEN") {
                        h.seen = on;
                    }
                    if named.contains("\\FLAGGED") {
                        h.flagged = on;
                    }
                    if named.contains("\\DELETED") {
                        h.deleted = on;
                    }
                    if !how.contains(".SILENT") {
                        out.extend_from_slice(format!("* {} FETCH (UID {} {})\r\n", i + 1, h.uid, flags_of(h)).as_bytes());
                    }
                }
                out.extend_from_slice(format!("{tag} OK STORE completed\r\n").as_bytes());
            }
            "COPY" | "MOVE" if verb == "MOVE" && !may_change => out.extend_from_slice(format!("{tag} NO [READ-ONLY] the mailbox was opened with EXAMINE\r\n").as_bytes()),
            "COPY" | "MOVE" => {
                let (set, to) = (args.first().cloned().unwrap_or_default(), args.get(1).cloned().unwrap_or_default());
                let mut b = box_.lock().unwrap();
                if b.held_in(&to).is_none() {
                    out.extend_from_slice(format!("{tag} NO [TRYCREATE] no mailbox {to}\r\n").as_bytes());
                } else {
                    let wanted = numbers(&set, u32::MAX);
                    let from = b.place(&open).expect("it was opened");
                    let chosen: Vec<(usize, Held)> = from.iter().enumerate().filter(|(i, h)| wanted.contains(&if by_uid { h.uid } else { *i as u32 + 1 })).map(|(i, h)| (i, h.clone())).collect();
                    if verb == "MOVE" {
                        for (i, _) in chosen.iter().rev() {
                            from.remove(*i);
                            out.extend_from_slice(format!("* {} EXPUNGE\r\n", i + 1).as_bytes());
                        }
                    }
                    for (_, mut h) in chosen {
                        h.uid = b.next_uid;
                        h.deleted = false;
                        b.next_uid += 1;
                        b.place(&to).expect("looked at").push(h);
                    }
                    out.extend_from_slice(format!("{tag} OK {verb} completed\r\n").as_bytes());
                }
            }
            "EXPUNGE" if !may_change => out.extend_from_slice(format!("{tag} NO [READ-ONLY] the mailbox was opened with EXAMINE\r\n").as_bytes()),
            "EXPUNGE" => {
                let only = by_uid.then(|| numbers(&args.first().cloned().unwrap_or_default(), u32::MAX));
                let mut b = box_.lock().unwrap();
                let held = b.place(&open).expect("it was opened");
                let mut i = 0;
                while i < held.len() {
                    if held[i].deleted && only.as_ref().is_none_or(|o| o.contains(&held[i].uid)) {
                        held.remove(i);
                        out.extend_from_slice(format!("* {} EXPUNGE\r\n", i + 1).as_bytes());
                    } else {
                        i += 1;
                    }
                }
                out.extend_from_slice(format!("{tag} OK EXPUNGE completed\r\n").as_bytes());
            }
            "LIST" => {
                let b = box_.lock().unwrap();
                out.extend_from_slice(b"* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n");
                for f in &b.folders {
                    out.extend_from_slice(format!("* LIST (\\HasNoChildren{}) \"/\" \"{}\"\r\n", f.role.map(|r| format!(" {r}")).unwrap_or_default(), f.name).as_bytes());
                }
                out.extend_from_slice(format!("{tag} OK LIST completed\r\n").as_bytes());
            }
            "NOOP" | "CLOSE" => out.extend_from_slice(format!("{tag} OK {verb} completed\r\n").as_bytes()),
            "LOGOUT" => {
                let _ = write.write_all(format!("* BYE Demido's stand-in says goodbye\r\n{tag} OK LOGOUT completed\r\n").as_bytes()).await;
                return;
            }
            // APPEND and the rest: this stand-in does none of them, and says so.
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
    let folder = |name: &str, role: &'static str| Folder { name: name.into(), role: Some(role), held: Vec::new() };
    let folders = vec![folder("Sent", "\\Sent"), folder("Drafts", "\\Drafts"), folder("Trash", "\\Trash"), folder("Archive", "\\Archive")];
    let next_uid = inbox.iter().map(|h| h.uid).max().unwrap_or(0) + 1000;
    let box_: Shared = Arc::new(Mutex::new(Mailbox { user: user.into(), password: password.into(), inbox, folders, next_uid, uid_validity: 7, ..Mailbox::default() }));
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
