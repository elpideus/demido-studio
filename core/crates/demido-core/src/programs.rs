//! Models through programs on this computer: the person's own Claude Code and OpenCode.
//!
//! These are not APIs. Each is a program with a loop and tools of its own, which Demido starts, gives the
//! person's message to, and watches: what it writes, what tools it uses, what it asks to be allowed to do.
//! Demido's own way of helping small models (the question of which tool, its tools, its tasks) does not run
//! inside them, and the session says so (DECISIONS U15–U24, research note 18).
//!
//! The rules both programs come with (research note 18):
//!
//! - **Demido's own copy, unchanged, run as documented** (`copies`): downloaded from its maker when the
//!   person asks, checked, kept in Demido's folder with a profile of its own. Claude Code through `claude -p`
//!   in its stream mode; OpenCode through `opencode acp`, the interface it documents for editors. Never one
//!   installed on the system, never a changed one.
//! - **Its login is its own.** Demido never reads, keeps or passes on a program's login or token. A person
//!   signs in in the program itself (`claude`, `/login`; `opencode auth login`).
//! - **Only for the person's own messages.** No program is run while nobody is there: OpenCode's terms for
//!   its hosted free models forbid that, and Claude Code's plans are made for one person's ordinary use.
//!
//! One program run serves one turn; the next turn goes on in the same program session (Claude Code's
//! `--resume`, OpenCode's `session/resume`).

mod acp;
mod claude_code;
pub mod copies;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncWrite};
use utoipa::ToSchema;

use crate::providers::prices;
use crate::{Error, Result};

pub(crate) use acp::{Approval, Approver, Choice};
pub(crate) use claude_code::Options as ClaudeCodeOptions;

/// What a program said during a turn, as it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// It started: the model it uses, and what it is billed to (Claude Code's `apiKeySource`).
    Started {
        model: Option<String>,
        billing: Option<String>,
    },
    Text(String),
    Thought(String),
    /// It began to use a tool of its own.
    Tool {
        id: String,
        name: String,
        input: Value,
    },
    /// What a tool of its own is used on, once the program knows it.
    ToolInput {
        id: String,
        input: Value,
    },
    /// A tool of its own ended.
    ToolDone {
        id: String,
        result: String,
        failed: bool,
    },
    /// It was not allowed to use a tool.
    Denied {
        id: String,
        why: String,
    },
    /// Its plan's limit, as it said it (Claude Code's `rate_limit_event`).
    Limit {
        status: String,
        resets_at: Option<i64>,
        used: Option<f64>,
    },
    /// A request of its own failed and is tried again.
    Retrying(String),
}

/// What a turn came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    /// Its final words, as the program reports them, when it does.
    pub result: Option<String>,
    /// What it says the turn cost, in dollars. Claude Code's figure is its own estimate at API prices; under
    /// a subscription nothing is billed per token.
    pub cost_usd: Option<f64>,
    pub tokens: prices::Tokens,
    /// The program's own name for the session, to go on in it next time.
    pub session: Option<String>,
    /// Why the turn failed, in the program's words.
    pub error: Option<String>,
    pub stopped: bool,
}

/// A started program: where to write to it and where to read it.
pub struct Started {
    pub stdin: Box<dyn AsyncWrite + Send + Unpin>,
    pub stdout: Box<dyn AsyncBufRead + Send + Unpin>,
    /// The last of what it wrote to stderr, for when it fails.
    pub stderr: Arc<Mutex<String>>,
    /// Asks it to end the turn (SIGINT). Where there is no such request (Windows), it is ended.
    pub interrupt: Box<dyn Fn() + Send + Sync>,
    /// The process, ended when this is dropped.
    pub child: Option<tokio::process::Child>,
}

/// Starts programs. Tests give one that talks to a stand-in instead.
pub trait Spawner: Send + Sync {
    fn spawn(&self, program: &Path, args: &[String], cwd: &Path, env: &[(&str, String)]) -> Result<Started>;
}

/// The real one.
pub struct ProcessSpawner;

/// How much of a program's stderr is kept.
const STDERR_KEPT: usize = 4000;

impl Spawner for ProcessSpawner {
    fn spawn(&self, program: &Path, args: &[String], cwd: &Path, env: &[(&str, String)]) -> Result<Started> {
        use std::process::Stdio;
        let mut command = tokio::process::Command::new(program);
        command.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().map_err(|e| Error::Invalid(format!("{} could not be started: {e}", program.display())))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let kept = Arc::new(Mutex::new(String::new()));
        let into = kept.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buffer = [0u8; 2048];
            while let Ok(n) = stderr.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                let mut kept = into.lock().unwrap_or_else(|e| e.into_inner());
                kept.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if kept.len() > STDERR_KEPT {
                    let cut = kept.len() - STDERR_KEPT;
                    let cut = (cut..kept.len()).find(|at| kept.is_char_boundary(*at)).unwrap_or(0);
                    kept.drain(..cut);
                }
            }
        });
        let pid = child.id();
        let interrupt: Box<dyn Fn() + Send + Sync> = Box::new(move || {
            #[cfg(unix)]
            if let Some(pid) = pid {
                let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
            #[cfg(not(unix))]
            let _ = pid;
        });
        Ok(Started { stdin: Box::new(stdin), stdout: Box::new(tokio::io::BufReader::new(stdout)), stderr: kept, interrupt, child: Some(child) })
    }
}

/// A program Demido runs, as it says it is.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Found {
    /// `claude-code` or `opencode`.
    pub provider: String,
    /// Its full path. `null`: not found.
    pub path: Option<String>,
    /// What it says its version is.
    pub version: Option<String>,
    /// How it is signed in, in words, as it says (Claude Code's `auth status`). Never a login itself.
    pub signed_in: Option<String>,
    /// Why it cannot be used, when it cannot.
    pub problem: Option<String>,
    /// For Claude Code: whether it said it keeps to the profile Demido gave it (`configDirectory` in its
    /// `auth status`). `false`: it does not, and Demido does not run it. `null`: it did not say (before 2.1.268).
    #[serde(default)]
    pub contained: Option<bool>,
}

/// What a program printed for a terminal, without the codes that colour it (OpenCode colours its version).
pub(crate) fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ … a letter
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// What a program says about itself: its version and, for Claude Code, how it is signed in. Asks nothing
/// of any model. `path` is the program to look at (`None`: there is none, and `missing` says why); `env` is
/// what it is run with (Demido's own copy is run with its own profile).
pub async fn look(provider: &str, path: Option<PathBuf>, env: &[(&str, String)], missing: &str) -> Found {
    let Some(path) = path else {
        return Found { provider: provider.into(), path: None, version: None, signed_in: None, problem: Some(missing.to_string()), contained: None };
    };
    let env: Vec<(String, String)> = env.iter().map(|(n, v)| (n.to_string(), v.clone())).collect();
    let run = |args: &'static [&'static str]| {
        let (path, env) = (path.clone(), env.clone());
        async move {
            let mut command = tokio::process::Command::new(&path);
            command.args(args).env("NO_COLOR", "1").stdin(std::process::Stdio::null());
            for (name, value) in &env {
                command.env(name, value);
            }
            // Just downloaded: its file may still be busy for a moment (`spawn`).
            match tokio::time::timeout(std::time::Duration::from_secs(15), crate::spawn::output_when_free(&mut command)).await {
                Ok(Ok(out)) => Some((out.status.success(), plain(String::from_utf8_lossy(&out.stdout).trim()))),
                _ => None,
            }
        }
    };
    let version = run(&["--version"]).await.filter(|(ok, _)| *ok).map(|(_, said)| said.lines().next().unwrap_or_default().to_string());
    // How Claude Code is signed in, in its own words (`authMethod`), and the folder it says it keeps to
    // (`configDirectory`). Nothing else of what it says is kept.
    let mut contained = None;
    let signed_in = if provider == "claude-code" {
        run(&["auth", "status"]).await.map(|(_, said)| {
            let told = serde_json::from_str::<Value>(&said).ok();
            contained = told.as_ref().and_then(|v| v["configDirectory"].as_str()).map(|dir| keeps_to(dir, &env));
            let method = told.and_then(|v| v["authMethod"].as_str().map(str::to_string)).unwrap_or_else(|| "none".into());
            match method.as_str() {
                "claude.ai" | "oauth_token" => "with a Claude account (your plan's limits)".to_string(),
                "api_key" | "api_key_helper" => "with an API key (billed per token)".to_string(),
                "third_party" => "through a cloud provider".to_string(),
                _ => "not signed in".to_string(),
            }
        })
    } else {
        None
    };
    let problem = if contained == Some(false) {
        Some("Claude Code says it does not keep to the profile Demido gave it, so Demido does not run it: it would reach your own Claude Code's settings and login. A newer or older copy may behave; Demido's own copy should, as downloaded.".into())
    } else if version.is_none() {
        Some(format!("{} is there but did not say its version: it may not start.", path.display()))
    } else if signed_in.as_deref() == Some("not signed in") {
        Some("Claude Code is not signed in. Sign in with the button here: it opens Anthropic's own page. Demido never sees or keeps the login.".into())
    } else {
        None
    };
    Found { provider: provider.into(), path: Some(path.display().to_string()), version, signed_in, problem, contained }
}

/// Whether the folder a program says it uses is the profile it was given (`CLAUDE_CONFIG_DIR`), compared as
/// the same place on disk.
fn keeps_to(said: &str, env: &[(String, String)]) -> bool {
    let Some((_, given)) = env.iter().find(|(n, _)| n == "CLAUDE_CONFIG_DIR") else { return false };
    let real = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    real(said.trim()) == real(given)
}

#[cfg(test)]
mod tests;

/// Runs a Claude Code turn.
pub(crate) async fn claude_code(started: Started, prompt: &str, stop: &std::sync::atomic::AtomicBool, on: &mut (dyn FnMut(Event) + Send)) -> Result<Turn> {
    claude_code::run(started, prompt, stop, on).await
}

/// The arguments of a Claude Code turn.
pub(crate) fn claude_code_args(options: &ClaudeCodeOptions) -> Vec<String> {
    claude_code::args(options)
}

/// The environment a Claude Code turn adds to Demido's own: a person's yes may take longer than its tools wait.
pub(crate) fn claude_code_env() -> Vec<(&'static str, String)> {
    claude_code::env()
}

/// The rules OpenCode is started with for a session's level, as configuration given at start
/// (`OPENCODE_CONFIG_CONTENT`), so nothing is written into the person's folder. OpenCode's own default is to
/// allow everything but reading `.env` files (opencode.ai/docs/permissions): left so, a session at Cautious
/// would run commands without anyone asked. Rules come after the person's own, and the last that matches
/// wins: at Cautious everything asks; at Balanced and Smart, commands, the internet and anything outside the
/// folder ask, and the person's own rules decide the rest; at Autonomous OpenCode's own rules are left alone.
pub(crate) fn opencode_env(level: crate::permissions::Level) -> Vec<(&'static str, String)> {
    use crate::permissions::Level;
    let ask = |action: &str| serde_json::json!({ "action": action, "resource": "*", "effect": "ask" });
    let rules = match level {
        Level::Autonomous => return Vec::new(),
        Level::Cautious => vec![ask("*")],
        Level::Balanced | Level::Smart => vec![ask("shell"), ask("webfetch"), ask("websearch"), ask("external_directory")],
    };
    vec![("OPENCODE_CONFIG_CONTENT", serde_json::json!({ "permissions": rules }).to_string())]
}

/// Runs an OpenCode turn.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn opencode(
    started: Started,
    cwd: &Path,
    model: &str,
    resume: Option<&str>,
    told_first: Option<&str>,
    prompt: &str,
    stop: &std::sync::atomic::AtomicBool,
    on: &mut (dyn FnMut(Event) + Send),
    approve: &Approver<'_>,
) -> Result<Turn> {
    acp::run(started, cwd, model, resume, told_first, prompt, stop, on, approve).await
}

/// The models OpenCode offers in a folder: its value and its name for each.
pub(crate) async fn opencode_models(started: Started, cwd: &Path) -> Result<Vec<(String, String)>> {
    acp::models(started, cwd).await
}
