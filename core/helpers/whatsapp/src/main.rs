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
use whatsapp_rust::sync_task::MajorSyncTask;
use whatsapp_rust::wacore::appstate::patch_decode::WAPatchName;

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
/// How many names came, and how many could be told by both of their ids: for the log, which keeps no name.
static NAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static BOTH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Someone's name, with every id they go by: WhatsApp names a person by their number (`…@s.whatsapp.net`) or by
/// a private id (`…@lid`), and a chat may be under either. The map between them is the library's.
async fn say_name(client: &Client, ids: &[String], name: &str, weak: bool) {
    let mut all: Vec<String> = ids.iter().filter(|i| !i.is_empty()).cloned().collect();
    for id in ids {
        if let Ok(jid) = id.parse::<Jid>()
            && let Ok(Some(e)) = client.get_lid_pn_entry(&jid).await
        {
            all.push(format!("{}@lid", e.lid));
            all.push(format!("{}@s.whatsapp.net", e.phone_number));
        }
    }
    all.sort();
    all.dedup();
    NAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if all.iter().any(|i| i.ends_with("@lid")) && all.iter().any(|i| i.ends_with("@s.whatsapp.net")) {
        BOTH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    say(json!({ "event": "name", "ids": all, "name": name, "weak": weak }));
}

/// Messages from history are old: they are kept, not counted as new. Each chat's unread count is WhatsApp's own.
async fn history(client: &Client, sync: &wa::HistorySync) {
    for p in &sync.pushnames {
        if let (Some(id), Some(pushname)) = (&p.id, &p.pushname)
            && !pushname.is_empty()
        {
            say_name(client, std::slice::from_ref(id), pushname, true).await;
        }
    }
    for c in &sync.conversations {
        let id = c.id.clone();
        let name = c.name.clone().or_else(|| c.display_name.clone()).unwrap_or_default();
        say(json!({ "event": "chat", "id": id, "name": name, "kind": kind_of(&id), "unread": c.unread_count.unwrap_or(0) }));
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
                "mine": mine, "text": text, "at": m.message_timestamp.unwrap_or(0) as i64 * 1000, "old": true,
            }));
        }
    }
}

/// The library's log on standard error, which the core keeps in the account's `helper.log`: warnings, and what
/// the app state sync does. It writes ids and counts, not what people say.
struct Log;
impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn || (m.target().contains("AppState") && m.level() <= log::Level::Debug)
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{} [{}] {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

#[tokio::main]
async fn main() {
    let _ = log::set_logger(&Log).map(|()| log::set_max_level(log::LevelFilter::Debug));
    let args: Vec<String> = std::env::args().collect();
    let Some(store) = arg(&args, "--store") else {
        eprintln!("demido-whatsapp: give --store <file>");
        std::process::exit(2);
    };
    let phone = arg(&args, "--phone");
    let names_read = Arc::new(std::path::PathBuf::from(format!("{store}.names-read")));
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
        .on_event_for(&[EventKind::PairSuccess, EventKind::Connected, EventKind::LoggedOut, EventKind::HistorySync, EventKind::ContactUpdate, EventKind::PushNameUpdate], move |event, client| {
            let names_read = names_read.clone();
            async move {
                match &*event {
                    Event::PairSuccess(p) => {
                        // A new link: its first sync brings the address book; nothing to ask again.
                        let _ = std::fs::write(names_read.as_path(), b"");
                        say(json!({ "event": "linked", "id": p.id.to_string(), "name": client.push_name(), "phone": format!("+{}", p.id.user) }))
                    }
                    Event::Connected(_) => {
                        if let Some(pn) = client.pn() {
                            say(json!({ "event": "linked", "id": pn.to_string(), "name": client.push_name(), "phone": format!("+{}", pn.user) }));
                        }
                        // The names of the person's address book come with the contacts collection, synced in full at
                        // linking. An account linked before Demido read them has them asked for once more, once.
                        if !names_read.exists() {
                            // The library keeps the version it has and skips a snapshot no newer: set back to none, as at linking.
                            let backend = client.persistence_manager().backend();
                            let collection = WAPatchName::CriticalUnblockLow.as_str();
                            let _ = backend.set_version(collection, Default::default()).await;
                            let _ = backend.clear_mutation_macs(collection).await;
                            client.process_sync_task(MajorSyncTask::AppStateSync { name: WAPatchName::CriticalUnblockLow, full_sync: true }).await;
                            let _ = std::fs::write(names_read.as_path(), b"");
                            eprintln!(
                                "demido-whatsapp: the address book was read again: {} names so far, {} with both ids",
                                NAMES.load(std::sync::atomic::Ordering::Relaxed),
                                BOTH.load(std::sync::atomic::Ordering::Relaxed)
                            );
                        }
                    }
                    // A contact as the person saved it on their phone: the strongest name there is.
                    Event::ContactUpdate(c) => {
                        let a = &c.action;
                        let name = a.full_name.clone().filter(|n| !n.is_empty()).or_else(|| a.first_name.clone().filter(|n| !n.is_empty())).or_else(|| a.username.clone().filter(|n| !n.is_empty()));
                        if let Some(n) = name {
                            let ids: Vec<String> = [Some(c.jid.to_string()), a.lid_jid.clone(), a.pn_jid.clone()].into_iter().flatten().collect();
                            say_name(&client, &ids, &n, false).await;
                        }
                    }
                    // The name someone gave themselves: used only where there is no better one.
                    Event::PushNameUpdate(p) if !p.new_push_name.is_empty() => say_name(&client, &[p.jid.to_string()], &p.new_push_name, true).await,
                    Event::LoggedOut(_) => say(json!({ "event": "gone", "why": "WhatsApp unlinked Demido (on the phone, under Linked devices, or for a reason of its own). Link it again." })),
                    Event::HistorySync(h) => {
                        if let Some(sync) = h.get() {
                            history(&client, sync).await;
                        }
                    }
                    _ => {}
                }
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
