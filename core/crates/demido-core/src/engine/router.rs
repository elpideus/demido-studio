//! The runtime's router: starting it, stopping it, and talking to it.
//!
//! What it answers was read from llama.cpp's server documentation and then seen on the real program
//! (research note 12; `core/dev/router-trial.py` shows it again).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::events::EventLog;
use crate::runtime::Installed;
use crate::{Error, Result, now_ms};

/// The note that says a router is running, so that a later Demido can end one that was left behind.
const NOTE: &str = "router.json";
const MIB: u64 = 1024 * 1024;

/// Everything needed to start the runtime's serving program.
pub struct Launch {
    /// The installed runtime's folder.
    pub root: PathBuf,
    /// The serving program, inside `root`.
    pub server: String,
    /// Folders inside `root` that hold libraries the program needs.
    pub lib_dirs: Vec<String>,
    pub args: Vec<String>,
    /// The port it was told to listen on, on this computer only.
    pub port: u16,
}

/// A started runtime.
pub trait Running: Send + Sync {
    fn alive(&mut self) -> bool;
    /// Asks it to end, waits a moment, and makes sure it has. May take seconds: call it off the request threads.
    fn stop(&mut self);
    /// Its process number, for the note. `None` when it is not a process of its own (a stand-in in tests).
    fn pid(&self) -> Option<u32>;
}

/// How the runtime is started. The real one starts the program; a test starts a stand-in.
pub trait Launcher: Send + Sync {
    fn launch(&self, plan: &Launch) -> Result<Box<dyn Running>>;
}

/// Starts the real program.
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, plan: &Launch) -> Result<Box<dyn Running>> {
        let mut command = crate::runtime::command(&plan.root, &plan.server, &plan.lib_dirs);
        command.args(&plan.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let child = command.spawn().map_err(|e| Error::Other(format!("The model runtime could not be started: {e}")))?;
        Ok(Box::new(Process(child)))
    }
}

struct Process(std::process::Child);

impl Running for Process {
    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }

    fn stop(&mut self) {
        if !self.alive() {
            return;
        }
        // Asked, not killed: the router ends its own model processes when it is asked to end.
        ask_to_end(self.0.id());
        let until = Instant::now() + Duration::from_secs(5);
        while self.alive() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(25));
        }
        if self.alive() {
            self.0.kill().ok();
        }
        self.0.wait().ok();
    }

    fn pid(&self) -> Option<u32> {
        Some(self.0.id())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Asks a process to end and lets it tidy up (SIGTERM). Windows has no such request for a program without a
/// window; there the caller ends it the hard way, after its models were unloaded one by one.
fn ask_to_end(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

/// Ends a router that an earlier Demido left running (it was killed, or crashed), so it stops holding memory.
/// Only a process that is named in the note and really is the runtime's program is touched.
pub(super) fn end_leftover(work: &Path) {
    let note = work.join(NOTE);
    let Some(pid) = std::fs::read(&note).ok().and_then(|text| serde_json::from_slice::<Value>(&text).ok()).and_then(|v| v["pid"].as_u64()) else { return };
    #[cfg(unix)]
    {
        let named = std::process::Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        if named.contains("llama-server") {
            tracing::warn!("A model runtime from an earlier run was still there (process {pid}). Ending it.");
            ask_to_end(pid as u32);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid; // Not built for Windows.
    }
    std::fs::remove_file(note).ok();
}

fn free_port() -> Result<u16> {
    // Ask the system for any free port, note it, and give it back. The runtime takes it a moment later.
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn new_key() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// A model as the router lists it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Seen {
    pub id: String,
    /// `unloaded`, `loading` or `loaded`.
    pub state: String,
    pub context_length: Option<u64>,
    /// What it needs on the first graphics device at the pinned context, in MiB. `None` until measured.
    pub needs_mb: Option<u64>,
    /// That device's whole memory, in MiB.
    pub device_total_mb: Option<u64>,
}

fn seen_in(listing: &Value) -> Vec<Seen> {
    listing["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let device = m["memory"]["min"]["devices"].as_array().and_then(|all| all.iter().find(|d| d["kind"] == "device"));
            let bytes = |d: &Value, part: &str| d[part].as_u64().unwrap_or(0);
            Some(Seen {
                id: m["id"].as_str()?.to_string(),
                state: m["status"]["value"].as_str().unwrap_or("unloaded").to_string(),
                context_length: m["context_length"].as_u64(),
                needs_mb: device.map(|d| (bytes(d, "model") + bytes(d, "context") + bytes(d, "compute")).div_ceil(MIB)),
                device_total_mb: device.and_then(|d| d["total"].as_u64()).map(|b| b / MIB),
            })
        })
        .collect()
}

pub(super) struct Router {
    running: Box<dyn Running>,
    base: String,
    key: String,
    http: reqwest::Client,
    note: PathBuf,
}

impl Router {
    /// Starts the router and waits until it answers.
    pub async fn start(launcher: &dyn Launcher, work: &Path, root: &Path, runtime: &Installed, models_dir: &Path, context: u64, margin_mb: u64) -> Result<Self> {
        let Some(server) = runtime.server.clone() else {
            return Err(Error::Other(format!("{} holds no serving program.", runtime.name)));
        };
        std::fs::create_dir_all(work)?;
        let (port, key) = (free_port()?, new_key());
        let key_file = work.join("router.key");
        crate::auth::write_private(&key_file, &format!("{key}\n"))?;
        let log = work.join("router.log");
        let text = |p: &Path| p.to_string_lossy().into_owned();
        #[rustfmt::skip]
        let args: Vec<String> = [
            "--models-dir", &text(models_dir),
            // This computer only, a port of our own, a key of our own.
            "--host", "127.0.0.1", "--port", &port.to_string(), "--api-key-file", &text(&key_file),
            // Nothing loads unless Demido asks. No second web UI. (Its own tools stay off unless asked for; they are not.)
            "--no-models-autoload", "--no-webui",
            // The limit is memory, not a count.
            "--models-max", "0", "--models-memory-margin", &margin_mb.to_string(),
            // Always pinned: left alone, a model takes the largest context that fits.
            "-c", &context.to_string(),
            "--log-file", &text(&log),
        ].iter().map(|s| s.to_string()).collect();
        let mut running = launcher.launch(&Launch { root: root.to_path_buf(), server, lib_dirs: runtime.lib_dirs.clone(), args, port })?;
        let note = work.join(NOTE);
        if let Some(pid) = running.pid() {
            std::fs::write(&note, json!({ "pid": pid, "port": port, "started_at": now_ms() }).to_string()).ok();
        }
        // Never through a proxy: this is a program on this computer.
        let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().map_err(|e| Error::Other(e.to_string()))?;
        let base = format!("http://127.0.0.1:{port}");
        let started = Instant::now();
        loop {
            if !running.alive() {
                std::fs::remove_file(&note).ok();
                return Err(Error::Other(format!("The model runtime ended as soon as it was started. What it said is in {}.", log.display())));
            }
            if http.get(format!("{base}/health")).send().await.is_ok_and(|r| r.status().is_success()) {
                break;
            }
            if started.elapsed() > Duration::from_secs(30) {
                running.stop();
                std::fs::remove_file(&note).ok();
                return Err(Error::Other(format!("The model runtime was started and did not answer within 30 seconds. What it said is in {}.", log.display())));
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        Ok(Self { running, base, key, http, note })
    }

    pub fn alive(&mut self) -> bool {
        self.running.alive()
    }

    pub fn address(&self) -> String {
        self.base.clone()
    }

    pub fn key(&self) -> String {
        self.key.clone()
    }

    async fn ask(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.http.request(method, format!("{}{path}", self.base)).bearer_auth(&self.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let res = request.send().await.map_err(|e| Error::Other(format!("The model runtime did not answer: {e}")))?;
        let status = res.status();
        let answer: Value = res.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Error::Other(format!("The model runtime refused ({status}): {}", answer["error"]["message"].as_str().unwrap_or("it did not say why"))));
        }
        Ok(answer)
    }

    /// The models the router knows. `look_again` makes it look at the folder again, so new files are seen.
    pub async fn models(&self, look_again: bool) -> Result<Vec<Seen>> {
        Ok(seen_in(&self.ask(reqwest::Method::GET, if look_again { "/models?reload=1" } else { "/models" }, None).await?))
    }

    pub async fn load(&self, id: &str) -> Result<()> {
        self.ask(reqwest::Method::POST, "/models/load", Some(json!({ "model": id }))).await.map(|_| ())
    }

    pub async fn unload(&self, id: &str) -> Result<()> {
        self.ask(reqwest::Method::POST, "/models/unload", Some(json!({ "model": id }))).await.map(|_| ())
    }

    /// What the runtime knows about a loaded model: its template, what the template supports, what the model takes in.
    pub async fn props(&self, id: &str) -> Result<Value> {
        self.ask(reqwest::Method::GET, &format!("/props?model={id}"), None).await
    }

    /// Ends it. May take a second or two.
    pub fn stop(mut self) {
        let mine = self.running.pid();
        self.running.stop();
        // A new router may have been started while this one was ending, and the note is then about that one.
        let noted = std::fs::read(&self.note).ok().and_then(|text| serde_json::from_slice::<Value>(&text).ok()).and_then(|v| v["pid"].as_u64());
        if mine.is_some() && noted == mine.map(u64::from) {
            std::fs::remove_file(&self.note).ok();
        }
    }
}

/// What a model can do, as the runtime reports it for a loaded model (research note 13).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Abilities {
    /// What thinking can be set to, besides off. Empty: the model does not think. `["on"]`: on or off.
    /// Otherwise the levels the model's template knows.
    pub thinking: Vec<String>,
    /// What it takes in besides text: `images`, `sound`, `video`.
    pub takes: Vec<String>,
    /// Whether its template has a place for tools of its own.
    pub tools: bool,
}

pub(super) fn abilities_in(props: &Value) -> Abilities {
    let caps = &props["chat_template_caps"];
    let levels: Vec<String> = caps["reasoning_effort_levels"].as_array().into_iter().flatten().filter_map(|l| l.as_str()).filter(|l| !l.eq_ignore_ascii_case("none")).map(str::to_string).collect();
    // A template that can be told to think or not names the switch. One with levels lists them.
    let thinking = if !levels.is_empty() {
        levels
    } else if props["chat_template"].as_str().is_some_and(|t| t.contains("enable_thinking")) {
        vec!["on".to_string()]
    } else {
        Vec::new()
    };
    let takes = [("vision", "images"), ("audio", "sound"), ("video", "video")].iter().filter(|(key, _)| props["modalities"][key] == true).map(|(_, word)| word.to_string()).collect();
    Abilities { thinking, takes, tools: caps["supports_tool_calls"] == true }
}

/// Tells the runtime to end the thought a model is in and go on to its answer. Works only for an answer
/// that was asked for with `reasoning_control`. False when there was nothing to end.
pub(super) async fn end_thought(base: &str, key: &str, model: &str, answer: &str) -> Result<bool> {
    let http = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().map_err(|e| Error::Other(e.to_string()))?;
    let res = http
        .post(format!("{base}/v1/chat/completions/control"))
        .bearer_auth(key)
        .json(&json!({ "id": answer, "action": "reasoning_end", "model": model }))
        .send()
        .await
        .map_err(|e| Error::Other(format!("The model runtime did not answer: {e}")))?;
    let said: Value = res.json().await.unwrap_or(Value::Null);
    Ok(said["success"] == true)
}

/// A piece of an answer as it arrives.
pub enum Piece<'a> {
    /// The runtime's own name for this answer. Comes first, once. It is what a thought is ended by.
    Started(&'a str),
    /// Part of what the model says to the person.
    Text(&'a str),
    /// Part of what the model thinks first, when it thinks.
    Thought(&'a str),
}

/// What the runtime sent back for one question.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub content: String,
    pub thought: String,
    pub timings: crate::sessions::Timings,
    /// True when it was stopped from outside before the model was done.
    pub stopped: bool,
}

/// Asks a loaded model and passes on its answer piece by piece. Ends when the model is done, when `stop` is
/// set (the connection is dropped, which is how the runtime is told to stop writing), or when it fails.
pub(super) async fn stream_answer(base: &str, key: &str, body: Value, stop: &std::sync::atomic::AtomicBool, heard: &mut (dyn FnMut(Piece) + Send)) -> Result<Answer> {
    use std::sync::atomic::Ordering;
    // No time limit on the whole answer: a long one is not a failure. A silence of two minutes is.
    let http = reqwest::Client::builder().no_proxy().read_timeout(Duration::from_secs(120)).build().map_err(|e| Error::Other(e.to_string()))?;
    let started = Instant::now();
    let mut res = http.post(format!("{base}/v1/chat/completions")).bearer_auth(key).json(&body).send().await.map_err(|e| Error::Other(format!("The model runtime did not answer: {e}")))?;
    if !res.status().is_success() {
        let said: Value = res.json().await.unwrap_or(Value::Null);
        let error = &said["error"];
        return Err(if error["type"] == "exceed_context_size_error" {
            Error::Invalid(format!(
                "This conversation is {} tokens long and the model is loaded with room for {}. It does not fit any more. Start a new session, or raise \"Context size\" in Settings. (Not built yet: making an old conversation shorter by itself.)",
                error["n_prompt_tokens"], error["n_ctx"]
            ))
        } else {
            Error::Other(format!("The model runtime refused: {}", error["message"].as_str().unwrap_or("it did not say why")))
        });
    }
    let mut answer = Answer::default();
    let mut kept = String::new();
    let mut named = false;
    loop {
        let piece = tokio::select! {
            piece = res.chunk() => piece,
            _ = async { while !stop.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(40)).await } } => {
                answer.stopped = true;
                break;
            }
        };
        let Some(piece) = piece.map_err(|e| Error::Other(format!("The answer broke off: {e}")))? else { break };
        kept.push_str(&String::from_utf8_lossy(&piece));
        while let Some(end) = kept.find('\n') {
            let line: String = kept.drain(..=end).collect();
            let Some(said) = line.trim().strip_prefix("data:").and_then(|text| serde_json::from_str::<Value>(text.trim()).ok()) else { continue };
            if !named && let Some(id) = said["id"].as_str() {
                named = true;
                heard(Piece::Started(id));
            }
            let delta = &said["choices"][0]["delta"];
            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                answer.content.push_str(text);
                heard(Piece::Text(text));
            }
            if let Some(text) = delta["reasoning_content"].as_str().filter(|t| !t.is_empty()) {
                answer.thought.push_str(text);
                heard(Piece::Thought(text));
            }
            // The last line before the end carries the runtime's own count of what it did.
            let t = &said["timings"];
            if t.is_object() {
                answer.timings.reused = t["cache_n"].as_u64().unwrap_or(0);
                answer.timings.read = t["prompt_n"].as_u64().unwrap_or(0);
                answer.timings.written = t["predicted_n"].as_u64().unwrap_or(0);
                answer.timings.per_second = t["predicted_per_second"].as_f64().unwrap_or(0.0);
            }
        }
    }
    answer.timings.took_ms = started.elapsed().as_millis() as u64;
    Ok(answer)
}

/// Reads the router's own news (`/models/sse`) for as long as it runs, and passes it on as Demido's.
pub(super) async fn pass_on_news(base: String, key: String, events: Arc<EventLog>) {
    let Ok(http) = reqwest::Client::builder().no_proxy().build() else { return };
    let Ok(mut res) = http.get(format!("{base}/models/sse")).bearer_auth(key).send().await else { return };
    let mut kept = String::new();
    while let Ok(Some(piece)) = res.chunk().await {
        kept.push_str(&String::from_utf8_lossy(&piece));
        // One piece of news per line that starts with "data:". A line may arrive in two pieces.
        while let Some(end) = kept.find('\n') {
            let line: String = kept.drain(..=end).collect();
            if let Some(said) = line.trim().strip_prefix("data:").and_then(|text| serde_json::from_str::<Value>(text.trim()).ok()) {
                super::news_from(&events, &said);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routers_list_is_read_as_the_real_one_sends_it() {
        // Shapes as the real router (llama.cpp b12359) sent them on 2026-11-21, shortened.
        let listing = json!({ "data": [
            { "id": "Qwen3-0.6B-Q8_0", "status": { "value": "loaded", "args": ["llama-server"] }, "context_length": 40960,
              "memory": { "status": "measured", "min": { "n_ctx": 4096, "devices": [
                  { "kind": "device", "name": "CUDA0", "model": 633495552u64, "context": 469762048u64, "compute": 323829760u64, "total": 12486574080u64, "free": 11523850240u64 },
                  { "kind": "host", "name": "Host", "model": 0, "context": 0, "compute": 16793600 } ] } } },
            { "id": "Qwen3-1.7B-Q8_0", "status": { "value": "unloaded" }, "context_length": 40960 },
            { "status": { "value": "loaded" } },
        ] });
        let seen = seen_in(&listing);
        assert_eq!(seen.len(), 2, "an entry without a name is not a model");
        assert_eq!(seen[0], Seen { id: "Qwen3-0.6B-Q8_0".into(), state: "loaded".into(), context_length: Some(40960), needs_mb: Some(1361), device_total_mb: Some(11908) });
        // Not measured yet: no numbers, and that is not an error.
        assert_eq!((seen[1].state.as_str(), seen[1].needs_mb, seen[1].device_total_mb), ("unloaded", None, None));
        assert!(seen_in(&json!({})).is_empty());
    }

    #[test]
    fn what_a_model_can_do_is_read_as_the_real_runtime_says_it() {
        // As the real runtime answered for Qwen3 1.7B on 2026-11-21, shortened.
        let qwen = json!({
            "chat_template": "{%- if enable_thinking is defined and enable_thinking is false %}<think>\n\n</think>{%- endif %}",
            "chat_template_caps": { "supports_tool_calls": true, "supports_reasoning_effort": false, "reasoning_effort_levels": [], "supports_system_role": true },
            "modalities": { "vision": false, "video": false, "audio": false },
        });
        assert_eq!(abilities_in(&qwen), Abilities { thinking: vec!["on".into()], takes: vec![], tools: true });
        // A model whose template knows levels of effort, and that reads pictures.
        let other = json!({ "chat_template": "…", "chat_template_caps": { "supports_tool_calls": false, "reasoning_effort_levels": ["none", "low", "medium", "high"] }, "modalities": { "vision": true, "audio": true } });
        assert_eq!(abilities_in(&other), Abilities { thinking: vec!["low".into(), "medium".into(), "high".into()], takes: vec!["images".into(), "sound".into()], tools: false });
        // One that says nothing about thinking does not think, as far as Demido can tell.
        assert_eq!(abilities_in(&json!({ "chat_template": "plain" })), Abilities::default());
    }
}
