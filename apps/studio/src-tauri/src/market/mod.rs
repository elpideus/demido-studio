//! Market data.
//!
//! The data itself comes from a small Node service (`sidecars/market`) that wraps
//! [TradingView-API](https://github.com/Mathieu2301/Tradingview-API) for real-time data and keeps
//! a local store of history downloaded from Dukascopy (and cached from TradingView). This module
//! starts that service on first use (or at launch when a download was left running), speaks JSON
//! lines with it over stdio, forwards its live updates to the UI, asks it to flush on exit, and
//! owns the TradingView session: the person signs in once in a pop-up window whose cookies are
//! captured (see [`auth`]).

pub mod auth;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;

use crate::secrets::{Secrets, TRADINGVIEW_SESSION};

pub const EVENT: &str = "market://event";
pub const STATUS_EVENT: &str = "market://status";

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketStatus {
    /// Node and the service script are installed.
    pub available: bool,
    pub reason: Option<String>,
    pub running: bool,
    pub logged_in: bool,
    pub username: Option<String>,
    /// A sign-in window is open.
    pub login_pending: bool,
    /// Why the last sign-in attempt failed, if it did.
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

impl RpcError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn is_auth(&self) -> bool {
        self.code == "NOT_LOGGED_IN" || self.code == "SESSION_EXPIRED"
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// The TradingView cookies the service authenticates with.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub session: String,
    pub signature: String,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>>;

pub struct MarketService {
    app: AppHandle,
    node: Option<PathBuf>,
    script: PathBuf,
    cache_dir: PathBuf,
    log_file: PathBuf,
    pub(crate) profile_dir: PathBuf,
    secrets: Arc<Secrets>,
    proc: tokio::sync::Mutex<Option<Proc>>,
    pending: Pending,
    next_id: AtomicU64,
    /// The app is exiting: a late call must not start a new service.
    closing: AtomicBool,
    status: RwLock<MarketStatus>,
    /// The sign-in flow in progress, shared by everyone waiting on it.
    pub(crate) login: tokio::sync::Mutex<Option<tokio::sync::watch::Receiver<auth::LoginState>>>,
    me: Weak<MarketService>,
}

impl MarketService {
    pub fn new(
        app: AppHandle,
        node: Option<PathBuf>,
        script: PathBuf,
        cache_dir: PathBuf,
        logs_dir: PathBuf,
        profile_dir: PathBuf,
        secrets: Arc<Secrets>,
    ) -> Arc<Self> {
        let reason = match (&node, script.is_file()) {
            (None, _) => Some("Node.js is not installed. Run the Demido Studio installer again to add it.".to_string()),
            (_, false) => Some(format!("The market data service is missing ({}).", script.display())),
            _ => None,
        };
        let has_session = secrets.get(TRADINGVIEW_SESSION).is_some();
        Arc::new_cyclic(|me| Self {
            app,
            status: RwLock::new(MarketStatus {
                available: reason.is_none(),
                reason,
                running: false,
                // Optimistic until the service confirms or rejects the saved session.
                logged_in: has_session,
                username: None,
                login_pending: false,
                last_error: None,
            }),
            node,
            script,
            cache_dir,
            log_file: logs_dir.join("market.log"),
            profile_dir,
            secrets,
            proc: tokio::sync::Mutex::new(None),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            closing: AtomicBool::new(false),
            login: tokio::sync::Mutex::new(None),
            me: me.clone(),
        })
    }

    pub fn status(&self) -> MarketStatus {
        self.status.read().clone()
    }

    pub(crate) fn update_status(&self, f: impl FnOnce(&mut MarketStatus)) {
        let snapshot = {
            let mut s = self.status.write();
            f(&mut s);
            s.clone()
        };
        let _ = self.app.emit(STATUS_EVENT, snapshot);
    }

    pub fn logged_in(&self) -> bool {
        self.status.read().logged_in
    }

    /// Calls a service method, starting the service when needed.
    pub async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        self.ensure_started().await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let line = format!("{}\n", json!({"id": id, "method": method, "params": params}));
        {
            let mut proc = self.proc.lock().await;
            let Some(p) = proc.as_mut() else {
                self.pending.lock().remove(&id);
                return Err(RpcError::new("UNAVAILABLE", "the market service is not running"));
            };
            if let Err(e) = p.stdin.write_all(line.as_bytes()).await {
                self.pending.lock().remove(&id);
                *proc = None;
                return Err(RpcError::new("UNAVAILABLE", format!("the market service stopped: {e}")));
            }
            let _ = p.stdin.flush().await;
        }
        let result = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcError::new("UNAVAILABLE", "the market service stopped")),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(RpcError::new(
                    "TIMEOUT",
                    format!("the market service did not answer {method} in time"),
                ))
            }
        };
        if let Err(e) = &result {
            if e.code == "SESSION_EXPIRED" {
                self.forget_session();
            } else if e.code == "NOT_LOGGED_IN" && self.logged_in() && self.saved_session().is_none() {
                self.update_status(|s| s.logged_in = false);
            }
        }
        result
    }

    async fn ensure_started(&self) -> Result<(), RpcError> {
        let mut proc = self.proc.lock().await;
        if let Some(p) = proc.as_mut() {
            if matches!(p.child.try_wait(), Ok(None)) {
                return Ok(());
            }
            *proc = None;
        }
        if self.closing.load(Ordering::Relaxed) {
            return Err(RpcError::new("UNAVAILABLE", "the app is closing"));
        }
        let status = self.status();
        let Some(node) = self.node.clone().filter(|_| status.available) else {
            return Err(RpcError::new(
                "UNAVAILABLE",
                status.reason.unwrap_or_else(|| "market data is unavailable".into()),
            ));
        };
        std::fs::create_dir_all(&self.cache_dir).ok();
        let mut cmd = Command::new(&node);
        cmd.arg(&self.script)
            .env("DEMIDO_CACHE_DIR", &self.cache_dir)
            .env("NODE_NO_WARNINGS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // The saved session travels with the process, so it is in force before any request.
        if let Some(session) = self.saved_session() {
            cmd.env("DEMIDO_TV_SESSION", &session.session)
                .env("DEMIDO_TV_SIGNATURE", &session.signature);
        }
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = cmd
            .spawn()
            .map_err(|e| RpcError::new("UNAVAILABLE", format!("could not start the market service: {e}")))?;
        crate::runtime::job::adopt(&child);
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let pending = self.pending.clone();
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(service) = me.upgrade() else { break };
                service.dispatch(&line);
            }
            for (_, tx) in pending.lock().drain() {
                let _ = tx.send(Err(RpcError::new("UNAVAILABLE", "the market service stopped")));
            }
            if let Some(service) = me.upgrade() {
                service.update_status(|s| s.running = false);
            }
        });
        let log_file = self.log_file.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                append_log(&log_file, &line);
            }
        });

        *proc = Some(Proc { child, stdin });
        drop(proc);
        self.update_status(|s| s.running = true);
        Ok(())
    }

    /// Handles one line from the service: a response, a live update or a log line.
    fn dispatch(&self, line: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            append_log(&self.log_file, line);
            return;
        };
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            if let Some(tx) = self.pending.lock().remove(&id) {
                let result = match msg.get("error") {
                    Some(err) => Err(RpcError::new(
                        err["code"].as_str().unwrap_or("ERROR"),
                        err["message"].as_str().unwrap_or("market service error"),
                    )),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(result);
            }
            return;
        }
        match msg.get("event").and_then(Value::as_str) {
            Some("log") => append_log(&self.log_file, msg["params"]["message"].as_str().unwrap_or_default()),
            Some("auth") => {
                let p = &msg["params"];
                if p["expired"].as_bool().unwrap_or(false) {
                    self.forget_session();
                } else {
                    let logged_in = p["loggedIn"].as_bool().unwrap_or(false);
                    let username = p["username"].as_str().map(str::to_string);
                    self.update_status(|s| {
                        s.logged_in = logged_in;
                        s.username = username;
                    });
                }
            }
            Some(_) => {
                let _ = self.app.emit(EVENT, &msg);
            }
            None => {}
        }
    }

    fn saved_session(&self) -> Option<Session> {
        self.secrets
            .get(TRADINGVIEW_SESSION)
            .and_then(|raw| serde_json::from_str(&raw).ok())
    }

    /// Hands a freshly captured session to the service; succeeds once TradingView accepts it.
    pub(crate) async fn apply_session(&self, session: &Session) -> Result<String, RpcError> {
        let result = self
            .call(
                "auth.set",
                json!({"session": session.session, "signature": session.signature}),
                Duration::from_secs(40),
            )
            .await?;
        let username = result["username"].as_str().unwrap_or("TradingView user").to_string();
        if let Ok(raw) = serde_json::to_string(session) {
            self.secrets.set(TRADINGVIEW_SESSION, &raw);
        }
        self.update_status(|s| {
            s.logged_in = true;
            s.username = Some(username.clone());
            s.last_error = None;
        });
        Ok(username)
    }

    fn forget_session(&self) {
        self.secrets.delete(TRADINGVIEW_SESSION);
        self.update_status(|s| {
            s.logged_in = false;
            s.username = None;
        });
    }

    /// Starts the service at launch when a session is saved, so the status is confirmed early,
    /// or when a download was still going at the last exit, so it resumes without waiting for
    /// the person to open a chart.
    pub async fn warm_up(&self) {
        let wanted = self.saved_session().is_some() || has_unfinished_jobs(&self.cache_dir.join("jobs"));
        if wanted && let Err(e) = self.ensure_started().await {
            tracing::warn!("market service did not start: {e}");
        }
    }

    /// Asks the service to flush and exit (the `shutdown` call, then closing its input), waiting
    /// at most `grace`, and keeps it from starting again. The caller kills whatever is left.
    pub async fn shutdown(&self, grace: Duration) {
        self.closing.store(true, Ordering::Relaxed);
        let deadline = tokio::time::Instant::now() + grace;
        // A call stuck writing to a hung service holds the lock; exiting must not wait on it.
        let Ok(mut proc) = tokio::time::timeout_at(deadline, self.proc.lock()).await else {
            return;
        };
        let Some(mut p) = proc.take() else {
            return;
        };
        drop(proc);
        if !matches!(p.child.try_wait(), Ok(None)) {
            return;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let line = format!("{}\n", json!({"id": id, "method": "shutdown", "params": {}}));
        let _ = tokio::time::timeout_at(deadline, async {
            let _ = p.stdin.write_all(line.as_bytes()).await;
            let _ = p.stdin.flush().await;
            // A closed input is the service's other signal to flush and exit.
            drop(p.stdin);
            p.child.wait().await
        })
        .await;
        let _ = p.child.start_kill();
    }

    /// Signs out: forgets the session and clears the sign-in window's cookies.
    pub async fn logout(&self) {
        if self.status.read().running {
            let _ = self.call("auth.clear", json!({}), Duration::from_secs(10)).await;
        }
        self.forget_session();
        auth::clear_profile(self).await;
    }

    pub fn kill_now(&self) {
        if let Ok(mut proc) = self.proc.try_lock()
            && let Some(p) = proc.as_mut()
        {
            let _ = p.child.start_kill();
        }
    }

    pub(crate) fn app(&self) -> &AppHandle {
        &self.app
    }
}

/// Whether `<cache>/market/jobs/*.json` holds a download that was running, queued or waiting.
fn has_unfinished_jobs(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.extension().is_some_and(|e| e == "json")
            && std::fs::read(&path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
                .is_some_and(|job| matches!(job["status"].as_str(), Some("running" | "queued" | "waiting")))
    })
}

fn append_log(path: &PathBuf, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%H:%M:%S"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unfinished_jobs_start_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = dir.path().join("jobs");
        assert!(!has_unfinished_jobs(&jobs), "no folder yet");
        std::fs::create_dir_all(&jobs).unwrap();
        std::fs::write(jobs.join("a.json"), r#"{"id": "a", "status": "done"}"#).unwrap();
        std::fs::write(jobs.join("b.json"), r#"{"id": "b", "status": "paused"}"#).unwrap();
        std::fs::write(jobs.join("c.json.tmp"), r#"{"id": "c", "status": "running"}"#).unwrap();
        std::fs::write(jobs.join("d.json"), "not json").unwrap();
        assert!(!has_unfinished_jobs(&jobs));
        for status in ["running", "queued", "waiting"] {
            std::fs::write(jobs.join("e.json"), format!(r#"{{"id": "e", "status": "{status}"}}"#)).unwrap();
            assert!(has_unfinished_jobs(&jobs), "{status}");
        }
    }
}
