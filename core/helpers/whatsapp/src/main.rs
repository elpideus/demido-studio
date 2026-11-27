//! demido-whatsapp: WhatsApp as a linked device, for Demido's core (DECISIONS G20; research note 37).
//!
//! Started by the core for one account, with `--store <file>` (WhatsApp's keys, in the account's own folder) and,
//! to link by a code typed on the phone instead of a QR code, `--phone <number>`. It speaks lines of JSON: what
//! WhatsApp says goes out on its standard output, the core's orders come in on its standard input (the core's
//! `linked/whatsapp.rs` says what each line is). Its log goes to standard error, which the core keeps in a file.
//!
//! It does only what the core asks and what WhatsApp sends: the chats and messages, a message sent as the person,
//! and unlinking. It reads no receipts out and sends none of its own.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::AsyncBufReadExt;
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;

/// One line out to the core. A lock keeps each line whole.
fn say(v: Value) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn kind_of(chat: &str) -> &'static str {
    if chat.ends_with("@g.us") { "group" } else { "direct" }
}

/// The readable text of a message: its words, or what it is when it has none.
fn text_of(m: &wa::Message) -> Option<String> {
    if let Some(t) = m.text_content() {
        return Some(t.to_string());
    }
    let what = if m.image_message.is_set() {
        "[a picture]"
    } else if m.video_message.is_set() {
        "[a video]"
    } else if m.audio_message.is_set() {
        "[a voice message]"
    } else if m.document_message.is_set() {
        "[a file]"
    } else if m.sticker_message.is_set() {
        "[a sticker]"
    } else {
        return None;
    };
    Some(what.to_string())
}

/// The chats and their messages the phone sent at linking (the newest 50 of each).
fn history(sync: &wa::HistorySync) {
    for c in &sync.conversations {
        let id = c.id.clone();
        let name = c.name.clone().or_else(|| c.display_name.clone()).unwrap_or_default();
        say(json!({ "event": "chat", "id": id, "name": if name.is_empty() { id.split('@').next().unwrap_or_default().to_string() } else { name.clone() }, "kind": kind_of(&id) }));
        let mut messages: Vec<&wa::WebMessageInfo> = c.messages.iter().filter_map(|m| m.message.as_option()).collect();
        messages.sort_by_key(|m| m.message_timestamp.unwrap_or(0));
        let start = messages.len().saturating_sub(50);
        for m in &messages[start..] {
            let Some(text) = m.message.as_option().and_then(text_of) else { continue };
            let key = &m.key;
            let mine = key.from_me.unwrap_or(false);
            let from = key.participant.clone().or_else(|| m.participant.clone()).unwrap_or_else(|| id.clone());
            say(json!({
                "event": "message", "chat": id, "chat_name": name, "kind": kind_of(&id),
                "id": key.id.clone().unwrap_or_default(), "from_id": from, "from_name": m.push_name.clone().unwrap_or_default(),
                "mine": mine, "text": text, "at": m.message_timestamp.unwrap_or(0) as i64 * 1000,
            }));
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(store) = arg(&args, "--store") else {
        eprintln!("demido-whatsapp: give --store <file>");
        std::process::exit(2);
    };
    let phone = arg(&args, "--phone");
    let backend = match whatsapp_rust::store::SqliteStore::new(&store).await {
        Ok(b) => b,
        Err(e) => {
            say(json!({ "event": "problem", "text": format!("the store could not be opened ({e})") }));
            std::process::exit(1);
        }
    };
    let mut builder = Bot::builder()
        .with_backend(backend)
        .with_device_props(whatsapp_rust::wacore::store::DevicePropsOverride::new().with_os("Demido Studio"))
        .on_qr_code(|code, _| async move { say(json!({ "event": "qr", "code": code })) })
        .on_pair_code(|code, _| async move { say(json!({ "event": "pair_code", "code": code })) })
        .on_event_for(&[EventKind::PairSuccess, EventKind::Connected, EventKind::LoggedOut, EventKind::HistorySync], |event, client| async move {
            match &*event {
                Event::PairSuccess(p) => say(json!({ "event": "linked", "id": p.id.to_string(), "name": client.push_name(), "phone": format!("+{}", p.id.user) })),
                Event::Connected(_) => {
                    if let Some(pn) = client.pn() {
                        say(json!({ "event": "linked", "id": pn.to_string(), "name": client.push_name(), "phone": format!("+{}", pn.user) }));
                    }
                }
                Event::LoggedOut(_) => say(json!({ "event": "gone", "why": "WhatsApp unlinked Demido (on the phone, under Linked devices, or for a reason of its own). Link it again." })),
                Event::HistorySync(h) => {
                    if let Some(sync) = h.get() {
                        history(sync);
                    }
                }
                _ => {}
            }
        })
        .on_message(|ctx| async move {
            let Some(text) = text_of(&ctx.message) else { return };
            let s = &ctx.info.source;
            let chat = s.chat.to_string();
            say(json!({
                "event": "message", "chat": chat, "chat_name": if s.is_group || s.is_from_me { String::new() } else { ctx.info.push_name.clone() },
                "kind": if s.is_group { "group" } else { "direct" }, "id": ctx.info.id.to_string(), "from_id": s.sender.to_string(),
                "from_name": ctx.info.push_name.clone(), "mine": s.is_from_me, "text": text, "at": ctx.info.timestamp.timestamp_millis(),
            }));
        });
    if let Some(phone) = phone {
        builder = builder.with_pair_code(PairCodeOptions { phone_number: phone, ..Default::default() });
    }
    let bot = match builder.build().await {
        Ok(b) => b,
        Err(e) => {
            say(json!({ "event": "problem", "text": format!("WhatsApp's client could not be made ({e})") }));
            std::process::exit(1);
        }
    };
    let handle = bot.spawn();
    let client: Arc<Client> = handle.client();
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v["do"].as_str() {
            Some("send") => {
                let (n, chat, text) = (v["n"].clone(), v["chat"].as_str().unwrap_or_default().to_string(), v["text"].as_str().unwrap_or_default().to_string());
                let jid: Jid = match chat.parse() {
                    Ok(j) => j,
                    Err(e) => {
                        say(json!({ "event": "answer", "n": n, "error": format!("{chat} is not a chat ({e})") }));
                        continue;
                    }
                };
                match client.send_message(jid, wa::Message::text(text.as_str())).await {
                    Ok(sent) => say(json!({ "event": "answer", "n": n, "line": {
                        "chat": chat, "id": sent.message_id.to_string(), "from_id": client.pn().map(|p| p.to_string()).unwrap_or_default(),
                        "from_name": client.push_name(), "mine": true, "text": text, "at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
                    } })),
                    Err(e) => say(json!({ "event": "answer", "n": n, "error": e.to_string() })),
                }
            }
            Some("sign_out") => {
                client.logout().await;
                say(json!({ "event": "gone", "why": "Unlinked from Demido." }));
                break;
            }
            _ => {}
        }
    }
    handle.shutdown().await;
}
