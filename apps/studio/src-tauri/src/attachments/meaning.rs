//! Search by meaning. A search model (a small embedding model, see `catalog/models.json`) turns
//! every passage of an attached file into a vector, and a question into another: passages whose
//! vectors point where the question's does are about what it asks, in other words or another
//! language. "How rich is Mr. Darcy?" finds "ten thousand a year", which shares no word with it.
//!
//! Passages are ranked by their similarity and their BM25 score together ([`rank`]): meaning
//! finds the paraphrase, words keep exact names and numbers on top. Until every passage searched
//! has a vector, and on a computer without a search model, words alone rank them
//! ([`super::search`]).
//!
//! The model runs in its own `llama-server`, started when there is something to embed and
//! stopped after a few idle minutes. The indexer embeds passages in the background, the newest
//! file's first, so a file is usually searchable by meaning before its question is asked. The
//! larger model is used when the GPU has the memory for it, the small one otherwise; vectors of
//! one cannot be compared with the other's, so a change of model indexes every file again.
//!
//! The chat model comes first. Before a local chat model loads, the search model finishes the
//! request in hand and stops ([`Embedder::make_room`]), and it cannot start again until the chat
//! model has loaded: the chat model gets the GPU memory it would get alone. Started again when
//! next needed, the search model is fitted by llama.cpp into what is left, on the CPU when
//! nothing is. Which search model is used does not change with the chat model: that would index
//! every file again.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use demido_catalog::SearchModel;
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::process::{Child, Command};
use tokio::sync::{Notify, OwnedRwLockWriteGuard, RwLock};
use tokio_util::sync::CancellationToken;

use crate::db::{Db, Passage};
use crate::models::ModelRegistry;

/// Passages embedded per request.
const BATCH: usize = 8;
/// Tokens of the server's one slot: a passage is about 650, and one of a dense script up to 2000.
const SLOT_TOKENS: u32 = 2048;
/// The server stops after this long without work, freeing its memory.
const IDLE_STOP: Duration = Duration::from_secs(180);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest a question waits for its chat's files to be indexed; words alone rank them after.
const QUERY_WAIT: Duration = Duration::from_secs(30);
/// After the server failed, the indexer tries again this much later (or when there is new work).
const RETRY_AFTER: Duration = Duration::from_secs(300);
/// Characters of a question that are embedded: its end, where the question usually is.
const QUERY_CHARS: usize = 2000;
/// A passage too long for the model is cut down to no less than this before it is given up on.
const MIN_CHARS: usize = 200;
/// Weight of the similarity in a passage's score; BM25 has the rest. Measured on questions with
/// known answers in a novel and a paper: meaning alone missed exact names, words alone missed
/// paraphrases, 0.85 of meaning found the most answers in the first three passages.
const MEANING_WEIGHT: f32 = 0.85;

/// The search model on this computer.
#[derive(Clone, Debug)]
pub struct Installed {
    pub model: SearchModel,
    pub path: PathBuf,
}

/// A question as the search model put it.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryVector {
    /// The model's catalog id: only that model's vectors compare with it.
    pub model: String,
    pub vector: Vec<f32>,
    /// Similarity from which a passage is taken as about the question (see the catalog).
    pub relevance: f32,
}

struct Server {
    model_id: String,
    port: u16,
    child: Child,
}

pub struct Embedder {
    db: Arc<Db>,
    models: Arc<ModelRegistry>,
    /// The llama-server executable; `None` without a local runtime.
    server: Option<PathBuf>,
    log_file: PathBuf,
    /// The search model this computer should use.
    preferred: SearchModel,
    http: reqwest::Client,
    running: tokio::sync::Mutex<Option<Server>>,
    /// Read while the model starts or answers; written by a chat model loading
    /// ([`Self::make_room`]), which then has the GPU to itself.
    gate: Arc<RwLock<()>>,
    last_used: Mutex<Instant>,
    retry_at: Mutex<Option<Instant>>,
    wake: Notify,
}

impl Embedder {
    pub fn new(
        db: Arc<Db>,
        models: Arc<ModelRegistry>,
        server: Option<PathBuf>,
        logs_dir: PathBuf,
        preferred: SearchModel,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            models,
            server,
            log_file: logs_dir.join("search-server.log"),
            preferred,
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
            running: tokio::sync::Mutex::new(None),
            gate: Arc::new(RwLock::new(())),
            last_used: Mutex::new(Instant::now()),
            retry_at: Mutex::new(None),
            wake: Notify::new(),
        })
    }

    /// The search model installed, the one this computer should use first. Found in the model
    /// folders where the installer and the download put it: `<repo>/<file>`.
    pub fn installed(&self) -> Option<Installed> {
        let dirs = self.models.model_dirs();
        let catalog = &demido_catalog::catalog().models.search.models;
        std::iter::once(&self.preferred).chain(catalog.iter()).find_map(|m| {
            let rel: PathBuf = m.repo.split('/').chain([m.file.as_str()]).collect();
            dirs.iter()
                .map(|d| d.join(&rel))
                .find(|p| p.is_file())
                .map(|path| Installed { model: m.clone(), path })
        })
    }

    /// GPU memory, in MiB, the installed search model takes when it runs beside a chat model (0
    /// without one).
    pub fn gpu_memory_mb(&self) -> u32 {
        self.installed().map_or(0, |m| m.model.gpu_memory_mb)
    }

    /// Asks the indexer to look for work now: a file was attached, or a search model arrived.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Stops the search model and keeps it stopped until the guard is dropped, so that a chat
    /// model about to load gets the GPU memory first. Waits for the request in hand, if any.
    pub async fn make_room(&self) -> OwnedRwLockWriteGuard<()> {
        let guard = self.gate.clone().write_owned().await;
        if let Some(mut s) = self.running.lock().await.take() {
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
            tracing::info!("search model stopped: a chat model is loading");
        }
        guard
    }

    /// Runs the indexer for as long as the app runs.
    pub fn spawn(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let woken = tokio::time::timeout(Duration::from_secs(30), me.wake.notified())
                    .await
                    .is_ok();
                let waiting = me.retry_at.lock().is_some_and(|at| Instant::now() < at);
                if woken || !waiting {
                    me.index().await;
                }
                me.stop_when_idle().await;
            }
        });
    }

    /// Embeds every passage without a vector of the installed model.
    async fn index(&self) {
        let Some(m) = self.installed() else {
            return;
        };
        loop {
            let pending = match self.db.unindexed_passages(&m.model.id, BATCH * 4) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("cannot list passages to index: {e}");
                    return;
                }
            };
            if pending.is_empty() {
                break;
            }
            for batch in pending.chunks(BATCH) {
                let texts: Vec<String> = batch
                    .iter()
                    .map(|(_, text)| format!("{}{text}", m.model.document_prefix))
                    .collect();
                let vectors = match self.embed(&m, texts).await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(model = %m.model.name, "indexing stopped: {e}");
                        *self.retry_at.lock() = Some(Instant::now() + RETRY_AFTER);
                        return;
                    }
                };
                let rows: Vec<(i64, Vec<f32>)> = batch.iter().map(|(id, _)| *id).zip(vectors).collect();
                if let Err(e) = self.db.store_vectors(&m.model.id, &rows) {
                    tracing::warn!("cannot store passage vectors: {e}");
                    return;
                }
                *self.retry_at.lock() = None;
            }
        }
    }

    /// The vector of `text` as a question about the files `ids`, once they are all indexed;
    /// `None` without a search model, or when they are not indexed in time or the model fails
    /// (the files are then searched by their words).
    pub async fn query(&self, ids: &[String], text: &str, cancel: &CancellationToken) -> Option<QueryVector> {
        if ids.is_empty() || text.trim().is_empty() {
            return None;
        }
        let m = self.installed()?;
        let deadline = Instant::now() + QUERY_WAIT;
        loop {
            match self.db.unindexed_count(ids, &m.model.id) {
                Ok(0) => break,
                Ok(left) if Instant::now() > deadline || cancel.is_cancelled() => {
                    tracing::info!("{left} passages are not indexed yet; searching by words");
                    return None;
                }
                Ok(_) => {}
                Err(_) => return None,
            }
            *self.retry_at.lock() = None;
            self.wake();
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let tail: String = {
            let chars: Vec<char> = text.trim().chars().collect();
            chars[chars.len().saturating_sub(QUERY_CHARS)..].iter().collect()
        };
        match self.embed(&m, vec![format!("{}{tail}", m.model.query_prefix)]).await {
            Ok(mut v) => Some(QueryVector {
                model: m.model.id.clone(),
                vector: v.pop()?,
                relevance: m.model.relevance,
            }),
            Err(e) => {
                tracing::warn!("the question could not be embedded: {e}");
                None
            }
        }
    }

    /// Vectors of `texts`, in order. A text the model cannot take (too many tokens) is cut down
    /// and tried again, and gets an empty vector, which matches nothing, when even a short start
    /// of it fails while the server runs.
    async fn embed(&self, m: &Installed, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        // Not while a chat model loads: it has the GPU first.
        let _turn = self.gate.read().await;
        let base = self.ensure(m).await?;
        *self.last_used.lock() = Instant::now();
        let result = self.request(&base, &texts).await;
        *self.last_used.lock() = Instant::now();
        match result {
            Ok(v) => Ok(v),
            Err(e) if !self.alive().await => Err(e),
            Err(_) => {
                let mut out = Vec::with_capacity(texts.len());
                for text in texts {
                    let mut text = text;
                    let vector = loop {
                        match self.request(&base, std::slice::from_ref(&text)).await {
                            Ok(mut v) => break v.pop().unwrap_or_default(),
                            Err(e) if !self.alive().await => return Err(e),
                            Err(e) if text.chars().count() <= MIN_CHARS => {
                                tracing::warn!("a passage cannot be embedded: {e}");
                                break Vec::new();
                            }
                            Err(_) => {
                                let half = text.chars().count() / 2;
                                text = text.chars().take(half.max(MIN_CHARS)).collect();
                            }
                        }
                    };
                    out.push(vector);
                }
                *self.last_used.lock() = Instant::now();
                Ok(out)
            }
        }
    }

    async fn request(&self, base: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Answer {
            data: Vec<Item>,
        }
        let resp = self
            .http
            .post(format!("{base}/embeddings"))
            .json(&serde_json::json!({ "input": texts, "model": "search" }))
            .send()
            .await
            .map_err(|e| format!("the search model did not answer: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("the search model answered {status}: {}", body.trim()));
        }
        let mut answer: Answer = resp
            .json()
            .await
            .map_err(|e| format!("the search model's answer could not be read: {e}"))?;
        if answer.data.len() != texts.len() {
            return Err(format!(
                "the search model returned {} vectors for {} texts",
                answer.data.len(),
                texts.len()
            ));
        }
        answer.data.sort_by_key(|i| i.index);
        Ok(answer.data.into_iter().map(|i| i.embedding).collect())
    }

    async fn alive(&self) -> bool {
        let mut running = self.running.lock().await;
        running.as_mut().is_some_and(|s| matches!(s.child.try_wait(), Ok(None)))
    }

    /// The `/v1` base URL of a server running `m`, starting one if needed.
    async fn ensure(&self, m: &Installed) -> Result<String, String> {
        let server = self.server.clone().ok_or("the local AI runtime is not installed")?;
        let mut running = self.running.lock().await;
        if let Some(s) = running.as_mut()
            && s.model_id == m.model.id
            && matches!(s.child.try_wait(), Ok(None))
        {
            return Ok(base_url(s.port));
        }
        if let Some(mut old) = running.take() {
            let _ = old.child.kill().await;
            let _ = old.child.wait().await;
        }
        let started = Instant::now();
        let (child, port) = self.launch(&server, m).await?;
        tracing::info!(
            model = %m.model.name,
            port,
            "search model ready in {:.1}s",
            started.elapsed().as_secs_f64()
        );
        *running = Some(Server {
            model_id: m.model.id.clone(),
            port,
            child,
        });
        Ok(base_url(port))
    }

    async fn launch(&self, server: &PathBuf, m: &Installed) -> Result<(Child, u16), String> {
        use std::io::Write;

        let port = crate::runtime::free_port().map_err(|e| format!("no free local port: {e}"))?;
        // One slot: texts are embedded one after the other, as fast as several slots on a GPU
        // and with a cache a quarter the size. A model that reads text both ways needs a whole
        // input in one micro-batch; see `SearchModel::micro_batch`. No prompt cache: no text is
        // embedded twice, and it would keep up to 8 GB of them in memory.
        let args: Vec<String> = vec![
            "-m".into(),
            m.path.to_string_lossy().into_owned(),
            "--embedding".into(),
            "-c".into(),
            SLOT_TOKENS.to_string(),
            "-np".into(),
            "1".into(),
            "-b".into(),
            SLOT_TOKENS.to_string(),
            "-ub".into(),
            m.model.micro_batch.min(SLOT_TOKENS).to_string(),
            "--cache-ram".into(),
            "0".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--no-webui".into(),
        ];
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
            .map_err(|e| format!("cannot write {}: {e}", self.log_file.display()))?;
        let _ = writeln!(
            log,
            "---- {} ----\n$ {} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            server.display(),
            args.join(" ")
        );
        let err_log = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(server);
        cmd.args(&args)
            .current_dir(server.parent().unwrap_or(std::path::Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err_log))
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start the search model: {e}"))?;
        crate::runtime::job::adopt(&child);

        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + LOAD_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "the search model stopped while loading ({status}); see {}",
                    self.log_file.display()
                ));
            }
            if let Ok(resp) = self.http.get(&health).timeout(Duration::from_secs(2)).send().await
                && resp.status().is_success()
            {
                return Ok((child, port));
            }
            if Instant::now() > deadline {
                let _ = child.kill().await;
                return Err("the search model took too long to load".into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn stop_when_idle(&self) {
        if self.last_used.lock().elapsed() < IDLE_STOP {
            return;
        }
        let mut running = self.running.lock().await;
        if let Some(mut s) = running.take() {
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
            tracing::info!("search model stopped: idle");
        }
    }

    /// Synchronous best-effort kill for app shutdown.
    pub fn kill_now(&self) {
        if let Ok(mut running) = self.running.try_lock()
            && let Some(s) = running.as_mut()
        {
            let _ = s.child.start_kill();
        }
    }
}

/// The search model for this computer: by the memory of the GPU the installed runtime uses, as
/// setup picks it.
pub fn for_this_computer(
    hardware: &demido_hardware::HardwareReport,
    backend: Option<demido_core::Backend>,
) -> SearchModel {
    let catalog = demido_catalog::catalog();
    let choices = demido_catalog::backend_choices(hardware, catalog);
    let backend = backend.unwrap_or_else(|| demido_catalog::default_backend(&choices));
    choices
        .iter()
        .find(|c| c.backend == backend)
        .map(|c| demido_catalog::search_model(c, catalog))
        .unwrap_or_else(|| catalog.models.search.for_memory(None))
        .clone()
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// Which passages [`find`] returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Matches {
    /// Only those about the question (by `relevance`) or with its words: for files the question
    /// may have nothing to do with, so small talk pulls nothing in.
    Strict,
    /// The best, once any passage is about the question or has its words: for files it is asked
    /// about. A question in another language than the file's scores lower than `relevance` on
    /// the very passage that answers it, and "summarise this" matches nothing, so the caller
    /// shows the file's opening.
    Best,
}

/// The passages of the files `ids` that match the question `q` (see [`Matches`]), its words
/// being the FTS5 query `fts`, best first, at most `limit`, with the rows of those about it.
/// `None` while any passage of the files has no vector of `q`'s model.
pub fn find(
    db: &Db,
    ids: &[String],
    q: &QueryVector,
    fts: Option<&str>,
    limit: usize,
    matches: Matches,
) -> Option<(Vec<Passage>, HashSet<i64>)> {
    if ids.is_empty() {
        return Some(Default::default());
    }
    let vectors = db.passage_vectors(ids, &q.model).ok()??;
    let lexical = fts
        .and_then(|fts| db.lexical_scores(ids, fts, 1000).ok())
        .unwrap_or_default();
    let matching = |s: &Scored| s.similarity >= q.relevance || s.lexical > 0.0;
    let all = rank(&q.vector, &vectors, &lexical);
    let ranked: Vec<Scored> = match matches {
        Matches::Strict => all.into_iter().filter(matching).take(limit).collect(),
        Matches::Best if all.iter().any(matching) => all.into_iter().take(limit).collect(),
        Matches::Best => Vec::new(),
    };
    let related = ranked
        .iter()
        .filter(|s| s.similarity >= q.relevance)
        .map(|s| s.rowid)
        .collect();
    let rowids: Vec<i64> = ranked.iter().map(|s| s.rowid).collect();
    Some((db.passages_by_rowid(&rowids).ok()?, related))
}

/// A passage's place in a ranking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scored {
    pub rowid: i64,
    pub score: f32,
    /// Cosine similarity with the question (the vectors are normalised).
    pub similarity: f32,
    /// BM25 score, 0 when no word of the question is in it.
    pub lexical: f32,
}

/// Ranks passages by meaning and words together, best first. Similarities are scaled to 0–1
/// between the least and the most similar passage, BM25 scores by the best one, and weighed
/// [`MEANING_WEIGHT`] to the rest. `vectors` has every passage searched; `lexical` the BM25 score
/// of those that contain words of the question. An empty vector (a passage the model could not
/// take) counts as the least similar.
pub fn rank(query: &[f32], vectors: &[(i64, Vec<f32>)], lexical: &HashMap<i64, f64>) -> Vec<Scored> {
    let sims: Vec<Option<f32>> = vectors
        .iter()
        .map(|(_, v)| (!v.is_empty()).then(|| v.iter().zip(query).map(|(a, b)| a * b).sum()))
        .collect();
    let lo = sims.iter().flatten().copied().fold(f32::INFINITY, f32::min);
    let hi = sims.iter().flatten().copied().fold(f32::NEG_INFINITY, f32::max);
    let spread = if hi > lo { hi - lo } else { 1.0 };
    let top = lexical.values().copied().fold(0.0f64, f64::max);
    let mut out: Vec<Scored> = vectors
        .iter()
        .zip(sims)
        .map(|((rowid, _), sim)| {
            let similarity = sim.unwrap_or(if lo.is_finite() { lo } else { 0.0 });
            let scaled = if lo.is_finite() {
                (similarity - lo) / spread
            } else {
                0.0
            };
            let lex = lexical.get(rowid).copied().unwrap_or(0.0);
            let lex_scaled = if top > 0.0 { (lex / top) as f32 } else { 0.0 };
            Scored {
                rowid: *rowid,
                score: MEANING_WEIGHT * scaled + (1.0 - MEANING_WEIGHT) * lex_scaled,
                similarity,
                lexical: lex as f32,
            }
        })
        .collect();
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.rowid.cmp(&b.rowid)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    #[test]
    fn meaning_leads_and_words_break_ties() {
        let q = unit(&[1.0, 0.0, 0.0]);
        let vectors = vec![
            (1, unit(&[0.0, 1.0, 0.0])), // unrelated, but has the words
            (2, unit(&[0.9, 0.1, 0.0])), // says it in other words
            (3, unit(&[0.6, 0.4, 0.0])),
            (4, unit(&[0.6, 0.4, 0.0])), // as similar as 3, with the words
        ];
        let lexical = HashMap::from([(1, 8.0), (4, 2.0)]);
        let ranked = rank(&q, &vectors, &lexical);
        let order: Vec<i64> = ranked.iter().map(|s| s.rowid).collect();
        assert_eq!(order, [2, 4, 3, 1]);
        assert!(ranked[0].similarity > 0.99);
        assert_eq!(ranked[3].lexical, 8.0);
    }

    #[test]
    fn passages_the_model_could_not_take_count_as_least_similar() {
        let q = unit(&[1.0, 0.0]);
        let vectors = vec![(1, Vec::new()), (2, unit(&[1.0, 1.0])), (3, unit(&[0.0, 1.0]))];
        let ranked = rank(&q, &vectors, &HashMap::new());
        assert_eq!(ranked.iter().map(|s| s.rowid).collect::<Vec<_>>(), [2, 1, 3]);
        assert_eq!(ranked[1].similarity, ranked[2].similarity);
        assert!(rank(&q, &[], &HashMap::new()).is_empty());
    }
}
