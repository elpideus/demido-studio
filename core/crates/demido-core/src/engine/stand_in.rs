//! A stand-in for the runtime's router, running inside a test.
//!
//! It answers the addresses Demido uses the way the real router was seen to answer them (research notes 12
//! and 13), and can be told to misbehave: die, claim a model needs more memory than there is, write slowly,
//! or refuse. Starting and stopping a real process is not tried with it; `core/dev/router-trial.py` does
//! that on a real runtime.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router as Routes};
use serde_json::{Value, json};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use super::{Launch, Launcher, Running};
use crate::Result;

const MIB: u64 = 1024 * 1024;

pub(crate) struct Stand {
    pub(crate) models_dir: PathBuf,
    pub(crate) states: StdMutex<HashMap<String, String>>,
    pub(crate) news: tokio::sync::broadcast::Sender<Value>,
    pub(crate) launches: AtomicUsize,
    pub(crate) loads_asked: AtomicUsize,
    /// The stand-in that is running now. Each start is one of its own, as each real process is.
    current: StdMutex<Option<Arc<Instance>>>,
    /// What each model "needs" on the card, in MiB. The card has `total_mb`.
    pub(crate) needs_mb: StdMutex<HashMap<String, u64>>,
    pub(crate) total_mb: u64,
    pub(crate) args: StdMutex<Vec<String>>,
    /// What the stand-in "model" says, word by word.
    pub(crate) reply: StdMutex<String>,
    /// What it "thinks" first, when asked to think. Empty: it does not think.
    pub(crate) thought: StdMutex<String>,
    /// Milliseconds between two words. Raised to make an answer last.
    pub(crate) word_every_ms: AtomicUsize,
    /// Every request for an answer, as it arrived.
    pub(crate) asked: StdMutex<Vec<Value>>,
    /// When set, the next request for an answer is refused with this body.
    pub(crate) refuse_with: StdMutex<Option<Value>>,
}

/// One start of the stand-in. Stopping an old one must never touch a newer one.
struct Instance {
    alive: AtomicBool,
    end: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Instance {
    fn die(&self) {
        self.alive.store(false, Ordering::Relaxed);
        if let Some(end) = self.end.lock().unwrap().take() {
            let _ = end.send(());
        }
    }
}

impl Stand {
    pub(crate) fn new(models_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            models_dir,
            states: StdMutex::default(),
            news: tokio::sync::broadcast::channel(64).0,
            launches: AtomicUsize::new(0),
            loads_asked: AtomicUsize::new(0),
            current: StdMutex::default(),
            needs_mb: StdMutex::default(),
            total_mb: 12_000,
            args: StdMutex::default(),
            reply: StdMutex::new("Red, blue and yellow are the three primary colours of paint.".into()),
            thought: StdMutex::default(),
            word_every_ms: AtomicUsize::new(1),
            asked: StdMutex::default(),
            refuse_with: StdMutex::default(),
        })
    }

    fn set(&self, id: &str, state: &str, extra: Value) {
        self.states.lock().unwrap().insert(id.to_string(), state.to_string());
        let mut data = json!({ "status": state });
        if let (Some(into), Some(from)) = (data.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        let _ = self.news.send(json!({ "model": id, "event": "status_change", "data": data }));
    }

    /// The router that is running now dies, as when its process is killed.
    pub(crate) fn die(&self) {
        if let Some(now) = self.current.lock().unwrap().as_ref() {
            now.die();
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.current.lock().unwrap().as_ref().is_some_and(|now| now.alive.load(Ordering::Relaxed))
    }
}

fn keyed(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Bearer ") && v.len() > 20)
}

async fn models(State(stand): State<Arc<Stand>>, headers: HeaderMap) -> Response {
    if !keyed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let states = stand.states.lock().unwrap().clone();
    let needs = stand.needs_mb.lock().unwrap().clone();
    let mut data = Vec::new();
    for entry in std::fs::read_dir(&stand.models_dir).unwrap().filter_map(|e| e.ok()) {
        let id = entry.path().file_stem().unwrap().to_string_lossy().into_owned();
        let mut m = json!({ "id": id, "status": { "value": states.get(&id).cloned().unwrap_or("unloaded".into()) }, "context_length": 40960 });
        if let Some(n) = needs.get(&id) {
            m["memory"] = json!({ "status": "measured", "min": { "n_ctx": 8192, "devices": [{ "kind": "device", "name": "CUDA0", "model": n * MIB, "context": 0, "compute": 0, "total": stand.total_mb * MIB }] } });
        }
        data.push(m);
    }
    Json(json!({ "data": data })).into_response()
}

async fn load(State(stand): State<Arc<Stand>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if !keyed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    stand.loads_asked.fetch_add(1, Ordering::Relaxed);
    let id = body["model"].as_str().unwrap_or_default().to_string();
    stand.set(&id, "loading", json!({ "progress": { "current": "text_model", "value": 0.0 } }));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        stand.set(&id, "loaded", json!({}));
    });
    Json(json!({ "success": true })).into_response()
}

async fn unload(State(stand): State<Arc<Stand>>, Json(body): Json<Value>) -> Json<Value> {
    stand.set(body["model"].as_str().unwrap_or_default(), "unloaded", json!({ "exit_code": 0 }));
    Json(json!({ "success": true }))
}

/// Answers the way the real runtime was seen to (research note 13): a line per piece, then a line with
/// the runtime's own count of what it did, then `[DONE]`.
async fn chat(State(stand): State<Arc<Stand>>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if !keyed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    stand.asked.lock().unwrap().push(body.clone());
    if let Some(refusal) = stand.refuse_with.lock().unwrap().take() {
        return (StatusCode::BAD_REQUEST, Json(refusal)).into_response();
    }
    let model = body["model"].as_str().unwrap_or_default().to_string();
    if stand.states.lock().unwrap().get(&model).map(String::as_str) != Some("loaded") {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": { "code": 400, "message": "model is not loaded", "type": "invalid_request_error" } }))).into_response();
    }
    let thinks = body["chat_template_kwargs"]["enable_thinking"] == true;
    let (reply, thought) = (stand.reply.lock().unwrap().clone(), stand.thought.lock().unwrap().clone());
    let every = Duration::from_millis(stand.word_every_ms.load(Ordering::Relaxed) as u64);
    let mut lines: Vec<String> = vec![json!({ "choices": [{ "delta": { "role": "assistant", "content": null } }] }).to_string()];
    if thinks {
        lines.extend(thought.split_inclusive(' ').map(|w| json!({ "choices": [{ "delta": { "reasoning_content": w } }] }).to_string()));
    }
    let words: Vec<&str> = reply.split_inclusive(' ').collect();
    lines.extend(words.iter().map(|w| json!({ "choices": [{ "delta": { "content": w } }] }).to_string()));
    lines.push(json!({ "choices": [{ "finish_reason": "stop", "delta": {} }] }).to_string());
    let prompt_tokens = body["messages"].to_string().len() as u64 / 4;
    lines.push(
        json!({ "choices": [], "timings": { "cache_n": prompt_tokens / 2, "prompt_n": prompt_tokens - prompt_tokens / 2, "predicted_n": words.len(), "predicted_per_second": 120.0 } }).to_string(),
    );
    lines.push("[DONE]".into());
    let stream = tokio_stream::iter(lines).then(move |line| async move {
        tokio::time::sleep(every).await;
        Ok::<_, std::convert::Infallible>(SseEvent::default().data(line))
    });
    Sse::new(stream).into_response()
}

async fn sse(State(stand): State<Arc<Stand>>) -> Sse<impl tokio_stream::Stream<Item = std::result::Result<SseEvent, std::convert::Infallible>>> {
    Sse::new(BroadcastStream::new(stand.news.subscribe()).filter_map(|n| n.ok()).map(|n| Ok(SseEvent::default().data(n.to_string()))))
}

struct StandRunning {
    me: Arc<Instance>,
}
impl Running for StandRunning {
    fn alive(&mut self) -> bool {
        self.me.alive.load(Ordering::Relaxed)
    }
    fn stop(&mut self) {
        self.me.die();
    }
    fn pid(&self) -> Option<u32> {
        None
    }
}

pub(crate) struct StandLauncher(pub(crate) Arc<Stand>);
impl Launcher for StandLauncher {
    fn launch(&self, plan: &Launch) -> Result<Box<dyn Running>> {
        let stand = self.0.clone();
        stand.launches.fetch_add(1, Ordering::Relaxed);
        *stand.args.lock().unwrap() = plan.args.clone();
        stand.states.lock().unwrap().clear();
        let listener = std::net::TcpListener::bind(("127.0.0.1", plan.port))?;
        listener.set_nonblocking(true)?;
        let (end, ended) = tokio::sync::oneshot::channel::<()>();
        let me = Arc::new(Instance { alive: AtomicBool::new(true), end: StdMutex::new(Some(end)) });
        *stand.current.lock().unwrap() = Some(me.clone());
        let routes = Routes::new()
            .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
            .route("/models", get(models))
            .route("/models/load", post(load))
            .route("/models/unload", post(unload))
            .route("/models/sse", get(sse))
            .route("/v1/chat/completions", post(chat))
            .with_state(stand.clone());
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            // Ending drops every connection at once, as a killed process does.
            tokio::select! { _ = axum::serve(listener, routes) => {}, _ = ended => {} }
        });
        Ok(Box::new(StandRunning { me }))
    }
}
