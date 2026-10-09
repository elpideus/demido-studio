//! The local inference runtime: one `llama-server` process serving the selected model.
//!
//! [`LocalRuntime::ensure`] is the only entry point that matters: it returns the address of a
//! server running exactly the requested launch settings, starting or restarting the process when
//! needed. One model is resident at a time, because a second one would compete for the GPU. The
//! search model makes room while one loads (see `attachments::meaning`).

pub mod job;

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::attachments::meaning::Embedder;

pub const STATUS_EVENT: &str = "runtime://status";
const LOG_LINES: usize = 400;
const LOAD_TIMEOUT: Duration = Duration::from_secs(420);
/// Tokens one picture may take, and the micro-batch llama.cpp decodes them in, when a model loads
/// its projector (see where the server's arguments are built).
const VISION_BATCH: u32 = 2048;
/// Least context llama.cpp may fit a model to: below it, a turn with a few tool results no longer
/// fits beside the system prompt and the tools (about 6000 tokens with every tool). A model that
/// does not fit the GPU with this much gets layers moved to the CPU instead.
const MIN_FIT_CONTEXT: u32 = 16384;
/// GPU memory, in MiB, llama.cpp leaves free when it fits a model (its own default), besides
/// the search model's. About 370 MiB of it stays free in the end on an RTX 3060: fitting does not
/// count the CUDA context.
const FIT_MARGIN_MB: u32 = 1024;

/// What to run. Two specs that compare equal can share a server.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSpec {
    pub model_id: String,
    pub model_name: String,
    pub path: PathBuf,
    /// `None` lets llama.cpp size it to the GPU's free memory (see where the server's arguments
    /// are built).
    pub context_length: Option<u32>,
    /// `None` lets llama.cpp fit as many layers as the GPU holds.
    pub gpu_layers: Option<u32>,
    pub mmproj: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeState {
    /// No model loaded.
    Idle,
    Loading,
    Ready,
    Error,
    /// This installation has no local runtime.
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub state: RuntimeState,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub message: Option<String>,
    pub context_length: Option<u32>,
    /// Seconds the last load took.
    pub load_seconds: Option<f64>,
}

/// A server running a model.
#[derive(Clone, Debug)]
pub struct Served {
    /// The `/v1` base URL.
    pub base_url: String,
    /// Tokens the model can see at once: the context of the server's one slot.
    pub context_length: u32,
}

struct Running {
    spec: LaunchSpec,
    port: u16,
    context_length: u32,
    child: Child,
}

pub struct LocalRuntime {
    app: AppHandle,
    /// The search model, which stops for a chat model to load.
    search: Arc<Embedder>,
    server: Option<PathBuf>,
    log_file: PathBuf,
    http: reqwest::Client,
    running: tokio::sync::Mutex<Option<Running>>,
    status: Mutex<RuntimeStatus>,
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl LocalRuntime {
    pub fn new(app: AppHandle, search: Arc<Embedder>, server: Option<PathBuf>, logs_dir: PathBuf) -> Self {
        job::init();
        let state = if server.is_some() {
            RuntimeState::Idle
        } else {
            RuntimeState::Unavailable
        };
        Self {
            app,
            search,
            server,
            log_file: logs_dir.join("llama-server.log"),
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            running: tokio::sync::Mutex::new(None),
            status: Mutex::new(RuntimeStatus {
                state,
                model_id: None,
                model_name: None,
                message: None,
                context_length: None,
                load_seconds: None,
            }),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES))),
        }
    }

    pub fn available(&self) -> bool {
        self.server.is_some()
    }

    pub fn status(&self) -> RuntimeStatus {
        self.status.lock().clone()
    }

    pub fn recent_logs(&self) -> Vec<String> {
        self.logs.lock().iter().cloned().collect()
    }

    fn set_status(&self, status: RuntimeStatus) {
        *self.status.lock() = status.clone();
        let _ = self.app.emit(STATUS_EVENT, status);
    }

    /// Returns the server running `spec`, starting one if needed.
    pub async fn ensure(&self, spec: &LaunchSpec) -> Result<Served, String> {
        let server = self.server.clone().ok_or_else(|| {
            "The local AI runtime is not installed. Run the Demido Studio installer again, or pick a cloud model."
                .to_string()
        })?;
        if !spec.path.is_file() {
            return Err(format!("The model file is missing: {}", spec.path.display()));
        }

        let mut running = self.running.lock().await;
        if let Some(r) = running.as_mut() {
            let alive = matches!(r.child.try_wait(), Ok(None));
            if alive && r.spec == *spec {
                return Ok(Served {
                    base_url: base_url(r.port),
                    context_length: r.context_length,
                });
            }
        }
        // The chat model comes first: the search model stops, and stays stopped while it loads.
        let _room = self.search.make_room().await;
        if let Some(mut old) = running.take() {
            let _ = old.child.kill().await;
            let _ = old.child.wait().await;
        }

        self.set_status(RuntimeStatus {
            state: RuntimeState::Loading,
            model_id: Some(spec.model_id.clone()),
            model_name: Some(spec.model_name.clone()),
            message: None,
            context_length: spec.context_length,
            load_seconds: None,
        });
        let started = Instant::now();
        match self.launch(&server, spec).await {
            Ok((child, port)) => {
                let seconds = started.elapsed().as_secs_f64();
                let context_length = match self.served_context(port).await {
                    Some(n) => n,
                    None => {
                        tracing::warn!("llama-server did not report its context size");
                        spec.context_length.unwrap_or(MIN_FIT_CONTEXT)
                    }
                };
                tracing::info!(model = %spec.model_name, port, context_length, "model ready in {seconds:.1}s");
                *running = Some(Running {
                    spec: spec.clone(),
                    port,
                    context_length,
                    child,
                });
                self.set_status(RuntimeStatus {
                    state: RuntimeState::Ready,
                    model_id: Some(spec.model_id.clone()),
                    model_name: Some(spec.model_name.clone()),
                    message: None,
                    context_length: Some(context_length),
                    load_seconds: Some(seconds),
                });
                Ok(Served {
                    base_url: base_url(port),
                    context_length,
                })
            }
            Err(err) => {
                tracing::error!(model = %spec.model_name, "model failed to load: {err}");
                self.set_status(RuntimeStatus {
                    state: RuntimeState::Error,
                    model_id: Some(spec.model_id.clone()),
                    model_name: Some(spec.model_name.clone()),
                    message: Some(err.clone()),
                    context_length: None,
                    load_seconds: None,
                });
                Err(err)
            }
        }
    }

    /// The model currently resident, if any.
    pub async fn loaded_model(&self) -> Option<String> {
        let mut running = self.running.lock().await;
        let r = running.as_mut()?;
        matches!(r.child.try_wait(), Ok(None)).then(|| r.spec.model_id.clone())
    }

    /// Unloads the model and stops the server.
    pub async fn stop(&self) {
        let mut running = self.running.lock().await;
        if let Some(mut r) = running.take() {
            let _ = r.child.kill().await;
            let _ = r.child.wait().await;
        }
        if self.available() {
            self.set_status(RuntimeStatus {
                state: RuntimeState::Idle,
                model_id: None,
                model_name: None,
                message: None,
                context_length: None,
                load_seconds: None,
            });
        }
    }

    /// Synchronous best-effort kill for app shutdown.
    pub fn kill_now(&self) {
        if let Ok(mut running) = self.running.try_lock()
            && let Some(r) = running.as_mut()
        {
            let _ = r.child.start_kill();
        }
    }

    async fn launch(&self, server: &PathBuf, spec: &LaunchSpec) -> Result<(Child, u16), String> {
        let port = free_port().map_err(|e| format!("no free local port: {e}"))?;
        let mut args: Vec<String> = vec![
            "-m".into(),
            spec.path.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "-np".into(),
            "1".into(),
            "--no-webui".into(),
            "--jinja".into(),
            "--reasoning-format".into(),
            "deepseek".into(),
            "--alias".into(),
            spec.model_id.clone(),
        ];
        match spec.context_length {
            Some(n) => args.extend(["-c".into(), n.to_string()]),
            // Without -c, llama.cpp takes the trained context, or as much of it as fits while this
            // much GPU memory stays free: its margin, and room for the search model, which runs
            // beside the chat model. It counts the projector and the micro-batch set below.
            None => args.extend([
                "--fit".into(),
                "on".into(),
                "--fit-ctx".into(),
                MIN_FIT_CONTEXT.to_string(),
                "--fit-target".into(),
                (FIT_MARGIN_MB + self.search.gpu_memory_mb()).to_string(),
            ]),
        }
        if let Some(layers) = spec.gpu_layers {
            args.push("-ngl".into());
            args.push(layers.to_string());
        }
        if let Some(mmproj) = &spec.mmproj {
            args.push("--mmproj".into());
            args.push(mmproj.to_string_lossy().into_owned());
            // A picture's tokens are decoded in one micro-batch, with attention across all of
            // them for models such as Gemma: a picture larger than the micro-batch (512 tokens by
            // default; a 1380×880 screenshot is 545 for Gemma 4 26B) stops llama-server on an
            // assertion. The micro-batch is made big enough, and a picture never larger.
            args.push("--ubatch-size".into());
            args.push(VISION_BATCH.to_string());
            args.push("--image-max-tokens".into());
            args.push(VISION_BATCH.to_string());
        }

        let mut cmd = Command::new(server);
        cmd.args(&args)
            .current_dir(server.parent().unwrap_or(std::path::Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        self.log_line(&format!(
            "---- {} ----\n$ {} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            server.display(),
            args.join(" ")
        ));
        let mut child = cmd.spawn().map_err(|e| format!("could not start llama-server: {e}"))?;
        job::adopt(&child);

        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let logs = self.logs.clone();
            let file = self.log_file.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                let mut out = std::fs::OpenOptions::new().create(true).append(true).open(&file).ok();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(f) = out.as_mut() {
                        let _ = writeln!(f, "{line}");
                    }
                    let mut buf = logs.lock();
                    if buf.len() >= LOG_LINES {
                        buf.pop_front();
                    }
                    buf.push_back(line);
                }
            });
        }

        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + LOAD_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "llama-server exited while loading the model ({status}).{}",
                    self.explain_failure()
                ));
            }
            if let Ok(resp) = self.http.get(&health).send().await
                && resp.status().is_success()
            {
                return Ok((child, port));
            }
            if Instant::now() > deadline {
                let _ = child.kill().await;
                return Err("the model took too long to load".into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// The context of the server's slot, as `/props` reports it.
    async fn served_context(&self, port: u16) -> Option<u32> {
        let props: serde_json::Value = self
            .http
            .get(format!("http://127.0.0.1:{port}/props"))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        props
            .pointer("/default_generation_settings/n_ctx")?
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0)
    }

    fn log_line(&self, line: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Turns the last log lines into a hint about why loading failed.
    fn explain_failure(&self) -> String {
        let logs = self.logs.lock();
        let tail: Vec<&String> = logs.iter().rev().take(40).collect();
        let joined = tail
            .iter()
            .rev()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .to_ascii_lowercase();
        if joined.contains("out of memory") || joined.contains("failed to allocate") {
            " The model does not fit in memory: pick a smaller model or lower its context length.".into()
        } else if joined.contains("unknown model architecture") {
            " This model's architecture is not supported by the installed runtime.".into()
        } else if joined.contains("failed to load model") || joined.contains("invalid") {
            " The model file could not be read; it may be damaged or incomplete.".into()
        } else {
            let last = logs.back().cloned().unwrap_or_default();
            if last.is_empty() {
                String::new()
            } else {
                format!(" Last message: {last}")
            }
        }
    }
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

pub(crate) fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
