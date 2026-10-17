//! The model download queue: in the order the person arranges it, as many at a time as they allow
//! (one by default, so the file they are waiting for gets the whole connection), sharing one speed
//! limit, resumable, and verified against Hugging Face's SHA-256 before it appears as a model.
//!
//! The queue is saved to `downloads.json` whenever a job changes state, and read back at start:
//! what was downloading or waiting carries on (each file resumes from its `.part`), what was
//! paused or failed stays so, and finished jobs are not kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use demido_fetch::{CancellationToken, DownloadRequest, Downloader, FetchError, RateLimit};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::hf::HfProjector;

/// One job changed: its progress or its state.
pub const CHANGED_EVENT: &str = "downloads://changed";
/// The queue changed as a whole (a job added, moved or removed): the full list, in order.
pub const LIST_EVENT: &str = "downloads://list";

/// Most downloads that may run at once.
pub const MAX_CONCURRENCY: u8 = 3;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Downloading,
    /// Every byte is here and the files are being checked against their SHA-256.
    Verifying,
    Paused,
    Done,
    Failed,
}

impl JobState {
    /// Holds one of the slots `concurrency` allows.
    fn running(self) -> bool {
        matches!(self, JobState::Downloading | JobState::Verifying)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub id: String,
    pub repo: String,
    pub name: String,
    pub quant: Option<String>,
    pub total: u64,
    pub downloaded: u64,
    pub bytes_per_second: f64,
    pub state: JobState,
    pub error: Option<String>,
    pub created_at: i64,
    /// Files it fetches: the model's parts and its projector.
    #[serde(default)]
    pub files: u32,
    /// The repo's projector comes with it.
    #[serde(default)]
    pub projector: bool,
}

/// What the Models window asks to download.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpec {
    pub repo: String,
    pub name: String,
    pub quant: Option<String>,
    pub paths: Vec<String>,
    pub sizes: Vec<u64>,
    pub sha256: Vec<Option<String>>,
    /// The repo's projector, saved next to the model unless one is there already.
    #[serde(default)]
    pub projector: Option<HfProjector>,
    /// The folder the files go to, when not `<models folder>/<repo>`: a projector fetched for a
    /// model already on disk goes next to it. Set by the backend only, and saved with the queue.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

impl DownloadSpec {
    /// The files to fetch, as (repo path, size, SHA-256): the model's parts, then its projector
    /// when `dir` has none of that name.
    fn files(&self, dir: &Path) -> Vec<(String, u64, Option<String>)> {
        let mut files: Vec<_> = self
            .paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let size = self.sizes.get(i).copied().unwrap_or(0);
                (path.clone(), size, self.sha256.get(i).cloned().flatten())
            })
            .collect();
        if let Some(p) = &self.projector
            && !dir.join(sanitize(file_name(&p.path))).is_file()
        {
            files.push((p.path.clone(), p.size, p.sha256.clone()));
        }
        files
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// How many run at once, how fast they may go together, and how they travel.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueOptions {
    pub concurrency: u8,
    /// Bytes per second for all downloads together; 0 is no limit.
    pub limit: u64,
    /// Fetch files the Hub keeps in Xet through Hugging Face's Xet engine rather than plain HTTP.
    #[serde(default)]
    pub xet: bool,
}

impl QueueOptions {
    fn clamped(self) -> Self {
        Self {
            concurrency: self.concurrency.clamp(1, MAX_CONCURRENCY),
            ..self
        }
    }
}

/// Where a job's task should leave it once it has stopped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// Paused by the person.
    Pause,
    /// Took out of its slot for another ("Start now"): back in the queue.
    Requeue,
    /// Cancelled: its partial files are deleted and it leaves the list.
    Discard,
}

struct Entry {
    job: DownloadJob,
    spec: DownloadSpec,
    cancel: Option<CancellationToken>,
    stop: Stop,
}

/// A job as `downloads.json` keeps it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    job: DownloadJob,
    spec: DownloadSpec,
    dir: Option<PathBuf>,
}

pub struct DownloadManager {
    app: AppHandle,
    models_dir: PathBuf,
    store: PathBuf,
    downloader: Downloader,
    token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    on_complete: Arc<dyn Fn() + Send + Sync>,
    entries: Mutex<Vec<Entry>>,
    options: Mutex<QueueOptions>,
    limit: RateLimit,
}

impl DownloadManager {
    pub fn new(
        app: AppHandle,
        models_dir: PathBuf,
        store: PathBuf,
        xet_cache: PathBuf,
        options: QueueOptions,
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        on_complete: Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Arc<Self>> {
        let options = options.clamped();
        let entries = restore(&store);
        Ok(Arc::new(Self {
            app,
            models_dir,
            store,
            downloader: Downloader::new(&format!("DemidoStudio/{}", demido_core::brand::VERSION))?
                .with_xet(xet_cache)?,
            token,
            on_complete,
            entries: Mutex::new(entries),
            options: Mutex::new(options),
            limit: RateLimit::new(options.limit),
        }))
    }

    /// Starts what the saved queue left waiting.
    pub fn start(self: &Arc<Self>) {
        self.pump();
    }

    pub fn list(&self) -> Vec<DownloadJob> {
        self.entries.lock().iter().map(|e| e.job.clone()).collect()
    }

    pub fn options(&self) -> QueueOptions {
        *self.options.lock()
    }

    /// Applies new options: a raised concurrency starts the next jobs at once, a lowered one lets
    /// the running ones finish, and the speed limit changes under the running ones.
    pub fn set_options(self: &Arc<Self>, options: QueueOptions) -> QueueOptions {
        let options = options.clamped();
        *self.options.lock() = options;
        self.limit.set(options.limit);
        self.pump();
        options
    }

    fn emit(&self, job: &DownloadJob) {
        let _ = self.app.emit(CHANGED_EVENT, job);
    }

    fn emit_list(&self) {
        let _ = self.app.emit(LIST_EVENT, self.list());
    }

    /// Saves the queue. Called on every change of state, never on progress alone.
    fn save(&self) {
        let saved: Vec<Saved> = self
            .entries
            .lock()
            .iter()
            .filter(|e| e.job.state != JobState::Done)
            .map(|e| Saved {
                job: e.job.clone(),
                spec: e.spec.clone(),
                dir: e.spec.dir.clone(),
            })
            .collect();
        if let Err(err) = demido_core::fsx::write_json(&self.store, &saved) {
            tracing::warn!("the download queue could not be saved: {err:#}");
        }
    }

    fn dest_dir(&self, spec: &DownloadSpec) -> PathBuf {
        if let Some(dir) = &spec.dir {
            return dir.clone();
        }
        let mut dir = self.models_dir.clone();
        for part in spec.repo.split('/') {
            dir.push(sanitize(part));
        }
        dir
    }

    pub fn enqueue(self: &Arc<Self>, spec: DownloadSpec) -> anyhow::Result<DownloadJob> {
        anyhow::ensure!(!spec.paths.is_empty(), "nothing to download");
        let mut entries = self.entries.lock();
        if let Some(existing) = entries
            .iter()
            .find(|e| e.spec.repo == spec.repo && e.spec.paths == spec.paths && e.job.state != JobState::Failed)
        {
            return Ok(existing.job.clone());
        }
        let files = spec.files(&self.dest_dir(&spec));
        let job = DownloadJob {
            id: crate::db::new_id(),
            repo: spec.repo.clone(),
            name: spec.name.clone(),
            quant: spec.quant.clone(),
            total: files.iter().map(|f| f.1).sum(),
            downloaded: 0,
            bytes_per_second: 0.0,
            state: JobState::Queued,
            error: None,
            created_at: crate::db::now_ms(),
            files: files.len() as u32,
            projector: files.len() > spec.paths.len(),
        };
        entries.push(Entry {
            job: job.clone(),
            spec,
            cancel: None,
            stop: Stop::Pause,
        });
        drop(entries);
        self.save();
        self.emit_list();
        self.pump();
        Ok(job)
    }

    /// Starts queued jobs, front first, while there are free slots.
    fn pump(self: &Arc<Self>) {
        let concurrency = usize::from(self.options().concurrency);
        loop {
            let entries = self.entries.lock();
            let running = entries.iter().filter(|e| e.job.state.running()).count();
            if running >= concurrency {
                return;
            }
            let Some(id) = entries
                .iter()
                .find(|e| e.job.state == JobState::Queued)
                .map(|e| e.job.id.clone())
            else {
                return;
            };
            drop(entries);
            self.launch(&id);
        }
    }

    /// Runs one job now, whatever the free slots.
    fn launch(self: &Arc<Self>, id: &str) {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        let cancel = CancellationToken::new();
        entry.cancel = Some(cancel.clone());
        entry.stop = Stop::Pause;
        entry.job.state = JobState::Downloading;
        entry.job.error = None;
        let spec = entry.spec.clone();
        let job = entry.job.clone();
        drop(entries);
        self.emit(&job);
        self.save();

        let me = self.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            let result = me.run(&id, &spec, &cancel).await;
            me.finish(&id, &spec, result);
            me.pump();
        });
    }

    async fn run(&self, id: &str, spec: &DownloadSpec, cancel: &CancellationToken) -> Result<(), FetchError> {
        let dir = self.dest_dir(spec);
        let token = (self.token)();
        let xet = self.options().xet;
        let mut done_before = 0u64;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        for (path, size, sha256) in spec.files(&dir) {
            let mut req = DownloadRequest::new(
                super::hf::file_url(&spec.repo, &path),
                dir.join(sanitize(file_name(&path))),
            )
            .bearer(token.clone())
            .limit(self.limit.clone())
            .xet(xet);
            if size > 0 {
                req = req.size(size);
            }
            if let Some(sha) = sha256 {
                req = req.sha256(sha);
            }
            self.downloader
                .download(&req, cancel, |p| {
                    // A change of phase always shows; progress at most four times a second.
                    let recent = last_emit.elapsed() < Duration::from_millis(250);
                    let state = if p.verifying {
                        JobState::Verifying
                    } else {
                        JobState::Downloading
                    };
                    let emitted = self.update(id, |job| {
                        if recent && job.state == state {
                            return false;
                        }
                        job.downloaded = done_before + p.downloaded;
                        job.bytes_per_second = p.bytes_per_second;
                        job.state = state;
                        true
                    });
                    if emitted {
                        last_emit = Instant::now();
                    }
                })
                .await?;
            done_before += size;
        }
        Ok(())
    }

    /// Applies `f` to a running job and emits it when `f` says it changed.
    fn update(&self, id: &str, f: impl FnOnce(&mut DownloadJob) -> bool) -> bool {
        let mut entries = self.entries.lock();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id && e.job.state.running()) else {
            return false;
        };
        let mut job = e.job.clone();
        if !f(&mut job) {
            return false;
        }
        e.job = job.clone();
        drop(entries);
        self.emit(&job);
        true
    }

    fn finish(&self, id: &str, spec: &DownloadSpec, result: Result<(), FetchError>) {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        entry.cancel = None;
        entry.job.bytes_per_second = 0.0;
        let mut completed = false;
        let mut removed = false;
        match result {
            Ok(()) => {
                entry.job.state = JobState::Done;
                entry.job.downloaded = entry.job.total;
                completed = true;
            }
            Err(FetchError::Cancelled) => match entry.stop {
                Stop::Discard => removed = true,
                Stop::Requeue => entry.job.state = JobState::Queued,
                Stop::Pause => entry.job.state = JobState::Paused,
            },
            Err(err) => {
                entry.job.state = JobState::Failed;
                entry.job.error = Some(failure(&err));
            }
        }
        let job = entry.job.clone();
        if removed {
            entries.retain(|e| e.job.id != id);
        }
        drop(entries);
        if removed {
            self.remove_partial(spec);
            self.emit_list();
        } else {
            self.emit(&job);
        }
        self.save();
        if completed {
            (self.on_complete)();
        }
    }

    /// Stops a running job, leaving it as `stop` says once its task has ended.
    fn stop_running(entry: &mut Entry, stop: Stop) -> bool {
        match &entry.cancel {
            Some(c) => {
                entry.stop = stop;
                c.cancel();
                true
            }
            None => false,
        }
    }

    pub fn pause(&self, id: &str) {
        let mut entries = self.entries.lock();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        if Self::stop_running(e, Stop::Pause) || e.job.state != JobState::Queued {
            return;
        }
        e.job.state = JobState::Paused;
        let job = e.job.clone();
        drop(entries);
        self.emit(&job);
        self.save();
    }

    /// Puts a paused or failed job back in the queue, where it stands in the order.
    pub fn resume(self: &Arc<Self>, id: &str) {
        if self.requeue(|e| e.job.id == id) {
            self.pump();
        }
    }

    /// Stops everything that runs or waits; each stays where it is in the order.
    pub fn pause_all(&self) {
        let mut entries = self.entries.lock();
        let mut changed = Vec::new();
        for e in entries.iter_mut() {
            if Self::stop_running(e, Stop::Pause) {
                continue;
            }
            if e.job.state == JobState::Queued {
                e.job.state = JobState::Paused;
                changed.push(e.job.clone());
            }
        }
        drop(entries);
        for job in &changed {
            self.emit(job);
        }
        self.save();
    }

    /// Puts every paused and failed job back in the queue.
    pub fn resume_all(self: &Arc<Self>) {
        if self.requeue(|_| true) {
            self.pump();
        }
    }

    fn requeue(&self, which: impl Fn(&Entry) -> bool) -> bool {
        let mut entries = self.entries.lock();
        let mut changed = Vec::new();
        for e in entries.iter_mut() {
            if which(e) && matches!(e.job.state, JobState::Paused | JobState::Failed) {
                e.job.state = JobState::Queued;
                e.job.error = None;
                changed.push(e.job.clone());
            }
        }
        drop(entries);
        for job in &changed {
            self.emit(job);
        }
        if !changed.is_empty() {
            self.save();
        }
        !changed.is_empty()
    }

    /// Moves a job that is not running to just before `before` in the order, or to the end.
    pub fn reorder(&self, id: &str, before: Option<&str>) {
        let mut entries = self.entries.lock();
        if !reorder(&mut entries, id, before) {
            return;
        }
        drop(entries);
        self.save();
        self.emit_list();
    }

    /// Runs a job now: first in the queue, and in a slot of its own, taking one from the job that
    /// started last when all are taken. That one goes back to the front of the queue.
    pub fn start_now(self: &Arc<Self>, id: &str) {
        let concurrency = usize::from(self.options().concurrency);
        {
            let mut entries = self.entries.lock();
            let Some(i) = entries.iter().position(|e| e.job.id == id) else {
                return;
            };
            if entries[i].job.state.running() || entries[i].job.state == JobState::Done {
                return;
            }
            let entry = entries.remove(i);
            entries.insert(0, entry);
            let running: Vec<usize> = (0..entries.len()).filter(|&i| entries[i].job.state.running()).collect();
            if running.len() >= concurrency
                && let Some(&last) = running.last()
            {
                Self::stop_running(&mut entries[last], Stop::Requeue);
            }
        }
        self.emit_list();
        self.launch(id);
    }

    /// Stops a job and deletes what it had downloaded.
    pub fn cancel(self: &Arc<Self>, id: &str) {
        let mut entries = self.entries.lock();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        if Self::stop_running(e, Stop::Discard) {
            return; // `finish` removes it.
        }
        let spec = e.spec.clone();
        entries.retain(|e| e.job.id != id);
        drop(entries);
        self.remove_partial(&spec);
        self.save();
        self.emit_list();
    }

    /// Takes a finished job off the list.
    pub fn remove(&self, id: &str) {
        let mut entries = self.entries.lock();
        let before = entries.len();
        entries.retain(|e| !(e.job.id == id && e.job.state == JobState::Done));
        let removed = entries.len() != before;
        drop(entries);
        if removed {
            self.emit_list();
        }
    }

    /// Deletes what a stopped job had downloaded of its files.
    fn remove_partial(&self, spec: &DownloadSpec) {
        let dir = self.dest_dir(spec);
        let projector = spec.projector.iter().map(|p| &p.path);
        for path in spec.paths.iter().chain(projector) {
            let name = sanitize(file_name(path));
            let _ = std::fs::remove_file(dir.join(format!("{name}.part")));
        }
    }

    /// Removes finished jobs from the list.
    pub fn clear_finished(&self) {
        self.entries.lock().retain(|e| !matches!(e.job.state, JobState::Done));
        self.emit_list();
    }
}

/// Moves `id` before `before` (or to the end). A running job keeps its place, and so does one
/// asked to move before itself.
fn reorder(entries: &mut Vec<Entry>, id: &str, before: Option<&str>) -> bool {
    if before == Some(id) {
        return false;
    }
    let Some(from) = entries.iter().position(|e| e.job.id == id) else {
        return false;
    };
    if entries[from].job.state.running() {
        return false;
    }
    let entry = entries.remove(from);
    let to = before
        .and_then(|b| entries.iter().position(|e| e.job.id == b))
        .unwrap_or(entries.len());
    entries.insert(to, entry);
    true
}

/// The sentence a failed job shows.
fn failure(err: &FetchError) -> String {
    match err {
        FetchError::Status { status, .. } if status.as_u16() == 401 || status.as_u16() == 403 => {
            "This repository needs a Hugging Face token with access to it.".into()
        }
        FetchError::Status { status, .. } if status.as_u16() == 404 => {
            "The file is no longer on Hugging Face. The repository may have been changed.".into()
        }
        FetchError::Checksum { file, .. } => {
            format!("{file} arrived damaged (its SHA-256 did not match) and was deleted. Retry downloads it again.")
        }
        FetchError::Http(e) if e.is_timeout() || e.is_connect() || e.is_body() => {
            "The connection to Hugging Face dropped. What arrived is kept, so Retry continues from there.".into()
        }
        other => other.to_string(),
    }
}

/// The queue `downloads.json` saved: what was running waits again, and the projector folder of a
/// job comes back with it.
fn restore(store: &Path) -> Vec<Entry> {
    if !store.exists() {
        return Vec::new();
    }
    let saved: Vec<Saved> = match demido_core::fsx::read_json(store) {
        Ok(saved) => saved,
        Err(err) => {
            tracing::warn!("the saved download queue could not be read: {err:#}");
            return Vec::new();
        }
    };
    saved
        .into_iter()
        .filter(|s| s.job.state != JobState::Done)
        .map(|Saved { mut job, mut spec, dir }| {
            spec.dir = dir;
            job.bytes_per_second = 0.0;
            if job.state.running() {
                job.state = JobState::Queued;
            }
            Entry {
                job,
                spec,
                cancel: None,
                stop: Stop::Pause,
            }
        })
        .collect()
}

fn sanitize(part: &str) -> String {
    part.chars()
        .map(|c| {
            if c.is_alphanumeric() || "._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(projector: Option<HfProjector>) -> DownloadSpec {
        DownloadSpec {
            repo: "unsloth/gemma-4-E4B-it-qat-GGUF".into(),
            name: "gemma-4-E4B-it-qat-UD-Q4_K_XL.gguf".into(),
            quant: Some("UD-Q4_K_XL".into()),
            paths: vec!["gemma-4-E4B-it-qat-UD-Q4_K_XL.gguf".into()],
            sizes: vec![5],
            sha256: vec![None],
            projector,
            dir: None,
        }
    }

    fn entry(id: &str, state: JobState) -> Entry {
        Entry {
            job: DownloadJob {
                id: id.into(),
                repo: "r".into(),
                name: id.into(),
                quant: None,
                total: 10,
                downloaded: 0,
                bytes_per_second: 0.0,
                state,
                error: None,
                created_at: 0,
                files: 1,
                projector: false,
            },
            spec: spec(None),
            cancel: None,
            stop: Stop::Pause,
        }
    }

    fn order(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.job.id.as_str()).collect()
    }

    #[test]
    fn the_projector_comes_with_the_model_unless_the_folder_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let projector = HfProjector {
            path: "mmproj-BF16.gguf".into(),
            size: 3,
            sha256: Some("ab".repeat(32)),
        };
        let with = spec(Some(projector.clone())).files(dir.path());
        assert_eq!(with.len(), 2);
        assert_eq!(with[1], ("mmproj-BF16.gguf".into(), 3, Some("ab".repeat(32))));
        assert_eq!(spec(None).files(dir.path()).len(), 1);

        // Another quant of the same repo already brought it.
        std::fs::write(dir.path().join("mmproj-BF16.gguf"), b"abc").unwrap();
        assert_eq!(spec(Some(projector)).files(dir.path()).len(), 1);
    }

    #[test]
    fn a_waiting_job_moves_before_another_or_to_the_end() {
        let mut entries = vec![
            entry("a", JobState::Downloading),
            entry("b", JobState::Queued),
            entry("c", JobState::Paused),
            entry("d", JobState::Queued),
        ];
        assert!(reorder(&mut entries, "d", Some("b")));
        assert_eq!(order(&entries), ["a", "d", "b", "c"]);
        assert!(reorder(&mut entries, "d", None));
        assert_eq!(order(&entries), ["a", "b", "c", "d"]);
        // Running jobs keep their place, and a job does not move before itself.
        assert!(!reorder(&mut entries, "a", None));
        assert!(!reorder(&mut entries, "b", Some("b")));
        assert_eq!(order(&entries), ["a", "b", "c", "d"]);
    }

    #[test]
    fn the_saved_queue_carries_on_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("downloads.json");
        let mut projector_job = entry("p", JobState::Downloading);
        projector_job.spec.dir = Some(dir.path().join("elsewhere"));
        let saved: Vec<Saved> = [
            projector_job,
            entry("paused", JobState::Paused),
            entry("done", JobState::Done),
            entry("waiting", JobState::Verifying),
        ]
        .into_iter()
        .map(|e| Saved {
            dir: e.spec.dir.clone(),
            job: e.job,
            spec: e.spec,
        })
        .collect();
        demido_core::fsx::write_json(&store, &saved).unwrap();

        let entries = restore(&store);
        assert_eq!(order(&entries), ["p", "paused", "waiting"]);
        assert_eq!(entries[0].job.state, JobState::Queued);
        assert_eq!(
            entries[0].spec.dir.as_deref(),
            Some(dir.path().join("elsewhere").as_path())
        );
        assert_eq!(entries[1].job.state, JobState::Paused);
        assert_eq!(entries[2].job.state, JobState::Queued);
        assert!(restore(&dir.path().join("missing.json")).is_empty());
    }

    #[test]
    fn options_stay_within_what_is_offered() {
        let o = QueueOptions {
            concurrency: 9,
            limit: 5,
            xet: true,
        }
        .clamped();
        assert_eq!(
            o,
            QueueOptions {
                concurrency: 3,
                limit: 5,
                xet: true,
            }
        );
        assert_eq!(
            QueueOptions {
                concurrency: 0,
                limit: 0,
                xet: false,
            }
            .clamped()
            .concurrency,
            1
        );
    }
}
