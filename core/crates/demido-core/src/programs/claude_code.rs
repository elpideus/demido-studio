//! Claude Code, run as `claude -p` with its stream of JSON lines both ways (code.claude.com/docs: headless,
//! cli-reference, agent-sdk/typescript; read on 2026-11-21, research note 18).
//!
//! What Demido passes, and why:
//!
//! - `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages`: the
//!   person's message goes in as one line; what Claude Code does comes out line by line, its words as they
//!   are written.
//! - `--session-id` the first time, `--resume` after: one Claude Code session for one Demido session.
//! - `--setting-sources user --strict-mcp-config`: a folder's own `.claude/settings.json` (its hooks) and
//!   `.mcp.json` (its servers) are not loaded. Without these a `-p` run would run them with no question, even
//!   in a folder never trusted before. The person's own settings in `~/.claude` are kept.
//! - `--permission-mode` from the session's level, and `--permission-prompt-tool` pointing at Demido's own
//!   tool (served by the Demido server for this turn only), so that what Claude Code would ask, the person
//!   is asked in Demido's window. Without the server, `--permission-prompts none`: nothing can ask, and
//!   Claude Code is told so.
//! - `--append-system-prompt` with Demido's way of answering (chat::INSTRUCTIONS). Claude Code's own prompt
//!   stays as it is.
//!
//! Its login is never touched, and no way of signing in is taken away: what Claude Code is signed in with,
//! it uses. Its own report of that (`apiKeySource`) is shown, so a key in the environment that bills per
//! token is not missed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use super::{Event, Started, Turn};
use crate::providers::prices;
use crate::{Error, Result};

/// How long Claude Code may wait for the person's yes, in milliseconds. Its own default for a tool on a
/// server like Demido's is a minute; a person may take longer.
const A_YES_MAY_TAKE_MS: u64 = 3_600_000;

/// What a turn is run with.
pub(crate) struct Options<'a> {
    /// `default`, `opus`, `sonnet`, `haiku`, `fable`, or a full name.
    pub model: &'a str,
    /// `low` … `max`, or `None` for Claude Code's own default.
    pub effort: Option<&'a str>,
    /// `manual`, `acceptEdits`, `auto` or `bypassPermissions`.
    pub mode: &'a str,
    /// The Claude Code session to go on in, or the id a new one is to have.
    pub session: &'a str,
    pub resume: bool,
    /// Where Demido's approval tool is for this turn. `None`: nobody can be asked.
    pub approve_at: Option<&'a str>,
    /// Added to Claude Code's own instructions.
    pub append: &'a str,
}

pub(super) fn args(o: &Options) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"].iter().map(|s| s.to_string()).collect();
    a.extend([if o.resume { "--resume" } else { "--session-id" }.to_string(), o.session.to_string()]);
    if o.model != "default" {
        a.extend(["--model".into(), o.model.into()]);
    }
    if let Some(effort) = o.effort {
        a.extend(["--effort".into(), effort.into()]);
    }
    a.extend(["--permission-mode".into(), o.mode.into(), "--setting-sources".into(), "user".into(), "--strict-mcp-config".into()]);
    match o.approve_at {
        Some(url) => {
            let servers = json!({ "mcpServers": { "demido": { "type": "http", "url": url } } });
            a.extend(["--mcp-config".into(), servers.to_string(), "--permission-prompt-tool".into(), "mcp__demido__approve".into()]);
        }
        None => a.extend(["--permission-prompts".into(), "none".into()]),
    }
    a.extend(["--append-system-prompt".into(), o.append.into()]);
    a
}

pub(super) fn env() -> Vec<(&'static str, String)> {
    vec![("MCP_TOOL_TIMEOUT", A_YES_MAY_TAKE_MS.to_string()), ("CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT", A_YES_MAY_TAKE_MS.to_string())]
}

/// A tool's result as Claude Code gives it: a string, or blocks of text.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// What one line of Claude Code's stream says. `done` is set at its `result`.
pub(super) fn read_line(said: &Value, text_so_far: &mut String, done: &mut Option<Turn>) -> Vec<Event> {
    let main = said["parent_tool_use_id"].is_null();
    let mut out = Vec::new();
    match (said["type"].as_str().unwrap_or_default(), said["subtype"].as_str()) {
        ("system", Some("init")) => out.push(Event::Started { model: said["model"].as_str().map(str::to_string), billing: said["apiKeySource"].as_str().map(str::to_string) }),
        ("system", Some("permission_denied")) => out.push(Event::Denied { id: said["tool_use_id"].as_str().unwrap_or_default().into(), why: said["message"].as_str().unwrap_or("not allowed").into() }),
        ("system", Some("api_retry")) => out.push(Event::Retrying(said["error"].as_str().unwrap_or("a request failed").into())),
        ("stream_event", _) if main => {
            let event = &said["event"];
            match event["type"].as_str() {
                // A new message after one that said something: a blank line between them.
                Some("message_start") if !text_so_far.trim().is_empty() && !text_so_far.ends_with("\n\n") => {
                    text_so_far.push_str("\n\n");
                    out.push(Event::Text("\n\n".into()));
                }
                Some("content_block_delta") => match event["delta"]["type"].as_str() {
                    Some("text_delta") => {
                        if let Some(text) = event["delta"]["text"].as_str().filter(|t| !t.is_empty()) {
                            text_so_far.push_str(text);
                            out.push(Event::Text(text.into()));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = event["delta"]["thinking"].as_str().filter(|t| !t.is_empty()) {
                            out.push(Event::Thought(text.into()));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        ("assistant", _) => {
            for block in said["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_use" {
                    out.push(Event::Tool { id: block["id"].as_str().unwrap_or_default().into(), name: block["name"].as_str().unwrap_or("?").into(), input: block["input"].clone() });
                }
            }
        }
        ("user", _) => {
            for block in said["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_result" {
                    out.push(Event::ToolDone { id: block["tool_use_id"].as_str().unwrap_or_default().into(), result: result_text(&block["content"]), failed: block["is_error"] == true });
                }
            }
        }
        ("rate_limit_event", _) => {
            let info = &said["rate_limit_info"];
            out.push(Event::Limit { status: info["status"].as_str().unwrap_or("allowed").into(), resets_at: info["resetsAt"].as_i64(), used: info["utilization"].as_f64() });
        }
        ("result", subtype) => {
            let u = &said["usage"];
            let failed = said["is_error"] == true || subtype.is_some_and(|s| s != "success");
            let words = said["result"].as_str().map(str::to_string);
            *done = Some(Turn {
                result: words.clone(),
                cost_usd: said["total_cost_usd"].as_f64(),
                tokens: prices::Tokens {
                    input: u["input_tokens"].as_u64().unwrap_or(0),
                    cached: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cache_written: u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    output: u["output_tokens"].as_u64().unwrap_or(0),
                },
                session: said["session_id"].as_str().map(str::to_string),
                error: failed.then(|| words.filter(|w| !w.trim().is_empty()).unwrap_or_else(|| format!("Claude Code ended the turn with {}.", subtype.unwrap_or("an error")))),
                stopped: false,
            });
        }
        _ => {}
    }
    out
}

/// Claude Code's words for "not signed in", for a person.
fn as_said(error: &str) -> String {
    let lower = error.to_lowercase();
    if lower.contains("not logged in") || lower.contains("/login") || lower.contains("invalid api key") || lower.contains("authentication") {
        format!("Claude Code is not signed in, or its login has ended. Sign in in a terminal: run claude, then /login. Demido never sees or keeps the login. (Claude Code said: {})", error.trim())
    } else {
        format!("Claude Code said: {}", error.trim())
    }
}

/// Ends when `stop` is set.
pub(super) async fn stopped(stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Gives Claude Code the person's message and reads what it does until the turn ends.
pub(super) async fn run(mut started: Started, prompt: &str, stop: &AtomicBool, on: &mut (dyn FnMut(Event) + Send)) -> Result<Turn> {
    let message = json!({ "type": "user", "message": { "role": "user", "content": prompt }, "parent_tool_use_id": null });
    let written = async {
        started.stdin.write_all(format!("{message}\n").as_bytes()).await?;
        started.stdin.flush().await
    };
    written.await.map_err(|e| Error::Other(format!("Claude Code could not be given the message: {e}")))?;
    let (mut text, mut done, mut asked_to_stop) = (String::new(), None, false);
    let mut line = String::new();
    loop {
        line.clear();
        let read = tokio::select! {
            read = started.stdout.read_line(&mut line) => read,
            _ = stopped(stop), if !asked_to_stop => {
                // Asked to stop: Claude Code ends the turn cleanly on SIGINT and still says how it ended.
                asked_to_stop = true;
                (started.interrupt)();
                continue;
            }
        };
        match read {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(said) = serde_json::from_str::<Value>(line.trim()) else { continue };
        for event in read_line(&said, &mut text, &mut done) {
            on(event);
        }
        if done.is_some() {
            break;
        }
    }
    // The turn is over: with its input closed, Claude Code ends.
    let _ = started.stdin.shutdown().await;
    match done {
        Some(mut turn) => {
            turn.stopped = asked_to_stop;
            if asked_to_stop {
                turn.error = None;
            }
            if let Some(e) = turn.error.take() {
                turn.error = Some(as_said(&e));
            }
            Ok(turn)
        }
        None if asked_to_stop => Ok(Turn { stopped: true, ..Turn::default() }),
        None => {
            let said = started.stderr.lock().unwrap_or_else(|e| e.into_inner()).trim().to_string();
            Err(Error::Other(if said.is_empty() { "Claude Code ended without saying how the turn went.".into() } else { as_said(&said) }))
        }
    }
}
