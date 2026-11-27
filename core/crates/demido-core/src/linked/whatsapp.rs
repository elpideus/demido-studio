//! WhatsApp as the person (DECISIONS G20; research note 37): a linked device, through `demido-whatsapp`, a helper
//! program beside the core built on `whatsapp-rust` (MIT). Its library cannot be built into the core: only one
//! SQLite may be linked into a program, and its store's is not the core's.
//!
//! The helper keeps WhatsApp's keys in the account's own folder (`linked/<id>/whatsapp.db`, readable by the
//! person's user only), and speaks to the core in lines of JSON, one a message:
//!
//! - the core says `{"do":"send","n":1,"chat":"<jid>","text":"…"}` and `{"do":"sign_out"}`;
//! - the helper says `{"event":"qr","code":"…"}` (to scan, a new one now and then), `{"event":"pair_code",
//!   "code":"ABCD-EFGH"}` (to type on the phone), `{"event":"linked","id","name","phone"}`, `{"event":"chat",
//!   "id","name","kind"}`, `{"event":"message","chat","chat_name","kind","id","from_id","from_name","mine","text",
//!   "at"}`, `{"event":"answer","n":1,"line":{…}}` or `{"event":"answer","n":1,"error":"…"}`, and
//!   `{"event":"gone","why":"…"}` when the phone unlinked it.
//!
//! WhatsApp gives no chat's history on demand: what came since linking (and what the phone sent at linking) is
//! what there is.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use super::{Heard, Line, NewChat, Order, Step};
use crate::{Error, Result};

const NAME: &str = if cfg!(windows) { "demido-whatsapp.exe" } else { "demido-whatsapp" };

/// Where the helper is: beside the `demido` program; in a development build, also in its own workspace's build.
pub fn helper_beside(exe: Option<PathBuf>) -> Option<PathBuf> {
    let exe = exe?;
    let dir = exe.parent()?;
    let beside = dir.join(NAME);
    if beside.is_file() {
        return Some(beside);
    }
    #[cfg(debug_assertions)]
    for up in dir.ancestors() {
        for kind in ["debug", "release"] {
            let built = up.join("helpers").join("whatsapp").join("target").join(kind).join(NAME);
            if built.is_file() {
                return Some(built);
            }
        }
    }
    None
}

/// A QR code as an SVG picture the window shows as it is.
fn qr_svg(code: &str) -> Option<String> {
    let qr = qrcode::QrCode::new(code.as_bytes()).ok()?;
    Some(qr.render::<qrcode::render::svg::Color>().min_dimensions(240, 240).quiet_zone(true).build())
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

fn line_from(v: &Value) -> Line {
    Line {
        chat: text(v, "chat"),
        id: text(v, "id"),
        from_id: text(v, "from_id"),
        from_name: v["from_name"].as_str().filter(|s| !s.is_empty()).unwrap_or("someone").to_string(),
        mine: v["mine"].as_bool().unwrap_or(false),
        text: text(v, "text"),
        at: v["at"].as_i64().unwrap_or_else(crate::now_ms),
    }
}

/// What the helper said, as the store hears it. `None`: nothing for the store.
pub(super) fn heard_of(v: &Value) -> Option<Heard> {
    match v["event"].as_str()? {
        "qr" => Some(Heard::Step(Step { kind: "qr".into(), hint: None, show: qr_svg(v["code"].as_str()?), problem: None })),
        "pair_code" => Some(Heard::Step(Step { kind: "pair_code".into(), hint: None, show: Some(text(v, "code")), problem: None })),
        "linked" => Some(Heard::Linked { me: text(v, "id"), name: v["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("WhatsApp").to_string(), handle: text(v, "phone") }),
        "chat" => Some(Heard::Chats(vec![NewChat { id: text(v, "id"), name: text(v, "name"), kind: text(v, "kind"), place: None }])),
        "message" => {
            let line = line_from(v);
            let name = v["chat_name"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| if line.mine { "A conversation".into() } else { line.from_name.clone() });
            Some(Heard::Said { chat: NewChat { id: line.chat.clone(), name, kind: v["kind"].as_str().unwrap_or("direct").to_string(), place: None }, line })
        }
        "gone" => Some(Heard::Gone(v["why"].as_str().unwrap_or("WhatsApp unlinked this device.").to_string())),
        _ => None,
    }
}

/// Speaks with the helper until it ends.
async fn talk<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(read: R, mut write: W, tx: mpsc::Sender<Heard>, mut orders: mpsc::Receiver<Order>) -> Result<()> {
    let mut lines = read.lines();
    let mut waiting: HashMap<u64, oneshot::Sender<Result<Line>>> = HashMap::new();
    let mut n = 0u64;
    loop {
        tokio::select! {
            got = lines.next_line() => {
                let Some(line) = got.map_err(|e| Error::Net(format!("WhatsApp's helper could not be read ({e}).")))? else {
                    return Err(Error::Net("WhatsApp's helper stopped.".into()));
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v["event"] == "answer" {
                    if let Some(done) = v["n"].as_u64().and_then(|n| waiting.remove(&n)) {
                        let _ = done.send(match v["error"].as_str() {
                            Some(e) => Err(Error::Net(format!("WhatsApp did not take it ({e})."))),
                            None => Ok(line_from(&v["line"])),
                        });
                    }
                    continue;
                }
                if v["event"] == "problem" {
                    tracing::warn!("WhatsApp's helper: {}", v["text"].as_str().unwrap_or_default());
                    continue;
                }
                if let Some(heard) = heard_of(&v) && tx.send(heard).await.is_err() {
                    return Ok(());
                }
            }
            order = orders.recv() => match order {
                Some(Order::Send { chat, text, done }) => {
                    n += 1;
                    let said = json!({ "do": "send", "n": n, "chat": chat, "text": text }).to_string() + "\n";
                    if write.write_all(said.as_bytes()).await.is_err() || write.flush().await.is_err() {
                        let _ = done.send(Err(Error::Net("WhatsApp's helper stopped.".into())));
                        return Err(Error::Net("WhatsApp's helper stopped.".into()));
                    }
                    waiting.insert(n, done);
                }
                // WhatsApp gives no history on demand: what was kept is all there is.
                Some(Order::History { done, .. }) => { let _ = done.send(Ok(Vec::new())); }
                Some(Order::SignOut { done }) => {
                    let _ = write.write_all(b"{\"do\":\"sign_out\"}\n").await;
                    let _ = write.flush().await;
                    // The helper says it is gone, or stops.
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(8), async { while let Ok(Some(_)) = lines.next_line().await {} }).await;
                    let _ = done.send(());
                    return Ok(());
                }
                None => return Ok(()),
            },
        }
    }
}

/// The helper's stand-in for the tests: it links when told, has a chat, and answers what is sent.
#[cfg(any(test, debug_assertions))]
#[derive(Default)]
pub struct StandIn {
    pub sent: std::sync::Mutex<Vec<(String, String)>>,
    pub signed_out: std::sync::atomic::AtomicBool,
}

#[cfg(any(test, debug_assertions))]
async fn stand_in(s: std::sync::Arc<StandIn>, phone: Option<String>, side: tokio::io::DuplexStream) {
    let (read, mut write) = tokio::io::split(side);
    let out = |v: Value| v.to_string() + "\n";
    let first = match &phone {
        Some(_) => json!({ "event": "pair_code", "code": "HERO-N123" }),
        None => json!({ "event": "qr", "code": "2@stand-in,qr" }),
    };
    for v in [
        first,
        json!({ "event": "linked", "id": "15550001@s.whatsapp.net", "name": "Ada", "phone": "+15550001" }),
        json!({ "event": "chat", "id": "15550002@s.whatsapp.net", "name": "Charles", "kind": "direct" }),
        json!({ "event": "message", "chat": "15550002@s.whatsapp.net", "chat_name": "Charles", "kind": "direct", "id": "m1", "from_id": "15550002@s.whatsapp.net", "from_name": "Charles", "mine": false, "text": "Tea at four?", "at": 2_000 }),
    ] {
        let _ = write.write_all(out(v).as_bytes()).await;
    }
    let read = tokio::io::BufReader::new(read);
    let mut lines = read.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        match v["do"].as_str() {
            Some("send") => {
                s.sent.lock().unwrap().push((text(&v, "chat"), text(&v, "text")));
                let answer = json!({ "event": "answer", "n": v["n"], "line": { "chat": v["chat"], "id": format!("s{}", v["n"]), "from_id": "15550001@s.whatsapp.net", "from_name": "Ada", "mine": true, "text": v["text"], "at": 3_000 } });
                let _ = write.write_all(out(answer).as_bytes()).await;
            }
            Some("sign_out") => {
                s.signed_out.store(true, std::sync::atomic::Ordering::Relaxed);
                return;
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub enum Way {
    /// The helper program, where it is (`None`: not with this Demido).
    Helper(Option<PathBuf>),
    #[cfg(any(test, debug_assertions))]
    StandIn(std::sync::Arc<StandIn>),
}

impl Way {
    pub async fn run(self, folder: PathBuf, phone: Option<String>, tx: mpsc::Sender<Heard>, orders: mpsc::Receiver<Order>) -> Result<()> {
        match self {
            Way::Helper(None) => Err(Error::Invalid(format!("WhatsApp's helper program ({NAME}) is not with this Demido. It is built apart from the core: see helpers/whatsapp."))),
            Way::Helper(Some(program)) => {
                std::fs::create_dir_all(&folder)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700));
                }
                let log = std::fs::File::create(folder.join("helper.log"))?;
                let mut command = tokio::process::Command::new(program);
                command.arg("--store").arg(folder.join("whatsapp.db"));
                if let Some(phone) = &phone {
                    command.arg("--phone").arg(phone);
                }
                let mut child = command
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(log)
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|e| Error::Other(format!("WhatsApp's helper did not start ({e}).")))?;
                let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Err(Error::Other("WhatsApp's helper has no input or output.".into())) };
                let ended = talk(tokio::io::BufReader::new(stdout), stdin, tx, orders).await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
                ended
            }
            #[cfg(any(test, debug_assertions))]
            Way::StandIn(s) => {
                let (core_side, helper_side) = tokio::io::duplex(64 * 1024);
                tokio::spawn(stand_in(s, phone, helper_side));
                let (read, write) = tokio::io::split(core_side);
                talk(tokio::io::BufReader::new(read), write, tx, orders).await
            }
        }
    }
}
