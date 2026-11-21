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
use crate::db::Db;
use crate::desktop::Appearance;
use crate::events::{Event, EventLog};
use crate::paths::Paths;
use crate::runtime::{Available, Installed, Progress, Runtimes, Source, Status};
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
}

impl Core {
    pub fn open(paths: &Paths) -> Result<Self> {
        paths.ensure()?;
        let db = Arc::new(Db::open(&paths.db())?);
        let events = Arc::new(EventLog::new(db.clone()));
        let settings = Arc::new(Settings::load(db, events.clone())?);
        let auth = Arc::new(Auth::open(&paths.key())?);
        let runtimes = Arc::new(Runtimes::new(paths.home(), events.clone(), runtime_source())?);
        Ok(Self { settings, events, auth, runtimes })
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
    /// Which runtime build suits this computer. Worked out once: the hardware does not change while the server runs.
    pub advice: Arc<tokio::sync::OnceCell<Advice>>,
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
    next.run(req).await
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

/// What this computer has, and which model runtime suits it. Read fresh on every call.
#[utoipa::path(get, path = "/api/v1/system", tag = "system", responses((status = 200, body = SystemInfo), (status = 401, body = Problem)))]
async fn system() -> Json<SystemInfo> {
    // Asking the driver can take a moment; keep it off the threads that answer requests.
    Json(tokio::task::spawn_blocking(crate::system::detect).await.expect("reading the hardware does not panic"))
}

/// What the desktop of this computer looks like: its accent colour, when it has one and says so.
/// Only means something to a window on the same computer as the core.
#[utoipa::path(get, path = "/api/v1/system/appearance", tag = "system", responses((status = 200, body = Appearance), (status = 401, body = Problem)))]
async fn appearance() -> Json<Appearance> {
    Json(tokio::task::spawn_blocking(crate::desktop::read).await.expect("asking the desktop does not panic"))
}

// ───────── the model runtime ─────────

impl App {
    async fn advice(&self) -> &Advice {
        self.advice.get_or_init(|| async { tokio::task::spawn_blocking(|| crate::system::detect().runtime).await.expect("reading the hardware does not panic") }).await
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

/// Deletes an installed runtime from this computer.
#[utoipa::path(delete, path = "/api/v1/runtime/installed/{id}", tag = "runtime", params(("id" = String, Path, description = "For example llama.cpp-b12359-cuda-13.5")), responses((status = 200, body = Installed), (status = 404, body = Problem)))]
async fn runtime_remove(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Installed>> {
    let runtimes = app.runtimes.clone();
    Ok(Json(tokio::task::spawn_blocking(move || runtimes.remove(&id)).await.expect("deleting a folder does not panic")?))
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
        .chain(BroadcastStream::new(live).filter_map(|e| e.ok()).filter(move |e| e.seq == 0 || e.seq > upto))
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
    app.ui.serve(req.uri().path()).await
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Demido Studio", description = "One API for every client: the window, a browser, the command line, your own programs. Send the key as `Authorization: Bearer <key>`. An error is always `{ error: { code, message } }`."),
    paths(health, system, appearance, runtime_status, runtime_available, runtime_install, runtime_stop, runtime_remove, settings_list, setting_get, setting_set, setting_reset, events_list, events_stream, link_new),
    tags((name = "server", description = "Is it there, and letting a browser in"), (name = "system", description = "This computer"), (name = "runtime", description = "The program that runs models: what is installed, what is offered, installing it"), (name = "settings", description = "Every setting"), (name = "events", description = "The log of what happened"))
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
        advice: Arc::default(),
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

    let served = axum::serve(listener, router(app)).with_graceful_shutdown(stop_signal()).await;
    std::fs::remove_file(opts.paths.running()).ok();
    core.events.append("server.stopped", None, json!({}))?;
    tracing::info!("Stopped.");
    Ok(served?)
}

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
        let db = Arc::new(Db::memory().unwrap());
        let events = Arc::new(EventLog::new(db.clone()));
        let settings = Arc::new(Settings::load(db, events.clone()).unwrap());
        let auth = Arc::new(Auth::with_key(KEY.into()));
        // A folder that does not exist: these tests install nothing.
        let runtimes = Arc::new(Runtimes::new(&std::env::temp_dir().join("demido-api-tests-no-runtimes"), events.clone(), Source::default()).unwrap());
        let app = App { settings, events, auth: auth.clone(), runtimes, advice: Arc::default(), ui: Ui::default(), started: Instant::now(), hosts: Arc::new(vec!["127.0.0.1:7341".into()]) };
        (router(app), auth)
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
