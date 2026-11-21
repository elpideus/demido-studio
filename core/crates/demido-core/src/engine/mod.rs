//! Running models.
//!
//! Demido does not keep one process per model itself. It runs one `llama-server` as a router and steers it
//! (DECISIONS K22, research note 12). The router keeps a process per loaded model, measures what each needs,
//! and makes room by unloading the least recently used. Demido decides the rest:
//!
//! - **When.** Nothing loads unless Demido asked (the router runs with autoload off). A model that has not
//!   been used for the time set in "Idle unload" is unloaded. A router with nothing loaded is stopped.
//! - **How much.** Demido always pins the context, because left alone a model takes all the memory it can
//!   (a 610 MB model took 5.3 GB in the trial). Demido always sets a memory margin ("Memory kept free"),
//!   which is also what makes the router measure. Demido checks that a model can fit before asking, because
//!   the router loads one that cannot fit anyway.
//! - **Who may talk to it.** Only Demido: the router listens on this computer only, on a port picked for
//!   the run, with a key made for the run. Its web UI and its own tools are off.
//!
//! The router is started the first time a model is needed, not when Demido starts.
//!
//! Not built yet: asking a loaded model anything (the chat loop), choosing the context per model, lending a
//! loaded model to agents, a model with several files being downloaded by Demido. Only tried on Linux with
//! an NVIDIA card.

mod known;
mod router;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use utoipa::ToSchema;

pub use router::{Answer, Launch, Launcher, Piece, ProcessLauncher, Running};

use crate::events::EventLog;
use crate::runtime::Runtimes;
use crate::settings::Settings;
use crate::{Error, Result, now_ms};

/// A router with nothing loaded is stopped after this long.
const ROUTER_IDLES_FOR: Duration = Duration::from_secs(300);
/// How long a model may take to load before Demido gives up on it. Large models on a slow disk take minutes.
const LOAD_MAY_TAKE: Duration = Duration::from_secs(900);

/// A model on this computer.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct LocalModel {
    /// Its name: the file's name without `.gguf`, or the folder's name for a model of several files.
    pub id: String,
    /// The file or folder, inside the models folder.
    pub file: String,
    /// Bytes on disk.
    pub size: u64,
    /// `unloaded`, `loading` or `loaded`.
    pub state: String,
    /// The most the model itself can hold in mind, in tokens. Known once the runtime has looked at it.
    pub context_length: Option<u64>,
    /// Memory it needs on the graphics card at the context Demido sets, in MiB, as the runtime measured it.
    pub needs_mb: Option<u64>,
    /// Seconds since it was last used. Only while loaded.
    pub idle_for_s: Option<u64>,
    /// Seconds until it is unloaded for being idle. `null` when idle unload is off, or it is not loaded.
    pub unloads_in_s: Option<u64>,
    /// How long it took to load the last time, in milliseconds. `null` until it has been loaded once.
    pub loads_in_ms: Option<u64>,
    /// How fast it writes on this computer, in tokens a second: a running figure from its real answers.
    /// `null` until it has answered something.
    pub tokens_per_second: Option<f64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct EngineStatus {
    /// Whether the runtime is running now. It is started when a model is first needed.
    pub running: bool,
    /// The runtime in use, for example `llama.cpp · CUDA 13.5 (b12359)`. `null` when none is installed.
    pub runtime: Option<String>,
    /// The context every model is loaded with, in tokens.
    pub context: u64,
    /// Graphics memory kept free, in MiB.
    pub margin_mb: u64,
    /// Minutes without use after which a model is unloaded. `null` means never.
    pub idle_unload_min: Option<u64>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ModelList {
    /// Where models are kept on this computer.
    pub folder: String,
    pub models: Vec<LocalModel>,
    pub engine: EngineStatus,
}

struct Live {
    router: router::Router,
    /// When each model was last used.
    used: HashMap<String, Instant>,
    /// How many answers each model is writing right now. A model that is writing is never unloaded for being idle.
    answering: HashMap<String, usize>,
    /// Since when nothing has been loaded.
    empty_since: Option<Instant>,
    /// Reads the router's news and passes it on. Ends with the router.
    news: tokio::task::JoinHandle<()>,
}

pub struct Engine {
    models_dir: PathBuf,
    work: PathBuf,
    runtimes: Arc<Runtimes>,
    settings: Arc<Settings>,
    events: Arc<EventLog>,
    launcher: Arc<dyn Launcher>,
    /// What was learned about each model, kept on disk.
    known: known::Known,
    live: Mutex<Option<Live>>,
    /// How long a router with nothing loaded is kept. Shortened in tests.
    router_idles_for: Duration,
    /// Tests only: stands in for the "Idle unload" setting, whose shortest choice is a minute.
    idle_for_tests: Option<Duration>,
}

impl Engine {
    /// `home` is Demido's data folder.
    pub fn new(home: &Path, runtimes: Arc<Runtimes>, settings: Arc<Settings>, events: Arc<EventLog>, launcher: Arc<dyn Launcher>) -> Self {
        let work = home.join("engine");
        let known = known::Known::open(work.join("known.json"));
        Self { models_dir: home.join("models"), work, runtimes, settings, events, launcher, known, live: Mutex::new(None), router_idles_for: ROUTER_IDLES_FOR, idle_for_tests: None }
    }

    /// Ends a runtime that an earlier Demido left running (it was killed, or crashed): it would hold
    /// graphics memory for nothing. Only the server calls this, once, when it starts; the command line
    /// must not, or it could end the runtime of a server that is running.
    pub fn end_leftover(&self) {
        router::end_leftover(&self.work);
    }

    /// Tests only: a model counts as idle after this long, where the setting's shortest choice is a minute.
    #[cfg(test)]
    pub(crate) fn idle_after(mut self, wait: Duration) -> Self {
        self.idle_for_tests = Some(wait);
        self
    }

    #[cfg(test)]
    fn stop_empty_router_after(mut self, wait: Duration) -> Self {
        self.router_idles_for = wait;
        self
    }

    fn context(&self) -> u64 {
        self.settings.value::<String>("models.context").ok().and_then(|v| v.parse().ok()).unwrap_or(8192)
    }

    fn margin_mb(&self) -> u64 {
        (self.settings.value::<f64>("models.reserve").unwrap_or(0.9) * 1024.0).round() as u64
    }

    /// `None` means "never".
    fn idle_unload(&self) -> Option<Duration> {
        if self.idle_for_tests.is_some() {
            return self.idle_for_tests;
        }
        self.settings.value::<String>("models.idleUnload").ok().and_then(|v| v.parse::<u64>().ok()).map(|minutes| Duration::from_secs(minutes * 60))
    }

    /// Which file a model's facts are about.
    fn identity(&self, m: &LocalModel) -> known::Identity {
        known::Identity::of(&self.models_dir.join(&m.file), m.size)
    }

    /// The runtime in use, by its id. Empty when none is installed.
    fn runtime_id(&self) -> String {
        self.runtimes.installed().into_iter().find(|r| r.version.is_some()).map(|r| r.id).unwrap_or_default()
    }

    fn status_with(&self, running: bool) -> EngineStatus {
        let runtime = self.runtimes.installed().into_iter().find(|r| r.version.is_some()).map(|r| format!("{} ({})", r.name, r.build));
        EngineStatus { running, runtime, context: self.context(), margin_mb: self.margin_mb(), idle_unload_min: self.idle_unload().map(|d| d.as_secs() / 60) }
    }

    /// Every model in the models folder, as the folder shows it: nothing is asked of the runtime.
    fn on_disk(&self) -> Vec<LocalModel> {
        let is_model = |p: &Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gguf")) && p.is_file();
        let Ok(entries) = std::fs::read_dir(&self.models_dir) else { return Vec::new() };
        let mut found: Vec<LocalModel> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let (id, size) = if is_model(&path) {
                    (path.file_stem()?.to_string_lossy().into_owned(), std::fs::metadata(&path).ok()?.len())
                } else if path.is_dir() {
                    // A model of several files (a vision part, or shards) is a folder.
                    let parts: Vec<u64> = std::fs::read_dir(&path).ok()?.filter_map(|f| f.ok()).filter(|f| is_model(&f.path())).filter_map(|f| f.metadata().ok().map(|m| m.len())).collect();
                    if parts.is_empty() {
                        return None;
                    }
                    (name.clone(), parts.iter().sum())
                } else {
                    return None;
                };
                Some(LocalModel {
                    id,
                    file: name,
                    size,
                    state: "unloaded".into(),
                    context_length: None,
                    needs_mb: None,
                    idle_for_s: None,
                    unloads_in_s: None,
                    loads_in_ms: None,
                    tokens_per_second: None,
                })
            })
            .collect();
        found.sort_by_key(|m| m.id.to_lowercase());
        found
    }

    /// The models on this computer and what each is doing. With the runtime running, it adds what the
    /// runtime knows (state, what each needs). It never starts the runtime: looking is free.
    pub async fn list(&self) -> Result<ModelList> {
        let mut models = self.on_disk();
        let live = self.live.lock().await;
        if let Some(live) = &*live
            && let Ok(known) = live.router.models(true).await
        {
            let idle = self.idle_unload();
            for m in &mut models {
                let Some(seen) = known.iter().find(|k| k.id == m.id) else { continue };
                m.state = seen.state.clone();
                m.context_length = seen.context_length;
                m.needs_mb = seen.needs_mb;
                if m.state == "loaded" {
                    let since = live.used.get(&m.id).map(|t| t.elapsed()).unwrap_or_default();
                    m.idle_for_s = Some(since.as_secs());
                    m.unloads_in_s = idle.map(|limit| limit.saturating_sub(since).as_secs());
                }
            }
        }
        let (context, runtime) = (self.context(), self.runtime_id());
        for m in &mut models {
            let file = self.identity(m);
            // What the runtime just said is written down.
            if m.context_length.is_some() || m.needs_mb.is_some() {
                self.known.learn(&m.id, file, m.context_length, m.needs_mb.map(|needs| (needs, context, runtime.as_str())), None);
            }
            // What it did not say, because it is not running or has not measured yet, comes from what was kept.
            if let Some(facts) = self.known.about(&m.id, file) {
                m.context_length = m.context_length.or(facts.context_length);
                m.needs_mb = m.needs_mb.or(facts.needs_mb(context, &runtime));
                m.loads_in_ms = facts.load_ms;
                m.tokens_per_second = facts.per_second;
            }
        }
        Ok(ModelList { folder: self.models_dir.display().to_string(), models, engine: self.status_with(live.is_some()) })
    }

    /// Starts the runtime if it is not running. Called with the lock held.
    async fn ensure(&self, live: &mut Option<Live>) -> Result<()> {
        if let Some(l) = live {
            if l.router.alive() {
                return Ok(());
            }
            self.gone(live, true);
        }
        let Some(runtime) = self.runtimes.installed().into_iter().find(|r| r.version.is_some()) else {
            return Err(Error::Invalid(if self.runtimes.installed().is_empty() {
                "No model runtime is installed, so no model can be loaded. Install one on the Setup screen, or with: demido runtime install".into()
            } else {
                "A model runtime is installed, but it does not start on this computer. `demido runtime` says why.".into()
            }));
        };
        std::fs::create_dir_all(&self.models_dir)?;
        let root = self.runtimes.folder().join(&runtime.id);
        let (context, margin) = (self.context(), self.margin_mb());
        let router = router::Router::start(&*self.launcher, &self.work, &root, &runtime, &self.models_dir, context, margin).await?;
        self.events.append("engine.started", Some(&runtime.id), json!({ "runtime": format!("{} ({})", runtime.name, runtime.build), "context": context, "margin_mb": margin }))?;
        let news = tokio::spawn(router::pass_on_news(router.address(), router.key(), self.events.clone()));
        *live = Some(Live { router, used: HashMap::new(), answering: HashMap::new(), empty_since: Some(Instant::now()), news });
        Ok(())
    }

    /// Lets go of the runtime: stops it if it is still there, and says so in the log. `unexpected` is true
    /// when it ended by itself. Stopping takes a second or two, so it happens off the request threads;
    /// the handle is for a caller that needs to wait until it is really gone.
    fn gone(&self, live: &mut Option<Live>, unexpected: bool) -> Option<tokio::task::JoinHandle<()>> {
        let l = live.take()?;
        l.news.abort();
        let _ = self.events.append("engine.stopped", None, json!({ "unexpected": unexpected }));
        let router = l.router;
        Some(tokio::task::spawn_blocking(move || router.stop()))
    }

    /// Loads a model and waits until it is ready. Starts the runtime if needed.
    pub async fn load(&self, id: &str) -> Result<LocalModel> {
        let Some(on_disk) = self.on_disk().into_iter().find(|m| m.id == id) else {
            return Err(Error::NotFound(format!("There is no model called \"{id}\" in {}. `demido models` lists what is there.", self.models_dir.display())));
        };
        let mut live = self.live.lock().await;
        self.ensure(&mut live).await?;
        let seen = live.as_ref().expect("just made sure").router.models(true).await?.into_iter().find(|m| m.id == id);
        let Some(seen) = seen else {
            return Err(Error::Other(format!("The runtime does not list \"{id}\", though the file is there. It may not be a model it can read.")));
        };
        if seen.state != "loaded" {
            // The runtime loads a model that cannot fit anyway, and it then fails or crawls. Say no first.
            if let (Some(needs), Some(total)) = (seen.needs_mb, seen.device_total_mb)
                && needs + self.margin_mb() > total
            {
                return Err(Error::Invalid(format!(
                    "\"{id}\" needs {needs} MiB on the graphics card at a context of {}. The card has {total} MiB, and {} MiB are kept free. It cannot fit. A smaller context, a smaller version of the model, or less memory kept free would make room.",
                    self.context(),
                    self.margin_mb()
                )));
            }
            live.as_ref().expect("still there").router.load(id).await?;
            let started = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let l = live.as_mut().expect("still there");
                if !l.router.alive() {
                    self.gone(&mut live, true);
                    return Err(Error::Other(format!("The runtime stopped while loading \"{id}\". `demido events` shows what it said.")));
                }
                match l.router.models(false).await?.into_iter().find(|m| m.id == id).map(|m| m.state) {
                    Some(state) if state == "loaded" => break,
                    Some(state) if state == "loading" && started.elapsed() < LOAD_MAY_TAKE => {}
                    Some(state) if state == "loading" => {
                        return Err(Error::Other(format!("\"{id}\" was still loading after {} minutes. It was left alone; unload it to stop it.", LOAD_MAY_TAKE.as_secs() / 60)));
                    }
                    // Loading, then not loaded: its process ended.
                    _ if started.elapsed() > Duration::from_millis(600) => {
                        return Err(Error::Other(format!("\"{id}\" did not load: its process ended. The usual reasons are too little memory, or a file the runtime cannot read.")));
                    }
                    _ => {}
                }
            }
            // How long it took is worth knowing the next time someone weighs loading it.
            self.known.learn(id, self.identity(&on_disk), None, None, Some(started.elapsed().as_millis() as u64));
        }
        let l = live.as_mut().expect("still there");
        l.used.insert(id.to_string(), Instant::now());
        l.empty_since = None;
        drop(live);
        self.list().await?.models.into_iter().find(|m| m.id == id).ok_or_else(|| Error::Other(format!("\"{id}\" was loaded and then could not be found.")))
    }

    /// Unloads a model: its process ends and its memory is free again. `why` goes into the log.
    pub async fn unload(&self, id: &str, why: &str) -> Result<()> {
        let mut live = self.live.lock().await;
        let Some(l) = live.as_mut() else {
            return Err(Error::Invalid(format!("\"{id}\" is not loaded: nothing is.")));
        };
        if !l.router.models(false).await?.iter().any(|m| m.id == id && m.state != "unloaded") {
            return Err(Error::Invalid(format!("\"{id}\" is not loaded.")));
        }
        l.router.unload(id).await?;
        l.used.remove(id);
        self.events.tell("model.unloading", Some(id), json!({ "why": why }));
        Ok(())
    }

    /// Asks a loaded model and passes its answer on piece by piece as it is written.
    ///
    /// `body` is the request as the runtime takes it (`model`, `messages` and the rest). The model must be
    /// loaded already: this never loads one, so that nothing loads unless someone asked for it by name.
    /// Setting `stop` ends the answer early; what was written until then is returned.
    pub async fn answer(&self, model: &str, body: Value, stop: &std::sync::atomic::AtomicBool, heard: &mut (dyn FnMut(Piece) + Send)) -> Result<Answer> {
        let (address, key) = {
            let mut live = self.live.lock().await;
            let Some(l) = live.as_mut() else {
                return Err(Error::Invalid(format!("\"{model}\" is not loaded: nothing is.")));
            };
            *l.answering.entry(model.to_string()).or_default() += 1;
            l.used.insert(model.to_string(), Instant::now());
            l.empty_since = None;
            (l.router.address(), l.router.key())
        };
        // The lock is not held while the model writes: looking at the list, or loading another model, must not wait for it.
        let outcome = router::stream_answer(&address, &key, body, stop, heard).await;
        if let Some(l) = self.live.lock().await.as_mut() {
            if let Some(n) = l.answering.get_mut(model) {
                *n = n.saturating_sub(1);
            }
            l.used.insert(model.to_string(), Instant::now());
        }
        // A real answer says how fast this model is on this computer. Short ones are too noisy to count.
        if let Ok(answer) = &outcome
            && answer.timings.written >= 16
            && let Some(on_disk) = self.on_disk().into_iter().find(|m| m.id == model)
        {
            self.known.learn_speed(model, self.identity(&on_disk), answer.timings.per_second);
        }
        outcome
    }

    /// Says that a model was just used, so it is not unloaded for being idle.
    pub async fn used(&self, id: &str) {
        if let Some(l) = self.live.lock().await.as_mut() {
            l.used.insert(id.to_string(), Instant::now());
        }
    }

    /// Looks after what is running. Called every few seconds while the server runs:
    /// unloads models nobody used for the set time, stops a runtime with nothing loaded, and notices one that ended by itself.
    pub async fn look_after(&self) {
        let mut live = self.live.lock().await;
        let Some(l) = live.as_mut() else { return };
        if !l.router.alive() {
            self.gone(&mut live, true);
            return;
        }
        let Ok(models) = l.router.models(false).await else { return };
        let idle = self.idle_unload();
        let mut busy = 0;
        for m in models.iter().filter(|m| m.state != "unloaded") {
            if l.answering.get(&m.id).is_some_and(|n| *n > 0) {
                // Writing an answer is use, however long the answer takes.
                l.used.insert(m.id.clone(), Instant::now());
            }
            let since = l.used.entry(m.id.clone()).or_insert_with(Instant::now).elapsed();
            if m.state == "loaded" && idle.is_some_and(|limit| since >= limit) {
                if l.router.unload(&m.id).await.is_ok() {
                    l.used.remove(&m.id);
                    self.events.tell("model.unloading", Some(&m.id), json!({ "why": "idle" }));
                }
            } else {
                busy += 1;
            }
        }
        if busy > 0 {
            l.empty_since = None;
        } else if l.empty_since.get_or_insert_with(Instant::now).elapsed() >= self.router_idles_for {
            self.gone(&mut live, false);
        }
    }

    /// Stops everything. Called when the server stops.
    pub async fn shutdown(&self) {
        let mut live = self.live.lock().await;
        if let Some(l) = live.as_mut() {
            // From here on the log is written here, not from the router's news: it is about to go quiet.
            l.news.abort();
            // Unload first, so that each model's process ends on its own terms, whatever the system does
            // with the children of a program that is stopped.
            if let Ok(models) = l.router.models(false).await {
                for m in models.iter().filter(|m| m.state != "unloaded") {
                    if l.router.unload(&m.id).await.is_ok() {
                        let _ = self.events.append("model.unloaded", Some(&m.id), json!({ "crashed": false, "why": "the server stopped" }));
                    }
                }
            }
        }
        if let Some(stopped) = self.gone(&mut live, false) {
            let _ = stopped.await;
        }
    }
}

/// What the router said about a change to one model, turned into Demido's own news.
/// Loading progress is told and not kept; "loaded" and "unloaded" are kept in the log.
pub(crate) fn news_from(events: &EventLog, said: &Value) {
    let (Some(model), Some(kind)) = (said["model"].as_str(), said["event"].as_str()) else { return };
    let data = &said["data"];
    match (kind, data["status"].as_str()) {
        ("status_change", Some("loading")) => events.tell("model.loading", Some(model), json!({ "progress": data["progress"]["value"], "stage": data["progress"]["current"] })),
        ("status_change", Some("loaded")) => {
            let _ = events.append("model.loaded", Some(model), json!({ "at": now_ms() }));
        }
        ("status_change", Some("unloaded")) => {
            // The runtime ends a model's process with code 0 when it was asked to. Anything else is the process dying.
            let crashed = data["exit_code"].as_i64().is_some_and(|code| code != 0);
            let _ = events.append("model.unloaded", Some(model), json!({ "crashed": crashed, "exit_code": data["exit_code"] }));
        }
        ("model_memory", Some("measured")) => events.tell("model.measured", Some(model), json!({})),
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod stand_in;
#[cfg(test)]
mod tests;
