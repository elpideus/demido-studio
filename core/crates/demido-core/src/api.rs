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

use crate::auth::{Auth, LINK_LIFETIME, cookie_name};
use crate::chat::{Chat, ContextUse, Turn};
use crate::db::Db;
use crate::desktop::Appearance;
use crate::engine::{Engine, LocalModel, ModelList, ProcessLauncher};
use crate::events::{Event, EventLog};
use crate::kept::Kept;
use crate::paths::Paths;
use crate::permissions;
use crate::providers::{Account, ProviderAccount, ProviderView, Providers, RemoteModel, Spending};
use crate::runtime::{Available, Installed, Progress, Runtimes, Source, Status};
use crate::sessions::{Change, Message, Session, Sessions, Summary, Task};
use crate::settings::{Entry, Section, Settings};
use crate::system::{Advice, SystemInfo};
use crate::ui::Ui;
use crate::vault::{Vault, VaultView};
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
    pub hub: Arc<crate::hub::Hub>,
    /// Models elsewhere, and the vault that keeps their keys.
    pub providers: Arc<Providers>,
    /// Schedules and monitors, and what runs them (DECISIONS section J).
    pub scheduler: Arc<crate::schedules::Scheduler>,
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
        let vault = Arc::new(Vault::open(paths.home()));
        let providers = Arc::new(Providers::new(db.clone(), vault.clone(), settings.clone(), events.clone()).with_programs(paths.home(), Arc::new(crate::programs::ProcessSpawner)));
        let sessions = Arc::new(Sessions::new(db.clone()));
        let schedules = Arc::new(crate::schedules::Schedules::new(db));
        let chat = Arc::new(
            Chat::new(sessions.clone(), engine.clone(), events.clone(), settings.clone())
                .with_providers(providers.clone())
                .keeping_programs_in(paths.home().join("programs").join("sessions"))
                .keeping_in(paths.home().join("saved-folders"))
                .keeping_outputs_in(paths.home().join("outputs"))
                .with_sandbox(crate::sandbox::Sandbox::find())
                .with_web(Arc::new(crate::web::Web::new(Some(paths.home().join("cache").join("web"))).reading_pdfs_apart(std::env::current_exe().ok()))),
        );
        let hub = Arc::new(crate::hub::Hub::new(paths.home().join("models"), events.clone()).with_vault(vault));
        let scheduler = Arc::new(crate::schedules::Scheduler::new(schedules, sessions.clone(), chat.clone(), events.clone()));
        Ok(Self { settings, events, auth, runtimes, engine, sessions, chat, hub, providers, scheduler })
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
    pub hub: Arc<crate::hub::Hub>,
    pub providers: Arc<Providers>,
    pub scheduler: Arc<crate::schedules::Scheduler>,
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
    /// Where the files a person deletes in the window are kept for 30 days, a folder for each session.
    pub deleted_in: PathBuf,
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
            Error::Changed(_) => (StatusCode::PRECONDITION_FAILED, "changed"),
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
    /// What of this computer decides whether a model fits: its graphics memory, its memory, what is kept free,
    /// and the context models are loaded with.
    async fn machine(&self) -> crate::hub::Machine {
        let system = self.system_info(false).await;
        let ram_mb = (system.memory.total_gb * 1024.0) as u64;
        // A card's own memory; graphics that share the computer's memory (Apple) may use about three quarters of it.
        let own = system.gpus.iter().filter_map(|g| g.vram_gb).fold(0.0, f64::max);
        let shared = system.gpus.iter().any(|g| g.shares_memory && g.vendor == "apple");
        let card_mb = if own > 0.0 {
            (own * 1024.0) as u64
        } else if shared {
            ram_mb * 3 / 4
        } else {
            0
        };
        let margin_mb = (self.settings.value::<f64>("models.reserve").unwrap_or(0.9) * 1024.0).round() as u64;
        let context = self.settings.value::<String>("models.context").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(8192);
        crate::hub::Machine { card_mb, ram_mb: if shared { ram_mb / 4 } else { ram_mb }, margin_mb, context }
    }

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

/// Deletes a model from the models folder: its file, or its folder of parts. A loaded model is unloaded
/// first; one writing an answer is not deleted. Cannot be undone.
#[utoipa::path(delete, path = "/api/v1/models/{id}", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), responses((status = 200, body = LocalModel), (status = 404, body = Problem), (status = 409, body = Problem)))]
async fn model_delete(State(app): State<App>, Path(id): Path<String>) -> Result<Json<LocalModel>> {
    Ok(Json(app.engine.delete(&id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ContextChange {
    /// What it is loaded with room for, in tokens. `null`: the context for every model again.
    pub context: Option<u64>,
}

/// Gives a model its own context, or the one for every model again. When nothing is being answered the
/// runtime is started again for it (loaded models unload, and load again when used).
#[utoipa::path(put, path = "/api/v1/models/{id}/context", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), request_body = ContextChange, responses((status = 200, body = LocalModel), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn model_context(State(app): State<App>, Path(id): Path<String>, Json(body): Json<ContextChange>) -> Result<Json<LocalModel>> {
    Ok(Json(app.engine.set_context(&id, body.context).await?))
}

/// How a model on this computer picks its words, thinking and not: what each is sent with and why (the
/// person's own settings, its maker's, the file's or the runtime's), and each of those.
#[utoipa::path(get, path = "/api/v1/models/{id}/settings", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), responses((status = 200, body = crate::engine::sampling::ModelSettings), (status = 404, body = Problem)))]
async fn model_settings(State(app): State<App>, Path(id): Path<String>) -> Result<Json<crate::engine::sampling::ModelSettings>> {
    Ok(Json(app.engine.model_settings(&id)?))
}

#[derive(Deserialize, ToSchema)]
pub struct SettingsChange {
    /// For when it thinks first. A setting left out is its maker's (or the runtime's).
    #[serde(default)]
    pub thinking: crate::engine::sampling::Sampling,
    /// For when it does not.
    #[serde(default)]
    pub plain: crate::engine::sampling::Sampling,
}

/// Sets the person's own settings for a model. Both empty: it follows its maker again.
#[utoipa::path(put, path = "/api/v1/models/{id}/settings", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), request_body = SettingsChange, responses((status = 200, body = crate::engine::sampling::ModelSettings), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn model_settings_set(State(app): State<App>, Path(id): Path<String>, Json(body): Json<SettingsChange>) -> Result<Json<crate::engine::sampling::ModelSettings>> {
    Ok(Json(app.engine.set_model_settings(&id, body.thinking, body.plain)?))
}

/// Forgets the person's own settings for a model: it follows its maker again.
#[utoipa::path(delete, path = "/api/v1/models/{id}/settings", tag = "models", params(("id" = String, Path, description = "The model's name, as the list shows it")), responses((status = 200, body = crate::engine::sampling::ModelSettings), (status = 404, body = Problem)))]
async fn model_settings_reset(State(app): State<App>, Path(id): Path<String>) -> Result<Json<crate::engine::sampling::ModelSettings>> {
    Ok(Json(app.engine.set_model_settings(&id, Default::default(), Default::default())?))
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
    /// A folder on this computer, by its full path, that the session's tools may read and change files in.
    /// Left out: the session has none, and is not offered those tools.
    pub folder: Option<String>,
}

/// Starts a session with a model. Nothing is loaded until something is said.
#[utoipa::path(post, path = "/api/v1/sessions", tag = "sessions", request_body = NewSession, responses((status = 200, body = Session), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_new(State(app): State<App>, body: Option<Json<NewSession>>) -> Result<Json<Session>> {
    let ask = body.map(|Json(b)| b).unwrap_or_default();
    // The folder is looked at first: a session is not made and then left without the folder it was asked for.
    if let Some(folder) = ask.folder.as_deref().map(str::trim).filter(|f| !f.is_empty())
        && !std::path::Path::new(folder).is_dir()
    {
        return Err(crate::Error::Invalid(format!("There is no folder \"{folder}\" on this computer.")));
    }
    let session = app.chat.start(ask.title.as_deref(), ask.model.as_deref()).await?;
    Ok(Json(match ask.folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(folder) => app.chat.set_folder(&session.id, Some(folder))?,
        None => session,
    }))
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
    /// The tasks that work in this session was split into, in the order they were made. Each names the
    /// answer it is part of (`message_id`) and, for a sub-task, its task (`parent_id`).
    pub tasks: Vec<Task>,
    /// What the model is shown before each answer when it may use tools: the list of tools and the
    /// question, word for word. `null` when tools are turned off.
    pub tools_told: Option<String>,
    /// A use of a tool that is waiting for the person's yes. The answer being written waits with it.
    /// Answer it at `/sessions/{id}/permissions/{request}`.
    pub asking: Option<permissions::Request>,
    /// The tools that need not ask again in this session, because the person said so. Forgotten when
    /// Demido stops, and when the session's level is changed.
    pub need_not_ask: Vec<String>,
    /// The changes tools made to files in the session's folder, oldest first. Each can be undone.
    pub changes: Vec<Change>,
    /// The sandbox this computer has for the programs a model runs, or why it has none.
    pub sandbox: crate::sandbox::Sandbox,
}

/// One session with everything said in it, and what the model is told first.
#[utoipa::path(get, path = "/api/v1/sessions/{id}", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = SessionDetail), (status = 404, body = Problem)))]
async fn session_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<SessionDetail>> {
    let session = app.sessions.get(&id)?;
    let thinking_choices = app.chat.thinking_choices(&session.model).await?;
    Ok(Json(SessionDetail {
        messages: app.sessions.messages(&id)?,
        instructions: app.chat.told_first(&session),
        answering: app.chat.answering(&id),
        thinking_choices,
        summaries: app.sessions.summaries(&id)?,
        context: app.chat.context_use(&id)?,
        tasks: app.sessions.tasks(&id)?,
        tools_told: app.chat.tools_told(&session),
        asking: app.chat.waiting_for(&id),
        need_not_ask: app.chat.need_not_ask(&id),
        changes: app.sessions.changes(&id)?,
        sandbox: app.chat.sandbox().clone(),
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
    /// The folder the session's tools may work in, by its full path. An empty text takes the folder away.
    pub folder: Option<String>,
    /// How much the session may do without asking: `cautious`, `balanced`, `smart` or `autonomous`.
    pub permission: Option<String>,
    /// The account at the model's provider it goes through, by its id. An empty text: the provider's default.
    pub account: Option<String>,
}

/// Changes what is given: the title, the model, whether the model thinks before it answers, the folder
/// its tools may work in, how much it may do without asking, the account it goes through.
#[utoipa::path(patch, path = "/api/v1/sessions/{id}", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = SessionChange, responses((status = 200, body = Session), (status = 404, body = Problem)))]
async fn session_change(State(app): State<App>, Path(id): Path<String>, Json(body): Json<SessionChange>) -> Result<Json<Session>> {
    let mut session = app.chat.change(&id, body.title.as_deref(), body.model.as_deref(), body.thinking.as_deref()).await?;
    if let Some(folder) = body.folder.as_deref() {
        session = app.chat.set_folder(&id, Some(folder))?;
    }
    if let Some(level) = body.permission.as_deref() {
        session = app.chat.set_permission(&id, level)?;
    }
    if let Some(account) = body.account.as_deref() {
        session = app.chat.set_account(&id, Some(account).filter(|a| !a.is_empty()))?;
    }
    Ok(Json(session))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct FolderQuery {
    /// A path inside the session's folder, with `/`. Left out: the folder itself.
    #[serde(default)]
    pub path: String,
}

/// The session's folder, the one the model works in.
fn folder_of(app: &App, id: &str) -> Result<std::path::PathBuf> {
    let session = app.sessions.get(id)?;
    session.folder.map(std::path::PathBuf::from).ok_or_else(|| Error::NotFound("This session has no folder.".into()))
}

/// What is in a folder of the session's folder: folders first, then files, with what each is shown as.
/// Nothing outside the session's folder is listed.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/files", tag = "sessions", params(("id" = String, Path, description = "The session's id"), FolderQuery), responses((status = 200, body = crate::folder::Listing), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_files(State(app): State<App>, Path(id): Path<String>, Query(q): Query<FolderQuery>) -> Result<Json<crate::folder::Listing>> {
    let folder = folder_of(&app, &id)?;
    Ok(Json(tokio::task::spawn_blocking(move || crate::folder::list(&folder, &q.path)).await.map_err(|e| Error::Other(e.to_string()))??))
}

/// A file of the session's folder, to look at. What a browser would run as a page (HTML and the like) is
/// sent as plain text, and every file with `nosniff` and a `sandbox` policy: it is never a page with
/// Demido's address. A `Range` is answered, so that a video can be moved through. A text file of up to
/// 8 MB comes with its version (`ETag`), which saving it again names.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/files/content", tag = "sessions", params(("id" = String, Path, description = "The session's id"), FolderQuery), responses((status = 200, description = "The file"), (status = 206, description = "Part of the file"), (status = 404, body = Problem), (status = 416, description = "A range not in the file")))]
async fn session_file(State(app): State<App>, Path(id): Path<String>, Query(q): Query<FolderQuery>, headers: HeaderMap) -> Result<Response> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let folder = folder_of(&app, &id)?;
    let opened = tokio::task::spawn_blocking(move || crate::folder::open(&folder, &q.path)).await.map_err(|e| Error::Other(e.to_string()))??;
    // A text file that can be saved is read whole, so that its version is of exactly what is sent.
    let whole = if opened.kind.is_text() && opened.size <= crate::folder::MOST_SAVED { Some(tokio::fs::read(&opened.path).await?) } else { None };
    let size = whole.as_ref().map_or(opened.size, |b| b.len() as u64);
    let wanted = crate::folder::range(headers.get(header::RANGE).and_then(|v| v.to_str().ok()), size);
    let Ok(wanted) = wanted else {
        return Ok((StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{size}"))]).into_response());
    };
    let (start, length) = wanted.unwrap_or((0, size));
    let version = whole.as_deref().map(crate::folder::version_of);
    let body = match whole {
        Some(bytes) => axum::body::Body::from(bytes[start as usize..(start + length) as usize].to_vec()),
        None => {
            let mut file = tokio::fs::File::open(&opened.path).await?;
            file.seek(std::io::SeekFrom::Start(start)).await?;
            axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(file.take(length)))
        }
    };
    // Opened by itself, it is a document of no address, which can run nothing and reach nothing. The window
    // draws PDFs itself (pdf.js), so none needs the browser's own viewer.
    let policy = "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'";
    let mut response = Response::builder()
        .status(if wanted.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, opened.content_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", policy)
        .header(header::CACHE_CONTROL, "no-store");
    if let Some((start, length)) = wanted {
        response = response.header(header::CONTENT_RANGE, format!("bytes {start}-{}/{size}", start + length - 1));
    }
    if let Some(version) = version {
        response = response.header(header::ETAG, version);
    }
    let printer = jiff::fmt::rfc2822::DateTimePrinter::new();
    if let Some(when) = opened.modified.and_then(|m| jiff::Timestamp::try_from(m).ok()).and_then(|t| printer.timestamp_to_rfc9110_string(&t).ok()) {
        response = response.header(header::LAST_MODIFIED, when);
    }
    response.body(body).map_err(|e| Error::Other(e.to_string()))
}

/// What is different between a text file of the session's folder, as it is now, and the person's edit of it
/// (the body): a unified diff, `-` for lines only the file has, `+` for lines only the edit has. For when the
/// file changed under an edit, before the person chooses which to keep.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/files/compare", tag = "sessions", params(("id" = String, Path, description = "The session's id"), FolderQuery), request_body(content = String, content_type = "text/plain", description = "The person's edit"), responses((status = 200, body = crate::folder::Compared), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_file_compare(State(app): State<App>, Path(id): Path<String>, Query(q): Query<FolderQuery>, mine: axum::body::Bytes) -> Result<Json<crate::folder::Compared>> {
    let folder = folder_of(&app, &id)?;
    Ok(Json(tokio::task::spawn_blocking(move || crate::folder::compare(&folder, &q.path, &mine)).await.map_err(|e| Error::Other(e.to_string()))??))
}

/// A new file for the session's folder, as the person asks.
#[derive(Deserialize, ToSchema)]
pub struct NewFile {
    /// From the session's folder, with `/`.
    pub path: String,
    /// What it holds to begin with: nothing, when left out.
    #[serde(default)]
    pub text: String,
}

/// Makes a file in the session's folder, as the person asks. Never over one that is there (409); the
/// folders on the way are made.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/files", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = NewFile, responses((status = 200, body = crate::folder::Entry), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_file_new(State(app): State<App>, Path(id): Path<String>, Json(body): Json<NewFile>) -> Result<Json<crate::folder::Entry>> {
    let folder = folder_of(&app, &id)?;
    let made = tokio::task::spawn_blocking(move || crate::folder::put(&folder, &body.path, body.text.as_bytes(), false)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": made.path, "size": made.size, "made": true }))?;
    Ok(Json(made))
}

/// Deletes a file of the session's folder, as the person asks: it is kept for 30 days, to be put back.
/// Only a file, not a folder or a link.
#[utoipa::path(delete, path = "/api/v1/sessions/{id}/files", tag = "sessions", params(("id" = String, Path, description = "The session's id"), FolderQuery), responses((status = 200, body = crate::folder::Deleted), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_file_delete(State(app): State<App>, Path(id): Path<String>, Query(q): Query<FolderQuery>) -> Result<Json<crate::folder::Deleted>> {
    let folder = folder_of(&app, &id)?;
    let kept_in = app.deleted_in.join(&id);
    let gone = tokio::task::spawn_blocking(move || crate::folder::delete(&folder, &q.path, &kept_in)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": gone.path, "deleted": true }))?;
    Ok(Json(gone))
}

/// The files deleted from the session's folder that are still kept, the most recent first.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/files/deleted", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Vec<crate::folder::Deleted>), (status = 404, body = Problem)))]
async fn session_files_deleted(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Vec<crate::folder::Deleted>>> {
    app.sessions.get(&id)?;
    let kept_in = app.deleted_in.join(&id);
    Ok(Json(tokio::task::spawn_blocking(move || crate::folder::deleted(&kept_in)).await.map_err(|e| Error::Other(e.to_string()))??))
}

/// Puts a deleted file back where it was; never over a file that is there now (409).
#[utoipa::path(post, path = "/api/v1/sessions/{id}/files/deleted/{deleted}/put-back", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("deleted" = String, Path, description = "The deleted file's id")), responses((status = 200, body = crate::folder::Entry), (status = 404, body = Problem), (status = 409, body = Problem)))]
async fn session_file_put_back(State(app): State<App>, Path((id, deleted)): Path<(String, String)>) -> Result<Json<crate::folder::Entry>> {
    let folder = folder_of(&app, &id)?;
    let kept_in = app.deleted_in.join(&id);
    let back = tokio::task::spawn_blocking(move || crate::folder::put_back(&folder, &kept_in, &deleted)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": back.path, "put_back": true }))?;
    Ok(Json(back))
}

/// A file or folder of the session's folder, and its new name.
#[derive(Deserialize, ToSchema)]
pub struct Renamed {
    pub from: String,
    pub to: String,
}

/// Renames a file or a folder of the session's folder, as the person asks: both inside it, and never over
/// one that is there (409).
#[utoipa::path(post, path = "/api/v1/sessions/{id}/files/rename", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = Renamed, responses((status = 200, body = crate::folder::Entry), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_file_rename(State(app): State<App>, Path(id): Path<String>, Json(body): Json<Renamed>) -> Result<Json<crate::folder::Entry>> {
    let folder = folder_of(&app, &id)?;
    let from = body.from.clone();
    let moved = tokio::task::spawn_blocking(move || crate::folder::rename(&folder, &body.from, &body.to)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": moved.path, "renamed_from": from }))?;
    Ok(Json(moved))
}

/// Saves a text file of the session's folder, as the person edited it in the window. `If-Match` says which
/// version it replaces: the `ETag` it was sent with, or `*` for whatever is there. When the file has
/// changed since (the model or another program wrote it), nothing is written and the answer is 412. Only a
/// text file that is there, inside the folder; a new file is not made here.
#[utoipa::path(put, path = "/api/v1/sessions/{id}/files/content", tag = "sessions", params(("id" = String, Path, description = "The session's id"), FolderQuery, ("If-Match" = String, Header, description = "The version this replaces, or `*`")), request_body(content = String, content_type = "text/plain", description = "The file's new text, in UTF-8"), responses((status = 200, body = crate::folder::Saved), (status = 404, body = Problem), (status = 412, body = Problem), (status = 413, description = "More than 8 MB"), (status = 422, body = Problem)))]
async fn session_file_save(State(app): State<App>, Path(id): Path<String>, Query(q): Query<FolderQuery>, headers: HeaderMap, text: axum::body::Bytes) -> Result<Response> {
    let Some(replaces) = headers.get(header::IF_MATCH).and_then(|v| v.to_str().ok()).map(str::to_string) else {
        return Err(Error::Invalid("Say which version of the file this replaces, with If-Match: the ETag it was sent with, or * for whatever is there.".into()));
    };
    let folder = folder_of(&app, &id)?;
    let saved = tokio::task::spawn_blocking(move || crate::folder::save(&folder, &q.path, &text, &replaces)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": saved.path, "size": saved.size }))?;
    Ok(([(header::ETAG, saved.version.clone())], Json(saved)).into_response())
}

/// An artifact with its versions, newest last.
#[derive(Serialize, ToSchema)]
pub struct ArtifactWithVersions {
    pub artifact: crate::sessions::Artifact,
    pub versions: Vec<crate::sessions::VersionFacts>,
}

/// The artifact, if it is this session's.
fn artifact_of(app: &App, id: &str, artifact: &str) -> Result<crate::sessions::Artifact> {
    app.sessions.artifact(artifact).ok().filter(|a| a.session_id == id).ok_or_else(|| Error::NotFound(format!("This session has no artifact \"{artifact}\".")))
}

/// The artifacts models gave in a session: files of its own, with every version kept. The first made first.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/artifacts", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Vec<crate::sessions::Artifact>), (status = 404, body = Problem)))]
async fn session_artifacts(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Vec<crate::sessions::Artifact>>> {
    app.sessions.get(&id)?;
    Ok(Json(app.sessions.artifacts(&id)?))
}

/// What the person keeps as a file: an answer, or part of one.
#[derive(Deserialize, ToSchema)]
pub struct KeptAsFile {
    /// With the ending that says what it is: plan.md, page.html. Without one, it is taken as Markdown.
    pub name: String,
    pub text: String,
    /// The answer it came from.
    #[serde(default)]
    pub message_id: Option<String>,
}

/// What keeping it made.
#[derive(Serialize, ToSchema)]
pub struct KeptArtifact {
    pub artifact: crate::sessions::Artifact,
    /// The version that holds the text now.
    pub n: i64,
}

/// Keeps text as a file of the session's own, as the person asks: an artifact, as if a model had given
/// it. The same name again is a new version of it.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/artifacts", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = KeptAsFile, responses((status = 200, body = KeptArtifact), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn session_artifact_keep(State(app): State<App>, Path(id): Path<String>, Json(body): Json<KeptAsFile>) -> Result<Json<KeptArtifact>> {
    let made = app.sessions.make_artifact(&id, &body.name, &body.text, body.message_id.as_deref())?;
    app.events.append("session.artifact_kept", Some(&id), json!({ "artifact": made.artifact.id, "name": made.artifact.name, "n": made.n }))?;
    Ok(Json(KeptArtifact { artifact: made.artifact, n: made.n }))
}

/// One artifact, and what each of its versions is.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/artifacts/{artifact}", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("artifact" = String, Path, description = "The artifact's id")), responses((status = 200, body = ArtifactWithVersions), (status = 404, body = Problem)))]
async fn session_artifact(State(app): State<App>, Path((id, artifact)): Path<(String, String)>) -> Result<Json<ArtifactWithVersions>> {
    let artifact = artifact_of(&app, &id, &artifact)?;
    let versions = app.sessions.artifact_versions(&artifact.id)?;
    Ok(Json(ArtifactWithVersions { artifact, versions }))
}

/// One version of an artifact, with its text.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/artifacts/{artifact}/versions/{n}", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("artifact" = String, Path, description = "The artifact's id"), ("n" = i64, Path, description = "The version: 1 for the first")), responses((status = 200, body = crate::sessions::Version), (status = 404, body = Problem)))]
async fn session_artifact_version(State(app): State<App>, Path((id, artifact, n)): Path<(String, String, i64)>) -> Result<Json<crate::sessions::Version>> {
    let artifact = artifact_of(&app, &id, &artifact)?;
    Ok(Json(app.sessions.artifact_version(&artifact.id, Some(n))?))
}

/// Where a version of an artifact goes in the session's folder.
#[derive(Deserialize, ToSchema)]
pub struct IntoFolder {
    /// From the session's folder, with `/`. Left out: the artifact's own name.
    #[serde(default)]
    pub path: Option<String>,
    /// Replace a file that is there. Without it, a file that is there is left as it is, and the answer is 409.
    #[serde(default)]
    pub replace: bool,
}

/// Puts a version of an artifact into the session's folder, as the person asks: the file a model gave,
/// kept on disk. Only inside the folder; a file that is there is replaced only when the person says so.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/artifacts/{artifact}/versions/{n}/into-folder", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("artifact" = String, Path, description = "The artifact's id"), ("n" = i64, Path, description = "The version")), request_body = IntoFolder, responses((status = 200, body = crate::folder::Entry), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_artifact_into_folder(State(app): State<App>, Path((id, artifact, n)): Path<(String, String, i64)>, Json(body): Json<IntoFolder>) -> Result<Json<crate::folder::Entry>> {
    let artifact = artifact_of(&app, &id, &artifact)?;
    let version = app.sessions.artifact_version(&artifact.id, Some(n))?;
    let folder = folder_of(&app, &id)?;
    let path = body.path.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| artifact.name.clone());
    let put = tokio::task::spawn_blocking(move || crate::folder::put(&folder, &path, version.text.as_bytes(), body.replace)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("session.file_saved", Some(&id), json!({ "path": put.path, "size": put.size, "artifact": artifact.id, "version": n }))?;
    Ok(Json(put))
}

/// One version of an artifact, as a file to download under its name. Like a file of the folder, it is
/// never a page with Demido's address: plain text, `nosniff`, and a `sandbox` policy.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/artifacts/{artifact}/versions/{n}/file", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("artifact" = String, Path, description = "The artifact's id"), ("n" = i64, Path, description = "The version: 1 for the first")), responses((status = 200, description = "The file", content_type = "text/plain"), (status = 404, body = Problem)))]
async fn session_artifact_file(State(app): State<App>, Path((id, artifact, n)): Path<(String, String, i64)>) -> Result<Response> {
    let artifact = artifact_of(&app, &id, &artifact)?;
    let version = app.sessions.artifact_version(&artifact.id, Some(n))?;
    // The name as it was given, for browsers that read it (RFC 6266), and one of plain letters for those that do not.
    let encoded: String = artifact.name.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let plain_name: String = artifact.name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' { c } else { '_' }).collect();
    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{plain_name}\"; filename*=UTF-8''{encoded}"))
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", "sandbox; default-src 'none'")
        .header(header::CACHE_CONTROL, "no-store")
        .body(axum::body::Body::from(version.text))
        .map_err(|e| Error::Other(e.to_string()))
}

#[derive(Deserialize, ToSchema)]
pub struct PermissionAnswer {
    /// `once`: yes, this once. `session`: yes, and do not ask again for this tool in this session. `no`.
    pub said: permissions::Said,
}

/// Answers a use of a tool that is waiting for the person's yes (`asking` in the session's detail, and
/// the `permission.asked` event). The answer being written goes on from there.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/permissions/{request}", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("request" = String, Path, description = "The request's id")), request_body = PermissionAnswer, responses((status = 204), (status = 404, body = Problem)))]
async fn session_permission(State(app): State<App>, Path((id, request)): Path<(String, String)>, Json(body): Json<PermissionAnswer>) -> Result<StatusCode> {
    app.chat.answer_request(&id, &request, body.said)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Undoes a change a tool made to a file in the session's folder: puts back what the file held before, or
/// removes a file the change created.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/changes/{change}/undo", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("change" = String, Path, description = "The change's id")), responses((status = 200, body = Change), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_undo(State(app): State<App>, Path((id, change)): Path<(String, String)>) -> Result<Json<Change>> {
    Ok(Json(app.chat.undo(&id, &change)?))
}

#[derive(Deserialize, ToSchema, utoipa::IntoParams)]
pub struct HubQuery {
    /// Words to look for in models' names.
    q: Option<String>,
    /// `trending` (the default), `downloads`, `likes` or `updated`.
    sort: Option<String>,
    /// Only models that take these in, comma-separated: `images`, `sound`.
    takes: Option<String>,
    /// Only models made to do these, comma-separated: `tools`, `thinking`, `embedding`.
    does: Option<String>,
    /// Only models whose usual size fits this computer.
    fits: Option<bool>,
    /// The next page, as the last page gave it.
    cursor: Option<String>,
}

/// Searches Hugging Face for models the runtime can run (GGUF), with what each says it does and whether its
/// usual size would fit this computer. Kept ten minutes.
#[utoipa::path(get, path = "/api/v1/hub/models", tag = "models", params(HubQuery), responses((status = 200, body = crate::hub::HubPage), (status = 502, body = Problem), (status = 409, body = Problem)))]
async fn hub_search(State(app): State<App>, Query(q): Query<HubQuery>) -> Result<Json<crate::hub::HubPage>> {
    let list = |s: &Option<String>| s.as_deref().unwrap_or_default().split(',').map(str::trim).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let machine = app.machine().await;
    Ok(Json(app.hub.search(q.q.as_deref().unwrap_or_default(), q.sort.as_deref().unwrap_or("trending"), &list(&q.takes), &list(&q.does), q.fits.unwrap_or(false), q.cursor.as_deref(), machine).await?))
}

/// A model's page: its files by quantization, what each would need on this computer, which is
/// recommended, and its model card. Kept an hour.
#[utoipa::path(get, path = "/api/v1/hub/models/{owner}/{name}", tag = "models", params(("owner" = String, Path, description = "Its owner on Hugging Face"), ("name" = String, Path, description = "Its name")), responses((status = 200, body = crate::hub::HubDetail), (status = 404, body = Problem)))]
async fn hub_model(State(app): State<App>, Path((owner, name)): Path<(String, String)>) -> Result<Json<crate::hub::HubDetail>> {
    let machine = app.machine().await;
    Ok(Json(app.hub.detail(&format!("{owner}/{name}"), machine).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct DownloadRequest {
    /// `owner/name` on Hugging Face.
    repo: String,
    /// As the model's page names it: `Q4_K_M`.
    quant: String,
}

/// Starts downloading a model into the models folder: its files, checked against the checksums Hugging Face
/// lists, and its vision part if it has one. Answers at once; progress comes on the event stream as
/// `model.download`. One download at a time: a second one waits.
#[utoipa::path(post, path = "/api/v1/hub/downloads", tag = "models", request_body = DownloadRequest, responses((status = 202, body = crate::hub::Download), (status = 400, body = Problem), (status = 404, body = Problem)))]
async fn hub_download(State(app): State<App>, Json(body): Json<DownloadRequest>) -> Result<(StatusCode, Json<crate::hub::Download>)> {
    let machine = app.machine().await;
    Ok((StatusCode::ACCEPTED, Json(app.hub.start(&body.repo, &body.quant, machine).await?)))
}

/// Whether a Hugging Face token is kept, for gated models: where, and its last four characters. Never its value.
#[utoipa::path(get, path = "/api/v1/hub/token", tag = "models", responses((status = 200, body = Option<crate::vault::KeyFacts>)))]
async fn hub_token(State(app): State<App>) -> Json<Option<crate::vault::KeyFacts>> {
    Json(app.providers.vault().facts(crate::hub::TOKEN))
}

/// Keeps the person's Hugging Face token, in place of one kept before. It is sent only to Hugging Face, with
/// the requests for a gated model's files.
#[utoipa::path(put, path = "/api/v1/hub/token", tag = "models", request_body = NewKey, responses((status = 200, body = crate::vault::KeyFacts), (status = 422, body = Problem)))]
async fn hub_token_set(State(app): State<App>, Json(body): Json<NewKey>) -> Result<Json<crate::vault::KeyFacts>> {
    let vault = app.providers.vault().clone();
    let facts = tokio::task::spawn_blocking(move || vault.put(crate::hub::TOKEN, &body.value)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("hub.token.added", None, json!({ "kept_in": facts.kept_in, "ends_with": facts.ends_with }))?;
    Ok(Json(facts))
}

/// Deletes the Hugging Face token. Models already downloaded stay.
#[utoipa::path(delete, path = "/api/v1/hub/token", tag = "models", responses((status = 200, body = crate::vault::KeyFacts), (status = 404, body = Problem)))]
async fn hub_token_remove(State(app): State<App>) -> Result<Json<crate::vault::KeyFacts>> {
    let vault = app.providers.vault().clone();
    let gone = tokio::task::spawn_blocking(move || vault.remove(crate::hub::TOKEN)).await.map_err(|e| Error::Other(e.to_string()))??;
    app.events.append("hub.token.removed", None, json!({ "kept_in": gone.kept_in }))?;
    Ok(Json(gone))
}

/// The downloads since the server started, the newest last.
#[utoipa::path(get, path = "/api/v1/hub/downloads", tag = "models", responses((status = 200, body = Vec<crate::hub::Download>)))]
async fn hub_downloads(State(app): State<App>) -> Json<Vec<crate::hub::Download>> {
    Json(app.hub.downloads())
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct StopQuery {
    /// Also remove what was downloaded of it.
    discard: Option<bool>,
}

/// Stops a download (what was downloaded is kept, and starting it again goes on from there), or forgets one
/// that has ended. With `discard`, what was downloaded is removed.
#[utoipa::path(delete, path = "/api/v1/hub/downloads/{id}", tag = "models", params(("id" = String, Path, description = "The download's id"), StopQuery), responses((status = 200, body = crate::hub::Download), (status = 404, body = Problem)))]
async fn hub_stop(State(app): State<App>, Path(id): Path<String>, Query(q): Query<StopQuery>) -> Result<Json<crate::hub::Download>> {
    Ok(Json(app.hub.stop(&id, q.discard.unwrap_or(false))?))
}

/// Text the API gives as it is, to be read by a person: a program's output, a diff.
fn plain(text: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

/// The whole of what a program printed, as a terminal would show it once the program had ended. A step of
/// `run_command` holds its id (`output`). The model was given a short view of it; this is all of it. Only
/// the newest 50 of a session are kept.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/outputs/{output}", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("output" = String, Path, description = "The output's id, from the step")), responses((status = 200, description = "The output, as plain text", content_type = "text/plain"), (status = 404, body = Problem)))]
async fn session_output(State(app): State<App>, Path((id, output)): Path<(String, String)>) -> Result<Response> {
    Ok(plain(app.chat.output(&id, &output)?))
}

/// What a change changed, as a unified diff made at the moment of the change: for a file, its lines that
/// went and came; for a program, every file it changed, made or removed in the folder.
#[utoipa::path(get, path = "/api/v1/sessions/{id}/changes/{change}/diff", tag = "sessions", params(("id" = String, Path, description = "The session's id"), ("change" = String, Path, description = "The change's id")), responses((status = 200, description = "The diff, as plain text", content_type = "text/plain"), (status = 404, body = Problem)))]
async fn session_diff(State(app): State<App>, Path((id, change)): Path<(String, String)>) -> Result<Response> {
    Ok(plain(app.chat.change_diff(&id, &change)?))
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
    let gone = app.chat.delete(&id)?;
    // The files deleted from its folder, kept to be put back, go with it.
    let _ = std::fs::remove_dir_all(app.deleted_in.join(&gone.id));
    Ok(Json(gone))
}

#[derive(Deserialize, ToSchema)]
pub struct Said {
    /// What the person wrote.
    pub content: String,
    /// Plan first: the model is asked for the steps of the request, and Demido then has it do them one at a
    /// time, as tasks. Left out or false: no plan, unless the model asks for one or the setting says always.
    #[serde(default)]
    pub plan: bool,
}

/// Says something in a session. Answers at once with what was said and the answer that is now being
/// written. The answer's words follow on the event stream as `message.delta`, several times a second,
/// and it is stored whole at `message.done` (or `message.stopped`, or `message.failed` with the reason).
/// The session's model is loaded first if it is not. One answer at a time in a session.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/messages", tag = "sessions", params(("id" = String, Path, description = "The session's id")), request_body = Said, responses((status = 202, body = Turn), (status = 404, body = Problem), (status = 409, body = Problem), (status = 422, body = Problem)))]
async fn session_say(State(app): State<App>, Path(id): Path<String>, Json(body): Json<Said>) -> Result<Response> {
    Ok((StatusCode::ACCEPTED, Json(app.chat.say_with(&id, &body.content, body.plan)?)).into_response())
}

/// Stops the answer being written in a session. What was written so far is kept.
#[utoipa::path(post, path = "/api/v1/sessions/{id}/stop", tag = "sessions", params(("id" = String, Path, description = "The session's id")), responses((status = 200, body = Stopped)))]
async fn session_stop(State(app): State<App>, Path(id): Path<String>) -> Json<Stopped> {
    Json(Stopped { stopped: app.chat.stop(&id) })
}

// ───────── keys, providers and what they cost ─────────

/// The keys Demido keeps: their names (`<provider>.<account>`), where each is kept and its last four
/// characters. Never a value. A key is added with its account (`POST /providers/{id}/accounts`).
#[utoipa::path(get, path = "/api/v1/vault", tag = "providers", responses((status = 200, body = VaultView)))]
async fn vault_get(State(app): State<App>) -> Json<VaultView> {
    Json(app.providers.vault().view())
}

#[derive(Deserialize, ToSchema)]
pub struct NewAccount {
    /// The person's own name for it: "Personal", "Work". None: "Account 1", "Account 2".
    pub label: Option<String>,
    /// For an API, its key, as the provider gave it: it is kept, and never given back. Claude Code takes
    /// none: its account signs in afterwards (`…/sign-in`).
    pub key: Option<String>,
}

/// Adds an account at a provider: a key for an API, or an account of Claude Code to sign in. The first one
/// is the default. What comes back holds no key's value.
#[utoipa::path(post, path = "/api/v1/providers/{id}/accounts", tag = "providers", params(("id" = String, Path, description = "anthropic, gemini, openrouter or claude-code")), request_body = NewAccount, responses((status = 201, body = ProviderAccount), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn account_add(State(app): State<App>, Path(id): Path<String>, Json(body): Json<NewAccount>) -> Result<Response> {
    let providers = app.providers.clone();
    let added = tokio::task::spawn_blocking(move || providers.add_account(&id, body.label.as_deref(), body.key.as_deref())).await.map_err(|e| Error::Other(e.to_string()))??;
    Ok((StatusCode::CREATED, Json(added)).into_response())
}

#[derive(Deserialize, ToSchema)]
pub struct AccountChange {
    /// Another name for it.
    pub label: Option<String>,
    /// True: sessions use this one unless they name another.
    pub default: Option<bool>,
}

/// Renames an account, or makes it the default.
#[utoipa::path(patch, path = "/api/v1/providers/{id}/accounts/{account}", tag = "providers", params(("id" = String, Path, description = "The provider"), ("account" = String, Path, description = "The account's id")), request_body = AccountChange, responses((status = 200, body = ProviderAccount), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn account_change(State(app): State<App>, Path((id, account)): Path<(String, String)>, Json(body): Json<AccountChange>) -> Result<Json<ProviderAccount>> {
    Ok(Json(app.providers.change_account(&id, &account, body.label.as_deref(), body.default == Some(true))?))
}

#[derive(Deserialize, ToSchema)]
pub struct NewKey {
    /// The key, as the provider gave it. It is kept, and never given back.
    pub value: String,
}

/// Gives an account of an API another key, in place of the one it had.
#[utoipa::path(put, path = "/api/v1/providers/{id}/accounts/{account}/key", tag = "providers", params(("id" = String, Path, description = "anthropic, gemini or openrouter"), ("account" = String, Path, description = "The account's id")), request_body = NewKey, responses((status = 200, body = ProviderAccount), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn account_key(State(app): State<App>, Path((id, account)): Path<(String, String)>, Json(body): Json<NewKey>) -> Result<Json<ProviderAccount>> {
    let providers = app.providers.clone();
    Ok(Json(tokio::task::spawn_blocking(move || providers.set_account_key(&id, &account, &body.value)).await.map_err(|e| Error::Other(e.to_string()))??))
}

/// Deletes an account: its key, or for Claude Code its profile with its login and history (signed out
/// first). Sessions that named it use the default from then on. What it cost stays counted.
#[utoipa::path(delete, path = "/api/v1/providers/{id}/accounts/{account}", tag = "providers", params(("id" = String, Path, description = "The provider"), ("account" = String, Path, description = "The account's id")), responses((status = 200, body = ProviderAccount), (status = 404, body = Problem)))]
async fn account_remove(State(app): State<App>, Path((id, account)): Path<(String, String)>) -> Result<Json<ProviderAccount>> {
    Ok(Json(app.providers.remove_account(&id, &account).await?))
}

/// The providers: each one's accounts (never a key's value), the models chosen there, and what Demido counted this month.
#[utoipa::path(get, path = "/api/v1/providers", tag = "providers", responses((status = 200, body = Vec<ProviderView>)))]
async fn providers_list(State(app): State<App>) -> Result<Json<Vec<ProviderView>>> {
    Ok(Json(app.providers.view().await?))
}

#[derive(Deserialize)]
struct ModelsQuery {
    fresh: Option<bool>,
}

/// The models a provider has, as it says. Its list is asked again after an hour, or with `fresh=true`.
#[utoipa::path(get, path = "/api/v1/providers/{id}/models", tag = "providers", params(("id" = String, Path, description = "anthropic, gemini or openrouter"), ("fresh" = Option<bool>, Query, description = "Ask the provider now, not from what was kept")), responses((status = 200, body = Vec<RemoteModel>), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn provider_models(State(app): State<App>, Path(id): Path<String>, Query(q): Query<ModelsQuery>) -> Result<Json<Vec<RemoteModel>>> {
    Ok(Json(app.providers.models(&id, q.fresh.unwrap_or(false)).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct ModelChoice {
    /// The model's name at the provider, or as a session names it (`provider/model`).
    pub model: String,
    /// True to choose it, false to let it go.
    pub chosen: bool,
}

/// Chooses a model of a provider, so that sessions may use it, or lets it go. Nothing is sent to a model
/// that is not chosen.
#[utoipa::path(put, path = "/api/v1/providers/{id}/chosen", tag = "providers", params(("id" = String, Path, description = "anthropic, gemini or openrouter")), request_body = ModelChoice, responses((status = 200, body = RemoteModel), (status = 404, body = Problem)))]
async fn provider_choose(State(app): State<App>, Path(id): Path<String>, Json(body): Json<ModelChoice>) -> Result<Json<RemoteModel>> {
    Ok(Json(app.providers.choose(&id, &body.model, body.chosen).await?))
}

#[derive(Deserialize)]
struct AccountQuery {
    account: Option<String>,
}

/// What the provider itself says about an account (the default, unless `account` names another): what it
/// spent, its limits. Only OpenRouter can be asked this with an ordinary key; for the others it says where
/// to look.
#[utoipa::path(get, path = "/api/v1/providers/{id}/account", tag = "providers", params(("id" = String, Path, description = "anthropic, gemini or openrouter"), ("account" = Option<String>, Query, description = "The account's id; the default when left out")), responses((status = 200, body = Account), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn provider_account(State(app): State<App>, Path(id): Path<String>, Query(q): Query<AccountQuery>) -> Result<Json<Account>> {
    Ok(Json(app.providers.account(&id, q.account.as_deref()).await?))
}

/// Downloads Demido's own copy of a program (`claude-code`, `opencode`) from its maker, checked, into Demido's
/// own folder. Answers at once; the download is told as it goes (`program.download`), and its end is in the
/// log (`program.downloaded`, `program.download.failed`). Nothing is installed on the system.
#[utoipa::path(post, path = "/api/v1/providers/{id}/program", tag = "providers", params(("id" = String, Path, description = "claude-code or opencode")), responses((status = 202, body = Stopped), (status = 404, body = Problem), (status = 409, body = Problem)))]
async fn program_download(State(app): State<App>, Path(id): Path<String>) -> Result<Response> {
    app.providers.start_download(&id)?;
    Ok((StatusCode::ACCEPTED, Json(Stopped { stopped: false })).into_response())
}

/// Deletes Demido's copy of a program. Its profiles (their settings and, for Claude Code, the logins) stay.
#[utoipa::path(delete, path = "/api/v1/providers/{id}/program", tag = "providers", params(("id" = String, Path, description = "claude-code or opencode")), responses((status = 200, body = crate::programs::copies::Kept), (status = 404, body = Problem)))]
async fn program_remove(State(app): State<App>, Path(id): Path<String>) -> Result<Json<crate::programs::copies::Kept>> {
    Ok(Json(app.providers.remove_copy(&id)?))
}

/// Stops the download of Demido's copy. What was downloaded is kept, so the next try goes on from there.
#[utoipa::path(delete, path = "/api/v1/providers/{id}/program/download", tag = "providers", params(("id" = String, Path, description = "claude-code or opencode")), responses((status = 200, body = Stopped)))]
async fn program_stop(State(app): State<App>, Path(id): Path<String>) -> Json<Stopped> {
    Json(Stopped { stopped: app.providers.copies().stop(&id) })
}

/// The newest version the program's maker offers, and how large it is.
#[utoipa::path(get, path = "/api/v1/providers/{id}/program/latest", tag = "providers", params(("id" = String, Path, description = "claude-code or opencode")), responses((status = 200, body = crate::programs::copies::Latest), (status = 502, body = Problem)))]
async fn program_latest(State(app): State<App>, Path(id): Path<String>) -> Result<Json<crate::programs::copies::Latest>> {
    Ok(Json(app.providers.copies().latest(&id).await?))
}

/// Starts Claude Code's own sign-in for one of its accounts: Anthropic's page opens in the browser, and
/// Claude Code keeps the login in the account's profile. Demido never reads, asks for or passes on the
/// login. Answers at once; how it goes is told (`program.signin`).
#[utoipa::path(post, path = "/api/v1/providers/{id}/accounts/{account}/sign-in", tag = "providers", params(("id" = String, Path, description = "claude-code"), ("account" = String, Path, description = "The account's id")), responses((status = 202, body = Stopped), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn program_sign_in(State(app): State<App>, Path((id, account)): Path<(String, String)>) -> Result<Response> {
    app.providers.sign_in(&id, &account)?;
    Ok((StatusCode::ACCEPTED, Json(Stopped { stopped: false })).into_response())
}

/// What Demido counted this month on models elsewhere, by model, and the month's limit.
#[utoipa::path(get, path = "/api/v1/usage", tag = "providers", responses((status = 200, body = Spending)))]
async fn usage(State(app): State<App>) -> Result<Json<Spending>> {
    Ok(Json(app.providers.spending()?))
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
async fn enter(State(app): State<App>, headers: HeaderMap, Query(q): Query<EnterQuery>) -> Response {
    match app.auth.redeem(&q.code) {
        Some(session) => {
            let name = cookie_name(headers.get(header::HOST).and_then(|h| h.to_str().ok()));
            ([(header::SET_COOKIE, format!("{name}={session}; HttpOnly; SameSite=Strict; Path=/"))], Redirect::to("/")).into_response()
        }
        None => (StatusCode::FORBIDDEN, "This link was already used, or is older than a minute. Run `demido open` again.\n").into_response(),
    }
}

/// Demido's approval tool, as an MCP server for one turn of Claude Code's (`--permission-prompt-tool`). The
/// token in the address is made for that turn and dies with it; it lets Claude Code ask the person, and
/// nothing else. A minimal server over MCP's HTTP transport: JSON answers, no stream.
async fn program_approve(State(app): State<App>, Path(token): Path<String>, Json(said): Json<Value>) -> Response {
    let Some(id) = said.get("id").cloned() else {
        // A notification (`notifications/initialized`): nothing to answer.
        return StatusCode::ACCEPTED.into_response();
    };
    let answer = |result: Value| Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response();
    match said["method"].as_str().unwrap_or_default() {
        "initialize" => answer(json!({
            "protocolVersion": said["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "demido", "version": VERSION },
        })),
        "ping" => answer(json!({})),
        "tools/list" => answer(json!({ "tools": [{
            "name": "approve",
            "description": "Asks the person, in Demido, whether a tool may be used.",
            "inputSchema": { "type": "object", "properties": { "tool_name": { "type": "string" }, "input": { "type": "object" }, "tool_use_id": { "type": "string" } }, "required": ["tool_name", "input"] },
        }] })),
        "tools/call" if said["params"]["name"] == "approve" => {
            let args = &said["params"]["arguments"];
            let (yes, why) = app.chat.program_approval(&token, args["tool_name"].as_str().unwrap_or("?"), args["input"].clone()).await;
            // Claude Code reads one text block: allow, or deny with the reason.
            let text = if yes { json!({ "behavior": "allow" }) } else { json!({ "behavior": "deny", "message": why }) };
            answer(json!({ "content": [{ "type": "text", "text": text.to_string() }] }))
        }
        other => Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Demido's approval tool does not offer {other}.") } })).into_response(),
    }
}

/// The approval tool has no stream of its own to offer.
async fn program_approve_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

// ───────── schedules ─────────

/// A sentence, to be read as a schedule.
#[derive(Deserialize, ToSchema)]
pub struct Sentence {
    /// "Give me my email report every morning at 7".
    pub text: String,
}

/// What a sentence is read as, before anything is kept ("Understood as …"): what to do, when, and for a
/// monitor what it watches for. Read by code, not by a model. Nothing is made.
#[utoipa::path(post, path = "/api/v1/schedules/understand", tag = "schedules", request_body = Sentence, responses((status = 200, body = crate::schedules::Understood)))]
async fn schedule_understand(State(app): State<App>, Json(body): Json<Sentence>) -> Json<crate::schedules::Understood> {
    Json(crate::schedules::when::understand(&body.text, now_ms(), app.scheduler.schedules().tz()))
}

/// Every schedule, the next due first.
#[utoipa::path(get, path = "/api/v1/schedules", tag = "schedules", responses((status = 200, body = Vec<crate::schedules::Schedule>)))]
async fn schedules_list(State(app): State<App>) -> Result<Json<Vec<crate::schedules::Schedule>>> {
    Ok(Json(app.scheduler.schedules().list()?))
}

/// Makes a schedule, and the session of its own its runs are in. It always runs autonomously, with every
/// tool (DECISIONS J2). Not more often than every 15 minutes, and never by a program such as Claude Code.
#[utoipa::path(post, path = "/api/v1/schedules", tag = "schedules", request_body = crate::schedules::New, responses((status = 200, body = crate::schedules::Schedule), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn schedule_new(State(app): State<App>, Json(body): Json<crate::schedules::New>) -> Result<Json<crate::schedules::Schedule>> {
    Ok(Json(app.scheduler.make(&body).await?))
}

/// A schedule, with its runs, the last first.
#[derive(Serialize, ToSchema)]
pub struct ScheduleWithRuns {
    pub schedule: crate::schedules::Schedule,
    pub runs: Vec<crate::schedules::Run>,
    /// It is running now.
    pub running: bool,
}

#[utoipa::path(get, path = "/api/v1/schedules/{id}", tag = "schedules", params(("id" = String, Path, description = "The schedule's id")), responses((status = 200, body = ScheduleWithRuns), (status = 404, body = Problem)))]
async fn schedule_get(State(app): State<App>, Path(id): Path<String>) -> Result<Json<ScheduleWithRuns>> {
    let schedule = app.scheduler.schedules().get(&id)?;
    let runs = app.scheduler.schedules().runs(&id, 50)?;
    Ok(Json(ScheduleWithRuns { running: app.scheduler.is_running(&id), schedule, runs }))
}

/// Changes what a schedule does, when, its limit, or pauses it and turns it on again. Turned on, or given a
/// new time, it is due at the next moment its rule gives.
#[utoipa::path(patch, path = "/api/v1/schedules/{id}", tag = "schedules", params(("id" = String, Path, description = "The schedule's id")), request_body = crate::schedules::Change, responses((status = 200, body = crate::schedules::Schedule), (status = 404, body = Problem), (status = 422, body = Problem)))]
async fn schedule_change(State(app): State<App>, Path(id): Path<String>, Json(body): Json<crate::schedules::Change>) -> Result<Json<crate::schedules::Schedule>> {
    let changed = app.scheduler.schedules().change(&id, &body)?;
    app.events.append("schedule.changed", Some(&id), json!({ "state": changed.state, "when": changed.when_said, "what": changed.what }))?;
    Ok(Json(changed))
}

/// Deletes a schedule and its runs. Its session stays, with everything its runs wrote in it.
#[utoipa::path(delete, path = "/api/v1/schedules/{id}", tag = "schedules", params(("id" = String, Path, description = "The schedule's id")), responses((status = 200, body = crate::schedules::Schedule), (status = 404, body = Problem), (status = 409, body = Problem)))]
async fn schedule_delete(State(app): State<App>, Path(id): Path<String>) -> Result<Json<crate::schedules::Schedule>> {
    if app.scheduler.is_running(&id) {
        return Err(Error::Busy("This schedule is running. Wait for the run to end, or stop its answer in its session.".into()));
    }
    let gone = app.scheduler.schedules().delete(&id)?;
    app.events.append("schedule.deleted", Some(&id), json!({ "what": gone.what, "session": gone.session_id }))?;
    Ok(Json(gone))
}

/// Runs a schedule now, as the person asked: its next time does not move. Returns at once; the run's end
/// is in the log (`schedule.ran`) and in the schedule's session.
#[utoipa::path(post, path = "/api/v1/schedules/{id}/run", tag = "schedules", params(("id" = String, Path, description = "The schedule's id")), responses((status = 202, body = crate::schedules::Schedule), (status = 404, body = Problem), (status = 409, body = Problem)))]
async fn schedule_run(State(app): State<App>, Path(id): Path<String>) -> Result<(StatusCode, Json<crate::schedules::Schedule>)> {
    let schedule = app.scheduler.schedules().get(&id)?;
    if app.scheduler.is_running(&id) {
        return Err(Error::Busy("This schedule is running already.".into()));
    }
    let scheduler = app.scheduler.clone();
    tokio::spawn(async move {
        if let Err(e) = scheduler.run(&id, true).await {
            tracing::warn!("A run of the schedule {id} could not be made: {e}");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(schedule)))
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
    paths(health, system, appearance, runtime_status, runtime_available, runtime_install, runtime_stop, runtime_check, runtime_remove, models_list, model_load, model_unload, model_delete, model_context, model_settings, model_settings_set, model_settings_reset, sessions_list, session_new, session_get, session_change, session_delete, session_say, session_stop, session_answer_now, session_compact, session_permission, session_undo, session_output, session_diff, session_files, session_file_new, session_file_rename, session_file_delete, session_files_deleted, session_file_put_back, session_file, session_file_save, session_file_compare, session_artifacts, session_artifact_keep, session_artifact, session_artifact_version, session_artifact_file, session_artifact_into_folder, hub_search, hub_model, hub_download, hub_downloads, hub_stop, hub_token, hub_token_set, hub_token_remove, vault_get, account_add, account_change, account_key, account_remove, providers_list, provider_models, provider_choose, provider_account, program_download, program_remove, program_stop, program_latest, program_sign_in, schedule_understand, schedules_list, schedule_new, schedule_get, schedule_change, schedule_delete, schedule_run, usage, settings_list, setting_get, setting_set, setting_reset, events_list, events_stream, link_new),
    tags((name = "server", description = "Is it there, and letting a browser in"), (name = "system", description = "This computer"), (name = "runtime", description = "The program that runs models: what is installed, what is offered, installing it"), (name = "models", description = "The models on this computer: loading and unloading them"), (name = "providers", description = "Models elsewhere: the keys Demido keeps for them, choosing them, and what they cost"), (name = "sessions", description = "Conversations with a model: what was said, saying something, stopping an answer"), (name = "schedules", description = "Work done with nobody there: schedules and monitors, each a session of its own, and their runs"), (name = "settings", description = "Every setting"), (name = "events", description = "The log of what happened"))
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
        .route("/models/{id}", axum::routing::delete(model_delete))
        .route("/models/{id}/load", post(model_load))
        .route("/models/{id}/context", axum::routing::put(model_context))
        .route("/models/{id}/settings", get(model_settings).put(model_settings_set).delete(model_settings_reset))
        .route("/hub/models", get(hub_search))
        .route("/hub/models/{owner}/{name}", get(hub_model))
        .route("/hub/downloads", get(hub_downloads).post(hub_download))
        .route("/hub/downloads/{id}", axum::routing::delete(hub_stop))
        .route("/hub/token", get(hub_token).put(hub_token_set).delete(hub_token_remove))
        .route("/models/{id}/unload", post(model_unload))
        .route("/sessions", get(sessions_list).post(session_new))
        .route("/sessions/{id}", get(session_get).patch(session_change).delete(session_delete))
        .route("/sessions/{id}/messages", post(session_say))
        .route("/sessions/{id}/stop", post(session_stop))
        .route("/sessions/{id}/answer-now", post(session_answer_now))
        .route("/sessions/{id}/compact", post(session_compact))
        .route("/sessions/{id}/permissions/{request}", post(session_permission))
        .route("/sessions/{id}/changes/{change}/undo", post(session_undo))
        .route("/sessions/{id}/changes/{change}/diff", get(session_diff))
        .route("/sessions/{id}/outputs/{output}", get(session_output))
        .route("/sessions/{id}/files", get(session_files).post(session_file_new).delete(session_file_delete))
        .route("/sessions/{id}/files/deleted", get(session_files_deleted))
        .route("/sessions/{id}/files/deleted/{deleted}/put-back", post(session_file_put_back))
        .route("/sessions/{id}/files/rename", post(session_file_rename))
        .route("/sessions/{id}/files/compare", post(session_file_compare).layer(axum::extract::DefaultBodyLimit::max(crate::folder::MOST_SAVED as usize)))
        .route("/sessions/{id}/artifacts", get(session_artifacts).post(session_artifact_keep))
        .route("/sessions/{id}/artifacts/{artifact}", get(session_artifact))
        .route("/sessions/{id}/artifacts/{artifact}/versions/{n}", get(session_artifact_version))
        .route("/sessions/{id}/artifacts/{artifact}/versions/{n}/file", get(session_artifact_file))
        .route("/sessions/{id}/artifacts/{artifact}/versions/{n}/into-folder", post(session_artifact_into_folder))
        // A saved text file may be up to 8 MB, more than the 2 MB a request may carry elsewhere.
        .route("/sessions/{id}/files/content", get(session_file).put(session_file_save).layer(axum::extract::DefaultBodyLimit::max(crate::folder::MOST_SAVED as usize)))
        .route("/vault", get(vault_get))
        .route("/providers", get(providers_list))
        .route("/providers/{id}/accounts", post(account_add))
        .route("/providers/{id}/accounts/{account}", axum::routing::patch(account_change).delete(account_remove))
        .route("/providers/{id}/accounts/{account}/key", axum::routing::put(account_key))
        .route("/providers/{id}/accounts/{account}/sign-in", post(program_sign_in))
        .route("/providers/{id}/models", get(provider_models))
        .route("/providers/{id}/chosen", axum::routing::put(provider_choose))
        .route("/providers/{id}/account", get(provider_account))
        .route("/providers/{id}/program", post(program_download).delete(program_remove))
        .route("/providers/{id}/program/download", axum::routing::delete(program_stop))
        .route("/providers/{id}/program/latest", get(program_latest))
        .route("/schedules", get(schedules_list).post(schedule_new))
        .route("/schedules/understand", post(schedule_understand))
        .route("/schedules/{id}", get(schedule_get).patch(schedule_change).delete(schedule_delete))
        .route("/schedules/{id}/run", post(schedule_run))
        .route("/usage", get(usage))
        .route("/settings", get(settings_list))
        .route("/settings/{id}", get(setting_get).put(setting_set).delete(setting_reset))
        .route("/events", get(events_list))
        .route("/events/stream", get(events_stream))
        .route("/session/link", post(link_new));
    Router::new()
        .nest("/api/v1", api)
        .route("/enter", get(enter))
        .route("/page-frame", get(|| async { crate::ui::page_frame() }))
        .route("/programs/approve/{token}", post(program_approve).get(program_approve_stream))
        .fallback(ui_file)
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
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
    // Where a program on this computer reaches Demido's approval tool.
    let reach_at = if local.ip().is_unspecified() { std::net::SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), local.port()) } else { local };
    core.chat.set_address(format!("http://{reach_at}"));
    let ui = Ui::find(opts.ui_dir);
    let app = App {
        settings: core.settings,
        events: core.events.clone(),
        auth: core.auth,
        runtimes: core.runtimes,
        engine: core.engine.clone(),
        sessions: core.sessions.clone(),
        chat: core.chat.clone(),
        hub: core.hub.clone(),
        providers: core.providers.clone(),
        scheduler: core.scheduler.clone(),
        advice: Arc::default(),
        system: Arc::new(Kept::new(SYSTEM_KEEPS)),
        appearance: Arc::new(Kept::new(APPEARANCE_KEEPS)),
        ui: ui.clone(),
        started: Instant::now(),
        hosts: Arc::new(hosts_for(&local)),
        deleted_in: opts.paths.home().join("deleted"),
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
    // So with a run of a schedule: it failed, and its schedule is due again by its rule.
    core.scheduler.schedules().close_unfinished()?;
    // Every few seconds: run the schedules that are due, one at a time (DECISIONS J). One missed while
    // Demido was not running is due now, and runs once, late.
    let schedules_kept = {
        let scheduler = core.scheduler.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(crate::schedules::runner::LOOK_EVERY);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                scheduler.look(now_ms()).await;
            }
        })
    };
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
        schedules_kept.abort();
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
    use crate::auth::COOKIE;
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
        let (router, auth, events, _) = app_with_sessions(home);
        (router, auth, events)
    }

    /// The same, with its sessions: for what only a model makes, such as an artifact.
    fn app_with_sessions(home: &std::path::Path) -> (Router, Arc<Auth>, Arc<EventLog>, Arc<Sessions>) {
        let db = Arc::new(Db::memory().unwrap());
        let events = Arc::new(EventLog::new(db.clone()));
        let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
        let providers = Arc::new(Providers::new(db.clone(), Arc::new(Vault::in_file(home)), settings.clone(), events.clone()));
        let sessions = Arc::new(Sessions::new(db.clone()));
        let auth = Arc::new(Auth::with_key(KEY.into()));
        let runtimes = Arc::new(Runtimes::new(home, events.clone(), Source::default()).unwrap());
        let engine = Arc::new(Engine::new(home, runtimes.clone(), settings.clone(), events.clone(), Arc::new(ProcessLauncher)));
        let chat = Arc::new(Chat::new(sessions.clone(), engine.clone(), events.clone(), settings.clone()).with_providers(providers.clone()));
        let schedules = Arc::new(crate::schedules::Schedules::new(db));
        let scheduler = Arc::new(crate::schedules::Scheduler::new(schedules, sessions.clone(), chat.clone(), events.clone()));
        let app = App {
            settings: settings.clone(),
            events: events.clone(),
            auth: auth.clone(),
            runtimes,
            engine: engine.clone(),
            sessions: sessions.clone(),
            chat,
            hub: Arc::new(crate::hub::Hub::new(home.join("models"), events.clone())),
            providers,
            scheduler,
            advice: Arc::default(),
            system: Arc::new(Kept::new(SYSTEM_KEEPS)),
            appearance: Arc::new(Kept::new(APPEARANCE_KEEPS)),
            ui: Ui::default(),
            started: Instant::now(),
            hosts: Arc::new(vec!["127.0.0.1:7341".into()]),
            deleted_in: home.join("deleted"),
        };
        let sessions = app.sessions.clone();
        (router(app), auth, events, sessions)
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
    async fn a_sessions_folder_is_listed_and_its_files_sent_never_as_a_page() {
        let home = std::env::temp_dir().join(format!("demido-api-folder-{}", uuid::Uuid::new_v4()));
        let work = home.join("work");
        std::fs::create_dir_all(home.join("models")).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(home.join("models/tiny.gguf"), b"not really a model").unwrap();
        std::fs::write(work.join("page.html"), "<script>alert(1)</script>").unwrap();
        std::fs::write(work.join("clip.mp4"), (0u8..100).collect::<Vec<u8>>()).unwrap();
        std::fs::write(home.join("secret.txt"), "not in the folder").unwrap();
        let (r, _, _) = app_at(&home);
        let key = |b: axum::http::request::Builder| b.header("authorization", format!("Bearer {KEY}")).header("content-type", "application/json");
        let (_, made) = send(&r, key(request("POST", "/api/v1/sessions")).body(Body::from("{}")).unwrap()).await;
        let id = made["id"].as_str().unwrap().to_string();
        // No folder yet: nothing to list.
        assert_eq!(send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/files"))).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
        let change = json!({ "folder": work.display().to_string() }).to_string();
        send(&r, key(request("PATCH", &format!("/api/v1/sessions/{id}"))).body(Body::from(change)).unwrap()).await;
        let (status, listed) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/files"))).body(Body::empty()).unwrap()).await;
        assert_eq!((status, listed["entries"].as_array().map(Vec::len)), (StatusCode::OK, Some(2)), "{listed}");
        // A page comes as text, and with the walls that keep it from being one.
        let res = r.clone().oneshot(key(request("GET", &format!("/api/v1/sessions/{id}/files/content?path=page.html"))).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let h = |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        assert_eq!((h("content-type").as_str(), h("x-content-type-options").as_str()), ("text/plain; charset=utf-8", "nosniff"));
        assert!(h("content-security-policy").starts_with("sandbox"), "{}", h("content-security-policy"));
        // Part of a video, as a browser asks for it to move through it.
        let res = r.clone().oneshot(key(request("GET", &format!("/api/v1/sessions/{id}/files/content?path=clip.mp4"))).header("range", "bytes=10-19").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers().get("content-range").unwrap(), "bytes 10-19/100");
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert_eq!(bytes.as_ref(), (10u8..20).collect::<Vec<u8>>().as_slice());
        // Nothing outside the folder.
        let (status, _) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/files/content?path=../secret.txt"))).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        std::fs::remove_dir_all(home).ok();
    }

    #[tokio::test]
    async fn a_text_file_is_saved_from_the_window_only_over_the_version_it_opened() {
        let home = crate::testing::TempDir::new("demido-api-save");
        let work = home.join("work");
        std::fs::create_dir_all(home.join("models")).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(home.join("models/tiny.gguf"), b"not really a model").unwrap();
        std::fs::write(work.join("notes.md"), "# Notes\n").unwrap();
        std::fs::write(work.join("photo.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let (r, _, _) = app_at(&home);
        let key = |b: axum::http::request::Builder| b.header("authorization", format!("Bearer {KEY}"));
        let (_, made) = send(&r, key(request("POST", "/api/v1/sessions")).header("content-type", "application/json").body(Body::from("{}")).unwrap()).await;
        let id = made["id"].as_str().unwrap().to_string();
        let change = json!({ "folder": work.display().to_string() }).to_string();
        send(&r, key(request("PATCH", &format!("/api/v1/sessions/{id}"))).header("content-type", "application/json").body(Body::from(change)).unwrap()).await;
        let at = |path: &str| format!("/api/v1/sessions/{id}/files/content?path={path}");
        let put = |path: &str, version: Option<&str>, text: &'static str| {
            let b = key(request("PUT", &at(path))).header("content-type", "text/plain; charset=utf-8");
            let b = match version {
                Some(v) => b.header("if-match", v),
                None => b,
            };
            b.body(Body::from(text)).unwrap()
        };

        // Opened: it comes with its version.
        let res = r.clone().oneshot(key(request("GET", &at("notes.md"))).body(Body::empty()).unwrap()).await.unwrap();
        let opened = res.headers().get("etag").unwrap().to_str().unwrap().to_string();
        assert_eq!(opened, crate::folder::version_of(b"# Notes\n"));
        assert!(res.headers().get("last-modified").unwrap().to_str().unwrap().ends_with(" GMT"));
        // A picture has none: it is not edited here.
        let res = r.clone().oneshot(key(request("GET", &at("photo.png"))).body(Body::empty()).unwrap()).await.unwrap();
        assert!(res.headers().get("etag").is_none());

        // Saved over the version opened.
        let (status, saved) = send(&r, put("notes.md", Some(&opened), "# Notes\n\nMore.\n")).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(std::fs::read_to_string(work.join("notes.md")).unwrap(), "# Notes\n\nMore.\n");
        assert_ne!(saved["version"], json!(opened));
        // The model writes it meanwhile; a save over the old version is refused, and nothing is lost.
        std::fs::write(work.join("notes.md"), "# The model's notes\n").unwrap();
        let (status, said) = send(&r, put("notes.md", saved["version"].as_str(), "# Mine\n")).await;
        assert_eq!((status, said["error"]["code"].as_str()), (StatusCode::PRECONDITION_FAILED, Some("changed")));
        assert_eq!(std::fs::read_to_string(work.join("notes.md")).unwrap(), "# The model's notes\n");
        // The person says to keep theirs.
        assert_eq!(send(&r, put("notes.md", Some("*"), "# Mine\n")).await.0, StatusCode::OK);
        assert_eq!(std::fs::read_to_string(work.join("notes.md")).unwrap(), "# Mine\n");
        // The person makes a file, and renames it; never over one that is there.
        let json_post = |path: &str, body: Value| key(request("POST", path)).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
        let (status, made) = send(&r, json_post(&format!("/api/v1/sessions/{id}/files"), json!({ "path": "drafts/plan.md" }))).await;
        assert_eq!((status, made["path"].as_str()), (StatusCode::OK, Some("drafts/plan.md")), "{made}");
        assert_eq!(send(&r, json_post(&format!("/api/v1/sessions/{id}/files"), json!({ "path": "drafts/plan.md" }))).await.0, StatusCode::CONFLICT);
        let (status, moved) = send(&r, json_post(&format!("/api/v1/sessions/{id}/files/rename"), json!({ "from": "drafts/plan.md", "to": "plan.md" }))).await;
        assert_eq!((status, moved["path"].as_str()), (StatusCode::OK, Some("plan.md")));
        assert_eq!(send(&r, json_post(&format!("/api/v1/sessions/{id}/files/rename"), json!({ "from": "plan.md", "to": "notes.md" }))).await.0, StatusCode::CONFLICT);
        // Deleted: kept, listed, and put back where it was.
        let (status, gone) = send(&r, key(request("DELETE", &format!("/api/v1/sessions/{id}/files?path=plan.md"))).body(Body::empty()).unwrap()).await;
        assert_eq!((status, gone["path"].as_str()), (StatusCode::OK, Some("plan.md")), "{gone}");
        assert!(!work.join("plan.md").exists());
        let (_, kept) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/files/deleted"))).body(Body::empty()).unwrap()).await;
        assert_eq!(kept[0]["id"], gone["id"]);
        let (status, back) = send(&r, json_post(&format!("/api/v1/sessions/{id}/files/deleted/{}/put-back", gone["id"].as_str().unwrap()), json!({}))).await;
        assert_eq!((status, back["path"].as_str()), (StatusCode::OK, Some("plan.md")));
        assert!(work.join("plan.md").exists());
        // Before choosing, the person can see what is different between the file now and their edit.
        let compare = key(request("POST", &format!("/api/v1/sessions/{id}/files/compare?path=notes.md"))).header("content-type", "text/plain").body(Body::from("# Mine\n\nMore.\n")).unwrap();
        let (status, compared) = send(&r, compare).await;
        assert_eq!((status, compared["stat"].as_str()), (StatusCode::OK, Some("+2 −0")), "{compared}");
        // A save that names no version is refused: it could overwrite what it never saw.
        assert_eq!(send(&r, put("notes.md", None, "# Blind\n")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        // Only text, only inside.
        assert_eq!(send(&r, put("photo.png", Some("*"), "x")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(send(&r, put("../models/tiny.gguf", Some("*"), "x")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(std::fs::read(home.join("models/tiny.gguf")).unwrap(), b"not really a model");
        // More than 2 MB is taken: the most for any other request.
        let big: &'static str = Box::leak("a".repeat(3_000_000).into_boxed_str());
        assert_eq!(send(&r, put("notes.md", Some("*"), big)).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_sessions_artifacts_are_listed_with_their_versions_and_downloaded_under_their_name() {
        let home = crate::testing::TempDir::new("demido-api-artifacts");
        std::fs::create_dir_all(home.join("models")).unwrap();
        std::fs::write(home.join("models/tiny.gguf"), b"not really a model").unwrap();
        let (r, _, _, sessions) = app_with_sessions(&home);
        let key = |b: axum::http::request::Builder| b.header("authorization", format!("Bearer {KEY}"));
        let (_, made) = send(&r, key(request("POST", "/api/v1/sessions")).header("content-type", "application/json").body(Body::from("{}")).unwrap()).await;
        let (_, other) = send(&r, key(request("POST", "/api/v1/sessions")).header("content-type", "application/json").body(Body::from("{}")).unwrap()).await;
        let (id, other) = (made["id"].as_str().unwrap().to_string(), other["id"].as_str().unwrap().to_string());
        let first = sessions.make_artifact(&id, "Café menu.html", "<script>alert(1)</script><h1>Menu</h1>", None).unwrap();
        sessions.make_artifact(&id, "Café menu.html", "<h1>Menu</h1><p>Coffee</p>", None).unwrap();
        let a = first.artifact.id;

        let (status, listed) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/artifacts"))).body(Body::empty()).unwrap()).await;
        assert_eq!((status, listed[0]["name"].as_str(), listed[0]["versions"].as_i64(), listed[0]["shown_as"].as_str()), (StatusCode::OK, Some("Café menu.html"), Some(2), Some("html")));
        let (_, one) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/artifacts/{a}"))).body(Body::empty()).unwrap()).await;
        assert_eq!(one["versions"].as_array().map(Vec::len), Some(2));
        let (_, v1) = send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/artifacts/{a}/versions/1"))).body(Body::empty()).unwrap()).await;
        assert_eq!(v1["text"], json!("<script>alert(1)</script><h1>Menu</h1>"));

        // Downloaded under its name, and never a page with Demido's address.
        let res = r.clone().oneshot(key(request("GET", &format!("/api/v1/sessions/{id}/artifacts/{a}/versions/1/file"))).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let h = |name: &str| res.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        assert_eq!(h("content-disposition"), "attachment; filename=\"Caf_ menu.html\"; filename*=UTF-8''Caf%C3%A9%20menu.html");
        assert_eq!((h("content-type").as_str(), h("x-content-type-options").as_str()), ("text/plain; charset=utf-8", "nosniff"));
        assert!(h("content-security-policy").starts_with("sandbox"));

        // The person keeps part of an answer as a file: the same as a model's, and a new version by its name.
        let keep = |name: &str, text: &str| {
            key(request("POST", &format!("/api/v1/sessions/{id}/artifacts"))).header("content-type", "application/json").body(Body::from(json!({ "name": name, "text": text }).to_string())).unwrap()
        };
        let (status, kept) = send(&r, keep("Café menu.html", "<h1>Menu</h1><p>Tea</p>")).await;
        assert_eq!((status, kept["n"].as_i64(), kept["artifact"]["id"].as_str()), (StatusCode::OK, Some(3), Some(a.as_str())));
        assert_eq!(send(&r, keep("../out.md", "x")).await.0, StatusCode::UNPROCESSABLE_ENTITY);

        // Put into the session's folder: not without a folder; a file that is there only when the person says so.
        let into = |body: Value| {
            key(request("POST", &format!("/api/v1/sessions/{id}/artifacts/{a}/versions/2/into-folder"))).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap()
        };
        assert_eq!(send(&r, into(json!({}))).await.0, StatusCode::NOT_FOUND, "the session has no folder yet");
        let work = home.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let change = json!({ "folder": work.display().to_string() }).to_string();
        send(&r, key(request("PATCH", &format!("/api/v1/sessions/{id}"))).header("content-type", "application/json").body(Body::from(change)).unwrap()).await;
        let (status, put) = send(&r, into(json!({}))).await;
        assert_eq!((status, put["path"].as_str(), put["shown_as"].as_str()), (StatusCode::OK, Some("Café menu.html"), Some("html")), "{put}");
        assert_eq!(std::fs::read_to_string(work.join("Café menu.html")).unwrap(), "<h1>Menu</h1><p>Coffee</p>");
        assert_eq!(send(&r, into(json!({}))).await.0, StatusCode::CONFLICT, "there now: not replaced unasked");
        assert_eq!(send(&r, into(json!({ "replace": true }))).await.0, StatusCode::OK);
        assert_eq!(send(&r, into(json!({ "path": "../out.html" }))).await.0, StatusCode::UNPROCESSABLE_ENTITY);

        // Only through its own session; a version that is not there is not found.
        assert_eq!(send(&r, key(request("GET", &format!("/api/v1/sessions/{other}/artifacts/{a}"))).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
        assert_eq!(send(&r, key(request("GET", &format!("/api/v1/sessions/{id}/artifacts/{a}/versions/4"))).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
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
        let told = detail["instructions"].as_str().unwrap();
        assert!(told.starts_with("Today's date: ") && told.ends_with(crate::chat::INSTRUCTIONS), "the day, and how to answer: {told}");
        assert!(detail["tools_told"].as_str().unwrap().contains("- none(): No tool is needed."), "what the model is shown of the tools is not hidden either");
        assert_eq!(detail["tasks"], json!([]));
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
    async fn a_sentence_is_read_as_a_schedule_before_anything_is_made() {
        let (r, _) = app();
        let key = format!("Bearer {KEY}");
        let post = |path: &str, body: Value| request("POST", path).header("authorization", &key).header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
        let (status, read) = send(&r, post("/api/v1/schedules/understand", json!({ "text": "Give me my email report every morning at 7" }))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            (read["what"].as_str(), read["when"]["kind"].as_str(), read["when"]["at"].as_str(), read["when_said"].as_str()),
            (Some("Give me my email report"), Some("daily"), Some("07:00"), Some("Every day at 07:00"))
        );
        let (_, list) = send(&r, request("GET", "/api/v1/schedules").header("authorization", &key).body(Body::empty()).unwrap()).await;
        assert_eq!(list, json!([]), "reading it made nothing");
        // Too often is refused, before any session is made.
        let (status, body) = send(&r, post("/api/v1/schedules", json!({ "said": "x", "what": "Check", "when": { "kind": "every", "minutes": 5 } }))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(body["error"]["message"].as_str().unwrap().contains("at most every 15 minutes"));
        let (status, _) = send(&r, request("GET", "/api/v1/schedules/nope").header("authorization", &key).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(&r, post("/api/v1/schedules/nope/run", json!({}))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
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
    async fn keys_go_in_with_their_accounts_and_never_come_out() {
        let home = std::env::temp_dir().join(format!("demido-api-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        let (r, _, events) = app_at(&home);
        let key = |b: axum::http::request::Builder| b.header("authorization", format!("Bearer {KEY}")).header("content-type", "application/json");
        let add = |label: &str, secret: &str| key(request("POST", "/api/v1/providers/openrouter/accounts")).body(Body::from(json!({ "label": label, "key": secret }).to_string())).unwrap();
        let (status, work) = send(&r, add("Work", "sk-or-v1-test-not-a-real-key-7f3a")).await;
        assert_eq!((status, work["key"]["ends_with"].as_str(), work["key"]["kept_in"].as_str(), work["is_default"].as_bool()), (StatusCode::CREATED, Some("7f3a"), Some("file"), Some(true)));
        let (status, mine) = send(&r, add("Personal", "sk-or-v1-test-not-a-real-key-9b2c")).await;
        assert_eq!((status, mine["is_default"].as_bool()), (StatusCode::CREATED, Some(false)));
        // Two accounts cannot have one name.
        assert_eq!(send(&r, add("work", "sk-or-v1-test-not-a-real-key-0000")).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        // Nowhere the API answers, and nowhere in the log, is a value.
        for path in ["/api/v1/vault", "/api/v1/providers", "/api/v1/events?after=0"] {
            let (status, said) = send(&r, key(request("GET", path)).body(Body::empty()).unwrap()).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(!said.to_string().contains("not-a-real-key"), "{path}: {said}");
        }
        let (_, providers) = send(&r, key(request("GET", "/api/v1/providers")).body(Body::empty()).unwrap()).await;
        let openrouter = providers.as_array().unwrap().iter().find(|p| p["id"] == "openrouter").unwrap();
        let labels: Vec<&str> = openrouter["accounts"].as_array().unwrap().iter().map(|a| a["label"].as_str().unwrap()).collect();
        assert_eq!((labels, openrouter["spent_usd"].as_f64()), (vec!["Work", "Personal"], Some(0.0)));
        // The second becomes the default; the first is deleted, key and all.
        let mine_at = format!("/api/v1/providers/openrouter/accounts/{}", mine["id"].as_str().unwrap());
        let (status, changed) = send(&r, key(request("PATCH", &mine_at)).body(Body::from(r#"{"default":true}"#)).unwrap()).await;
        assert_eq!((status, changed["is_default"].as_bool()), (StatusCode::OK, Some(true)));
        let work_at = format!("/api/v1/providers/openrouter/accounts/{}", work["id"].as_str().unwrap());
        assert_eq!(send(&r, key(request("DELETE", &work_at)).body(Body::empty()).unwrap()).await.0, StatusCode::OK);
        let (_, vault) = send(&r, key(request("GET", "/api/v1/vault")).body(Body::empty()).unwrap()).await;
        assert_eq!(vault["keys"].as_array().unwrap().iter().map(|k| k["ends_with"].as_str().unwrap()).collect::<Vec<_>>(), vec!["9b2c"]);
        // A provider Demido does not know of has no accounts.
        let (status, _) = send(&r, key(request("POST", "/api/v1/providers/bank/accounts")).body(Body::from(r#"{"key":"0123456789abcdef"}"#)).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let kinds: Vec<String> = events.after(0, 50).unwrap().into_iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&"provider.account.added".to_string()) && kinds.contains(&"provider.account.removed".to_string()), "{kinds:?}");
        // A model elsewhere that was not chosen cannot be given to a session.
        let (status, said) = send(&r, key(request("POST", "/api/v1/sessions")).body(Body::from(r#"{"model":"anthropic/claude-opus-5-5"}"#)).unwrap()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{said}");
        assert!(said["error"]["message"].as_str().unwrap().contains("not chosen"), "{said}");
        std::fs::remove_dir_all(home).ok();
    }

    #[tokio::test]
    async fn the_approval_tool_speaks_mcp_and_a_token_of_no_turn_is_refused() {
        let (r, _) = app();
        let call = |body: Value| request("POST", "/programs/approve/no-such-turn").header("content-type", "application/json").body(Body::from(body.to_string())).unwrap();
        // No key is needed (the token is the key), but the host must be this server.
        let (status, said) = send(&r, call(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }))).await;
        assert_eq!((status, said["result"]["protocolVersion"].as_str(), said["result"]["serverInfo"]["name"].as_str()), (StatusCode::OK, Some("2025-06-18"), Some("demido")));
        let (status, _) = send(&r, call(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let (_, said) = send(&r, call(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))).await;
        assert_eq!(said["result"]["tools"][0]["name"], "approve");
        let (_, said) =
            send(&r, call(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "approve", "arguments": { "tool_name": "Bash", "input": { "command": "rm -rf /" } } } })))
                .await;
        let answer: Value = serde_json::from_str(said["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(answer["behavior"], "deny");
        let foreign =
            axum::http::Request::builder().method("POST").uri("/programs/approve/x").header("host", "evil.example").header("content-type", "application/json").body(Body::from("{}")).unwrap();
        assert_eq!(send(&r, foreign).await.0, StatusCode::FORBIDDEN);
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
