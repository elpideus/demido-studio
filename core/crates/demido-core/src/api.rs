//! The API, and the server that offers it.
//!
//! One API for every client. Paths start with `/api/v1`. JSON in, JSON out. An error is always
//! `{ "error": { "code", "message" } }`, and the message is written for a person.
//!
//! Every request passes the guard first:
//! 1. The `Host` must be this server. A page on another site that points a made-up name at this computer
//!    is refused here (the DNS-rebinding trick).
//! 2. API calls need the key (`Authorization: Bearer …`) or a session cookie. `health` and the API
//!    description are open, so a client can tell that a server is there before it has the key.
//! 3. A change made with a cookie must come from this server's own pages.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use utoipa::{OpenApi, ToSchema};

use crate::auth::{Auth, COOKIE, LINK_LIFETIME};
use crate::chat::{Chat, ContextUse, Turn};
use crate::db::Db;
use crate::desktop::Appearance;
use crate::engine::{Engine, LocalModel, ModelList, ProcessLauncher};
use crate::events::{Event, EventLog};
use crate::kept::Kept;
use crate::paths::Paths;
use crate::runtime::{Available, Installed, Progress, Runtimes, Source, Status};
use crate::sessions::{Message, Session, Sessions, Summary};
use crate::settings::{Entry, Section, Settings};
use crate::system::{Advice, SystemInfo};
use crate::ui::Ui;
use crate::{Error, Result, VERSION, now_ms};

/// The parts of the core that work without a server: what the command line uses directly.
pub struct Core {
    pub settings: Arc<Settings>,
    pub events: Arc<EventLog>,
    pub auth: Arc<Auth>,
    pub runtimes: Arc<Runtimes>,
    pub engine: Arc<Engine>,
    pub sessions: Arc<Sessions>,
    pub chat: Arc<Chat>,
}

impl Core {
    pub fn open(paths: &Paths) -> Result<Self> {
        paths.ensure()?;
        let db = Arc::new(Db::open(&paths.db())?);
        let events = Arc::new(EventLog::new(db.clone()));
        let settings = Arc::new(Settings::load(db.clone(), events.clone())?);
        let auth = Arc::new(Auth::open(&paths.key())?);
        let runtimes = Arc::new(Runtimes::new(paths.home(), events.clone(), runtime_source())?);
        let engine = Arc::new(Engine::new(paths.home(), runtimes.clone(), settings.clone(), events.clone(), Arc::new(ProcessLauncher)));
        let sessions = Arc::new(Sessions::new(db));
        let chat = Arc::new(Chat::new(sessions.clone(), engine.clone(), events.clone(), settings.clone()));
        Ok(Self { settings, events, auth, runtimes, engine, sessions, chat })
    }
}

/// Where the runtime's files come from: its publisher.
///
/// A development build can be pointed at a stand-in on this computer (`DEMIDO_DEV_RUNTIME_SOURCE`), to try
/// the whole installation without the internet. A release build has no such switch: the address that decides
/// which program gets installed is not something to leave open.
fn runtime_source() -> Source {
    #[cfg(debug_assertions)]
    if let Ok(base) = std::env::var("DEMIDO_DEV_RUNTIME_SOURCE") {
        let base = base.trim_end_matches('/').to_string();
        tracing::warn!("Development build: runtime files come from a stand-in publisher at {base}, not from llama.cpp's releases.");
        return Source { api: base.clone(), repo: "o/r".into(), downloads: format!("{base}/dl/") };
    }
    Source::default()
}

#[derive(Clone)]
pub struct App {
    pub settings: Arc<Settings>,
    pub events: Arc<EventLog>,
    pub auth: Arc<Auth>,
    pub runtimes: Arc<Runtimes>,
    pub engine: Arc<Engine>,
    pub sessions: Arc<Sessions>,
    pub chat: Arc<Chat>,
    /// Which runtime build suits this computer. Worked out once: the hardware does not change while the server runs.
    pub advice: Arc<tokio::sync::OnceCell<Advice>>,
    /// This computer as last read. Memory in use moves, so it is kept only for seconds.
    pub system: Arc<Kept<SystemInfo>>,
    /// The desktop's look as last read.
    pub appearance: Arc<Kept<Appearance>>,
    pub ui: Ui,
    pub started: Instant,
    /// The names this server answers to, for example `127.0.0.1:7341`. Empty means any (not loopback).
    pub hosts: Arc<Vec<String>>,
}

// ───────── errors ─────────

#[derive(Serialize, ToSchema)]
pub struct Problem {
    pub error: ProblemDetail,
}
#[derive(Serialize, ToSchema)]
pub struct ProblemDetail {
    /// A short, stable word for programs: `no_key`, `unknown_setting`, `invalid`.
    pub code: String,
    /// What went wrong and what to do, for a person.
    pub message: String,
}

fn problem(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(Problem { error: ProblemDetail { code: code.into(), message: message.into() } })).into_response()
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Error::UnknownSetting(_) => (StatusCode::NOT_FOUND, "unknown_setting"),
            Error::Invalid(_) => (StatusCode::UNPROCESSABLE_ENTITY, "invalid"),
            Error::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Error::Busy(_) => (StatusCode::CONFLICT, "busy"),
            Error::Stopped(_) => (StatusCode::CONFLICT, "stopped"),
            Error::Net(_) => (StatusCode::BAD_GATEWAY, "unreachable"),
            Error::Json(_) => (StatusCode::BAD_REQUEST, "bad_json"),
            Error::Db(_) | Error::Io(_) | Error::Other(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        if status.is_server_error() {
            tracing::error!("{self}");
        }
        problem(status, code, self.to_string())
    }
}

// ───────── the guard ─────────

async fn guard(State(app): State<App>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let text = |name: header::HeaderName| headers.get(name).and_then(|v| v.to_str().ok());
    let host = text(header::HOST).unwrap_or("");
    if !app.hosts.is_empty() && !app.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return problem(StatusCode::FORBIDDEN, "wrong_host", format!("This server does not answer to \"{host}\". Use {}.", app.hosts[0]));
    }
    let path = req.uri().path();
    let needs_key = path.starts_with("/api/") && path != "/api/v1/health" && path != "/api/v1/openapi.json";
    if needs_key {
        let by_key = app.auth.bearer_ok(text(header::AUTHORIZATION));
        let by_cookie = !by_key && app.auth.cookie_ok(text(header::COOKIE));
        if !by_key && !by_cookie {
            return problem(StatusCode::UNAUTHORIZED, "no_key", "This needs the key. Programs on this computer read it from Demido's data folder; a browser gets in with `demido open`.");
        }
        // A browser sends cookies along with requests that other sites trigger. A change must come from our own pages.
        let changes = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
        if by_cookie && changes {
            let own = text(header::ORIGIN).and_then(|o| o.split("://").nth(1)).is_some_and(|o| o.eq_ignore_ascii_case(host));
            if !own {
                return problem(StatusCode::FORBIDDEN, "wrong_origin", "This change did not come from Demido's own pages, so it was refused.");
            }
        }
    }
    let api = path.starts_with("/api/");
    let mut res = next.run(req).await;
    // What the API says is true now and may not be a second later. A browser must not keep it.
    // (Keeping answers is done on purpose, in the core and in the UI's own store, never by accident in between.)
    if api && !res.headers().contains_key(header::CACHE_CONTROL) {
        res.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    }
    res
}

// ───────── what the API offers ─────────

#[derive(Serialize, ToSchema)]
pub struct Health {
    /// Always `ok` when the server answers.
    pub status: String,
    pub version: String,
    pub uptime_s: u64,
}

/// Is a Demido server here? Needs no key.
#[utoipa::path(get, path = "/api/v1/health", tag = "server", responses((status = 200, body = Health)))]
async fn health(State(app): State<App>) -> Json<Health> {
    Json(Health { status: "ok".into(), version: VERSION.into(), uptime_s: app.started.elapsed().as_secs() })
}

#[derive(Deserialize)]
struct FreshQuery {
    fresh: Option<bool>,
}

/// What this computer has, and which model runtime suits it. A reading is kept for five seconds;
/// `fresh=true` reads again now.
#[utoipa::path(get, path = "/api/v1/system", tag = "system", params(("fresh" = Option<bool>, Query, description = "Read again now, even if a reading is seconds old")), responses((status = 200, body = SystemInfo), (status = 401, body = Problem)))]
async fn system(State(app): State<App>, Query(q): Query<FreshQuery>) -> Json<SystemInfo> {
    Json(app.system_info(q.fresh.unwrap_or(false)).await)
}

/// What the desktop of this computer looks like: its accent colour, when it has one and says so.
/// Only means something to a window on the same computer as the core.
#[utoipa::path(get, path = "/api/v1/system/appearance", tag = "system", responses((status = 200, body = Appearance), (status = 401, body = Problem)))]
async fn appearance(State(app): State<App>) -> Json<Appearance> {
    // A window asks when it gets the focus back, often twice in the same instant. One asking serves both.
    Json(app.appearance.get(false, || async { tokio::task::spawn_blocking(crate::desktop::read).await.expect("asking the desktop does not panic") }).await)
}

// ───────── the model runtime ─────────

impl App {
    /// This computer. Reading it asks the graphics driver, which takes a moment, so a reading is kept for a
    /// few seconds and shared by everyone who asks meanwhile. `fresh` reads again now.
    async fn system_info(&self, fresh: bool) -> SystemInfo {
        self.system.get(fresh, || async { tokio::task::spawn_blocking(crate::system::detect).await.expect("reading the hardware does not panic") }).await
    }

    async fn advice(&self) -> &Advice {
        self.advice.get_or_init(|| async { self.system_info(false).await.runtime }).await
    }
}

/// The model runtimes on this computer, and the installation under way if there is one.
#[utoipa::path(get, path = "/api/v1/runtime", tag = "runtime", responses((status = 200, body = Status), (status = 401, body = Problem)))]
async fn runtime_status(State(app): State<App>) -> Json<Status> {
    let runtimes = app.runtimes.clone();
    Json(tokio::task::spawn_blocking(move || runtimes.status()).await.expect("listing a folder does not panic"))
}

#[derive(Deserialize)]
struct AvailableQuery {
    fresh: Option<bool>,
}

/// What the runtime's publisher offers for this computer, the recommended build first. Asks the publisher
/// (GitHub) at most once an hour; `fresh=true` asks again now.
#[utoipa::path(get, path = "/api/v1/runtime/available", tag = "runtime", params(("fresh" = Option<bool>, Query, description = "Ask the publisher now, even if it was asked within the hour")), responses((status = 200, body = Available), (status = 502, body = Problem)))]
async fn runtime_available(State(app): State<App>, Query(q): Query<AvailableQuery>) -> Result<Json<Available>> {
    Ok(Json(app.runtimes.available(app.advice().await, q.fresh.unwrap_or(false)).await?))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct InstallRequest {
    /// Which offer, by its id: `cuda-13.5`, `vulkan`, `cpu`. Left out: the recommended one.
    pub backend: Option<String>,
    /// Also download the offer's extra files (NVIDIA's libraries for a CUDA build). Left out: only if this system lacks them.
    pub extra: Option<bool>,
}

/// Starts downloading and installing one build of the runtime. Answers at once. Progress comes on the event
/// stream as `runtime.install.progress` several times a second, and the end as `runtime.installed`,
/// `runtime.install.failed` or `runtime.install.stopped`. One installation at a time.
#[utoipa::path(post, path = "/api/v1/runtime/install", tag = "runtime", request_body = InstallRequest, responses((status = 202, body = Progress), (status = 404, body = Problem), (status = 409, body = Problem), (status = 502, body = Problem)))]
async fn runtime_install(State(app): State<App>, body: Option<Json<InstallRequest>>) -> Result<Response> {
    let ask = body.map(|Json(b)| b).unwrap_or_default();
    // Refusals the caller can act on are given now, not left in the log: nothing offered, or one already running.
    if let Some(busy) = app.runtimes.status().installing {
        return Err(Error::Busy(format!("{} is being installed. Wait for it, or stop it first.", busy.name)));
    }
    let advice = app.advice().await.clone();
    let available = app.runtimes.available(&advice, false).await?;
    let offer = match &ask.backend {
        Some(id) => available.offers.iter().find(|o| &o.id == id),
        None => available.offers.iter().find(|o| o.recommended).or(available.offers.first()),
    };
    let Some(offer) = offer.cloned() else {
        let offered = available.offers.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().join(", ");
        return Err(Error::NotFound(format!("There is no \"{}\" build of the runtime for this computer. Offered: {offered}.", ask.backend.as_deref().unwrap_or("recommended"))));
    };
    let extra = ask.extra.unwrap_or(offer.needs_extra);
    let total = offer.size + if extra { offer.extra_size } else { 0 };
    let started = Progress { id: format!("{}-{}-{}", available.runtime, available.build, offer.id), name: offer.name.clone(), step: "looking".into(), file: None, done: 0, total };
    let runtimes = app.runtimes.clone();
    tokio::spawn(async move {
        // The outcome is written to the log by `install` itself.
        let _ = runtimes.install(&advice, &offer.id, extra).await;
    });
    Ok((StatusCode::ACCEPTED, Json(started)).into_response())
}

#[derive(Serialize, ToSchema)]
pub struct Stopped {
    /// False when nothing was being installed.
    pub stopped: bool,
}

/// Stops the installation under way. What was downloaded is kept, so the next try goes on from there.
#[utoipa::path(delete, path = "/api/v1/runtime/install", tag = "runtime", responses((status = 200, body = Stopped)))]
async fn runtime_stop(State(app): State<App>) -> Json<Stopped> {
    Json(Stopped { stopped: app.runtimes.stop() })
}

/// Tries an installed runtime again: does it start, and which graphics cards does it see? Nothing is
/// downloaded. For after a driver update.
#[utoipa::path(post, path = "/api/v1/runtime/installed/{id}/check", tag = "runtime", params(("id" = String, Path, description = "For example llama.cpp-b12359-cuda-13.5")), responses((status = 200, body = Installed), (status = 404, body = Problem)))]
async fn runtime_check(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Installed>> {
    let runtimes = app.runtimes.clone();
    Ok(Json(tokio::task::spawn_blocking(move || runtimes.check(&id)).await.expect("trying a program does not panic")?))
}

/// Deletes an installed runtime from this computer.
#[utoipa::path(delete, path = "/api/v1/runtime/installed/{id}", tag = "runtime", params(("id" = String, Path, description = "For example llama.cpp-b12359-cuda-13.5")), responses((status = 200, body = Installed), (status = 404, body = Problem)))]
async fn runtime_remove(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Installed>> {
    let runtimes = app.runtimes.clone();
    Ok(Json(tokio::task::spawn_blocking(move || runtimes.remove(&id)).await.expect("deleting a folder does not panic")?))
}

// ───────── models ─────────

/// The models on this computer and what each is doing: unloaded, loading or loaded, what it needs, and
/// when it will be unloaded for being idle. Looking starts nothing.
#[utoipa::path(get, path = "/api/v1/models", tag = "models", responses((status = 200, body = ModelList), (status = 401, body = Problem)))]
async fn models_list(State(app): State<App>) -> Result<Json<ModelList>> {
    Ok(Json(app.engine.list().await?))
}

/// Loads a model and answers when it is ready, which takes from a second to minutes. Starts the model
/// runtime if it is not running. Progress comes on the event stream as `model.loading`. A model that
/// cannot fit on the graphics card is refused, with the numbers.
#[utoipa::path(post, path = "/api/v1/models/{id}/load", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), responses((status = 200, body = LocalModel), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn model_load(State(app): State<App>, Path(id): Path<String>) -> Result<Json<LocalModel>> {
    Ok(Json(app.engine.load(&id).await?))
}

#[derive(Serialize, ToSchema)]
pub struct Unloaded {
    pub unloaded: bool,
}

/// Unloads a model: its process ends and its memory is free again.
#[utoipa::path(post, path = "/api/v1/models/{id}/unload", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), responses((status = 200, body = Unloaded), (status = 422, body = Problem)))]
async fn model_unload(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Unloaded>> {
    app.engine.unload(&id, "asked").await?;
    Ok(Json(Unloaded { unloaded: true }))
}

// ───────── sessions ─────────

/// Every session, the one spoken in most recently first.
#[utoipa::path(get, path = "/api/v1/sessions", tag = "sessions", responses((status = 200, body = Vec<Session>), (status = 401, body = Problem)))]
async fn sessions_list(State(app): State<App>) -> Result<Json<Vec<Session>>> {
    Ok(Json(app.sessions.list()?))
}

#[derive(Deserialize, ToSchema, Default)]
pub struct NewSession {
    /// Left out: the session takes its title from the first thing said in it.
    pub title: Option<String>,
    /// The model to talk to, by its name. Left out: one that is loaded, or else the first on this computer.
    pub model: Option<String>,
}

/// Starts a session with a model. Nothing is loaded until something is said.
#[utoipa::path(post, path = "/api/v1/sessions", tag = "sessions", request_body = NewSession, responses((status = 200, body = Session), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_new(State(app): State<App>, body: Option<Json<NewSession>>) -> Result<Json<Session>> {
    let ask = body.map(|Json(b)| b).unwrap_or_default();
    Ok(Json(app.chat.start(ask.title.as_deref(), ask.model.as_deref()).await?))
}

#[derive(Serialize, ToSchema)]
pub struct SessionDetail {
    pub session: Session,
    /// What was said, in order.
    pub messages: Vec<Message>,
    /// What the model is told before the conversation, word for word. Nothing a model is told is hidden.
    pub instructions: String,
    /// True while an answer is being written.
    pub answering: bool,
    /// What thinking can be set to for this session's model, besides `off`. Empty: the model does not think.
    /// `null`: not known yet, because the model has never been loaded.
    pub thinking_choices: Option<Vec<String>>,
    /// Each time the session was made shorter, oldest first. The model is given the last account in place
    /// of the messages up to its `upto_seq`; the messages themselves are all still in `messages`.
    pub summaries: Vec<Summary>,
    /// How much of what the model can hold the session is using.
    pub context: ContextUse,
}

/// One session with everything said in it, and what the model is told first.
#[utoipa::path(get, path = "/api/v1/sessions/{id}", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = SessionDetail), (status = 404, body = Problem)))]
async fn session_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<SessionDetail>> {
    let session = app.sessions.get(&id)?;
    let thinking_choices = app.engine.list().await?.models.into_iter().find(|m| m.id == session.model).and_then(|m| m.thinking);
    Ok(Json(SessionDetail {
        messages: app.sessions.messages(&id)?,
        instructions: crate::chat::INSTRUCTIONS.to_string(),
        answering: app.chat.answering(&id),
        thinking_choices,
        summaries: app.sessions.summaries(&id)?,
        context: app.chat.context_use(&id)?,
        session,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct SessionChange {
    pub title: Option<String>,
    /// Another model on this computer, by its name. The next answer comes from it.
    pub model: Option<String>,
    /// Whether the model thinks before it answers: `off`, `on`, or one of the model's own levels.
    pub thinking: Option<String>,
}

/// Changes what is given: the title, the model, whether the model thinks before it answers.
#[utoipa::path(patch, path = "/api/v1/sessions/{id}", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = SessionChange, responses((status = 200, body = Session), (status = 404, body = Problem)))]
async fn session_change(State(app): State<App>, Path(id): Path<String>, Json(body): Json<SessionChange>) -> Result<Json<Session>> {
    Ok(Json(app.chat.change(&id, body.title.as_deref(), body.model.as_deref(), body.thinking.as_deref()).await?))
}

/// Makes a session shorter for the model: it writes a short account of the older part of the conversation,
/// and is given that account from then on in place of those messages. The messages stay in the session.
/// Answers when the account is written, which takes a few seconds. Also happens by itself when a session
/// nears what the model can hold (the settings under "Context and compaction").
#[utoipa::path(post, path = "/api/v1/sessions/{id}/compact", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Summary), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_compact(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Summary>> {
    Ok(Json(app.chat.compact(&id, "asked").await?))
}

/// Ends the thought the session's model is in, so that it goes on to its answer now.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/answer-now", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Stopped), (status = 404, body = Problem)))]
async fn session_answer_now(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Stopped>> {
    Ok(Json(Stopped { stopped: app.chat.answer_now(&id).await? }))
}

/// Deletes a session and everything said in it. An answer being written in it is stopped.
#[utoipa::path(delete, path = "/api/v1/sessions/{id}", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Session), (status = 404, body = Problem)))]
async fn session_delete(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Session>> {
    Ok(Json(app.chat.delete(&id)?))
}

#[derive(Deserialize, ToSchema)]
pub struct Said {
    /// What the person wrote.
    pub content: String,
}

/// Says something in a session. Answers at once with what was said and the answer that is now being
/// written. The answer's words follow on the event stream as `message.delta`, several times a second,
/// and it is stored whole at `message.done` (or `message.stopped`, or `message.failed` with the reason).
/// The session's model is loaded first if it is not. One answer at a time in a session.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/messages", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = Said, responses((status = 202, body = Turn), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_say(State(app): State<App>, Path(id): Path<String>, Json(body): Json<Said>) -> Result<Response> {
    Ok((StatusCode::ACCEPTED, Json(app.chat.say(&id, &body.content)?)).into_response())
}

/// Stops the answer being written in a session. What was written so far is kept.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/stop", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Stopped)))]
async fn session_stop(State(app): State<App>, Path(id): Path<String>) -> Json<Stopped> {
    Json(Stopped { stopped: app.chat.stop(&id) })
}

#[derive(Serialize, ToSchema)]
pub struct SettingsList {
    pub sections: Vec<Section>,
    pub settings: Vec<Entry>,
}

/// Every setting: what it means, its default, its value now.
#[utoipa::path(get, path = "/api/v1/settings", tag = "settings", responses((status = 200, body = SettingsList), (status = 401, body = Problem)))]
async fn settings_list(State(app): State<App>) -> Json<SettingsList> {
    Json(SettingsList { sections: app.settings.sections().to_vec(), settings: app.settings.list() })
}

/// One setting.
#[utoipa::path(get, path = "/api/v1/settings/{id}", tag = "settings", params(("id" = String, Path, description = "For example events.keepDays")), responses((status = 200, body = Entry), (status = 404, body = Problem)))]
async fn setting_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Entry>> {
    Ok(Json(app.settings.get(&id)?))
}

#[derive(Deserialize, ToSchema)]
pub struct NewValue {
    #[schema(value_type = Object)]
    pub value: Value,
}

/// Changes a setting. The value must fit the setting's kind; the error says what it takes.
#[utoipa::path(put, path = "/api/v1/settings/{id}", tag = "settings", params(("id" = String, Path, description = "For example events.keepDays")), request_body = NewValue, responses((status = 200, body = Entry), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn setting_set(State(app): State<App>, Path(id): Path<String>, Json(body): Json<NewValue>) -> Result<Json<Entry>> {
    Ok(Json(app.settings.set(&id, body.value)?))
}

/// Puts a setting back to its default.
#[utoipa::path(delete, path = "/api/v1/settings/{id}", tag = "settings", params(("id" = String, Path, description = "For example events.keepDays")), responses((status = 200, body = Entry), (status = 404, body = Problem)))]
async fn setting_reset(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Entry>> {
    Ok(Json(app.settings.reset(&id)?))
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<u32>,
}

/// The log of what happened. With `after`, the events that came later; without it, the newest ones.
#[utoipa::path(get, path = "/api/v1/events", tag = "events", params(("after" = Option<i64>, Query, description = "Only events with a number greater than this"), ("limit" = Option<u32>, Query, description = "At most this many. Default 100, at most 1000")), responses((status = 200, body = Vec<Event>)))]
async fn events_list(State(app): State<App>, Query(q): Query<EventsQuery>) -> Result<Json<Vec<Event>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(match q.after {
        Some(seq) => app.events.after(seq, limit)?,
        None => app.events.latest(limit)?,
    }))
}

/// The log, live, as Server-Sent Events. A client that reconnects sends `Last-Event-ID` and misses nothing.
#[utoipa::path(get, path = "/api/v1/events/stream", tag = "events", responses((status = 200, description = "A stream of events. Each one's id is its number in the log.", content_type = "text/event-stream")))]
async fn events_stream(State(app): State<App>, headers: HeaderMap) -> Sse<impl Stream<Item = std::result::Result<SseEvent, Infallible>>> {
    let seen = headers.get("last-event-id").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<i64>().ok());
    // Listen first, then read what was missed: an event appended in between arrives twice and is dropped below.
    let live = app.events.listen();
    let missed = seen.map(|seq| app.events.after(seq, 1000).unwrap_or_default()).unwrap_or_default();
    let upto = missed.last().map(|e| e.seq).or(seen).unwrap_or(0);
    let stream = tokio_stream::iter(missed)
        // Number 0 is news that is told and not kept (progress). It has no place in the log, so it carries no id.
        // The stream ends when the server says it is stopping. A stream left open would keep the server from stopping at all.
        .chain(BroadcastStream::new(live).filter_map(|e| e.ok()).take_while(|e| e.kind != STOPPING).filter(move |e| e.seq == 0 || e.seq > upto))
        .map(|e| {
            let line = SseEvent::default().event(e.kind.clone()).data(serde_json::to_string(&e).unwrap_or_default());
            Ok(if e.seq > 0 { line.id(e.seq.to_string()) } else { line })
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize, ToSchema)]
pub struct Link {
    /// Open this in a browser on this computer. It works once.
    pub path: String,
    pub expires_in_s: u64,
}

/// Makes a one-use link that lets a browser in without the key ever appearing in an address.
#[utoipa::path(post, path = "/api/v1/session/link", tag = "server", responses((status = 200, body = Link), (status = 401, body = Problem)))]
async fn link_new(State(app): State<App>) -> Json<Link> {
    Json(Link { path: format!("/enter?code={}", app.auth.new_link()), expires_in_s: LINK_LIFETIME.as_secs() })
}

#[derive(Deserialize)]
struct EnterQuery {
    code: String,
}

/// Where a one-use link lands: it is traded for a session cookie, then the UI opens.
async fn enter(State(app): State<App>, Query(q): Query<EnterQuery>) -> Response {
    match app.auth.redeem(&q.code) {
        Some(session) => ([(header::SET_COOKIE, format!("{COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/"))], Redirect::to("/")).into_response(),
        None => (StatusCode::FORBIDDEN, "This link was already used, or is older than a minute. Run `demido open` again.\n").into_response(),
    }
}

async fn ui_file(State(app): State<App>, req: Request) -> Response {
    if req.uri().path().starts_with("/api/") {
        return problem(StatusCode::NOT_FOUND, "not_found", format!("There is nothing at {}. The API is described at /api/v1/openapi.json.", req.uri().path()));
    }
    let has = req.headers().get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    app.ui.serve(req.uri().path(), has).await
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Demido Studio", description = "One API for every client: the window, a browser, the command line, your own programs. Send the key as `Authorization: Bearer <key>`. An error is always `{ error: { code, message } }`."),
    paths(health, system, appearance, runtime_status, runtime_available, runtime_install, runtime_stop, runtime_check, runtime_remove, models_list, model_load, model_unload, sessions_list, session_new, session_get, session_change, session_delete, session_say, session_stop, session_answer_now, session_compact, settings_list, setting_get, setting_set, setting_reset, events_list, events_stream, link_new),
    tags((name = "server", description = "Is it there, and letting a browser in"), (name = "system", description = "This computer"), (name = "runtime", description = "The program that runs models: what is installed, what is offered, installing it"), (name = "models", description = "The models on this computer: loading and unloading them"), (name = "sessions", description = "Conversations with a model: what was said, saying something, stopping an answer"), (name = "settings", description = "Every setting"), (name = "events", description = "The log of what happened"))
)]
struct Doc;

/// The API's own description, generated from the code. Needs no key.
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    let mut doc = Doc::openapi();
    doc.info.version = VERSION.into();
    Json(doc)
}

pub fn router(app: App) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/system", get(system))
        .route("/system/appearance", get(appearance))
        .route("/runtime", get(runtime_status))
        .route("/runtime/available", get(runtime_available))
        .route("/runtime/install", post(runtime_install).delete(runtime_stop))
        .route("/runtime/installed/{id}", axum::routing::delete(runtime_remove))
        .route("/runtime/installed/{id}/check", post(runtime_check))
        .route("/models", get(models_list))
        .route("/models/{id}/load", post(model_load))
        .route("/models/{id}/unload", post(model_unload))
        .route("/sessions", get(sessions_list).post(session_new))
        .route("/sessions/{id}", get(session_get).patch(session_change).delete(session_delete))
        .route("/sessions/{id}/messages", post(session_say))
        .route("/sessions/{id}/stop", post(session_stop))
        .route("/sessions/{id}/answer-now", post(session_answer_now))
        .route("/sessions/{id}/compact", post(session_compact))
        .route("/settings", get(settings_list))
        .route("/settings/{id}", get(setting_get).put(setting_set).delete(setting_reset))
        .route("/events", get(events_list))
        .route("/events/stream", get(events_stream))
        .route("/session/link", post(link_new));
    Router::new().nest("/api/v1", api).route("/enter", get(enter)).fallback(ui_file).layer(middleware::from_fn_with_state(app.clone(), guard)).with_state(app)
}

// ───────── the server ─────────

pub struct Options {
    pub paths: Paths,
    /// Overrides the `server.address` setting for this run.
    pub listen: Option<String>,
    pub ui_dir: Option<PathBuf>,
}

/// The names a server on `address` answers to. Loopback gets its three spellings; anything else gets none,
/// which means "any" (use from other devices is not built yet, and will bring its own check).
fn hosts_for(address: &std::net::SocketAddr) -> Vec<String> {
    if !address.ip().is_loopback() {
        return Vec::new();
    }
    let port = address.port();
    vec![format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
}

/// Runs the server until it is told to stop (Ctrl+C, or the system's stop signal).
pub async fn serve(opts: Options) -> Result<()> {
    let core = Core::open(&opts.paths)?;
    let keep: u32 = core.settings.value("events.keepDays")?;
    let forgotten = core.events.forget_older_than(keep)?;
    let address = match opts.listen {
        Some(a) => a,
        None => core.settings.value::<String>("server.address")?,
    };
    let listener = tokio::net::TcpListener::bind(&address).await.map_err(|e| Error::Other(format!("Could not listen on {address}: {e}. If another Demido is running, `demido status` shows it.")))?;
    let local = listener.local_addr()?;
    let ui = Ui::find(opts.ui_dir);
    let app = App {
        settings: core.settings,
        events: core.events.clone(),
        auth: core.auth,
        runtimes: core.runtimes,
        engine: core.engine.clone(),
        sessions: core.sessions.clone(),
        chat: core.chat.clone(),
        advice: Arc::default(),
        system: Arc::new(Kept::new(SYSTEM_KEEPS)),
        appearance: Arc::new(Kept::new(APPEARANCE_KEEPS)),
        ui: ui.clone(),
        started: Instant::now(),
        hosts: Arc::new(hosts_for(&local)),
    };

    std::fs::write(opts.paths.running(), json!({ "address": local.to_string(), "pid": std::process::id(), "started_at": now_ms(), "version": VERSION }).to_string())?;
    core.events.append("server.started", None, json!({ "address": local.to_string(), "version": VERSION }))?;
    tracing::info!("Demido {VERSION} is listening on http://{local}");
    tracing::info!("Files: {}", opts.paths.home().display());
    match &ui.dir {
        Some(dir) => tracing::info!("Web UI: {}", dir.display()),
        None => tracing::warn!("Web UI: not found. The API works; pages do not. Build it with `pnpm app:build`."),
    }
    if forgotten > 0 {
        tracing::info!("Forgot {forgotten} log entries older than {keep} days.");
    }
    if !local.ip().is_loopback() {
        tracing::warn!("Listening beyond this computer. Use from other devices is not built yet: there is no encryption and no passkey. Do not expose this.");
    }

    let (stopping, mut stopped) = tokio::sync::watch::channel(false);
    // A runtime left running by an earlier Demido that was killed would hold graphics memory for nothing.
    core.engine.end_leftover();
    // An answer that was being written when Demido last stopped was never finished. Say so in its place.
    core.sessions.close_unfinished()?;
    // Every few seconds: unload what nobody uses, stop a runtime with nothing loaded, notice one that died.
    let caretaker = {
        let engine = core.engine.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(LOOK_AFTER_EVERY);
            loop {
                every.tick().await;
                engine.look_after().await;
            }
        })
    };
    let (events, runtimes, engine) = (core.events.clone(), app.runtimes.clone(), core.engine.clone());
    let asked_to_stop = async move {
        stop_signal().await;
        tracing::info!("Stopping.");
        caretaker.abort();
        runtimes.stop(); // a download under way keeps what it has, and says so in the log
        engine.shutdown().await; // loaded models are unloaded and the model runtime ends
        events.tell(STOPPING, None, json!({})); // ends every open event stream
        let _ = stopping.send(true);
    };
    let served = tokio::select! {
        served = axum::serve(listener, router(app)).with_graceful_shutdown(asked_to_stop) => served,
        // Requests under way get a moment to finish. A connection that will not close does not get to keep the server alive.
        _ = async { let _ = stopped.changed().await; tokio::time::sleep(LAST_MOMENT).await } => Ok(()),
    };
    std::fs::remove_file(opts.paths.running()).ok();
    core.events.append("server.stopped", None, json!({}))?;
    tracing::info!("Stopped.");
    Ok(served?)
}

/// How long a reading of the hardware is used. Long enough for a screen that asks three times while it opens;
/// short enough that "memory in use" is not old news.
const SYSTEM_KEEPS: std::time::Duration = std::time::Duration::from_secs(5);
/// How long a reading of the desktop's look is used.
const APPEARANCE_KEEPS: std::time::Duration = std::time::Duration::from_secs(2);

/// How often loaded models and the model runtime are looked after.
const LOOK_AFTER_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Told (not kept) when the server is asked to stop.
const STOPPING: &str = "server.stopping";
/// How long requests under way get to finish once the server is asked to stop.
const LAST_MOMENT: std::time::Duration = std::time::Duration::from_secs(5);

async fn stop_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok() };
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("the stop signal can be watched");
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    fn app() -> (Router, Arc<Auth>) {
        let (router, auth, _) = app_and_log();
        (router, auth)
    }

    fn app_and_log() -> (Router, Arc<Auth>, Arc<EventLog>) {
        // A folder that does not exist: these tests install nothing.
        app_at(&std::env::temp_dir().join("demido-api-tests-no-runtimes"))
    }

    /// The API over a data folder of the caller's choosing.
    fn app_at(home: &std::path::Path) -> (Router, Arc<Auth>, Arc<EventLog>) {
        let db = Arc::new(Db::memory().unwrap());
        let events = Arc::new(EventLog::new(db.clone()));
        let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
        let sessions = Arc::new(Sessions::new(db));
        let auth = Arc::new(Auth::with_key(KEY.into()));
        let runtimes = Arc::new(Runtimes::new(home, events.clone(), Source::default()).unwrap());
        let engine = Arc::new(Engine::new(home, runtimes.clone(), settings.clone(), events.clone(), Arc::new(ProcessLauncher)));
        let app = App {
            settings: settings.clone(),
            events: events.clone(),
            auth: auth.clone(),
            runtimes,
            engine: engine.clone(),
            sessions: sessions.clone(),
            chat: Arc::new(Chat::new(sessions, engine, events.clone(), settings.clone())),
            advice: Arc::default(),
            system: Arc::new(Kept::new(SYSTEM_KEEPS)),
            appearance: Arc::new(Kept::new(APPEARANCE_KEEPS)),
            ui: Ui::default(),
            started: Instant::now(),
            hosts: Arc::new(vec!["127.0.0.1:7341".into()]),
        };
        (router(app), auth, events)
    }

    fn request(method: &str, path: &str) -> axum::http::request::Builder {
        axum::http::Request::builder().method(method).uri(path).header("host", "127.0.0.1:7341")
    }

    async fn send(router: &Router, req: axum::http::Request<Body>) -> (StatusCode, Value) {
        let res = router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// A window keeps the event stream open for as long as it lives. The server must still be able to stop.
    #[tokio::test]
    async fn the_event_stream_carries_news_and_ends_when_the_server_stops() {
        let (r, _, events) = app_and_log();
        let res = r.clone().oneshot(request("GET", "/api/v1/events/stream").header("authorization", format!("Bearer {KEY}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        events.append("test.kept", Some("a"), json!({})).unwrap();
        events.tell("test.told", Some("b"), json!({ "done": 1 }));
        events.tell(STOPPING, None, json!({}));
        events.append("test.after", None, json!({})).unwrap();
        // The body ends by itself: reading it to the end would hang forever if the stream stayed open.
        let body = tokio::time::timeout(std::time::Duration::from_secs(5), axum::body::to_bytes(res.into_body(), 1 << 20)).await.expect("the stream ended").unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("event: test.kept") && text.contains("id: 1"), "{text}");
        // News that is told and not kept has no number to come back to.
        let told = text.split("\n\n").find(|block| block.contains("event: test.told")).expect("the told event arrived");
        assert!(!told.contains("id:"), "{told}");
        assert!(!text.contains("test.after"), "nothing is sent after the stop");
    }

    #[tokio::test]
    async fn a_session_is_made_spoken_in_read_renamed_and_deleted() {
        // A model file, and no runtime to run it: everything works up to the answer, which says what is missing.
        let home = std::env::temp_dir().join(format!("demido-api-sessions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(home.join("models")).unwrap();
        std::fs::write(home.join("models/tiny.gguf"), b"not really a model").unwrap();
        let (r, _, events) = app_at(&home);
        let key = |b: axum::http::request::Builder| b.header("authorization", format!("Bearer {KEY}")).header("content-type", "application/json");

        let (status, made) = send(&r, key(request("POST", "/api/v1/sessions")).body(Body::from("{}")).unwrap()).await;
        assert_eq!((status, made["model"].as_str(), made["title"].as_str()), (StatusCode::OK, Some("tiny"), Some("New session")));
        let id = made["id"].as_str().unwrap().to_string();
        let (status, _) = send(&r, key(request("POST", "/api/v1/sessions")).body(Body::from(r#"{"model":"no-such"}"#)).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, turn) = send(&r, key(request("POST", &format!("/api/v1/sessions/{id}/messages"))).body(Body::from(r#"{"content":"Hello there"}"#)).unwrap()).await;
        assert_eq!((status, turn["said"]["content"].as_str(), turn["answer"]["state"].as_str()), (StatusCode::ACCEPTED, Some("Hello there"), Some("answering")));
        let (status, _) = send(&r, key(request("POST", &format!("/api/v1/sessions/{id}/messages"))).body(Body::from(r#"{"content":"  "}"#)).unwrap()).await;
        assert!(status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::CONFLICT);

        // The answer ends by itself: there is no runtime, and it says so in the message.
        let mut detail = Value::Null;
        for _ in 0..300 {
            detail = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}"))).body(Body::empty()).unwrap()).await.1;
            if detail["answering"] == false {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(detail["session"]["title"], "Hello there");
        assert_eq!(detail["instructions"], crate::chat::INSTRUCTIONS);
        let answer = &detail["messages"][1];
        assert_eq!(answer["state"], "failed");
        assert!(answer["problem"].as_str().unwrap().contains("No model runtime is installed"), "{answer}");
        assert!(events.after(0, 50).unwrap().iter().any(|e| e.kind == "message.failed"));

        let (status, renamed) = send(&r, key(request("PATCH", &format!("/api/v1/sessions/{id}"))).body(Body::from(r#"{"title":"Greetings"}"#)).unwrap()).await;
        assert_eq!((status, renamed["title"].as_str()), (StatusCode::OK, Some("Greetings")));
        let (_, list) = send(&r, key(request("GET", "/api/v1/sessions")).body(Body::empty()).unwrap()).await;
        assert_eq!((list.as_array().unwrap().len(), list[0]["messages"].as_u64()), (1, Some(2)));
        let (status, _) = send(&r, key(request("POST", &format!("/api/v1/sessions/{id}/stop"))).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&r, key(request("DELETE", &format!("/api/v1/sessions/{id}"))).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}"))).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(home).ok();
    }

    #[tokio::test]
    async fn what_the_api_says_is_never_kept_by_a_browser() {
        let (r, _) = app();
        for path in ["/api/v1/health", "/api/v1/settings", "/api/v1/nothing-here"] {
            let res = r.clone().oneshot(request("GET", path).header("authorization", format!("Bearer {KEY}")).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(res.headers().get(header::CACHE_CONTROL).and_then(|v| v.to_str().ok()), Some("no-store"), "{path}");
        }
    }

    #[tokio::test]
    async fn health_is_open_and_everything_else_needs_the_key() {
        let (r, _) = app();
        let (status, body) = send(&r, request("GET", "/api/v1/health").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        let (status, body) = send(&r, request("GET", "/api/v1/settings").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "no_key");
        let (status, _) = send(&r, request("GET", "/api/v1/settings").header("authorization", format!("Bearer {KEY}")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_foreign_host_is_refused() {
        let (r, _) = app();
        let req = axum::http::Request::builder().uri("/api/v1/health").header("host", "evil.example").body(Body::empty()).unwrap();
        let (status, body) = send(&r, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "wrong_host");
    }

    #[tokio::test]
    async fn a_setting_can_be_changed_read_and_reset_and_it_is_logged() {
        let (r, _) = app();
        let key = format!("Bearer {KEY}");
        let put = request("PUT", "/api/v1/settings/events.keepDays").header("authorization", &key).header("content-type", "application/json").body(Body::from(r#"{"value":30}"#)).unwrap();
        let (status, body) = send(&r, put).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["value"], 30);
        assert_eq!(body["changed"], true);

        let bad = request("PUT", "/api/v1/settings/events.keepDays").header("authorization", &key).header("content-type", "application/json").body(Body::from(r#"{"value":"soon"}"#)).unwrap();
        let (status, body) = send(&r, bad).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "invalid");

        let (status, _) = send(&r, request("GET", "/api/v1/settings/no.such").header("authorization", &key).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (_, body) = send(&r, request("DELETE", "/api/v1/settings/events.keepDays").header("authorization", &key).body(Body::empty()).unwrap()).await;
        assert_eq!(body["changed"], false);

        let (_, log) = send(&r, request("GET", "/api/v1/events?after=0").header("authorization", &key).body(Body::empty()).unwrap()).await;
        let kinds: Vec<&str> = log.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["setting.changed", "setting.changed"]);
    }

    #[tokio::test]
    async fn a_browser_gets_in_with_a_one_use_link_and_may_only_change_things_from_our_own_pages() {
        let (r, auth) = app();
        let session = auth.redeem(&auth.new_link()).unwrap();
        let cookie = format!("{COOKIE}={session}");
        let (status, _) = send(&r, request("GET", "/api/v1/settings").header("cookie", &cookie).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        // A change triggered by another site carries the cookie but not our origin.
        let foreign = request("DELETE", "/api/v1/settings/events.keepDays").header("cookie", &cookie).header("origin", "https://evil.example").body(Body::empty()).unwrap();
        assert_eq!(send(&r, foreign).await.0, StatusCode::FORBIDDEN);
        let own = request("DELETE", "/api/v1/settings/events.keepDays").header("cookie", &cookie).header("origin", "http://127.0.0.1:7341").body(Body::empty()).unwrap();
        assert_eq!(send(&r, own).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_api_describes_itself() {
        let (r, _) = app();
        let (status, doc) = send(&r, request("GET", "/api/v1/openapi.json").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc["paths"]["/api/v1/settings/{id}"]["put"].is_object(), "the settings path is missing from the description");
        assert_eq!(doc["info"]["version"], VERSION);
    }
}
