//! OpenCode, run as `opencode acp`: the Agent Client Protocol, newline-delimited JSON-RPC over its stdin and
//! stdout, protocol version 1 (opencode.ai/docs/cli/acp, read on 2026-11-21; tried with OpenCode 2.2.7).
//!
//! Demido is the client, as an editor is. It says it reads and writes no files and runs no terminal for
//! OpenCode (OpenCode does those itself, under its own permission rules), creates or resumes a session in
//! the session's folder, sets the model, and sends the person's message. What OpenCode asks to be allowed
//! to do comes as `session/request_permission` and is answered with the person's yes or no.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use super::claude_code::stopped;
use super::{Event, Started, Turn};
use crate::providers::prices;
use crate::{Error, Result};

/// What OpenCode asks to be allowed to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Approval {
    pub id: String,
    /// What it would do, in its words: "Edit src/app.ts".
    pub title: String,
    /// ACP's kind of tool: `read`, `edit`, `delete`, `move`, `search`, `execute`, `think`, `fetch`, `other`.
    pub kind: String,
    pub input: Value,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Choice {
    Once,
    Reject,
}

/// Answers what OpenCode asks to be allowed to do.
pub(crate) type Approver<'a> = dyn Fn(Approval) -> Pin<Box<dyn Future<Output = Choice> + Send + 'a>> + Send + Sync + 'a;

/// How long OpenCode may take to answer anything but a prompt.
const ANSWERS_WITHIN: Duration = Duration::from_secs(90);

struct Rpc {
    started: Started,
    next: u64,
}

/// What OpenCode said about the session while a request was answered.
#[derive(Default)]
struct Seen {
    /// The session's whole cost in dollars, as its last `usage_update` said.
    cost: Option<f64>,
}

impl Rpc {
    async fn write(&mut self, value: &Value) -> Result<()> {
        let line = format!("{value}\n");
        let sent = async {
            self.started.stdin.write_all(line.as_bytes()).await?;
            self.started.stdin.flush().await
        };
        sent.await.map_err(|e| Error::Other(format!("OpenCode could not be written to: {e}")))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    /// Sends a request and reads until its answer. On the way, what OpenCode tells is passed to `on`, and
    /// what it asks is answered. With `stop`, setting it cancels the prompt (`session/cancel`).
    async fn call(&mut self, method: &str, params: Value, stop: Option<(&AtomicBool, &str)>, on: &mut (dyn FnMut(Event) + Send), approve: &Approver<'_>, seen: &mut Seen) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        let mut cancelled = false;
        let mut line = String::new();
        loop {
            line.clear();
            let read = match stop {
                Some((stop, session)) => {
                    let wait_stop = !cancelled;
                    tokio::select! {
                        read = self.started.stdout.read_line(&mut line) => read,
                        _ = stopped(stop), if wait_stop => {
                            cancelled = true;
                            self.notify("session/cancel", json!({ "sessionId": session })).await?;
                            continue;
                        }
                    }
                }
                None => match tokio::time::timeout(ANSWERS_WITHIN, self.started.stdout.read_line(&mut line)).await {
                    Ok(read) => read,
                    Err(_) => return Err(Error::Other(format!("OpenCode did not answer {method} within {} seconds.", ANSWERS_WITHIN.as_secs()))),
                },
            };
            match read {
                Ok(0) | Err(_) => {
                    let said = self.started.stderr.lock().unwrap_or_else(|e| e.into_inner()).trim().to_string();
                    return Err(Error::Other(if said.is_empty() { "OpenCode ended without answering.".into() } else { format!("OpenCode ended: {said}") }));
                }
                Ok(_) => {}
            }
            let Ok(said) = serde_json::from_str::<Value>(line.trim()) else { continue };
            match (said.get("id"), said.get("method").and_then(Value::as_str)) {
                // Its answer to this request.
                (Some(got), None) if got.as_u64() == Some(id) => {
                    if let Some(e) = said.get("error") {
                        return Err(refused(e));
                    }
                    return Ok(said["result"].clone());
                }
                // Something it asks of Demido.
                (Some(asked), Some(what)) => {
                    let answer = if what == "session/request_permission" {
                        let p = &said["params"];
                        let call = &p["toolCall"];
                        let approval = Approval {
                            id: call["toolCallId"].as_str().unwrap_or_default().into(),
                            title: call["title"].as_str().unwrap_or("a tool").into(),
                            kind: call["kind"].as_str().unwrap_or("other").into(),
                            input: call["rawInput"].clone(),
                        };
                        let choice = if cancelled { Choice::Reject } else { approve(approval).await };
                        let wanted = if choice == Choice::Once { "allow_once" } else { "reject_once" };
                        let options = p["options"].as_array().cloned().unwrap_or_default();
                        let option = options.iter().find(|o| o["kind"] == wanted).and_then(|o| o["optionId"].as_str()).unwrap_or(if choice == Choice::Once { "once" } else { "reject" });
                        json!({ "jsonrpc": "2.0", "id": asked, "result": { "outcome": { "outcome": "selected", "optionId": option } } })
                    } else {
                        // Demido said it reads no files and runs no terminal for OpenCode: nothing else is asked of it.
                        json!({ "jsonrpc": "2.0", "id": asked, "error": { "code": -32601, "message": format!("Demido does not offer {what}.") } })
                    };
                    self.write(&answer).await?;
                }
                // Something it tells.
                (None, Some("session/update")) => {
                    for event in updated(&said["params"]["update"], seen) {
                        on(event);
                    }
                }
                _ => {}
            }
        }
    }
}

/// An error OpenCode answered with, for a person.
fn refused(e: &Value) -> Error {
    let words = e["message"].as_str().unwrap_or("it did not say why");
    let more = e["data"].as_str().or(e["data"]["message"].as_str()).map(|d| format!(" ({d})")).unwrap_or_default();
    if words.to_lowercase().contains("auth") {
        Error::Invalid(format!("OpenCode needs to be signed in for this model. Sign in in a terminal: opencode auth login. Demido never sees or keeps the login. (OpenCode said: {words}{more})"))
    } else {
        Error::Invalid(format!("OpenCode said: {words}{more}"))
    }
}

/// A tool's content, as OpenCode tells it: text, or the files it changed.
fn content_text(update: &Value) -> String {
    let mut parts = Vec::new();
    for c in update["content"].as_array().into_iter().flatten() {
        match c["type"].as_str() {
            Some("content") => {
                if let Some(text) = c["content"]["text"].as_str() {
                    parts.push(text.to_string());
                }
            }
            Some("diff") => parts.push(format!("Changed {}", c["path"].as_str().unwrap_or("a file"))),
            _ => {}
        }
    }
    if parts.is_empty()
        && let Some(raw) = update.get("rawOutput").filter(|r| !r.is_null())
    {
        parts.push(raw.as_str().map(str::to_string).unwrap_or_else(|| raw.to_string()));
    }
    parts.join("\n")
}

/// What one `session/update` says.
fn updated(update: &Value, seen: &mut Seen) -> Vec<Event> {
    let text = |u: &Value| u["content"]["text"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
    let mut out = Vec::new();
    match update["sessionUpdate"].as_str().unwrap_or_default() {
        "agent_message_chunk" => out.extend(text(update).map(Event::Text)),
        "agent_thought_chunk" => out.extend(text(update).map(Event::Thought)),
        "tool_call" => {
            let id = update["toolCallId"].as_str().unwrap_or_default().to_string();
            let name = update["title"].as_str().or(update["kind"].as_str()).unwrap_or("a tool").to_string();
            out.push(Event::Tool { id: id.clone(), name, input: if update["rawInput"].is_null() { json!({}) } else { update["rawInput"].clone() } });
            if let Some(status @ ("completed" | "failed")) = update["status"].as_str() {
                out.push(Event::ToolDone { id, result: content_text(update), failed: status == "failed" });
            }
        }
        "tool_call_update" => {
            if let Some(status @ ("completed" | "failed")) = update["status"].as_str() {
                out.push(Event::ToolDone { id: update["toolCallId"].as_str().unwrap_or_default().into(), result: content_text(update), failed: status == "failed" });
            }
        }
        "usage_update" => seen.cost = update["cost"]["amount"].as_f64().or(seen.cost),
        "session_info_update" => {
            if let Some(retry) = update["_meta"]["opencode/retry"].as_object() {
                out.push(Event::Retrying(retry.get("error").and_then(|e| e["message"].as_str()).unwrap_or("a request failed").to_string()));
            }
        }
        _ => {}
    }
    out
}

fn no_events(_: Event) {}

fn approve_nothing<'a>(_: Approval) -> Pin<Box<dyn Future<Output = Choice> + Send + 'a>> {
    Box::pin(async { Choice::Reject })
}

async fn initialize(rpc: &mut Rpc) -> Result<Value> {
    let params = json!({ "protocolVersion": 1, "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }, "clientInfo": { "name": "Demido Studio", "version": crate::VERSION } });
    let said = rpc.call("initialize", params, None, &mut no_events, &approve_nothing, &mut Seen::default()).await?;
    if said["protocolVersion"].as_u64() != Some(1) {
        return Err(Error::Invalid(format!("OpenCode speaks ACP version {}, and Demido speaks version 1.", said["protocolVersion"])));
    }
    Ok(said)
}

/// The model options of a session, as OpenCode lists them: value and name.
fn model_options(session: &Value) -> Vec<(String, String)> {
    session["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["category"] == "model" || o["id"] == "model")
        .flat_map(|o| o["options"].as_array().cloned().unwrap_or_default())
        .filter_map(|m| Some((m["value"].as_str()?.to_string(), m["name"].as_str().unwrap_or_default().to_string())))
        .collect()
}

/// Runs one turn in a session of OpenCode's. `told_first` goes with the first message of a new session, as
/// context for the model only. The turn's `cost_usd` is the session's whole cost so far, as OpenCode says it.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    started: Started,
    cwd: &Path,
    model: &str,
    resume: Option<&str>,
    told_first: Option<&str>,
    prompt: &str,
    stop: &AtomicBool,
    on: &mut (dyn FnMut(Event) + Send),
    approve: &Approver<'_>,
) -> Result<Turn> {
    let mut rpc = Rpc { started, next: 0 };
    let mut seen = Seen::default();
    initialize(&mut rpc).await?;
    let place = cwd.to_string_lossy().to_string();
    let mut opened = None;
    if let Some(id) = resume {
        // A session that is gone is begun again; the conversation is then given to it from Demido's side.
        if let Ok(said) = rpc.call("session/resume", json!({ "sessionId": id, "cwd": place, "mcpServers": [] }), None, on, approve, &mut seen).await {
            opened = Some((id.to_string(), said, false));
        }
    }
    let (session, said, new) = match opened {
        Some(open) => open,
        None => {
            let said = rpc.call("session/new", json!({ "cwd": place, "mcpServers": [] }), None, on, approve, &mut seen).await?;
            let id = said["sessionId"].as_str().ok_or_else(|| Error::Other("OpenCode made a session without a name.".into()))?.to_string();
            (id, said, true)
        }
    };
    let options = model_options(&said);
    if !options.iter().any(|(value, _)| value == model) {
        return Err(Error::Invalid(format!("OpenCode does not offer {model} in this folder (any more). Choose another on the Providers screen. Free models come and go.")));
    }
    let current =
        said["configOptions"].as_array().into_iter().flatten().find(|o| o["category"] == "model" || o["id"] == "model").and_then(|o| o["currentValue"].as_str()).unwrap_or_default().to_string();
    if current != model {
        rpc.call("session/set_config_option", json!({ "sessionId": session, "configId": "model", "value": model }), None, on, approve, &mut seen).await?;
    }
    let mut blocks = Vec::new();
    if new && let Some(first) = told_first {
        blocks.push(json!({ "type": "text", "text": first, "annotations": { "audience": ["assistant"] } }));
    }
    blocks.push(json!({ "type": "text", "text": prompt }));
    let answered = rpc.call("session/prompt", json!({ "sessionId": session, "prompt": blocks }), Some((stop, &session)), on, approve, &mut seen).await;
    let _ = rpc.started.stdin.shutdown().await;
    let answered = answered?;
    let usage = &answered["usage"];
    let reason = answered["stopReason"].as_str().unwrap_or("end_turn");
    Ok(Turn {
        result: None,
        cost_usd: seen.cost,
        tokens: prices::Tokens {
            input: usage["inputTokens"].as_u64().unwrap_or(0),
            cached: usage["cachedReadTokens"].as_u64().unwrap_or(0),
            cache_written: 0,
            output: usage["outputTokens"].as_u64().unwrap_or(0),
        },
        session: Some(session),
        error: match reason {
            "refusal" => Some("The model declined to answer this.".into()),
            "max_tokens" => Some("The model ran out of room before it finished.".into()),
            _ => None,
        },
        stopped: reason == "cancelled",
    })
}

/// The models OpenCode offers in a folder: a session is made to read them, and deleted again.
pub(super) async fn models(started: Started, cwd: &Path) -> Result<Vec<(String, String)>> {
    let mut rpc = Rpc { started, next: 0 };
    let mut seen = Seen::default();
    initialize(&mut rpc).await?;
    let said = rpc.call("session/new", json!({ "cwd": cwd.to_string_lossy(), "mcpServers": [] }), None, &mut no_events, &approve_nothing, &mut seen).await?;
    let options = model_options(&said);
    if let Some(id) = said["sessionId"].as_str() {
        let _ = rpc.call("session/delete", json!({ "sessionId": id }), None, &mut no_events, &approve_nothing, &mut seen).await;
    }
    let _ = rpc.started.stdin.shutdown().await;
    Ok(options)
}

#[cfg(test)]
pub(super) fn read_update(update: &Value) -> (Vec<Event>, Option<f64>) {
    let mut seen = Seen::default();
    (updated(update, &mut seen), seen.cost)
}
