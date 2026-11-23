//! Chromium's debugging protocol, through a pipe (DECISIONS Z2).
//!
//! The browser is started with `--remote-debugging-pipe`: it reads messages on its file descriptor 3 and
//! writes on 4, each a JSON object ended by a NUL byte. A pipe is open only to the one who started the
//! browser; a debugging port on this computer would be open to every program and every user on it, and
//! whoever reaches that port drives the browser. Tried in research note 31: the first answer came in 0.05 s.
//!
//! One request is a command with a number; its answer comes back with the same number. Everything else that
//! comes is news (an event), heard by whoever listens.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, oneshot};

/// How long a command may take before it is given up on.
const A_COMMAND_TAKES_AT_MOST: Duration = Duration::from_secs(30);

/// News from the browser: what happened, in which of its sessions (a page), and the facts of it.
#[derive(Clone, Debug)]
pub struct News {
    pub method: String,
    pub session: Option<String>,
    pub params: Value,
}

type Waiting = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// A running browser, and the pipe to it.
pub struct Cdp {
    to: tokio::sync::Mutex<tokio::fs::File>,
    waiting: Waiting,
    news: broadcast::Sender<News>,
    next: AtomicU64,
    child: Mutex<Option<tokio::process::Child>>,
}

impl Cdp {
    /// Starts `program` with `args` and the pipe. Unix only: on Windows the pipe is given as handles (Z2).
    #[cfg(unix)]
    pub fn start(program: &std::path::Path, args: &[String]) -> std::io::Result<Arc<Self>> {
        use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
        let pipe = || -> std::io::Result<(OwnedFd, OwnedFd)> {
            let mut fds = [0; 2];
            // SAFETY: pipe writes two descriptors into the array it is given. macOS has no pipe2, so each end is
            // then marked to close on exec, which the program started is not to inherit but through 3 and 4.
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: both are fresh descriptors that nothing else owns.
            let ends = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
            for fd in fds {
                // SAFETY: a descriptor of ours, and a flag on it.
                if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(ends)
        };
        // The browser reads what we write on one, and writes what we read on the other.
        let (its_in, our_out) = pipe()?;
        let (our_in, its_out) = pipe()?;
        let (its_in_fd, its_out_fd) = (std::os::fd::AsRawFd::as_raw_fd(&its_in), std::os::fd::AsRawFd::as_raw_fd(&its_out));
        let mut command = tokio::process::Command::new(program);
        command.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
        // SAFETY: only calls that are safe between fork and exec (fcntl, dup2). The two descriptors are first
        // moved out of the way, so that putting one on 3 cannot close the other if it happened to be 3 or 4.
        unsafe {
            command.pre_exec(move || {
                let a = libc::fcntl(its_in_fd, libc::F_DUPFD, 10);
                let b = libc::fcntl(its_out_fd, libc::F_DUPFD, 10);
                if a < 0 || b < 0 || libc::dup2(a, 3) < 0 || libc::dup2(b, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop((its_in, its_out));
        // SAFETY: the descriptors are ours alone from here on.
        let to = tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(our_out.into_raw_fd()) });
        use tokio::io::AsyncReadExt;
        let mut from = tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(our_in.into_raw_fd()) });
        let (news, _) = broadcast::channel(512);
        let cdp = Arc::new(Self { to: tokio::sync::Mutex::new(to), waiting: Waiting::default(), news, next: AtomicU64::new(1), child: Mutex::new(Some(child)) });
        let (waiting, told) = (cdp.waiting.clone(), cdp.news.clone());
        tokio::spawn(async move {
            let (mut kept, mut piece) = (Vec::new(), vec![0u8; 1 << 16]);
            loop {
                let n = match from.read(&mut piece).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                kept.extend_from_slice(&piece[..n]);
                while let Some(end) = kept.iter().position(|b| *b == 0) {
                    let message: Vec<u8> = kept.drain(..=end).collect();
                    let Ok(value) = serde_json::from_slice::<Value>(&message[..message.len() - 1]) else { continue };
                    if let Some(id) = value.get("id").and_then(Value::as_u64) {
                        if let Some(answer) = waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                            let got = match value.get("error") {
                                Some(error) => Err(error.get("message").and_then(Value::as_str).unwrap_or("the browser refused").to_string()),
                                None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = answer.send(got);
                        }
                    } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                        let _ = told.send(News {
                            method: method.into(),
                            session: value.get("sessionId").and_then(Value::as_str).map(str::to_string),
                            params: value.get("params").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
            // The browser ended: nobody waiting will be answered.
            waiting.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
        Ok(cdp)
    }

    #[cfg(not(unix))]
    pub fn start(_program: &std::path::Path, _args: &[String]) -> std::io::Result<Arc<Self>> {
        Err(std::io::Error::other("Demido's browser is not built for Windows yet"))
    }

    /// Sends a command, in one of the browser's sessions (a page) or to the browser itself, and waits for its answer.
    pub async fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        let (answer, answered) = oneshot::channel();
        self.waiting.lock().unwrap_or_else(|e| e.into_inner()).insert(id, answer);
        let mut bytes = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
        bytes.push(0);
        {
            let mut to = self.to.lock().await;
            if to.write_all(&bytes).await.is_err() || to.flush().await.is_err() {
                self.waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                return Err("the browser has stopped".into());
            }
        }
        match tokio::time::timeout(A_COMMAND_TAKES_AT_MOST, answered).await {
            Ok(Ok(got)) => got,
            Ok(Err(_)) => Err("the browser has stopped".into()),
            Err(_) => {
                self.waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                Err(format!("the browser did not answer {method} in time"))
            }
        }
    }

    /// Hears the news from now on.
    pub fn listen(&self) -> broadcast::Receiver<News> {
        self.news.subscribe()
    }

    /// Whether the browser is still running.
    pub fn running(&self) -> bool {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// Ends the browser: asked first, then made to.
    pub async fn end(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(3), self.call("Browser.close", json!({}), None)).await;
        let child = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(mut child) = child
            && tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err()
        {
            let _ = child.kill().await;
        }
    }
}
