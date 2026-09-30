//! Asks llama.cpp what a local model can do.
//!
//! [`run`] starts `llama-server` with the model (and its projector) on the CPU only, reads what
//! the server reports once it is up, and stops it. Nothing is generated. llama.cpp's own answers
//! are used, never a guess from the file name or the chat template:
//!
//! - `GET /props`: `modalities` (what the projector lets it read) and `chat_template_caps`
//!   (whether the chat template can express tool calls);
//! - thinking, which the server works out from the template at start and states only in its
//!   trace log (`chat template, thinking = 1`).
//!
//! When llama.cpp moves to a new build, check that both are still there (see `docs/catalog.md`).

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::capabilities::Capabilities;
use crate::runtime::{free_port, job};

/// A model on a slow disk can take a while even without its weights being read.
const START_TIMEOUT: Duration = Duration::from_secs(180);
const THINKING_VERDICT: &str = "chat template, thinking = ";

/// Starts `server` on `model` and returns what it reports. With a projector that llama.cpp
/// refuses, the model is asked again without it: it then reads neither images nor sound.
pub async fn run(
    server: &Path,
    model: &Path,
    projector: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<Capabilities, String> {
    match ask(server, model, projector, cancel).await {
        Err(e) if projector.is_some() && !cancel.is_cancelled() => {
            tracing::warn!(model = %model.display(), "llama.cpp refused the projector, asking without it: {e}");
            ask(server, model, None, cancel).await
        }
        other => other,
    }
}

async fn ask(
    server: &Path,
    model: &Path,
    projector: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<Capabilities, String> {
    let port = free_port().map_err(|e| format!("no free local port: {e}"))?;
    let mut cmd = Command::new(server);
    cmd.arg("-m").arg(model);
    if let Some(p) = projector {
        cmd.arg("--mmproj").arg(p).arg("--no-mmproj-offload");
    }
    cmd.args([
        "--jinja",
        // Nothing on the GPU: the model the person is using keeps all of it.
        "--device",
        "none",
        "-ngl",
        "0",
        "--no-warmup",
        "--no-repack",
        "--fit",
        "off",
        // `--numa` turns off llama.cpp's prefetch of the whole file (`llama-mmap.cpp`), so only
        // what loading touches is read from disk.
        "--numa",
        "distribute",
        "-c",
        "512",
        "-np",
        "1",
        "--no-webui",
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        // The thinking verdict is a trace message.
        "--log-verbosity",
        "4",
        "--log-colors",
        "off",
    ])
    .current_dir(server.parent().unwrap_or(Path::new(".")))
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000 | 0x0000_4000); // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS

    let mut child = cmd.spawn().map_err(|e| format!("could not start llama-server: {e}"))?;
    job::adopt(&child);

    let log = Arc::new(Mutex::new(Log::default()));
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn AsyncRead + Unpin + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let log = log.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log.lock().push(line);
            }
        });
    }

    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + START_TIMEOUT;
    let result = loop {
        if cancel.is_cancelled() {
            break Err("stopped".to_string());
        }
        if let Ok(Some(status)) = child.try_wait() {
            break Err(format!(
                "llama-server could not load the model ({status}). {}",
                log.lock().tail()
            ));
        }
        if let Ok(resp) = http.get(format!("{base}/health")).send().await
            && resp.status().is_success()
        {
            break match http.get(format!("{base}/props")).send().await {
                Ok(resp) => resp.json::<Value>().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
        }
        if Instant::now() > deadline {
            break Err("llama-server took too long to load the model".to_string());
        }
        tokio::select! {
            _ = cancel.cancelled() => {}
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    };
    let _ = child.kill().await;
    let _ = child.wait().await;

    let props = result?;
    let thinking = log.lock().thinking;
    Ok(from_props(&props, thinking))
}

/// What the server printed: the thinking verdict, and the last lines for an error message.
#[derive(Default)]
struct Log {
    thinking: Option<bool>,
    last: Vec<String>,
}

impl Log {
    fn push(&mut self, line: String) {
        if let Some(verdict) = thinking_verdict(&line) {
            self.thinking = Some(verdict);
        }
        if self.last.len() == 12 {
            self.last.remove(0);
        }
        self.last.push(line);
    }

    fn tail(&self) -> String {
        self.last
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(|l| format!("Last message: {}", l.trim()))
            .unwrap_or_default()
    }
}

/// `srv init: init: chat template, thinking = 1` → `Some(true)`.
fn thinking_verdict(line: &str) -> Option<bool> {
    let rest = &line[line.find(THINKING_VERDICT)? + THINKING_VERDICT.len()..];
    match rest.trim().chars().next()? {
        '0' => Some(false),
        '1' => Some(true),
        _ => None,
    }
}

/// Capabilities from `/props` and the thinking verdict. A field the server did not report
/// stays unknown.
fn from_props(props: &Value, thinking: Option<bool>) -> Capabilities {
    let modality = |name: &str| props["modalities"][name].as_bool();
    Capabilities {
        vision: modality("vision"),
        audio: modality("audio"),
        // Tools are usable when the template can express a call, which is also what lets
        // llama.cpp parse one out of the answer; tool descriptions it can fill in itself.
        tools: props["chat_template_caps"]["supports_tool_calls"].as_bool(),
        thinking,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_thinking_verdict_from_the_trace_log() {
        assert_eq!(
            thinking_verdict("0.06.669.416 I srv          init: init: chat template, thinking = 1"),
            Some(true)
        );
        assert_eq!(thinking_verdict("srv init: chat template, thinking = 0"), Some(false));
        assert_eq!(thinking_verdict("srv load_model: loading model"), None);
    }

    #[test]
    fn capabilities_come_from_props() {
        // What llama.cpp b11146 reports for Gemma 4 E4B with its projector.
        let props = json!({
            "modalities": {"vision": true, "video": true, "audio": true},
            "chat_template_caps": {"supports_tool_calls": true, "supports_tools": true},
        });
        assert_eq!(
            from_props(&props, Some(true)),
            Capabilities {
                vision: Some(true),
                audio: Some(true),
                tools: Some(true),
                thinking: Some(true),
            }
        );
        let bare = json!({
            "modalities": {"vision": false, "audio": false},
            "chat_template_caps": {"supports_tool_calls": false},
        });
        assert_eq!(
            from_props(&bare, None),
            Capabilities {
                vision: Some(false),
                audio: Some(false),
                tools: Some(false),
                thinking: None,
            }
        );
    }
}
