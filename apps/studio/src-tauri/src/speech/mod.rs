//! Speech: what the person says, written down for a chat model that cannot hear.
//!
//! The composer records the person's voice (`src/voice/`). A model that hears gets the recording
//! itself, as a voice note; any other model gets what a speech model (Qwen3-ASR, see
//! `catalog/models.json`) writes down of it: dictation, typed into the composer as it is written,
//! or the transcript of an earlier voice note, made when the note first reaches a model that
//! cannot hear it ([`transcribe_note`]).
//!
//! The speech model runs in its own `llama-server` with its audio encoder (`--mmproj`), like the
//! search model (`attachments::meaning`): started when the person starts recording, so it has
//! loaded by the time they stop, and stopped after a few idle minutes. The chat model comes
//! first: before a local chat model loads, the speech model finishes what it is writing and
//! stops ([`Transcriber::make_room`]), and starts again, in whatever memory is left, when next
//! needed.
//!
//! A recording is sent to the server's `/v1/audio/transcriptions`, which streams the words as
//! they are written; a server without it is asked through `/v1/chat/completions` instead, with
//! the recording as `input_audio`. Every transcription is recorded (model, length of the
//! recording, time taken), without its words.

pub mod clean;
pub mod microphone;
pub mod wav;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use demido_catalog::SpeechModel;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};
use tokio_util::sync::CancellationToken;

use crate::db::{Attachment, Db, TranscriptionRecord, new_id, now_ms};
use crate::llm::sse::SseDecoder;
use crate::models::ModelRegistry;
use crate::settings::SettingsStore;
use clean::Cleaner;

/// The server stops after this long without work, freeing its memory.
const IDLE_STOP: Duration = Duration::from_secs(180);
const LOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest a recording may take to write down: five minutes take seconds on a GPU and about a
/// minute on a CPU.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(300);
/// Longest recording the composer makes. Five minutes of 16 kHz 16-bit mono are 9.2 MiB, under
/// the 10 MB a sound attached to a message can be.
pub const MAX_SECONDS: u32 = 300;
/// Most a local model hears in one clip: Gemma 4 takes 30 seconds. A longer voice note is cut
/// into parts ([`wav::split`]).
pub const LOCAL_CLIP_SECONDS: u32 = 30;

/// The speech model on this computer, and its audio encoder.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub model: SpeechModel,
    pub path: PathBuf,
    pub projector: PathBuf,
}

/// What a speech model wrote down of a recording.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub text: String,
    /// The language it heard, as it names it ("English").
    pub language: Option<String>,
    pub model_id: String,
    /// The speech model's name.
    pub model: String,
    pub audio_ms: u64,
    /// Time it took to write down, once the model had loaded.
    pub duration_ms: u64,
    pub via: Via,
}

/// A speech model the person can choose in Settings.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechChoice {
    pub id: String,
    pub name: String,
    /// Bytes to download, the model and its audio encoder.
    pub size: u64,
    pub installed: bool,
    /// The one the app picks for this computer's memory when the choice is left to it.
    pub for_this_computer: bool,
}

/// How the server was asked.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Via {
    /// `/v1/audio/transcriptions`.
    Endpoint,
    /// `/v1/chat/completions`, with the recording as `input_audio`.
    Chat,
}

impl Via {
    fn as_str(self) -> &'static str {
        match self {
            Via::Endpoint => "endpoint",
            Via::Chat => "chat",
        }
    }
}

/// Where a recording comes from, for the record.
#[derive(Clone, Copy, Debug, Default)]
pub struct Origin<'a> {
    pub chat_id: Option<&'a str>,
    /// The voice note, `None` for dictation.
    pub attachment_id: Option<&'a str>,
}

struct Server {
    model_id: String,
    port: u16,
    child: Child,
}

#[derive(Debug)]
enum Failure {
    Cancelled,
    Failed(String),
}

impl From<String> for Failure {
    fn from(e: String) -> Self {
        Failure::Failed(e)
    }
}

pub struct Transcriber {
    db: Arc<Db>,
    models: Arc<ModelRegistry>,
    settings: Arc<SettingsStore>,
    /// The llama-server executable; `None` without a local runtime.
    server: Option<PathBuf>,
    log_file: PathBuf,
    /// The speech model this computer should use when Settings leave the choice to the app.
    preferred: SpeechModel,
    http: reqwest::Client,
    running: tokio::sync::Mutex<Option<Server>>,
    /// Read while the model starts or writes; written by a chat model loading
    /// ([`Self::make_room`]), which then has the GPU to itself.
    gate: Arc<RwLock<()>>,
    /// One voice note is written down at a time, so two asking for the same one get one
    /// transcript.
    notes: tokio::sync::Mutex<()>,
    last_used: Mutex<Instant>,
    /// Dictations being written down, by the id the composer gave them, to stop one.
    jobs: Mutex<HashMap<String, CancellationToken>>,
}

impl Transcriber {
    pub fn new(
        db: Arc<Db>,
        models: Arc<ModelRegistry>,
        settings: Arc<SettingsStore>,
        server: Option<PathBuf>,
        logs_dir: PathBuf,
        preferred: SpeechModel,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            models,
            settings,
            server,
            log_file: logs_dir.join("speech-server.log"),
            preferred,
            http: reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            running: tokio::sync::Mutex::new(None),
            gate: Arc::new(RwLock::new(())),
            notes: tokio::sync::Mutex::new(()),
            last_used: Mutex::new(Instant::now()),
            jobs: Mutex::new(HashMap::new()),
        })
    }

    /// The speech model chosen in Settings, or the one for this computer.
    pub fn wanted(&self) -> SpeechModel {
        let speech = &demido_catalog::catalog().models.speech;
        self.settings
            .get()
            .speech_model
            .and_then(|id| speech.get(&id).cloned())
            .unwrap_or_else(|| self.preferred.clone())
    }

    /// The speech model installed, with its audio encoder: the one chosen in Settings, or, left
    /// to the app, the one for this computer first. Found where setup and the download put them,
    /// `<repo>/<file>` in a models folder.
    pub fn installed(&self) -> Option<Installed> {
        let dirs = self.models.model_dirs();
        let choice = self.settings.get().speech_model;
        candidates(
            choice.as_deref(),
            &self.preferred,
            &demido_catalog::catalog().models.speech.models,
        )
        .into_iter()
        .find_map(|m| locate(m, &dirs))
    }

    /// The speech model to offer for download, when none that may be used is installed.
    pub fn missing(&self) -> Option<SpeechModel> {
        self.installed().is_none().then(|| self.wanted())
    }

    /// Every speech model in the catalog, for Settings: whether it is installed, and which one
    /// the app picks for this computer.
    pub fn choices(&self) -> Vec<SpeechChoice> {
        let dirs = self.models.model_dirs();
        demido_catalog::catalog()
            .models
            .speech
            .models
            .iter()
            .map(|m| SpeechChoice {
                id: m.id.clone(),
                name: m.name.clone(),
                size: m.download_size(),
                installed: locate(m, &dirs).is_some(),
                for_this_computer: m.id == self.preferred.id,
            })
            .collect()
    }

    /// Starts the speech model in the background, so it has loaded when the recording ends.
    pub fn warm_up(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let Some(m) = me.installed() else {
                return;
            };
            let _turn = me.gate.read().await;
            if let Err(e) = me.ensure(&m).await {
                tracing::warn!(model = %m.model.name, "the speech model could not start: {e}");
            }
            *me.last_used.lock() = Instant::now();
        });
    }

    /// Stops the speech model and keeps it stopped until the guard is dropped, so that a chat
    /// model about to load gets the GPU memory first. Waits for the recording in hand, if any.
    pub async fn make_room(&self) -> OwnedRwLockWriteGuard<()> {
        let guard = self.gate.clone().write_owned().await;
        if let Some(mut s) = self.running.lock().await.take() {
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
            tracing::info!("speech model stopped: a chat model is loading");
        }
        guard
    }

    /// Stops the speech model once it has been idle a while, for as long as the app runs.
    pub fn spawn(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                me.stop_when_idle().await;
            }
        });
    }

    /// A token that stops the dictation `job`, until [`Self::end_job`].
    pub fn start_job(&self, job: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.jobs.lock().insert(job.to_string(), token.clone());
        token
    }

    pub fn end_job(&self, job: &str) {
        self.jobs.lock().remove(job);
    }

    /// Stops writing down the dictation `job`. False when it is not being written down.
    pub fn cancel_job(&self, job: &str) -> bool {
        match self.jobs.lock().get(job) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Writes down a WAV recording, passing the words written so far to `progress` as they come,
    /// and records the transcription. `Err` says why it could not, in words for the person.
    pub async fn transcribe(
        &self,
        recording: &[u8],
        origin: Origin<'_>,
        cancel: &CancellationToken,
        progress: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Transcript, String> {
        let Some(m) = self.installed() else {
            return Err(format!(
                "No speech model is installed. Download {} in the composer or in Settings, General.",
                self.wanted().name
            ));
        };
        let audio_ms = wav::seconds(recording).map_or(0, |s| (s * 1000.0) as u64);
        let result = self.write_down(&m, recording, audio_ms, cancel, progress).await;
        let record = TranscriptionRecord {
            id: new_id(),
            chat_id: origin.chat_id.map(str::to_string),
            attachment_id: origin.attachment_id.map(str::to_string),
            created_at: now_ms(),
            model_id: m.model.id.clone(),
            audio_ms,
            duration_ms: match &result {
                Ok(t) => t.duration_ms,
                Err(_) => 0,
            },
            via: result.as_ref().ok().map(|t| t.via.as_str().to_string()),
            language: result.as_ref().ok().and_then(|t| t.language.clone()),
            error: match &result {
                Ok(_) => None,
                Err(Failure::Cancelled) => Some("cancelled".into()),
                Err(Failure::Failed(e)) => Some(e.clone()),
            },
        };
        if let Err(e) = self.db.record_transcription(&record) {
            tracing::warn!("could not record a transcription: {e}");
        }
        match result {
            Ok(t) => {
                tracing::info!(
                    model = %t.model,
                    via = t.via.as_str(),
                    "{:.1} s of speech written down in {:.1} s",
                    t.audio_ms as f64 / 1000.0,
                    t.duration_ms as f64 / 1000.0
                );
                Ok(t)
            }
            Err(Failure::Cancelled) => Err("Stopped.".into()),
            Err(Failure::Failed(e)) => {
                tracing::warn!(model = %m.model.name, "a recording could not be written down: {e}");
                Err(format!("{} could not write down the recording: {e}", m.model.name))
            }
        }
    }

    async fn write_down(
        &self,
        m: &Installed,
        recording: &[u8],
        audio_ms: u64,
        cancel: &CancellationToken,
        progress: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Transcript, Failure> {
        // Not while a chat model loads: it has the GPU first.
        let _turn = tokio::select! {
            turn = self.gate.read() => turn,
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
        };
        let base = tokio::select! {
            base = self.ensure(m) => base?,
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
        };
        *self.last_used.lock() = Instant::now();
        let started = Instant::now();
        let mut cleaner = Cleaner::default();
        let mut shown = String::new();
        let mut on_delta = |delta: &str| {
            let words = cleaner.push(delta);
            if !words.is_empty() {
                shown.push_str(&words);
                progress(&shown);
            }
        };
        let asked = tokio::time::timeout(
            TRANSCRIBE_TIMEOUT,
            Self::ask(&self.http, &base, recording, cancel, &mut on_delta, self.alive()),
        )
        .await;
        *self.last_used.lock() = Instant::now();
        let (raw, via) = match asked {
            Ok(result) => result?,
            Err(_) => return Err(Failure::Failed("it took too long".into())),
        };
        let (language, text) = clean::clean(&raw);
        progress(&text);
        Ok(Transcript {
            text,
            language,
            model_id: m.model.id.clone(),
            model: m.model.name.clone(),
            audio_ms,
            duration_ms: started.elapsed().as_millis() as u64,
            via,
        })
    }

    /// Asks the server at `base` to write `recording` down: by its transcription endpoint, or,
    /// when that fails and the server still runs (`up`), through chat completions, as servers
    /// without the endpoint understand.
    async fn ask(
        http: &reqwest::Client,
        base: &str,
        recording: &[u8],
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
        up: impl Future<Output = bool>,
    ) -> Result<(String, Via), Failure> {
        match Self::by_endpoint(http, base, recording, cancel, on_delta).await {
            Ok(raw) => Ok((raw, Via::Endpoint)),
            Err(Failure::Failed(e)) if up.await => {
                tracing::info!("the transcription endpoint failed ({e}); asking through chat completions");
                Self::by_chat(http, base, recording, cancel)
                    .await
                    .map(|raw| (raw, Via::Chat))
            }
            Err(e) => Err(e),
        }
    }

    /// `/v1/audio/transcriptions`, streamed. Returns the whole answer as the model wrote it.
    async fn by_endpoint(
        http: &reqwest::Client,
        base: &str,
        recording: &[u8],
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> Result<String, Failure> {
        let (content_type, body) = multipart(recording);
        let request = http
            .post(format!("{base}/audio/transcriptions"))
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send();
        let resp = tokio::select! {
            r = request => r.map_err(|e| format!("the speech model did not answer: {e}"))?,
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("the speech model answered {status}: {}", body.trim()).into());
        }
        let streamed = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if !streamed {
            let answer: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("the speech model's answer could not be read: {e}"))?;
            let text = answer["text"].as_str().unwrap_or_default().to_string();
            on_delta(&text);
            return Ok(text);
        }
        let mut stream = resp.bytes_stream();
        let mut sse = SseDecoder::default();
        let mut text = String::new();
        let mut done: Option<String> = None;
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = cancel.cancelled() => return Err(Failure::Cancelled),
            };
            let ended = chunk.is_none();
            let events = match chunk {
                Some(Ok(bytes)) => sse.push(&bytes),
                Some(Err(e)) => return Err(format!("the speech model stopped answering: {e}").into()),
                None => sse.finish().into_iter().collect(),
            };
            for data in &events {
                if data == "[DONE]" {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                    continue;
                };
                if let Some(message) = event["error"]["message"].as_str() {
                    return Err(format!("the speech model failed: {message}").into());
                }
                match event["type"].as_str() {
                    Some("transcript.text.delta") => {
                        let delta = event["delta"].as_str().unwrap_or_default();
                        text.push_str(delta);
                        on_delta(delta);
                    }
                    Some("transcript.text.done") => done = event["text"].as_str().map(str::to_string),
                    _ => {}
                }
            }
            if ended {
                break;
            }
        }
        Ok(done.unwrap_or(text))
    }

    /// `/v1/chat/completions` with the recording as `input_audio`: a speech model's chat template
    /// asks it to write down what it hears.
    async fn by_chat(
        http: &reqwest::Client,
        base: &str,
        recording: &[u8],
        cancel: &CancellationToken,
    ) -> Result<String, Failure> {
        use base64::Engine;
        let body = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_audio",
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(recording),
                        "format": "wav",
                    },
                }],
            }],
            "temperature": 0,
            "stream": false,
        });
        let request = http.post(format!("{base}/chat/completions")).json(&body).send();
        let resp = tokio::select! {
            r = request => r.map_err(|e| format!("the speech model did not answer: {e}"))?,
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("the speech model answered {status}: {}", body.trim()).into());
        }
        let answer: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("the speech model's answer could not be read: {e}"))?;
        Ok(answer["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
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
            "speech model ready in {:.1}s",
            started.elapsed().as_secs_f64()
        );
        *running = Some(Server {
            model_id: m.model.id.clone(),
            port,
            child,
        });
        Ok(base_url(port))
    }

    async fn launch(&self, server: &Path, m: &Installed) -> Result<(Child, u16), String> {
        use std::io::Write;

        let port = crate::runtime::free_port().map_err(|e| format!("no free local port: {e}"))?;
        // One slot, as long as the longest recording and its transcript (see the catalog). No
        // prompt cache: no recording is written down twice, and it would keep them in memory.
        let args: Vec<String> = vec![
            "-m".into(),
            m.path.to_string_lossy().into_owned(),
            "--mmproj".into(),
            m.projector.to_string_lossy().into_owned(),
            "-c".into(),
            m.model.context_length.to_string(),
            "-np".into(),
            "1".into(),
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
            .current_dir(server.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err_log))
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start the speech model: {e}"))?;
        crate::runtime::job::adopt(&child);

        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + LOAD_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "the speech model stopped while loading ({status}); see {}",
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
                return Err("the speech model took too long to load".into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn stop_when_idle(&self) {
        if self.last_used.lock().elapsed() < IDLE_STOP || !self.jobs.lock().is_empty() {
            return;
        }
        let Ok(_turn) = self.gate.try_read() else {
            return;
        };
        let mut running = self.running.lock().await;
        if let Some(mut s) = running.take() {
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
            tracing::info!("speech model stopped: idle");
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

/// Writes down a voice note that has no transcript yet and stores it with the note, where every
/// later request, the chat's summary and `search_files` find it. The note as it is after; the
/// error says why it could not be written down.
pub async fn transcribe_note(
    state: &crate::state::AppState,
    note: &Attachment,
    cancel: &CancellationToken,
) -> Result<Attachment, String> {
    let speech = &state.speech;
    let _one = speech.notes.lock().await;
    let fresh = state
        .db
        .get_attachment(&note.id)
        .map_err(|e| e.to_string())?
        .ok_or("the voice note no longer exists")?;
    if fresh.voice.as_ref().is_some_and(|v| v.transcript.is_some()) {
        return Ok(fresh);
    }
    let (recording, _) = state
        .db
        .attachment_media(&note.id)
        .map_err(|e| e.to_string())?
        .ok_or("the recording is missing")?;
    let origin = Origin {
        chat_id: fresh.chat_id.as_deref(),
        attachment_id: Some(&note.id),
    };
    let t = speech.transcribe(&recording, origin, cancel, &mut |_| {}).await?;
    state
        .db
        .set_voice_transcript(&note.id, &t.text, t.language.as_deref(), &t.model)
        .map_err(|e| e.to_string())?;
    if !t.text.is_empty() {
        state.embedder.wake();
    }
    let note = state
        .db
        .get_attachment(&note.id)
        .map_err(|e| e.to_string())?
        .ok_or("the voice note no longer exists")?;
    Ok(note)
}

/// The speech models to look for, in order: the one chosen in Settings alone, or, left to the
/// app (or a choice the catalog no longer has), the one for this computer and then the others.
fn candidates<'a>(choice: Option<&str>, preferred: &'a SpeechModel, all: &'a [SpeechModel]) -> Vec<&'a SpeechModel> {
    if let Some(chosen) = choice.and_then(|id| all.iter().find(|m| m.id == id)) {
        return vec![chosen];
    }
    std::iter::once(preferred)
        .chain(all.iter().filter(|m| m.id != preferred.id))
        .collect()
}

/// `m` and the audio encoder the catalog pins for it, in the first models folder that has both
/// in `<repo>/`. Another encoder in the folder (the repo's bf16 one, say) is not taken for it.
fn locate(m: &SpeechModel, dirs: &[PathBuf]) -> Option<Installed> {
    let folder: PathBuf = m.repo.split('/').collect();
    dirs.iter().map(|d| d.join(&folder)).find_map(|dir| {
        let path = dir.join(&m.file);
        let projector = dir.join(&m.projector.file);
        (path.is_file() && projector.is_file()).then(|| Installed {
            model: m.clone(),
            path,
            projector,
        })
    })
}

/// The speech model for this computer: by the memory of the GPU the installed runtime uses, as
/// setup picks it.
pub fn for_this_computer(
    hardware: &demido_hardware::HardwareReport,
    backend: Option<demido_core::Backend>,
) -> SpeechModel {
    let catalog = demido_catalog::catalog();
    let choices = demido_catalog::backend_choices(hardware, catalog);
    let backend = backend.unwrap_or_else(|| demido_catalog::default_backend(&choices));
    choices
        .iter()
        .find(|c| c.backend == backend)
        .map(|c| demido_catalog::speech_model(c, catalog))
        .unwrap_or_else(|| catalog.models.speech.for_memory(None))
        .clone()
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// A `multipart/form-data` body with the recording as `file`, asking for the words as they are
/// written (`stream`). llama.cpp answers only in `json`.
fn multipart(recording: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("demido-{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(recording.len() + 512);
    let field = |body: &mut Vec<u8>, name: &str, value: &str| {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
        );
    };
    field(&mut body, "response_format", "json");
    field(&mut body, "stream", "true");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"recording.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(recording);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech() -> &'static [SpeechModel] {
        &demido_catalog::catalog().models.speech.models
    }

    fn by_id(id: &str) -> &'static SpeechModel {
        speech().iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn the_chosen_speech_model_is_the_only_one_looked_for() {
        let small = by_id("qwen3-asr-0.6b");
        let ids = |c: Vec<&SpeechModel>| c.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(candidates(Some("qwen3-asr-1.7b"), small, speech())),
            ["qwen3-asr-1.7b"]
        );
        // Left to the app: the one for this computer's memory first, then any other.
        assert_eq!(
            ids(candidates(None, small, speech())),
            ["qwen3-asr-0.6b", "qwen3-asr-1.7b"]
        );
        assert_eq!(
            ids(candidates(None, by_id("qwen3-asr-1.7b"), speech())),
            ["qwen3-asr-1.7b", "qwen3-asr-0.6b"]
        );
        assert_eq!(
            ids(candidates(Some("whisper"), small, speech())),
            ["qwen3-asr-0.6b", "qwen3-asr-1.7b"],
            "a choice the catalog no longer has"
        );
    }

    #[test]
    fn a_speech_model_is_installed_with_its_own_encoder_only() {
        let m = by_id("qwen3-asr-0.6b");
        let models = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let dirs = vec![other.path().to_path_buf(), models.path().to_path_buf()];
        let folder = models.path().join("ggml-org").join(m.repo.split('/').nth(1).unwrap());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(&m.file), b"gguf").unwrap();
        assert_eq!(locate(m, &dirs), None, "no encoder: it hears nothing");

        // The repo's other encoders are not the one the catalog pins.
        std::fs::write(folder.join("mmproj-Qwen3-ASR-0.6B-bf16.gguf"), b"gguf").unwrap();
        assert_ne!(m.projector.file, "mmproj-Qwen3-ASR-0.6B-bf16.gguf");
        assert_eq!(locate(m, &dirs), None);

        std::fs::write(folder.join(&m.projector.file), b"gguf").unwrap();
        let found = locate(m, &dirs).unwrap();
        assert_eq!(found.path, folder.join(&m.file));
        assert_eq!(found.projector, folder.join(&m.projector.file));
    }

    #[test]
    fn the_recording_goes_as_a_form_file() {
        let (content_type, body) = multipart(b"RIFFdata");
        let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
        let body = String::from_utf8(body).unwrap();
        assert!(body.starts_with(&format!("--{boundary}\r\n")));
        assert!(body.ends_with(&format!("\r\n--{boundary}--\r\n")));
        assert!(body.contains("name=\"stream\"\r\n\r\ntrue\r\n"));
        assert!(body.contains("name=\"response_format\"\r\n\r\njson\r\n"));
        assert!(body.contains("filename=\"recording.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFFdata\r\n"));
    }

    /// A server answering one request at a time with `answers` (status, content type, body), in
    /// order. Returns its `/v1` base URL and, once all are answered, the request lines.
    async fn serve(
        answers: Vec<(&'static str, &'static str, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut lines = Vec::new();
            for (status, kind, body) in answers {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let read = socket.read(&mut buf).await.unwrap_or(0);
                    request.extend_from_slice(&buf[..read]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(str::to_string)
                            })
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if read == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request);
                lines.push(text.lines().next().unwrap_or_default().to_string());
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
            lines
        });
        (base, task)
    }

    #[tokio::test]
    async fn the_words_stream_from_the_transcription_endpoint() {
        let events = [
            r#"{"type":"transcript.text.delta","delta":"language English<asr_text>Buy"}"#,
            r#"{"type":"transcript.text.delta","delta":" EURUSD."}"#,
            r#"{"type":"transcript.text.done","text":"language English<asr_text>Buy EURUSD."}"#,
        ]
        .iter()
        .map(|e| format!("data: {e}\n\n"))
        .collect::<String>();
        let (base, requests) = serve(vec![("200 OK", "text/event-stream", events)]).await;
        let mut deltas = Vec::new();
        let (raw, via) = Transcriber::ask(
            &reqwest::Client::new(),
            &base,
            b"RIFF",
            &CancellationToken::new(),
            &mut |d: &str| deltas.push(d.to_string()),
            async { true },
        )
        .await
        .unwrap();
        assert_eq!(via, Via::Endpoint);
        assert_eq!(deltas, ["language English<asr_text>Buy", " EURUSD."]);
        assert_eq!(clean::clean(&raw), (Some("English".into()), "Buy EURUSD.".into()));
        assert_eq!(requests.await.unwrap(), ["POST /v1/audio/transcriptions HTTP/1.1"]);
    }

    #[tokio::test]
    async fn a_server_without_the_endpoint_is_asked_through_chat() {
        let chat = r#"{"choices":[{"message":{"content":"language English<asr_text>Sell."}}]}"#;
        let (base, requests) = serve(vec![
            (
                "404 Not Found",
                "application/json",
                r#"{"error":{"message":"File Not Found"}}"#.into(),
            ),
            ("200 OK", "application/json", chat.into()),
        ])
        .await;
        let (raw, via) = Transcriber::ask(
            &reqwest::Client::new(),
            &base,
            b"RIFF",
            &CancellationToken::new(),
            &mut |_: &str| {},
            async { true },
        )
        .await
        .unwrap();
        assert_eq!(via, Via::Chat);
        assert_eq!(clean::clean(&raw).1, "Sell.");
        assert_eq!(
            requests.await.unwrap(),
            [
                "POST /v1/audio/transcriptions HTTP/1.1",
                "POST /v1/chat/completions HTTP/1.1"
            ]
        );

        // A server that stopped is not asked again.
        let (base, _) = serve(vec![("500 Internal Server Error", "text/plain", "crashed".into())]).await;
        let failed = Transcriber::ask(
            &reqwest::Client::new(),
            &base,
            b"RIFF",
            &CancellationToken::new(),
            &mut |_: &str| {},
            async { false },
        )
        .await;
        assert!(matches!(failed, Err(Failure::Failed(e)) if e.contains("500")));
    }
}
