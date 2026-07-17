//! Bundled SearXNG: a local metasearch instance, run through the portable Python runtime
//! (`python.rs`). Gives Demido a private, non-scraping-on-our-end search fallback that
//! covers ~200 upstream engines (Google, Bing, Brave, etc.) via SearXNG's own maintained
//! engine list — we never touch those engines directly, only SearXNG's search code.
//!
//! **No server, no port.** SearXNG's Flask app is driven *in-process* inside a worker
//! child via Flask's test client (`worker.py`), and the worker speaks JSON-lines over its
//! own stdin/stdout pipes. Nothing binds a socket, so nothing else on the machine — not
//! another local process, not another user — can reach this instance; only Demido, which
//! owns the pipe. The worker is long-lived because SearXNG's engine initialisation costs
//! a second or two and would otherwise be paid on every query.
//!
//! Source is pulled from SearXNG's `master` branch tarball (SearXNG has no stable release
//! channel; tracking master is the documented deployment practice). Our own `settings.yml`
//! stays minimal via `use_default_settings: true`, only overriding search/cache.

use futures_util::StreamExt;
use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::local::python;

const SOURCE_URL: &str = "https://github.com/searxng/searxng/archive/refs/heads/master.tar.gz";

/// Drives SearXNG's Flask app in-process through its test client — same code path the HTTP
/// server would take, minus the socket. One JSON request per stdin line, one JSON response
/// per stdout line. SearXNG logs to stderr, which we redirect to a file, so stdout stays a
/// clean protocol channel.
const WORKER_PY: &str = r#"import json, sys

def _emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

try:
    from searx.webapp import app
    client = app.test_client()
except Exception as e:  # noqa: BLE001 - startup failure must reach Rust as data
    _emit({"error": "searxng init failed: %s" % e})
    sys.exit(1)

_emit({"ready": True})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
        resp = client.get("/search", query_string={"q": req["q"], "format": "json"})
        if resp.status_code != 200:
            _emit({"error": "searxng status %d" % resp.status_code})
            continue
        _emit({"results": json.loads(resp.get_data(as_text=True)).get("results", [])})
    except Exception as e:  # noqa: BLE001 - never let one bad query kill the worker
        _emit({"error": str(e)})
"#;

struct Running {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// `running` is an `Arc` so blocking work (spawn, query round-trips) can be moved onto
/// `spawn_blocking` without borrowing the managed state across an await.
#[derive(Default, Clone)]
pub struct SearxngEngine {
    running: std::sync::Arc<Mutex<Option<Running>>>,
}

impl SearxngEngine {
    /// Whether a worker is live (does not check liveness deeply — callers that need
    /// certainty should just try the query and fall through on failure).
    pub fn is_running(&self) -> bool {
        let mut guard = self.running.lock().unwrap();
        match guard.as_mut() {
            Some(r) => {
                if matches!(r.child.try_wait(), Ok(None)) {
                    true
                } else {
                    *guard = None;
                    false
                }
            }
            None => false,
        }
    }

    pub fn stop(&self) {
        if let Some(mut r) = self.running.lock().unwrap().take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }

    /// One round-trip on the worker pipe. Blocking — call from `spawn_blocking`.
    fn request(&self, query: &str) -> Result<Vec<serde_json::Value>, String> {
        let mut guard = self.running.lock().unwrap();
        let r = guard.as_mut().ok_or("SearXNG is not running")?;
        let line = serde_json::json!({ "q": query }).to_string();
        r.stdin
            .write_all(format!("{}\n", line).as_bytes())
            .and_then(|_| r.stdin.flush())
            .map_err(|e| format!("SearXNG worker write failed: {}", e))?;
        let mut resp = String::new();
        if r.stdout
            .read_line(&mut resp)
            .map_err(|e| format!("SearXNG worker read failed: {}", e))?
            == 0
        {
            *guard = None;
            return Err("SearXNG worker exited".into());
        }
        let v: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| format!("SearXNG worker bad JSON: {}", e))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(format!("SearXNG error: {}", err));
        }
        Ok(v.get("results")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default())
    }
}

fn base_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir").join("searxng")
}

fn src_dir(app: &AppHandle) -> PathBuf {
    base_dir(app).join("src")
}

fn settings_path(app: &AppHandle) -> PathBuf {
    base_dir(app).join("settings.yml")
}

pub fn source_ready(app: &AppHandle) -> bool {
    src_dir(app).join("searx").join("webapp.py").exists()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    stage: String,
}

fn emit_stage(app: &AppHandle, stage: &str) {
    let _ = app.emit("searxng_install_progress", InstallProgress { stage: stage.into() });
}

/// Download SearXNG's source tarball, stripping the single top-level directory GitHub's
/// codeload wraps it in, so `searx/` lands directly under `src_dir`.
async fn install_source(app: &AppHandle, client: &reqwest::Client) -> Result<(), String> {
    emit_stage(app, "downloading SearXNG source");
    let dir = src_dir(app);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let resp = client
        .get(SOURCE_URL)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("SearXNG source download failed: {}", e))?;
    let archive = base_dir(app).join("searxng-src.tar.gz");
    let mut f = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        f.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);

    extract_targz_stripped(&archive, &dir)?;
    let _ = std::fs::remove_file(&archive);

    if !source_ready(app) {
        return Err("Extraction succeeded but searx/webapp.py not found".into());
    }
    Ok(())
}

/// Extracts a `.tar.gz`, dropping each entry's first path component (GitHub codeload
/// tarballs wrap everything in a single `<repo>-<branch>/` directory).
fn extract_targz_stripped(archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let rel = entry.path().map_err(|e| e.to_string())?.into_owned();
        let mut components = rel.components();
        components.next(); // drop the wrapping top-level dir
        let stripped: PathBuf = components.collect();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&stripped);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn install_deps(app: &AppHandle) -> Result<(), String> {
    emit_stage(app, "installing dependencies (this can take a few minutes the first time)");
    let req = src_dir(app).join("requirements.txt");
    let out = Command::new(python::python_bin(app))
        .args(["-m", "pip", "install", "--disable-pip-version-check", "--no-warn-script-location", "-r"])
        .arg(&req)
        .output()
        .map_err(|e| format!("Failed to run pip: {}", e))?;
    if !out.status.success() {
        return Err(format!("pip install failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(())
}

/// `server.port` / `bind_address` are still required keys for SearXNG's settings schema even
/// though nothing ever listens — the Flask app is only ever driven through its test client.
fn write_settings(app: &AppHandle, secret: &str) -> Result<(), String> {
    let yml = format!(
        r#"use_default_settings: true
general:
  debug: false
server:
  port: 0
  bind_address: "127.0.0.1"
  secret_key: "{secret}"
  limiter: false
  image_proxy: false
search:
  formats:
    - json
redis:
  url: false
"#,
        secret = secret,
    );
    std::fs::write(settings_path(app), yml).map_err(|e| e.to_string())
}

/// SearXNG's `valkeydb.py` unconditionally `import pwd` (POSIX-only) for a log line that
/// only fires when a Valkey/Redis connection fails — which never happens here since
/// `settings.yml` disables Redis. On Windows there's no real `pwd` module, so drop a stub
/// straight into site-packages to satisfy the import.
fn write_windows_shims(app: &AppHandle) -> Result<(), String> {
    if !cfg!(windows) {
        return Ok(());
    }
    let dir = python::windows_site_packages(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stub = "class _PwEntry:\n    pw_name = \"demido\"\n    pw_uid = 0\n\n\ndef getpwuid(uid):\n    return _PwEntry()\n";
    std::fs::write(dir.join("pwd.py"), stub).map_err(|e| e.to_string())
}

/// Install Python + SearXNG source + deps without starting the worker. Used by the
/// first-launch provisioner and the Engine > Python tab's Install button.
pub async fn install(app: &AppHandle, client: &reqwest::Client) -> Result<(), String> {
    if !python::python_ready(app) {
        emit_stage(app, "installing portable Python runtime");
        python::ensure_python(app, client).await?;
    }
    write_windows_shims(app)?;
    if !source_ready(app) {
        install_source(app, client).await?;
        let app2 = app.clone();
        // pip resolves + builds wheels synchronously for minutes; keep it off the async pool.
        tokio::task::spawn_blocking(move || install_deps(&app2))
            .await
            .map_err(|e| format!("SearXNG dependency task failed: {}", e))??;
    }
    emit_stage(app, "ready");
    Ok(())
}

/// Remove SearXNG's source, settings and worker. The Python runtime stays (it is generic);
/// the pip-installed deps live inside it and are left behind — a later reinstall reuses them.
pub fn uninstall(app: &AppHandle, engine: &SearxngEngine) -> Result<(), String> {
    engine.stop();
    let dir = base_dir(app);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove SearXNG: {}", e))
}

/// Ensure Python + SearXNG source + deps are installed, then (re)spawn the worker and wait
/// for its `ready` line. Heavy the first time (downloads + pip install); a no-op if the
/// worker is already live.
pub async fn ensure_running(
    app: &AppHandle,
    client: &reqwest::Client,
    engine: &SearxngEngine,
) -> Result<(), String> {
    if engine.is_running() {
        return Ok(());
    }

    install(app, client).await?;

    let secret = uuid::Uuid::new_v4().simple().to_string();
    write_settings(app, &secret)?;
    let worker = base_dir(app).join("worker.py");
    std::fs::write(&worker, WORKER_PY).map_err(|e| e.to_string())?;

    emit_stage(app, "starting SearXNG");
    let app = app.clone();
    let engine = engine.clone();
    // Blocking: spawn plus SearXNG's engine init reads the pipe synchronously for seconds.
    tokio::task::spawn_blocking(move || spawn_worker(&app, &engine, &worker))
        .await
        .map_err(|e| format!("SearXNG start task failed: {}", e))?
}

/// Bring the worker up in the background at launch, if the user enabled SearXNG and it is
/// actually installed. Never installs anything: what gets downloaded is the first-run setup
/// wizard's decision, not a side effect of launching.
///
/// Failures are silent by design: `web_search` falls through to the other providers, and
/// the Engine > Python tab surfaces the real state.
pub fn start_on_startup(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<crate::commands::AppState>() else {
            return;
        };
        let enabled = {
            let conn = state.conn.lock().unwrap();
            crate::db::settings::get(&conn, "websearch_searxng_enabled")
                .ok()
                .flatten()
                .unwrap_or_else(|| "false".into())
                == "true"
        };

        if !enabled || !python::python_ready(&app) || !source_ready(&app) {
            return;
        }
        let client = state.http_client.clone();
        let engine = state.searxng_engine.clone();
        let _ = ensure_running(&app, &client, &engine).await;
    });
}

fn spawn_worker(app: &AppHandle, engine: &SearxngEngine, worker: &std::path::Path) -> Result<(), String> {
    // SearXNG logs to stderr; captured to a file (not piped) so a startup crash surfaces the
    // real Python traceback, and so a full stderr pipe can never deadlock the worker.
    let log_path = base_dir(app).join("searxng.log");
    let log_file = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut child = Command::new(python::python_bin(app))
        .arg(worker)
        .current_dir(src_dir(app))
        .env("SEARXNG_SETTINGS_PATH", settings_path(app))
        .env("PYTHONPATH", src_dir(app))
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log_file)
        .spawn()
        .map_err(|e| format!("Failed to start SearXNG: {}", e))?;

    let stdin = child.stdin.take().ok_or("no worker stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("no worker stdout")?);

    let mut line = String::new();
    let read = stdout.read_line(&mut line);
    let ok = match read {
        Ok(0) | Err(_) => false,
        Ok(_) => serde_json::from_str::<serde_json::Value>(&line)
            .map(|v| v.get("ready").and_then(|r| r.as_bool()).unwrap_or(false))
            .unwrap_or(false),
    };
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        let detail = serde_json::from_str::<serde_json::Value>(&line)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_default();
        return Err(format!("SearXNG failed to start. {}\n{}", detail, tail_log(&log_path)));
    }

    *engine.running.lock().unwrap() = Some(Running { child, stdin, stdout });
    emit_stage(app, "ready");
    Ok(())
}

/// Last ~2000 bytes of the startup log, for surfacing a Python traceback in error messages.
fn tail_log(path: &std::path::Path) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let bytes = content.as_bytes();
    let start = bytes.len().saturating_sub(2000);
    let tail = String::from_utf8_lossy(&bytes[start..]);
    format!("--- last output ---\n{}", tail)
}

/// Query the running worker over its pipe. Returns formatted result text.
pub async fn search(engine: &SearxngEngine, query: &str) -> Result<String, String> {
    let engine = engine.clone();
    let query = query.to_string();
    let results = tokio::task::spawn_blocking(move || engine.request(&query))
        .await
        .map_err(|e| format!("SearXNG task failed: {}", e))??;
    if results.is_empty() {
        return Ok(String::new());
    }
    let mut lines = Vec::new();
    for (i, r) in results.iter().take(15).enumerate() {
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
        lines.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, content));
    }
    Ok(lines.join("\n\n"))
}
