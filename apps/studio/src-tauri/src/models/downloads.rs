//! The model download queue: one download at a time (full bandwidth to the file the person is
//! waiting for), resumable, verified against Hugging Face's SHA-256 before it appears as a model.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use demido_fetch::{CancellationToken, DownloadRequest, Downloader, FetchError};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

pub const CHANGED_EVENT: &str = "downloads://changed";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Downloading,
    Paused,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
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
}

/// What the Models tab asks to download.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpec {
    pub repo: String,
    pub name: String,
    pub quant: Option<String>,
    pub paths: Vec<String>,
    pub sizes: Vec<u64>,
    pub sha256: Vec<Option<String>>,
}

struct Entry {
    job: DownloadJob,
    spec: DownloadSpec,
    cancel: Option<CancellationToken>,
    /// Delete partial files when the running task stops.
    discard: bool,
}

pub struct DownloadManager {
    app: AppHandle,
    models_dir: PathBuf,
    downloader: Downloader,
    token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    on_complete: Arc<dyn Fn() + Send + Sync>,
    entries: Mutex<Vec<Entry>>,
}

impl DownloadManager {
    pub fn new(
        app: AppHandle,
        models_dir: PathBuf,
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        on_complete: Arc<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            app,
            models_dir,
            downloader: Downloader::new(&format!("DemidoStudio/{}", demido_core::brand::VERSION))?,
            token,
            on_complete,
            entries: Mutex::new(Vec::new()),
        }))
    }

    pub fn list(&self) -> Vec<DownloadJob> {
        self.entries.lock().iter().map(|e| e.job.clone()).collect()
    }

    fn emit(&self, job: &DownloadJob) {
        let _ = self.app.emit(CHANGED_EVENT, job);
    }

    fn dest_dir(&self, repo: &str) -> PathBuf {
        let mut dir = self.models_dir.clone();
        for part in repo.split('/') {
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
        let job = DownloadJob {
            id: crate::db::new_id(),
            repo: spec.repo.clone(),
            name: spec.name.clone(),
            quant: spec.quant.clone(),
            total: spec.sizes.iter().sum(),
            downloaded: 0,
            bytes_per_second: 0.0,
            state: JobState::Queued,
            error: None,
            created_at: crate::db::now_ms(),
        };
        entries.push(Entry {
            job: job.clone(),
            spec,
            cancel: None,
            discard: false,
        });
        drop(entries);
        self.emit(&job);
        self.pump();
        Ok(job)
    }

    /// Starts the next queued job when nothing is downloading.
    fn pump(self: &Arc<Self>) {
        let mut entries = self.entries.lock();
        if entries.iter().any(|e| e.job.state == JobState::Downloading) {
            return;
        }
        let Some(entry) = entries.iter_mut().find(|e| e.job.state == JobState::Queued) else {
            return;
        };
        let cancel = CancellationToken::new();
        entry.cancel = Some(cancel.clone());
        entry.discard = false;
        entry.job.state = JobState::Downloading;
        entry.job.error = None;
        let id = entry.job.id.clone();
        let spec = entry.spec.clone();
        let job = entry.job.clone();
        drop(entries);
        self.emit(&job);

        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = me.run(&id, &spec, &cancel).await;
            me.finish(&id, &spec, result);
            me.pump();
        });
    }

    async fn run(&self, id: &str, spec: &DownloadSpec, cancel: &CancellationToken) -> Result<(), FetchError> {
        let dir = self.dest_dir(&spec.repo);
        let token = (self.token)();
        let mut done_before = 0u64;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        for (i, path) in spec.paths.iter().enumerate() {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            let mut req = DownloadRequest::new(super::hf::file_url(&spec.repo, path), dir.join(sanitize(file_name)))
                .bearer(token.clone());
            if let Some(size) = spec.sizes.get(i).copied().filter(|s| *s > 0) {
                req = req.size(size);
            }
            if let Some(Some(sha)) = spec.sha256.get(i) {
                req = req.sha256(sha.clone());
            }
            self.downloader
                .download(&req, cancel, |p| {
                    if last_emit.elapsed() < Duration::from_millis(250) {
                        return;
                    }
                    last_emit = Instant::now();
                    let mut entries = self.entries.lock();
                    if let Some(e) = entries.iter_mut().find(|e| e.job.id == id) {
                        e.job.downloaded = done_before + p.downloaded;
                        e.job.bytes_per_second = p.bytes_per_second;
                        let job = e.job.clone();
                        drop(entries);
                        self.emit(&job);
                    }
                })
                .await?;
            done_before += spec.sizes.get(i).copied().unwrap_or(0);
        }
        Ok(())
    }

    fn finish(&self, id: &str, spec: &DownloadSpec, result: Result<(), FetchError>) {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        entry.cancel = None;
        entry.job.bytes_per_second = 0.0;
        let mut completed = false;
        match result {
            Ok(()) => {
                entry.job.state = JobState::Done;
                entry.job.downloaded = entry.job.total;
                completed = true;
            }
            Err(FetchError::Cancelled) if entry.discard => {
                let dir = self.dest_dir(&spec.repo);
                for path in &spec.paths {
                    let name = sanitize(path.rsplit('/').next().unwrap_or(path));
                    let _ = std::fs::remove_file(dir.join(format!("{name}.part")));
                }
            }
            Err(FetchError::Cancelled) => entry.job.state = JobState::Paused,
            Err(err) => {
                entry.job.state = JobState::Failed;
                entry.job.error = Some(match &err {
                    FetchError::Status { status, .. } if status.as_u16() == 401 || status.as_u16() == 403 => {
                        "This repository needs a Hugging Face token with access to it.".into()
                    }
                    other => other.to_string(),
                });
            }
        }
        let job = entry.job.clone();
        if entry.discard {
            entries.retain(|e| e.job.id != id);
        }
        drop(entries);
        self.emit(&job);
        if completed {
            (self.on_complete)();
        }
    }

    pub fn pause(&self, id: &str) {
        let mut entries = self.entries.lock();
        if let Some(e) = entries.iter_mut().find(|e| e.job.id == id) {
            match (&e.cancel, e.job.state) {
                (Some(c), _) => c.cancel(),
                (None, JobState::Queued) => {
                    e.job.state = JobState::Paused;
                    let job = e.job.clone();
                    drop(entries);
                    self.emit(&job);
                }
                _ => {}
            }
        }
    }

    pub fn resume(self: &Arc<Self>, id: &str) {
        {
            let mut entries = self.entries.lock();
            if let Some(e) = entries.iter_mut().find(|e| e.job.id == id)
                && matches!(e.job.state, JobState::Paused | JobState::Failed)
            {
                e.job.state = JobState::Queued;
                e.job.error = None;
                let job = e.job.clone();
                drop(entries);
                self.emit(&job);
            }
        }
        self.pump();
    }

    /// Stops a job and deletes what it had downloaded.
    pub fn cancel(self: &Arc<Self>, id: &str) {
        let mut entries = self.entries.lock();
        let Some(e) = entries.iter_mut().find(|e| e.job.id == id) else {
            return;
        };
        e.discard = true;
        if let Some(c) = &e.cancel {
            c.cancel();
            return; // `finish` removes it.
        }
        let spec = e.spec.clone();
        let mut job = e.job.clone();
        entries.retain(|e| e.job.id != id);
        drop(entries);
        let dir = self.dest_dir(&spec.repo);
        for path in &spec.paths {
            let name = sanitize(path.rsplit('/').next().unwrap_or(path));
            let _ = std::fs::remove_file(dir.join(format!("{name}.part")));
        }
        job.state = JobState::Failed;
        job.error = Some("cancelled".into());
        self.emit(&job);
    }

    /// Removes finished jobs from the list.
    pub fn clear_finished(&self) {
        self.entries.lock().retain(|e| !matches!(e.job.state, JobState::Done));
    }
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
