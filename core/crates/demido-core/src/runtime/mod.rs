//! The model runtime: finding it, downloading it, keeping it.
//!
//! Demido does not ship the program that runs models. It downloads llama.cpp's own build for this computer
//! from llama.cpp's own releases, checks it against the SHA-256 its publisher lists, unpacks it without
//! trusting the archive, and tries that it starts.
//!
//! The parts:
//!
//! - [`catalog`]  what is published, and which of it suits this computer
//! - `fetch`      one download: it can stop and go on, and it is checked
//! - `unpack`     opening an archive without letting it write outside its folder
//!
//! On disk, under Demido's data folder:
//!
//! ```text
//! runtimes/llama.cpp/
//!   llama.cpp-b12359-cuda-13.5/     one installed runtime
//!     demido-runtime.json           what it is, where it came from, whether it starts
//!     …                             the files as published
//!   downloads/                      unfinished downloads (`*.part`), kept so a new try goes on
//! ```
//!
//! The folder is the truth: an installed runtime is a folder with its note in it. Nothing about it is in
//! the database, so deleting the folder removes it and copying the folder moves it.
//!
//! Not built yet: starting it with a model, and stopping it when idle. Checking a download against the
//! publisher's signed build statement (the SHA-256 that is checked comes from the same place as the file).

pub mod catalog;
mod fetch;
mod unpack;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::system::Advice;
use crate::{Error, Result, VERSION, now_ms};

pub const RUNTIME: &str = "llama.cpp";
const NOTE: &str = "demido-runtime.json";
/// How long an answer from the publisher is used without asking again. After that it is asked "only if
/// changed", which is one small question (see `catalog`).
const ANSWER_KEEPS_MS: i64 = 3_600_000;
/// The publisher's last answer, kept beside the runtimes so that a restart does not ask again.
const KEPT_ANSWER: &str = "published.json";

/// Where the runtime's files come from. One fixed place in the real program; a test names a stand-in.
#[derive(Clone, Debug)]
pub struct Source {
    /// Where the list of releases is asked for.
    pub api: String,
    /// Whose releases.
    pub repo: String,
    /// Every file must come from an address that starts like this. Anything else is refused, whatever the list says.
    pub downloads: String,
}

impl Default for Source {
    fn default() -> Self {
        Self { api: "https://api.github.com".into(), repo: "ggml-org/llama.cpp".into(), downloads: "https://github.com/ggml-org/llama.cpp/releases/download/".into() }
    }
}

impl Source {
    fn allows(&self, address: &str) -> Result<()> {
        if address.starts_with(&self.downloads) {
            return Ok(());
        }
        Err(Error::Net(format!("The release list names a file at \"{address}\". That is not where this runtime's files live ({}), so nothing was downloaded.", self.downloads)))
    }
}

/// A file as its publisher lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RemoteFile {
    pub name: String,
    pub url: String,
    /// In bytes.
    pub size: u64,
    /// As the publisher lists it. `null` when it lists none: the download is then not checked, and says so.
    pub sha256: Option<String>,
}

/// One build of the runtime that suits this computer.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Offer {
    /// For example `cuda-13.5`, `vulkan`, `cpu`.
    pub id: String,
    pub name: String,
    /// Why it is, or is not, the one to take.
    pub why: String,
    pub recommended: bool,
    pub file: RemoteFile,
    /// Files it may also need. For a CUDA build: NVIDIA's libraries, unless the system already has them.
    pub extra: Vec<RemoteFile>,
    /// Bytes to download for `file`.
    pub size: u64,
    /// Bytes to download for `extra`.
    pub extra_size: u64,
    /// True when `extra` should be taken: this system does not seem to have those libraries itself.
    pub needs_extra: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Available {
    pub runtime: String,
    /// The publisher's stable release, for example `v1.0.0`.
    pub release: String,
    /// The build that release stands for, for example `b12359`.
    pub build: String,
    /// What suits this computer, the recommended one first.
    pub offers: Vec<Offer>,
    /// When the publisher was last asked, in milliseconds since 1970. Within an hour of it, nothing is asked.
    pub asked_at: i64,
    /// `null` normally. When the publisher could not be reached, this says why, and the rest is its last
    /// answer, as old as `asked_at` says.
    pub stale: Option<String>,
}

/// A runtime on this computer.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Installed {
    /// For example `llama.cpp-b12359-cuda-13.5`. Also the name of its folder.
    pub id: String,
    pub runtime: String,
    pub build: String,
    /// For example `cuda-13.5`.
    pub backend: String,
    pub name: String,
    /// What was downloaded, with the checksums that were checked.
    pub files: Vec<RemoteFile>,
    /// True when every file matched a checksum its publisher listed.
    pub checked: bool,
    /// The program that serves models, inside the folder. `null` if the archive did not hold one.
    pub server: Option<String>,
    /// Folders inside it that hold libraries the program needs when it starts.
    pub lib_dirs: Vec<String>,
    /// What the program said when asked for its version. `null` if it did not start.
    pub version: Option<String>,
    /// Why it does not start, in the program's own words. `null` when it does.
    pub problem: Option<String>,
    /// What it can run models on, as it lists them itself: `CUDA0: NVIDIA GeForce RTX 3060 (11908 MiB)`.
    /// Empty for a build that runs on the processor.
    #[serde(default)]
    pub devices: Vec<String>,
    /// Set when it starts but something is not as it should be. For example: a build for a graphics card
    /// that sees no card, so models would run on the processor.
    #[serde(default)]
    pub caution: Option<String>,
    pub installed_at: i64,
    /// Bytes on disk.
    pub size: u64,
}

/// An installation under way.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Progress {
    pub id: String,
    pub name: String,
    /// `looking`, `downloading`, `unpacking` or `trying`.
    pub step: String,
    /// The file being downloaded.
    pub file: Option<String>,
    /// Bytes downloaded so far, over all files.
    pub done: u64,
    /// Bytes to download in all. 0 while not known.
    pub total: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Status {
    pub runtime: String,
    /// Where runtimes are kept on this computer.
    pub folder: String,
    pub installed: Vec<Installed>,
    pub installing: Option<Progress>,
}

#[derive(Default)]
struct State {
    installing: Option<Progress>,
    stop: Option<Arc<AtomicBool>>,
    /// The publisher's last answer. Read from disk the first time it is needed.
    published: Option<catalog::Published>,
    looked_on_disk: bool,
}

pub struct Runtimes {
    dir: PathBuf,
    source: Source,
    http: reqwest::Client,
    events: Arc<EventLog>,
    state: Mutex<State>,
}

impl Runtimes {
    /// `home` is Demido's data folder.
    pub fn new(home: &Path, events: Arc<EventLog>, source: Source) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("demido-studio/{VERSION}"))
            .connect_timeout(Duration::from_secs(15))
            // Between two pieces of an answer, not for the whole download.
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| Error::Other(format!("The part of Demido that downloads could not start: {e}")))?;
        Ok(Self { dir: home.join("runtimes").join(RUNTIME), source, http, events, state: Mutex::default() })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where runtimes are kept. An installed runtime is the folder in here named after its id.
    pub fn folder(&self) -> &Path {
        &self.dir
    }

    /// Every runtime on this computer, the newest first. A folder without a readable note is not one.
    pub fn installed(&self) -> Vec<Installed> {
        let Ok(folders) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut found: Vec<Installed> = folders
            .filter_map(|f| f.ok())
            .filter_map(|f| {
                let text = std::fs::read(f.path().join(NOTE)).ok()?;
                let note: Installed = serde_json::from_slice(&text).ok()?;
                // The note is believed only for the folder it sits in.
                (f.file_name().to_str() == Some(note.id.as_str())).then_some(note)
            })
            .collect();
        found.sort_by(|a, b| b.installed_at.cmp(&a.installed_at).then_with(|| a.id.cmp(&b.id)));
        found
    }

    pub fn status(&self) -> Status {
        Status { runtime: RUNTIME.into(), folder: self.dir.display().to_string(), installed: self.installed(), installing: self.state().installing.clone() }
    }

    /// The publisher's last answer, from memory, or from disk the first time.
    fn known(&self) -> Option<catalog::Published> {
        let mut state = self.state();
        if !state.looked_on_disk {
            state.looked_on_disk = true;
            state.published = std::fs::read(self.dir.join(KEPT_ANSWER)).ok().and_then(|text| serde_json::from_slice(&text).ok());
        }
        state.published.clone()
    }

    fn keep(&self, answer: &catalog::Published) {
        self.state().published = Some(answer.clone());
        // Written beside its final place and moved there, so a crash never leaves half a file.
        let saved = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&self.dir)?;
            let beside = self.dir.join(format!("{KEPT_ANSWER}.new"));
            std::fs::write(&beside, serde_json::to_vec(answer).unwrap_or_default())?;
            std::fs::rename(beside, self.dir.join(KEPT_ANSWER))
        })();
        if let Err(e) = saved {
            tracing::warn!("The publisher's answer could not be kept on disk, so it will be asked again after a restart: {e}");
        }
    }

    /// What the publisher offers for this computer.
    ///
    /// Within an hour of the last answer nothing is asked: the answer is kept, on disk too. After that,
    /// or with `fresh`, the publisher is asked whether anything changed, which is usually one small question.
    /// If it cannot be reached and there is an earlier answer, that answer is given, marked `stale`.
    pub async fn available(&self, advice: &Advice, fresh: bool) -> Result<Available> {
        let known = self.known();
        let young = known.as_ref().is_some_and(|k| (0..ANSWER_KEEPS_MS).contains(&(now_ms() - k.asked_at)));
        let (published, stale) = match known {
            Some(known) if young && !fresh => (known, None),
            known => match catalog::read(&self.http, &self.source, known.as_ref()).await {
                Ok(answer) => {
                    self.keep(&answer);
                    (answer, None)
                }
                // Better the last answer, saying how old it is, than none.
                Err(Error::Net(why)) if known.is_some() => (known.expect("checked"), Some(why)),
                Err(e) => return Err(e),
            },
        };
        let mut offers = catalog::offers(advice, &published.build, &published.files);
        for offer in offers.iter_mut().filter(|o| !o.extra.is_empty()) {
            // "cuda-13.5" needs NVIDIA's libraries of generation 13.
            let generation = offer.id.strip_prefix("cuda-").and_then(|v| v.split('.').next()).unwrap_or("").to_string();
            offer.needs_extra = !tokio::task::spawn_blocking(move || cuda_libraries_here(&generation)).await.unwrap_or(false);
        }
        Ok(Available { runtime: RUNTIME.into(), release: published.release, build: published.build, offers, asked_at: published.asked_at, stale })
    }

    fn tell(&self, progress: Progress) {
        self.events.tell("runtime.install.progress", Some(&progress.id), serde_json::to_value(&progress).unwrap_or_default());
        self.state().installing = Some(progress);
    }

    /// Downloads, checks, unpacks and tries one build. `backend` is an offer's id; `with_extra` also takes
    /// the files the offer lists as extra. Takes as long as the download takes. One at a time.
    pub async fn install(&self, advice: &Advice, backend: &str, with_extra: bool) -> Result<Installed> {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut state = self.state();
            if let Some(busy) = &state.installing {
                return Err(Error::Busy(format!("{} is being installed. Wait for it, or stop it first.", busy.name)));
            }
            state.installing = Some(Progress { id: backend.into(), name: format!("{RUNTIME} · {backend}"), step: "looking".into(), file: None, done: 0, total: 0 });
            state.stop = Some(stop.clone());
        }
        let mut id = backend.to_string();
        let outcome = self.install_steps(advice, backend, with_extra, &stop, &mut id).await;
        {
            let mut state = self.state();
            state.installing = None;
            state.stop = None;
        }
        let logged = match &outcome {
            Ok(done) => self.events.append(
                "runtime.installed",
                Some(&done.id),
                json!({ "name": done.name, "build": done.build, "version": done.version, "problem": done.problem, "checked": done.checked, "bytes": done.size }),
            ),
            Err(Error::Stopped(why)) => self.events.append("runtime.install.stopped", Some(&id), json!({ "message": why })),
            Err(why) => self.events.append("runtime.install.failed", Some(&id), json!({ "message": why.to_string() })),
        };
        if let Err(e) = logged {
            tracing::error!("The log could not be written: {e}");
        }
        outcome
    }

    async fn install_steps(&self, advice: &Advice, backend: &str, with_extra: bool, stop: &Arc<AtomicBool>, id: &mut String) -> Result<Installed> {
        let available = self.available(advice, false).await?;
        let Some(offer) = available.offers.iter().find(|o| o.id == backend).cloned() else {
            let offered = available.offers.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().join(", ");
            return Err(Error::NotFound(format!("There is no \"{backend}\" build of {RUNTIME} {} for this computer. Offered: {offered}.", available.build)));
        };
        *id = format!("{RUNTIME}-{}-{}", available.build, offer.id);
        let id = id.clone();
        let mut files = vec![offer.file.clone()];
        if with_extra {
            files.extend(offer.extra.iter().cloned());
        }
        for f in &files {
            self.source.allows(&f.url)?;
            // A file name becomes a name on disk. Only plain ones.
            if f.name.is_empty() || f.name.starts_with('.') || !f.name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
                return Err(Error::Net(format!("The release list names a file \"{}\", a name Demido does not accept. Nothing was downloaded.", f.name)));
            }
        }
        let total: u64 = files.iter().map(|f| f.size).sum();
        self.events.append(
            "runtime.install.started",
            Some(&id),
            json!({ "name": offer.name, "release": available.release, "build": available.build, "files": files.iter().map(|f| &f.name).collect::<Vec<_>>(), "bytes": total }),
        )?;

        // 1. Download, and check each file against what its publisher lists.
        let downloads = self.dir.join("downloads");
        tokio::fs::create_dir_all(&downloads).await?;
        let (mut before, mut checked) = (0u64, true);
        for f in &files {
            let mut last: Option<Instant> = None;
            let heard = |done: u64| {
                // Often enough to see it move, not so often that it is all the window does.
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(200)) {
                    last = Some(Instant::now());
                    self.tell(Progress { id: id.clone(), name: offer.name.clone(), step: "downloading".into(), file: Some(f.name.clone()), done: before + done, total });
                }
            };
            checked &= fetch::download(&self.http, f, &downloads.join(&f.name), stop, heard).await?;
            before += f.size;
        }

        // 2. Unpack beside the others, under a name that says it is not finished.
        self.tell(Progress { id: id.clone(), name: offer.name.clone(), step: "unpacking".into(), file: None, done: total, total });
        let partial = self.dir.join(format!("{id}.partial"));
        let archives: Vec<PathBuf> = files.iter().map(|f| downloads.join(&f.name)).collect();
        let unpacked = {
            let (partial, stop, archives) = (partial.clone(), stop.clone(), archives.clone());
            tokio::task::spawn_blocking(move || -> Result<(Option<String>, Vec<String>, u64)> {
                std::fs::remove_dir_all(&partial).ok();
                let mut bytes = unpack::unpack(&archives[0], &partial, &stop)?;
                // Extra libraries go beside the program: that is where it looks first.
                let server = find_server(&partial);
                let beside = server.as_ref().and_then(|s| partial.join(s).parent().map(Path::to_path_buf)).unwrap_or_else(|| partial.clone());
                for extra in &archives[1..] {
                    bytes += unpack::unpack(extra, &beside, &stop)?;
                }
                Ok((server, lib_dirs(&partial), bytes))
            })
            .await
            .map_err(|e| Error::Other(e.to_string()))?
        };
        let (server, lib_dirs, size) = match unpacked {
            Ok(found) => found,
            Err(e) => {
                tokio::fs::remove_dir_all(&partial).await.ok();
                // An archive that will not unpack is not worth keeping for a second try.
                if !matches!(e, Error::Stopped(_)) {
                    for a in &archives {
                        tokio::fs::remove_file(a).await.ok();
                    }
                }
                return Err(e);
            }
        };

        // 3. Try it: a program that does not start on this system should be known now, not at the first question.
        self.tell(Progress { id: id.clone(), name: offer.name.clone(), step: "trying".into(), file: None, done: total, total });
        let tried = {
            let (root, server, dirs, backend) = (partial.clone(), server.clone(), lib_dirs.clone(), offer.id.clone());
            tokio::task::spawn_blocking(move || try_it(&root, server.as_deref(), &dirs, &backend)).await.map_err(|e| Error::Other(e.to_string()))?
        };

        // 4. Write the note, give the folder its real name, and drop the archives.
        let done = Installed {
            id: id.clone(),
            runtime: RUNTIME.into(),
            build: available.build.clone(),
            backend: offer.id.clone(),
            name: offer.name.clone(),
            files,
            checked,
            server,
            lib_dirs,
            version: tried.version,
            problem: tried.problem,
            devices: tried.devices,
            caution: tried.caution,
            installed_at: now_ms(),
            size,
        };
        let place = self.dir.join(&id);
        let written: Result<()> = (|| {
            std::fs::write(partial.join(NOTE), serde_json::to_vec_pretty(&done)?)?;
            if place.exists() {
                std::fs::remove_dir_all(&place)?;
            }
            Ok(std::fs::rename(&partial, &place)?)
        })();
        if let Err(e) = written {
            std::fs::remove_dir_all(&partial).ok();
            return Err(e);
        }
        for a in &archives {
            tokio::fs::remove_file(a).await.ok();
        }
        Ok(done)
    }

    /// Stops the installation under way. What was downloaded is kept, so a new try goes on from there.
    /// Returns false when nothing was being installed.
    pub fn stop(&self) -> bool {
        match &self.state().stop {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Tries an installed runtime again and writes down what it says now. Nothing is downloaded.
    /// For after a driver was updated, or when the first try did not look at everything it looks at today.
    pub fn check(&self, id: &str) -> Result<Installed> {
        let Some(mut found) = self.installed().into_iter().find(|i| i.id == id) else {
            return Err(Error::NotFound(format!("No runtime called \"{id}\" is installed. `demido runtime` lists what is.")));
        };
        let root = self.dir.join(&found.id);
        let tried = try_it(&root, found.server.as_deref(), &found.lib_dirs, &found.backend);
        let changed = (&found.version, &found.problem, &found.devices, &found.caution) != (&tried.version, &tried.problem, &tried.devices, &tried.caution);
        (found.version, found.problem, found.devices, found.caution) = (tried.version, tried.problem, tried.devices, tried.caution);
        if changed {
            std::fs::write(root.join(NOTE), serde_json::to_vec_pretty(&found)?)?;
            self.events.append("runtime.checked", Some(&found.id), json!({ "version": found.version, "problem": found.problem, "devices": found.devices, "caution": found.caution }))?;
        }
        Ok(found)
    }

    /// Deletes an installed runtime from this computer.
    pub fn remove(&self, id: &str) -> Result<Installed> {
        // Only something that is listed can be removed: the id never becomes a path by itself.
        let Some(found) = self.installed().into_iter().find(|i| i.id == id) else {
            return Err(Error::NotFound(format!("No runtime called \"{id}\" is installed. `demido runtime` lists what is.")));
        };
        std::fs::remove_dir_all(self.dir.join(&found.id))?;
        self.events.append("runtime.removed", Some(&found.id), json!({ "name": found.name, "bytes": found.size }))?;
        Ok(found)
    }
}

/// Whether this system has NVIDIA's CUDA libraries of one generation ("13") where programs find them.
/// A guess from names, good enough to choose a default: the real answer is whether the program starts.
fn cuda_libraries_here(generation: &str) -> bool {
    if generation.is_empty() || !generation.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if cfg!(target_os = "linux") {
        let Ok(out) = std::process::Command::new("ldconfig").arg("-p").output() else { return false };
        let known = String::from_utf8_lossy(&out.stdout);
        ["libcudart.so.", "libcublas.so."].iter().all(|lib| known.contains(&format!("{lib}{generation} ")))
    } else if cfg!(windows) {
        let name = format!("cudart64_{generation}.dll");
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(&name).is_file()))
    } else {
        false
    }
}

const SERVER_NAMES: [&str; 2] = ["llama-server", "llama-server.exe"];

/// Every folder under `root`, `root` first, a few levels deep. Releases have put the programs at the top, in `bin/` and in `build/bin/`.
fn folders(root: &Path) -> Vec<PathBuf> {
    let mut all = vec![root.to_path_buf()];
    let mut at = 0;
    while at < all.len() {
        let here = all[at].clone();
        at += 1;
        if here.strip_prefix(root).map(|p| p.components().count()).unwrap_or(0) >= 5 {
            continue;
        }
        if let Ok(list) = std::fs::read_dir(&here) {
            let mut inner: Vec<PathBuf> = list.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
            inner.sort();
            all.extend(inner);
        }
    }
    all
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The serving program, as a path inside `root`.
fn find_server(root: &Path) -> Option<String> {
    folders(root).iter().find_map(|dir| SERVER_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file()).map(|p| relative(root, &p)))
}

/// Folders inside `root` that hold shared libraries.
fn lib_dirs(root: &Path) -> Vec<String> {
    let is_lib = |name: &str| name.ends_with(".so") || name.contains(".so.") || name.ends_with(".dylib") || name.to_ascii_lowercase().ends_with(".dll");
    folders(root).iter().filter(|dir| std::fs::read_dir(dir).is_ok_and(|list| list.filter_map(|e| e.ok()).any(|e| e.file_name().to_str().is_some_and(is_lib)))).map(|dir| relative(root, dir)).collect()
}

/// The command that starts the serving program of a runtime in `root`, with the libraries it ships in reach.
pub fn command(root: &Path, server: &str, lib_dirs: &[String]) -> std::process::Command {
    let program = root.join(server);
    let mut cmd = std::process::Command::new(&program);
    if let Some(home) = program.parent() {
        cmd.current_dir(home);
    }
    let name = if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let mut places: Vec<PathBuf> = lib_dirs.iter().map(|d| root.join(d)).collect();
    if let Some(already) = std::env::var_os(name) {
        places.extend(std::env::split_paths(&already));
    }
    if let Ok(joined) = std::env::join_paths(places) {
        cmd.env(name, joined);
    }
    cmd
}

/// What a try found out.
struct Tried {
    version: Option<String>,
    problem: Option<String>,
    devices: Vec<String>,
    caution: Option<String>,
}

/// Runs the program with one argument and waits for it to end. `Ok` is whether it ended well, and all it said.
fn run_briefly(root: &Path, server: &str, lib_dirs: &[String], argument: &str, seconds: u64) -> std::result::Result<(bool, String), String> {
    use std::process::Stdio;
    let mut child = command(root, server, lib_dirs).arg(argument).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("It could not be started: {e}"))?;
    let until = Instant::now() + Duration::from_secs(seconds);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!("It was started and did not answer within {seconds} seconds."));
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("It started, but what it said could not be read: {e}"))?;
    Ok((out.status.success(), format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))
}

/// The lines under "Available devices:" in what the program says when asked to list them, without the
/// memory that happened to be free at that moment: that number is old the second after it is written down.
fn devices_in(said: &str) -> Vec<String> {
    let lasting = |line: &str| -> String {
        let line: String = line.chars().take(200).collect();
        match (line.strip_suffix(" free)"), line.rfind(", ")) {
            (Some(_), Some(comma)) if line[..comma].contains('(') => format!("{})", &line[..comma]),
            _ => line,
        }
    };
    said.lines().skip_while(|l| !l.to_ascii_lowercase().contains("available devices")).skip(1).map(str::trim).take_while(|l| !l.is_empty()).map(lasting).collect()
}

/// Tries an unpacked runtime: does it start, what does it call itself, and what can it run models on?
/// A program that does not start on this system should be known now, not at the first question to a model.
fn try_it(root: &Path, server: Option<&str>, lib_dirs: &[String], backend: &str) -> Tried {
    let nothing = |problem: String| Tried { version: None, problem: Some(problem), devices: Vec::new(), caution: None };
    let Some(server) = server else {
        return nothing(format!("The download holds no llama-server program. {RUNTIME}'s releases may have changed their layout."));
    };
    let short = |l: &str| l.chars().take(200).collect::<String>();
    // A version is said at once. A program still silent after twenty seconds is not going to say it.
    let version = match run_briefly(root, server, lib_dirs, "--version", 20) {
        Err(why) => return nothing(why),
        Ok((well, said)) => {
            let lines: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            if !well {
                let words = lines.iter().take(3).map(|l| short(l)).collect::<Vec<_>>().join(" / ");
                return nothing(if words.is_empty() { "It stopped at once, without a word.".into() } else { words });
            }
            lines.iter().find(|l| l.to_ascii_lowercase().contains("version")).or(lines.first()).map(|l| short(l)).unwrap_or_else(|| "It started and said nothing.".into())
        }
    };
    // Listing devices loads the part that talks to the graphics card. If a library that part needs is
    // missing, this is where it shows: the program still starts, and sees no card.
    let devices = run_briefly(root, server, lib_dirs, "--list-devices", 30).map(|(_, said)| devices_in(&said)).unwrap_or_default();
    let caution = (devices.is_empty() && backend != "cpu")
        .then(|| "It starts, but it sees no graphics card to run models on, so they would run on the processor only. A library it needs may be missing.".to_string());
    Tried { version: Some(version), problem: None, devices, caution }
}

#[cfg(test)]
mod tests;
