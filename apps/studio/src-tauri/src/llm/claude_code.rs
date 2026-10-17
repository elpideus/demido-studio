//! Claude through Claude Code, the official CLI, signed in with a Claude subscription (see
//! [`crate::claude_code`]).
//!
//! Each model call is one run of `claude -p`, holding nothing between runs: Demido keeps the
//! history and runs the tools itself, as with any other model. The CLI's own tools are off (bar
//! the built-ins a provider turns on), so Demido's tools are described in the system prompt and
//! called in text, `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`, which this client
//! takes out of the stream. Claude sometimes falls back to the `<invoke>` form of its native tool
//! use in long conversations; that is read as a call too. The history goes as one user message, a
//! content block per turn with the last one marked for caching, so each request finds the one
//! before in the prompt cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::{ChatRequest, CloudModel, Completion, LlmError, LlmMessage, StreamEvent, ToolSpec, retry};
use crate::claude_code::{self as cc, BUILTIN_TOOLS};
use crate::db::ToolCall;
use crate::models::capabilities::Capabilities;

const CALL_OPEN: &str = "<tool_call>";
const CALL_CLOSE: &str = "</tool_call>";

/// The models a subscription can use in Claude Code.
pub fn models() -> Vec<CloudModel> {
    [
        (
            "claude-fable-5-1",
            "Claude Fable 5.1",
            "Anthropic's most capable model, for the hardest reasoning and long agentic work.",
        ),
        (
            "claude-opus-5-5",
            "Claude Opus 5.5",
            "Frontier intelligence for complex agentic work and coding.",
        ),
        (
            "claude-sonnet-5-5",
            "Claude Sonnet 5.5",
            "Fast and capable, for most everyday work.",
        ),
        (
            "claude-haiku-5-5",
            "Claude Haiku 5.5",
            "The fastest model, for quick answers.",
        ),
    ]
    .into_iter()
    .map(|(id, name, description)| CloudModel {
        id: id.into(),
        display_name: name.into(),
        description: description.into(),
        input_token_limit: 1_000_000,
        output_token_limit: 128_000,
        thinking: true,
        always_thinks: false,
        capabilities: Some(Capabilities {
            vision: Some(true),
            audio: Some(false),
            tools: Some(true),
            thinking: Some(true),
        }),
        free: false,
        effort: true,
        // Claude Code takes all five, and lowers one a model lacks to the nearest it has.
        efforts: ["low", "medium", "high", "xhigh", "max"].map(String::from).to_vec(),
        default_effort: None,
    })
    .collect()
}

#[derive(Clone)]
pub struct ClaudeCodeClient {
    pub model: String,
    /// Claude Code's own tools this provider turns on (none by default).
    pub builtin_tools: Vec<String>,
    /// The Claude Code profile (signed-in account) to run with.
    pub profile: String,
}

impl ClaudeCodeClient {
    pub fn new(model: String, builtin_tools: &[String], profile: &str) -> Self {
        Self {
            model,
            profile: profile.to_string(),
            builtin_tools: builtin_tools
                .iter()
                .filter(|t| BUILTIN_TOOLS.contains(&t.as_str()))
                .cloned()
                .collect(),
        }
    }

    fn args(&self, req: &ChatRequest, system_file: &str) -> Vec<String> {
        let tools = self.builtin_tools.join(",");
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--setting-sources",
            "",
            "--no-session-persistence",
            "--permission-mode",
            "dontAsk",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        args.extend(["--model".into(), self.model.clone()]);
        args.extend(["--tools".into(), tools.clone()]);
        if !tools.is_empty() {
            args.extend(["--allowedTools".into(), tools]);
        }
        // Answering directly keeps its thinking short: Claude Code's models cannot turn it off.
        let effort = match (req.params.thinking, &req.params.effort) {
            (Some(false), _) => Some("low"),
            (_, effort) => effort.as_deref(),
        };
        if let Some(effort) = effort {
            args.extend(["--effort".into(), effort.into()]);
        }
        args.extend(["--system-prompt-file".into(), system_file.into()]);
        args
    }

    /// Whether the history's last block is marked for caching (see [`input_message`]). Not with
    /// Claude Code's own tools on: when it runs one, it marks the newest messages of its next
    /// request itself, and with Demido's mark that is one more than the API allows.
    fn marks_cache(&self) -> bool {
        self.builtin_tools.is_empty() && !CACHE_MARK_REFUSED.load(Ordering::Relaxed)
    }

    /// What the trace keeps: the command line, the system prompt and the message sent.
    pub fn body(&self, req: &ChatRequest) -> Value {
        json!({
            "cli": self.args(req, "<system>"),
            "system": system_prompt(req),
            "input": input_message(&req.messages, self.marks_cache()),
        })
    }

    /// Runs the CLI once for one answer. Busy answers are tried again by [`super::Client::stream`].
    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let Some(binary) = cc::find_binary() else {
            let help = cc::install_help();
            return Err(LlmError::Provider(format!(
                "Claude Code is not installed. Install it ({}: `{}`, or see {}), then sign in from Settings, Providers.",
                help.shell, help.command, help.docs
            )));
        };
        let marked = self.marks_cache();
        match self.run(&binary, req, cancel, &mut on_event).await {
            // Should a newer Claude Code mark more of the request itself, the API refuses the
            // request before answering: it goes again without the mark, as do later ones.
            Err(LlmError::Provider(said)) if marked && said.contains("cache_control") => {
                CACHE_MARK_REFUSED.store(true, Ordering::Relaxed);
                self.run(&binary, req, cancel, &mut on_event).await
            }
            other => other,
        }
    }

    async fn run(
        &self,
        binary: &Path,
        req: &ChatRequest,
        cancel: &CancellationToken,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, LlmError> {
        let system_file = SystemFile::write(&system_prompt(req))
            .map_err(|e| LlmError::Provider(format!("Could not write the system prompt for Claude Code: {e}")))?;
        let input = input_message(&req.messages, self.marks_cache());
        let mut cmd = cc::command(binary, &self.profile);
        cmd.args(self.args(req, &system_file.0.display().to_string()))
            .stdin(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| LlmError::Provider(format!("Could not start Claude Code: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            let line = format!("{input}\n");
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| LlmError::Provider(format!("Could not talk to Claude Code: {e}")))?;
            drop(stdin);
        }
        let stderr = child.stderr.take().map(|mut s| {
            tokio::spawn(async move {
                let mut text = String::new();
                let _ = s.read_to_string(&mut text).await;
                text
            })
        });
        let Some(stdout) = child.stdout.take() else {
            return Err(LlmError::Provider("Claude Code gave no output.".into()));
        };
        let mut lines = cc::lines(stdout);
        let mut run = Run::new(
            Completion {
                request_body: self.body(req),
                ..Default::default()
            },
            &req.tools,
        );
        loop {
            let line = tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.start_kill();
                    return Err(LlmError::Cancelled);
                }
                l = lines.next_line() => l,
            };
            match line {
                Ok(Some(line)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        run.apply(&v, on_event);
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(LlmError::Provider(format!("Could not read Claude Code's answer: {e}"))),
            }
        }
        let exit = child.wait().await.ok();
        let stderr = match stderr {
            Some(task) => task.await.unwrap_or_default(),
            None => String::new(),
        };
        drop(system_file);
        run.finish(exit.is_some_and(|e| e.success()), stderr.trim(), on_event)
    }
}

/// The system prompt in a file of its own: a long prompt does not fit on a Windows command line.
struct SystemFile(PathBuf);

impl SystemFile {
    fn write(text: &str) -> std::io::Result<Self> {
        let path = cc::work_dir().join(format!("system-{}.md", crate::db::new_id()));
        std::fs::write(&path, text)?;
        Ok(Self(path))
    }
}

impl Drop for SystemFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Demido's system prompt, followed by how the conversation is sent and how to call its tools.
/// What the model is asked to do goes here rather than after the conversation, where it would sit
/// in the way of the next request's cache hit.
fn system_prompt(req: &ChatRequest) -> String {
    let mut out = req.system.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(
        "# The conversation\n\n\
         The user message holds the conversation so far: the user's messages in <user> tags, your earlier \
         turns in <assistant> tags and tool results in <tool_result> tags. Write the assistant's next turn: \
         answer the latest message, or carry on from the latest tool results. Write only the turn itself, \
         without the <assistant> tags.",
    );
    if req.tools.is_empty() {
        return out;
    }
    out.push_str(
        "\n\n# Calling tools\n\n\
         You can use the tools listed below. To call one, write the call on its own, exactly in this form:\n\n\
         <tool_call>{\"name\": \"tool_name\", \"arguments\": {\"parameter\": \"value\"}}</tool_call>\n\n\
         The arguments are a JSON object matching the tool's parameters. You may make several calls in a row. \
         After your last call, stop: the calls are run and their results come back in <tool_result> blocks. \
         Never write a <tool_result> yourself, and do not describe a call instead of making it. Keep to this \
         form for every call, however long the conversation gets.\n\n\
         ## Tools\n",
    );
    for tool in &req.tools {
        out.push_str(&describe_tool(tool));
    }
    out
}

fn describe_tool(tool: &ToolSpec) -> String {
    format!(
        "\n### {}\n{}\nParameters: {}\n",
        tool.name,
        tool.description.trim(),
        serde_json::to_string(&tool.parameters).unwrap_or_else(|_| "{}".into())
    )
}

/// Set once the API has refused [`input_message`]'s cache mark, so later runs go without it.
static CACHE_MARK_REFUSED: AtomicBool = AtomicBool::new(false);

/// The block after the history. Something must follow the marked block: the API does not find
/// a mark on a message's last block again once more is added after it.
const NEXT_TURN: &str = "(Write the assistant's next turn.)";

/// The history as one user message for `--input-format stream-json`, the turns tagged. Each user
/// and assistant turn is a content block of its own (tool results join the turn that called them,
/// images sit where they were attached), so a request repeats the blocks of the one before
/// unchanged. When `mark`, the last of them is marked for caching, and the next request reads
/// everything up to it from the cache. Claude Code's own mark is on the context it adds after
/// the history, which no later request starts with. The mark lasts an hour, as Claude Code's do:
/// a shorter one may not come before them.
fn input_message(history: &[LlmMessage], mark: bool) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    // Adds text to the last block when `join` and it is text, else as a block of its own.
    let text = |blocks: &mut Vec<Value>, s: &str, join: bool| match blocks.last_mut() {
        Some(last) if join && last["type"] == "text" => {
            let joined = format!("{}{s}", last["text"].as_str().unwrap_or_default());
            last["text"] = json!(joined);
        }
        _ => blocks.push(json!({"type": "text", "text": s})),
    };
    for m in history {
        match m {
            LlmMessage::User { content, media } => {
                text(&mut blocks, "<user>\n", false);
                for image in media.iter().filter(|m| m.is_image()) {
                    blocks.push(json!({"type": "image", "source": {"type": "base64", "media_type": image.mime, "data": image.data}}));
                }
                text(&mut blocks, &format!("{}\n</user>\n", content.trim_end()), true);
            }
            LlmMessage::Assistant {
                content, tool_calls, ..
            } => {
                let mut turn = String::from("<assistant>\n");
                let content = history_text(content);
                if !content.trim().is_empty() {
                    turn.push_str(content.trim_end());
                    turn.push('\n');
                }
                for call in tool_calls {
                    let arguments = super::parse_arguments(&call.arguments).unwrap_or_else(|_| json!({}));
                    let call = json!({"name": call.name, "arguments": arguments});
                    turn.push_str(&format!("{CALL_OPEN}{call}{CALL_CLOSE}\n"));
                }
                turn.push_str("</assistant>\n");
                text(&mut blocks, &turn, false);
            }
            LlmMessage::Tool { call_id, name, content } => {
                text(
                    &mut blocks,
                    &format!(
                        "<tool_result call_id=\"{call_id}\" name=\"{name}\">\n{}\n</tool_result>\n",
                        content.trim_end()
                    ),
                    true,
                );
            }
        }
    }
    match blocks.last_mut() {
        None => text(&mut blocks, "(The conversation is empty.)", false),
        Some(last) => {
            if mark {
                last["cache_control"] = json!({"type": "ephemeral", "ttl": "1h"});
            }
            text(&mut blocks, NEXT_TURN, false);
        }
    }
    json!({"type": "user", "message": {"role": "user", "content": blocks}})
}

/// What goes back to the model in place of a call it wrote in a form that was not read.
const UNREAD_CALL: &str = "[A tool call written in a form that was not read was here. It did not run.]";

/// An earlier answer's text as it goes back to the model. Answers from before the `<invoke>` form
/// was read can hold calls in it, which never ran: they are replaced by a note, so the model
/// neither copies the form nor thinks they ran. An empty `<invoke>` was only the wrapper of a
/// `<tool_call>` that did run, and is left out.
fn history_text(content: &str) -> String {
    if !content.contains('<') {
        return content.to_string();
    }
    let mut filter = CallFilter::default();
    let mut pieces = filter.push(content);
    pieces.extend(filter.finish());
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(t) => out.push_str(&t),
            Piece::Invoke { body, .. } if body.trim().is_empty() => {}
            Piece::Call(_) | Piece::Invoke { .. } => {
                if !out.trim_end().ends_with(UNREAD_CALL) {
                    out.push_str(UNREAD_CALL);
                }
            }
        }
    }
    out
}

/// Takes tool calls out of streamed text: text is passed on as it comes, except a tail that
/// could be the start of a call, which waits for the next piece.
#[derive(Default)]
struct CallFilter {
    pending: String,
    /// The call being read, once its opening tag has come.
    open: Option<Open>,
}

/// A call's opening tag.
enum Open {
    /// `<tool_call>`, the JSON form the system prompt asks for.
    Call,
    /// `<invoke name="...">`, the form of Claude's native tool use, its parameters in
    /// `<parameter name="...">` tags. Ends at `close`.
    Invoke { name: String, close: String },
}

impl Open {
    fn close(&self) -> &str {
        match self {
            Open::Call => CALL_CLOSE,
            Open::Invoke { close, .. } => close,
        }
    }

    fn piece(self, body: String) -> Piece {
        match self {
            Open::Call => Piece::Call(body),
            Open::Invoke { name, .. } => Piece::Invoke { name, body },
        }
    }
}

enum Piece {
    Text(String),
    /// The inside of a `<tool_call>`.
    Call(String),
    /// The inside of an `<invoke>`.
    Invoke {
        name: String,
        body: String,
    },
}

/// What text from a `<` on is.
enum Scan {
    /// A call's opening tag, this long.
    Open(Open, usize),
    /// A tag that only wraps calls, this long, left out: Claude's native form puts its calls in
    /// `<function_calls>`, and a `<tool_call>` sometimes comes inside an `<invoke>`.
    Wrapper(usize),
    /// Too short yet to tell.
    Partial,
    Text,
}

fn scan(s: &str) -> Scan {
    if s.starts_with(CALL_OPEN) {
        return Scan::Open(Open::Call, CALL_OPEN.len());
    }
    let (closing, ns, name) = tag_parts(&s[1..]);
    let head = s.len() - name.len();
    if let Some(rest) = name.strip_prefix("invoke ").filter(|_| !closing) {
        return match rest.find('>') {
            Some(end) => match attribute(&rest[..end], "name") {
                Some(tool) => Scan::Open(
                    Open::Invoke {
                        name: tool,
                        close: format!("</{ns}invoke>"),
                    },
                    head + "invoke ".len() + end + 1,
                ),
                None => Scan::Text,
            },
            None if rest.len() < 200 => Scan::Partial,
            None => Scan::Text,
        };
    }
    if name.starts_with("function_calls>") {
        return Scan::Wrapper(head + "function_calls>".len());
    }
    if closing && name.starts_with("invoke>") {
        return Scan::Wrapper(head + "invoke>".len());
    }
    // A tag's name still coming.
    let so_far = s[1..].trim_start_matches('/');
    if s.len() < 30 && so_far.chars().all(|c| c.is_ascii_alphabetic() || c == '_' || c == ':') {
        Scan::Partial
    } else {
        Scan::Text
    }
}

/// The text after a tag's `<`, split into whether the tag closes, the namespace its name has
/// (`ns:`, as Claude's native tool-use tags can) and the rest, from the name on.
fn tag_parts(after_lt: &str) -> (bool, &str, &str) {
    let (closing, rest) = match after_lt.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, after_lt),
    };
    let ns = rest
        .find(':')
        .filter(|&i| i > 0 && i <= 12 && rest[..i].chars().all(|c| c.is_ascii_alphabetic()))
        .map_or(0, |i| i + 1);
    (closing, &rest[..ns], &rest[ns..])
}

/// The value of `name="..."` among a tag's attributes.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let at = attributes.find(&format!("{name}="))?;
    let rest = &attributes[at + name.len() + 1..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    let value = &rest[..rest.find(quote)?];
    (!value.is_empty()).then(|| value.to_string())
}

impl CallFilter {
    fn push(&mut self, s: &str) -> Vec<Piece> {
        self.pending.push_str(s);
        let mut out = Vec::new();
        loop {
            if let Some(open) = &self.open {
                let close = open.close();
                let Some(end) = self.pending.find(close) else {
                    break;
                };
                let body: String = self.pending.drain(..end).collect();
                self.pending.drain(..close.len());
                if let Some(open) = self.open.take() {
                    out.push(open.piece(body));
                }
                continue;
            }
            // The first `<` that starts a tag of calls, or could still.
            let mut from = 0;
            let found = loop {
                let Some(i) = self.pending[from..].find('<') else {
                    break None;
                };
                match scan(&self.pending[from + i..]) {
                    Scan::Text => from += i + 1,
                    tag => break Some((from + i, tag)),
                }
            };
            let cut = found.as_ref().map_or(self.pending.len(), |(at, _)| *at);
            if cut > 0 {
                out.push(Piece::Text(self.pending.drain(..cut).collect()));
            }
            match found {
                Some((_, Scan::Wrapper(len))) => {
                    self.pending.drain(..len);
                }
                Some((_, Scan::Open(open, len))) => {
                    self.pending.drain(..len);
                    self.open = Some(open);
                }
                _ => break,
            }
        }
        out
    }

    /// What is left at the end: a call never closed is still a call.
    fn finish(&mut self) -> Vec<Piece> {
        let rest = std::mem::take(&mut self.pending);
        match self.open.take() {
            Some(open) => vec![open.piece(rest)],
            None if rest.is_empty() => Vec::new(),
            None => vec![Piece::Text(rest)],
        }
    }
}

/// One run of the CLI as its events arrive.
struct Run {
    out: Completion,
    filter: CallFilter,
    /// Messages the API sent so far (more than one when a built-in tool ran).
    messages: usize,
    /// The final `result` event.
    result: Option<Value>,
    /// The error an assistant message carried (`authentication_failed`, say).
    error: Option<String>,
    /// Each tool's parameters schema, to read `<invoke>` calls' values by.
    schemas: HashMap<String, Value>,
}

impl Run {
    fn new(out: Completion, tools: &[ToolSpec]) -> Self {
        Self {
            out,
            filter: CallFilter::default(),
            messages: 0,
            result: None,
            error: None,
            schemas: tools.iter().map(|t| (t.name.clone(), t.parameters.clone())).collect(),
        }
    }

    fn apply(&mut self, v: &Value, on_event: &mut dyn FnMut(StreamEvent)) {
        match v["type"].as_str().unwrap_or_default() {
            "stream_event" => self.apply_api_event(&v["event"], on_event),
            "assistant" => {
                if let Some(error) = v["error"].as_str().or_else(|| v["message"]["error"].as_str()) {
                    self.error = Some(error.to_string());
                }
            }
            "result" => self.result = Some(v.clone()),
            _ => {}
        }
    }

    fn apply_api_event(&mut self, e: &Value, on_event: &mut dyn FnMut(StreamEvent)) {
        match e["type"].as_str().unwrap_or_default() {
            "message_start" => {
                self.messages += 1;
                if let Some(model) = e["message"]["model"].as_str() {
                    self.out.model = Some(model.to_string());
                }
                if self.messages > 1 && !self.out.content.is_empty() {
                    self.text("\n\n", on_event);
                }
            }
            "content_block_start" => {
                let block = &e["content_block"];
                // One of Claude Code's own tools, when a provider turned it on.
                if matches!(block["type"].as_str(), Some("tool_use" | "server_tool_use")) {
                    let note = format!("\n\nUsing {}…\n\n", block["name"].as_str().unwrap_or("a tool"));
                    self.out.reasoning.push_str(&note);
                    on_event(StreamEvent::Reasoning(note));
                }
            }
            "content_block_delta" => {
                let delta = &e["delta"];
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => self.text(delta["text"].as_str().unwrap_or_default(), on_event),
                    "thinking_delta" => {
                        let t = delta["thinking"].as_str().unwrap_or_default();
                        if !t.is_empty() {
                            self.out.reasoning.push_str(t);
                            on_event(StreamEvent::Reasoning(t.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = e["delta"]["stop_reason"].as_str() {
                    self.out.finish_reason = Some(reason.to_string());
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, s: &str, on_event: &mut dyn FnMut(StreamEvent)) {
        let pieces = self.filter.push(s);
        self.take(pieces, on_event);
    }

    fn take(&mut self, pieces: Vec<Piece>, on_event: &mut dyn FnMut(StreamEvent)) {
        for piece in pieces {
            match piece {
                Piece::Text(t) => {
                    self.out.content.push_str(&t);
                    on_event(StreamEvent::Content(t));
                }
                Piece::Call(raw) => match parse_call(&raw) {
                    Some((name, arguments)) => self.call(name, arguments, on_event),
                    // Not a call after all: it was text.
                    None => {
                        let t = format!("{CALL_OPEN}{raw}{CALL_CLOSE}");
                        self.out.content.push_str(&t);
                        on_event(StreamEvent::Content(t));
                    }
                },
                // A `<tool_call>` inside an `<invoke>` is the call itself.
                Piece::Invoke { body, .. } if body.contains(CALL_OPEN) => {
                    let mut inner = CallFilter::default();
                    let mut pieces = inner.push(&body);
                    pieces.extend(inner.finish());
                    pieces.retain(|p| !matches!(p, Piece::Text(t) if t.trim().is_empty()));
                    self.take(pieces, on_event);
                }
                Piece::Invoke { name, body } => {
                    let arguments = invoke_arguments(&body, self.schemas.get(&name));
                    self.call(name, arguments.to_string(), on_event);
                }
            }
        }
    }

    fn call(&mut self, name: String, arguments: String, on_event: &mut dyn FnMut(StreamEvent)) {
        let index = self.out.tool_calls.len();
        on_event(StreamEvent::ToolCall {
            index,
            name: name.clone(),
        });
        self.out.tool_calls.push(ToolCall {
            id: format!("cc_{}", crate::db::new_id().replace('-', "")),
            name,
            arguments,
        });
    }

    fn finish(
        mut self,
        exited_ok: bool,
        stderr: &str,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<Completion, LlmError> {
        let rest = self.filter.finish();
        self.take(rest, on_event);
        // The calls are the end of the turn; text the model wrote around them is kept.
        let trimmed = self.out.content.trim_end().len();
        self.out.content.truncate(trimmed);

        let result = self.result.take().unwrap_or(Value::Null);
        let failed = result["is_error"].as_bool().unwrap_or(result.is_null() || !exited_ok);
        if failed || self.error.is_some() {
            let said = result["result"]
                .as_str()
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
                .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
                .unwrap_or_else(|| "Claude Code stopped without answering.".into());
            let status = result["api_error_status"].as_u64().map(|s| s as u16);
            return Err(run_error(self.error.as_deref(), status, &said));
        }
        let u = &result["usage"];
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        self.out.usage.cached_tokens = n("cache_read_input_tokens");
        self.out.usage.prompt_tokens =
            n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
        self.out.usage.completion_tokens = n("output_tokens");
        self.out.usage.reasoning_tokens = u["output_tokens_details"]["thinking_tokens"].as_u64().unwrap_or(0);
        if self.out.model.is_none() {
            self.out.model = result["modelUsage"].as_object().and_then(|m| m.keys().next().cloned());
        }
        if self.out.content.is_empty() && self.out.tool_calls.is_empty() {
            return Err(LlmError::Provider("Claude returned an empty answer. Try again.".into()));
        }
        Ok(self.out)
    }
}

/// What went wrong in a run, in words the person can act on.
fn run_error(error: Option<&str>, status: Option<u16>, said: &str) -> LlmError {
    let lower = said.to_ascii_lowercase();
    if error == Some("authentication_failed")
        || lower.contains("not logged in")
        || lower.contains("/login")
        || status == Some(401)
    {
        return LlmError::Provider(
            "Claude Code is not signed in. Sign in with your Claude account in Settings, Providers.".into(),
        );
    }
    // A plan's usage limit lasts hours: not worth waiting for.
    if lower.contains("usage limit") || lower.contains("hit your limit") || lower.contains("limit reached") {
        return LlmError::Provider(format!("Your Claude plan's usage limit is reached: {said}"));
    }
    if status == Some(404) || error == Some("model_not_found") {
        return LlmError::Unavailable(said.to_string());
    }
    if status.is_some_and(|s| retry::busy_status(s, false)) || retry::sounds_busy(said) {
        return LlmError::Busy {
            message: said.to_string(),
            retry_after: None,
        };
    }
    LlmError::Provider(format!("Claude Code: {said}"))
}

/// A call's JSON: `{"name": ..., "arguments": {...}}`. `None` when it names no tool. JSON too
/// broken to read is still a call when the tool's name can be found in it: its arguments are the
/// broken text, so the model is told they could not be read and writes the call again, where
/// taking it for text would end the turn.
fn parse_call(raw: &str) -> Option<(String, String)> {
    let Ok(v) = super::parse_arguments(raw) else {
        return Some((named_in(raw)?, raw.trim().to_string()));
    };
    let name = v["name"].as_str().filter(|n| !n.is_empty())?.to_string();
    let arguments = match v.get("arguments").or_else(|| v.get("parameters")) {
        Some(Value::String(s)) => s.clone(),
        Some(a) if a.is_object() => a.to_string(),
        _ => "{}".into(),
    };
    Some((name, arguments))
}

/// The tool a broken call's JSON names, from its `"name": "..."`.
fn named_in(raw: &str) -> Option<String> {
    let rest = &raw[raw.find("\"name\"")? + 6..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let name = &rest[..rest.find('"')?];
    let plain =
        !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    plain.then(|| name.to_string())
}

/// An `<invoke>` call's arguments, from its `<parameter name="...">value</parameter>` tags. A
/// parameter the tool's schema says is a string is taken as written; others are read as JSON,
/// or taken as text when they are not.
fn invoke_arguments(body: &str, schema: Option<&Value>) -> Value {
    let mut args = serde_json::Map::new();
    let mut rest = body;
    while let Some(lt) = rest.find('<') {
        let (closing, ns, tag) = tag_parts(&rest[lt + 1..]);
        let Some(after) = tag.strip_prefix("parameter ").filter(|_| !closing) else {
            rest = &rest[lt + 1..];
            continue;
        };
        let Some(end) = after.find('>') else {
            break;
        };
        let close = format!("</{ns}parameter>");
        let from_value = &after[end + 1..];
        let (value, next) = match from_value.find(&close) {
            Some(stop) => (&from_value[..stop], &from_value[stop + close.len()..]),
            None => (from_value, ""),
        };
        if let Some(name) = attribute(&after[..end], "name") {
            let property = schema.map_or(&Value::Null, |s| &s["properties"][name.as_str()]);
            args.insert(name, parameter_value(value, property));
        }
        rest = next;
    }
    Value::Object(args)
}

fn parameter_value(value: &str, property: &Value) -> Value {
    if property["type"] == "string" {
        let value = value.strip_prefix('\n').unwrap_or(value);
        let value = value.strip_suffix('\n').unwrap_or(value);
        return Value::String(value.to_string());
    }
    serde_json::from_str(value.trim()).unwrap_or_else(|_| Value::String(value.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{GenParams, Media};

    fn run_text(chunks: &[&str]) -> (Vec<StreamEvent>, Completion) {
        run_with_tools(chunks, &[])
    }

    fn run_with_tools(chunks: &[&str], tools: &[ToolSpec]) -> (Vec<StreamEvent>, Completion) {
        let mut run = Run::new(Completion::default(), tools);
        let mut events = Vec::new();
        for c in chunks {
            run.text(c, &mut |e| events.push(e));
        }
        run.result = Some(
            json!({"type": "result", "is_error": false, "usage": {"input_tokens": 3, "cache_read_input_tokens": 7, "output_tokens": 5}}),
        );
        let done = run.finish(true, "", &mut |e| events.push(e)).unwrap();
        (events, done)
    }

    #[test]
    fn calls_are_taken_out_of_the_text() {
        let (events, done) = run_text(&[
            "Let me check.\n<tool",
            "_call>{\"name\": \"get_time\", \"argu",
            "ments\": {\"tz\": \"UTC\"}}</tool_call>\n<tool_call>{\"name\": \"b\"}",
        ]);
        assert_eq!(done.content, "Let me check.");
        assert_eq!(done.tool_calls.len(), 2);
        assert_eq!(done.tool_calls[0].name, "get_time");
        assert_eq!(done.tool_calls[0].arguments, "{\"tz\":\"UTC\"}");
        // Unclosed at the end, still a call.
        assert_eq!(done.tool_calls[1].name, "b");
        assert_eq!(done.tool_calls[1].arguments, "{}");
        assert!(done.tool_calls[0].id.starts_with("cc_"));
        assert_eq!(done.usage.prompt_tokens, 10);
        let streamed: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Content(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(!streamed.contains("<tool"), "{streamed}");
    }

    #[test]
    fn text_that_only_looks_like_a_call_stays_text() {
        let (_, done) = run_text(&["a <b> and <tool", "s> and <tool_call>not json</tool_call> end"]);
        assert_eq!(
            done.content,
            "a <b> and <tools> and <tool_call>not json</tool_call> end"
        );
        assert!(done.tool_calls.is_empty());
    }

    fn tool(name: &str, properties: Value) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: String::new(),
            parameters: json!({"type": "object", "properties": properties}),
        }
    }

    fn streamed(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Content(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn calls_in_claudes_native_form_are_read() {
        // Its tags can come with a namespace.
        let ns = "x:";
        let answer = format!(
            "Moving them.\n<{ns}function_calls>\n<{ns}invoke name=\"mail_move\">\n\
             <{ns}parameter name=\"ids\">[\"m1\", \"m2\"]</{ns}parameter>\n\
             <{ns}parameter name=\"to\">Trash</{ns}parameter>\n</{ns}invoke>\n\
             <invoke name=\"run_python\">\n<parameter name=\"code\">print(\"a<b\")\nx = 1\n</parameter>\n</invoke>\n\
             <invoke name=\"other\"><parameter name=\"n\">5</parameter><parameter name=\"s\">five</parameter></invoke>\n\
             </{ns}function_calls>"
        );
        let chars: Vec<char> = answer.chars().collect();
        let chunks: Vec<String> = chars.chunks(7).map(|c| c.iter().collect()).collect();
        let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let tools = [
            tool("mail_move", json!({"ids": {"type": "array"}, "to": {"type": "string"}})),
            tool("run_python", json!({"code": {"type": "string"}})),
        ];
        let (events, done) = run_with_tools(&chunks, &tools);
        assert_eq!(done.content, "Moving them.");
        assert!(!streamed(&events).contains('<'), "{}", streamed(&events));
        let calls: Vec<(&str, Value)> = done
            .tool_calls
            .iter()
            .map(|c| (c.name.as_str(), serde_json::from_str(&c.arguments).unwrap()))
            .collect();
        assert_eq!(
            calls,
            vec![
                ("mail_move", json!({"ids": ["m1", "m2"], "to": "Trash"})),
                ("run_python", json!({"code": "print(\"a<b\")\nx = 1"})),
                ("other", json!({"n": 5, "s": "five"})),
            ]
        );
    }

    #[test]
    fn a_tool_call_inside_an_invoke_is_the_call() {
        let (_, done) = run_text(&[
            "<invoke name=\"load_tools\">\n<tool_call>{\"name\": \"load_tools\", \"arguments\": {\"groups\": [\"mail\"]}}</tool_call>\n</invoke>",
        ]);
        assert_eq!(done.content, "");
        assert_eq!(done.tool_calls.len(), 1);
        assert_eq!(done.tool_calls[0].name, "load_tools");
        assert_eq!(done.tool_calls[0].arguments, "{\"groups\":[\"mail\"]}");
    }

    #[test]
    fn an_unclosed_invoke_at_the_end_is_still_a_call() {
        let tools = [tool("mail_list", json!({"limit": {"type": "integer"}}))];
        let (_, done) = run_with_tools(&["<invoke name=\"mail_list\">\n<parameter name=\"limit\">100"], &tools);
        assert_eq!(done.tool_calls.len(), 1);
        assert_eq!(done.tool_calls[0].arguments, "{\"limit\":100}");
    }

    #[test]
    fn a_call_with_broken_json_still_reaches_the_tool() {
        let (_, done) =
            run_text(&["<tool_call>{\"name\": \"mail_move\", \"arguments\": {\"ids\": [\"m1\"}}</tool_call>"]);
        assert_eq!(done.tool_calls.len(), 1);
        assert_eq!(done.tool_calls[0].name, "mail_move");
        // The tool's run says the arguments could not be read, so the model writes them again.
        assert!(crate::llm::parse_arguments(&done.tool_calls[0].arguments).is_err());
    }

    #[test]
    fn calls_that_never_ran_are_left_out_of_the_history() {
        let history = vec![
            LlmMessage::Assistant {
                content: "<invoke name=\"load_tools\">\n</invoke>".into(),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "a".into(),
                    name: "load_tools".into(),
                    arguments: "{}".into(),
                }],
                provider_meta: None,
            },
            LlmMessage::Assistant {
                content: "Sorting.\n<invoke name=\"run_python\">\n<parameter name=\"code\">x</parameter>\n</invoke>\n\
                          <invoke name=\"run_python\">\n<parameter name=\"code\">y</parameter>\n</invoke>"
                    .into(),
                reasoning: None,
                tool_calls: vec![],
                provider_meta: None,
            },
        ];
        let m = input_message(&history, false);
        let content = m["message"]["content"].as_array().unwrap();
        assert_eq!(
            content[0]["text"],
            "<assistant>\n<tool_call>{\"arguments\":{},\"name\":\"load_tools\"}</tool_call>\n</assistant>\n"
        );
        assert_eq!(
            content[1]["text"],
            format!("<assistant>\nSorting.\n{UNREAD_CALL}\n</assistant>\n")
        );
    }

    #[test]
    fn errors_say_what_to_do() {
        assert!(
            run_error(Some("authentication_failed"), None, "Not logged in · Please run /login")
                .to_string()
                .contains("Sign in")
        );
        assert!(matches!(
            run_error(None, Some(529), "Overloaded"),
            LlmError::Busy { .. }
        ));
        assert!(
            run_error(None, Some(429), "Claude AI usage limit reached|1760000000")
                .to_string()
                .contains("usage limit")
        );
    }

    #[test]
    fn the_history_is_one_tagged_message_with_images() {
        let history = vec![
            LlmMessage::User {
                content: "What is this?".into(),
                media: vec![Media::new("image/png", &[1, 2])],
            },
            LlmMessage::Assistant {
                content: "Checking.".into(),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "x".into(),
                    name: "look".into(),
                    arguments: "{\"a\":1}".into(),
                }],
                provider_meta: None,
            },
            LlmMessage::Tool {
                call_id: "x".into(),
                name: "look".into(),
                content: "a cat".into(),
            },
        ];
        let m = input_message(&history, true);
        let content = m["message"]["content"].as_array().unwrap();
        assert_eq!(content.len(), 5);
        assert_eq!(content[0]["text"], "<user>\n");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[2]["text"], "What is this?\n</user>\n");
        // The tool's result joins the turn that called it.
        assert_eq!(
            content[3]["text"],
            "<assistant>\nChecking.\n<tool_call>{\"arguments\":{\"a\":1},\"name\":\"look\"}</tool_call>\n</assistant>\n\
             <tool_result call_id=\"x\" name=\"look\">\na cat\n</tool_result>\n"
        );
        assert_eq!(content[3]["cache_control"]["ttl"], "1h");
        assert_eq!(content[4]["text"], NEXT_TURN);
        assert!(content[4].get("cache_control").is_none());
        let unmarked = input_message(&history, false);
        assert!(unmarked.to_string().matches("cache_control").next().is_none());
    }

    #[test]
    fn each_request_starts_with_the_blocks_of_the_one_before() {
        let user = |s: &str| LlmMessage::User {
            content: s.into(),
            media: vec![],
        };
        let step = |id: &str| {
            vec![
                LlmMessage::Assistant {
                    content: String::new(),
                    reasoning: None,
                    tool_calls: vec![ToolCall {
                        id: id.into(),
                        name: "look".into(),
                        arguments: "{}".into(),
                    }],
                    provider_meta: None,
                },
                LlmMessage::Tool {
                    call_id: id.into(),
                    name: "look".into(),
                    content: "seen".into(),
                },
            ]
        };
        // The history's blocks, unmarked, and the place of the mark.
        let blocks = |history: &[LlmMessage]| {
            let mut blocks = input_message(history, true)["message"]["content"]
                .as_array()
                .unwrap()
                .clone();
            let marked: Vec<usize> = (0..blocks.len())
                .filter(|&i| blocks[i].as_object_mut().unwrap().remove("cache_control").is_some())
                .collect();
            assert_eq!(blocks.pop().unwrap()["text"], NEXT_TURN);
            (blocks, marked)
        };
        let mut history = vec![user("Look twice.")];
        let (mut before, _) = blocks(&history);
        for id in ["a", "b"] {
            history.extend(step(id));
            let (now, marked) = blocks(&history);
            assert!(now.len() > before.len());
            assert_eq!(now[..before.len()], before[..]);
            // Only the latest turn is marked, so the next request finds it again.
            assert_eq!(marked, [now.len() - 1]);
            assert!(
                now.last().unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .ends_with("</tool_result>\n")
            );
            before = now;
        }
        let req = ChatRequest {
            system: "sys".into(),
            messages: history,
            tools: vec![],
            params: GenParams::default(),
        };
        assert!(system_prompt(&req).starts_with("sys\n\n# The conversation"));
    }

    #[test]
    fn builtin_tools_are_off_unless_turned_on() {
        let req = ChatRequest {
            system: "sys".into(),
            messages: vec![],
            tools: vec![],
            params: GenParams {
                thinking: Some(false),
                ..Default::default()
            },
        };
        let off = ClaudeCodeClient::new("claude-opus-5-5".into(), &[], cc::DEFAULT_PROFILE).args(&req, "f");
        let at = off.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(off[at + 1], "");
        assert!(!off.contains(&"--allowedTools".to_string()));
        assert!(off.windows(2).any(|w| w[0] == "--effort" && w[1] == "low"));
        let mut xhigh = req.clone();
        xhigh.params = GenParams {
            effort: Some("xhigh".into()),
            ..Default::default()
        };
        let args = ClaudeCodeClient::new("claude-opus-5-5".into(), &[], cc::DEFAULT_PROFILE).args(&xhigh, "f");
        assert!(args.windows(2).any(|w| w[0] == "--effort" && w[1] == "xhigh"));
        let on = ClaudeCodeClient::new(
            "claude-opus-5-5".into(),
            &["WebSearch".into(), "WebFetch".into()],
            cc::DEFAULT_PROFILE,
        );
        let args = on.args(&req, "f");
        let at = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[at + 1], "WebSearch");
        // With its own tools on, Claude Code marks its follow-up requests itself.
        assert!(!on.marks_cache());
    }

    /// The profile the live tests run with: `DEMIDO_CLAUDE_PROFILE`, or the default one.
    fn live_profile() -> String {
        std::env::var("DEMIDO_CLAUDE_PROFILE").unwrap_or_else(|_| cc::DEFAULT_PROFILE.into())
    }

    /// Runs the installed CLI for real: `cargo test -- --ignored live_claude_code`. Signed in, it
    /// answers with a tool call; signed out, it says to sign in.
    #[tokio::test]
    #[ignore]
    async fn live_claude_code() {
        let req = ChatRequest {
            system: "You are a helpful assistant.".into(),
            messages: vec![LlmMessage::User {
                content: "What time is it in UTC? Use the tool.".into(),
                media: vec![],
            }],
            tools: vec![ToolSpec {
                name: "get_time".into(),
                description: "The current time.".into(),
                parameters: json!({"type": "object", "properties": {"tz": {"type": "string"}}}),
            }],
            params: GenParams {
                thinking: Some(false),
                ..Default::default()
            },
        };
        let client = ClaudeCodeClient::new("claude-haiku-5-5".into(), &[], &live_profile());
        match client.stream(&req, &CancellationToken::new(), |_| {}).await {
            Ok(done) => {
                assert_eq!(
                    done.tool_calls.first().map(|c| c.name.as_str()),
                    Some("get_time"),
                    "{done:?}"
                );
            }
            Err(e) => assert!(e.to_string().contains("not signed in"), "{e}"),
        }
    }

    /// Runs the installed CLI twice for real: `cargo test -- --ignored live_cache_hits`. The second
    /// request carries the first one's history and one more turn, and reads that history from the
    /// prompt cache. Signed out, it says to sign in.
    #[tokio::test]
    #[ignore]
    async fn live_cache_hits() {
        let long: String = (1..=1500)
            .map(|i| format!("Item {i}: a line to make the history long.\n"))
            .collect();
        let mut req = ChatRequest {
            system: "You are a helpful assistant. Answer in one word.".into(),
            messages: vec![LlmMessage::User {
                content: format!("{long}\nHow many items are there?"),
                media: vec![],
            }],
            tools: vec![],
            params: GenParams {
                thinking: Some(false),
                ..Default::default()
            },
        };
        let client = ClaudeCodeClient::new("claude-haiku-5-5".into(), &[], &live_profile());
        let first = match client.stream(&req, &CancellationToken::new(), |_| {}).await {
            Ok(done) => done,
            Err(e) => return assert!(e.to_string().contains("not signed in"), "{e}"),
        };
        req.messages.push(LlmMessage::Assistant {
            content: first.content.clone(),
            reasoning: None,
            tool_calls: vec![],
            provider_meta: None,
        });
        req.messages.push(LlmMessage::User {
            content: "And how many lines?".into(),
            media: vec![],
        });
        let second = client.stream(&req, &CancellationToken::new(), |_| {}).await.unwrap();
        eprintln!("first: {:?}\nsecond: {:?}", first.usage, second.usage);
        // The history alone is about 15000 tokens: more than the system prompt could account for.
        assert!(second.usage.cached_tokens > 10_000, "{:?}", second.usage);
    }
}
