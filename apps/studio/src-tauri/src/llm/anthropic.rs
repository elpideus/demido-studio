//! Anthropic's Claude through its native Messages API, with an API key from the Claude Console.
//!
//! Thinking is replayed exactly as the model wrote it (signatures and all), kept in the message's
//! `provider_meta`. The newest models bind each thinking block to the conversation that produced
//! it; Demido's tool list grows during a turn, so requests ask the API to drop a block that no
//! longer matches rather than refuse the whole request.

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::openai::error_message;
use super::sse::SseDecoder;
use super::{ChatRequest, CloudModel, Completion, LlmError, LlmMessage, StreamEvent, retry};
use crate::db::ToolCall;
use crate::models::capabilities::Capabilities;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2023-06-01";
/// Lets a request drop a replayed thinking block bound to a different conversation.
const BINDING_BETA: &str = "thinking-binding-controls-2026-08-01";
/// `fallbacks: "default"`: a request the model's safety classifiers decline is run again on the
/// model Anthropic recommends, inside the same call.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Longest answer asked for when the person set no limit. Streaming keeps long answers safe from
/// timeouts; the model's own limit caps it.
const DEFAULT_MAX_TOKENS: u64 = 64_000;
/// Image types the API reads.
const IMAGE_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// Models that bind their thinking to the conversation (preserved thinking).
fn binds_thinking(model: &str) -> bool {
    [
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
    ]
    .iter()
    .any(|m| model.starts_with(m))
}

/// Models with a server-side fallback for declined requests (Haiku has none and refuses the
/// parameter).
fn has_fallback(model: &str) -> bool {
    [
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
    ]
    .iter()
    .any(|m| model == *m || model.starts_with(&format!("{m}-2")))
}

#[derive(Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Thinks adaptively (Claude 4.6 and later): sampling settings are refused, depth is set by
    /// effort.
    adaptive: bool,
    /// Takes `output_config.effort`.
    effort: bool,
    output_limit: u64,
}

impl AnthropicClient {
    /// `listed` is the model as the provider's list described it, when it is still listed.
    pub fn new(
        http: reqwest::Client,
        base_url: String,
        api_key: String,
        model: String,
        listed: Option<&CloudModel>,
    ) -> Self {
        Self {
            http,
            base_url,
            api_key,
            adaptive: listed.is_none_or(|m| m.thinking),
            effort: listed.is_none_or(|m| m.effort),
            output_limit: listed
                .map(|m| m.output_token_limit)
                .filter(|l| *l > 0)
                .unwrap_or(DEFAULT_MAX_TOKENS),
            model,
        }
    }

    fn betas(&self) -> Vec<&'static str> {
        let mut betas = Vec::new();
        if self.adaptive && binds_thinking(&self.model) {
            betas.push(BINDING_BETA);
        }
        if has_fallback(&self.model) {
            betas.push(FALLBACK_BETA);
        }
        betas
    }

    pub fn body(&self, req: &ChatRequest) -> Value {
        let p = &req.params;
        let max_tokens = p
            .max_tokens
            .map(u64::from)
            .unwrap_or_else(|| DEFAULT_MAX_TOKENS.min(self.output_limit));
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "stream": true,
            "messages": messages(&req.messages, &self.model),
            // Caches the longest prefix seen before: the system prompt, the tools and the history.
            "cache_control": {"type": "ephemeral"},
        });
        if !req.system.is_empty() {
            body["system"] = json!(req.system);
        }
        if !req.tools.is_empty() {
            let tools: Vec<Value> = req
                .tools
                .iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": input_schema(&t.parameters)}))
                .collect();
            body["tools"] = json!(tools);
        }
        if self.adaptive {
            let mut thinking = json!({"type": "adaptive", "display": "summarized"});
            if binds_thinking(&self.model) {
                thinking["block_binding"] = json!({"prefix_mismatch_behavior": "drop_block"});
            }
            body["thinking"] = thinking;
            if self.effort {
                // Answering directly: the newest models keep thinking on, so it is kept short
                // instead.
                let effort = match (p.thinking, &p.effort) {
                    (Some(false), _) => Some("low"),
                    (_, effort) => effort.as_deref(),
                };
                if let Some(effort) = effort {
                    body["output_config"] = json!({"effort": effort});
                }
            }
        } else {
            // Older models take sampling settings, though not temperature and top_p together.
            if let Some(v) = p.temperature {
                body["temperature"] = json!(v);
            } else if let Some(v) = p.top_p {
                body["top_p"] = json!(v);
            }
            if let Some(v) = p.top_k {
                body["top_k"] = json!(v);
            }
        }
        if has_fallback(&self.model) {
            body["fallbacks"] = json!("default");
        }
        body
    }

    /// Streams one answer, once: [`super::Client::stream`] tries again when Claude is overloaded
    /// or rate limited.
    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let body = self.body(req);
        let mut request = self
            .http
            .post(format!("{}/messages", self.base_url.trim_end_matches('/')))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body);
        let betas = self.betas();
        if !betas.is_empty() {
            request = request.header("anthropic-beta", betas.join(","));
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled),
            r = request.send() => r?,
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let asked = retry::asked_by_headers(response.headers());
            let text = response.text().await.unwrap_or_default();
            return Err(status_error(status, error_message(status, &text), asked));
        }

        let mut answer = Answer::new(Completion {
            request_body: body.clone(),
            ..Default::default()
        });
        let mut decoder = SseDecoder::default();
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(LlmError::Cancelled),
                c = stream.next() => c,
            };
            let (events, ended) = match chunk {
                Some(Ok(bytes)) => (decoder.push(&bytes), false),
                Some(Err(e)) => return Err(LlmError::Network(e.to_string())),
                None => (decoder.finish().into_iter().collect::<Vec<_>>(), true),
            };
            for data in events {
                let Ok(v) = serde_json::from_str::<Value>(&data) else {
                    tracing::warn!(model = %self.model, "Claude sent an event that is not JSON: {data}");
                    continue;
                };
                answer.apply(&v, &mut on_event)?;
            }
            if ended {
                break;
            }
        }
        answer.finish(&self.model)
    }
}

/// An answer as its events arrive.
struct Answer {
    out: Completion,
    /// Content blocks by their index in the stream.
    blocks: BTreeMap<u64, Value>,
    /// Tool input JSON as it streams, by block index.
    inputs: BTreeMap<u64, String>,
    /// Which tool call each `tool_use` block is.
    calls: BTreeMap<u64, usize>,
    stop_details: Option<Value>,
    /// Text has been streamed since the last thinking, so the next thinking starts a paragraph.
    thought: bool,
}

impl Answer {
    fn new(out: Completion) -> Self {
        Self {
            out,
            blocks: BTreeMap::new(),
            inputs: BTreeMap::new(),
            calls: BTreeMap::new(),
            stop_details: None,
            thought: false,
        }
    }

    fn apply(&mut self, v: &Value, on_event: &mut dyn FnMut(StreamEvent)) -> Result<(), LlmError> {
        let index = v["index"].as_u64().unwrap_or(0);
        match v["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let message = &v["message"];
                // With a fallback this names the model that took over.
                self.out.model = message["model"].as_str().map(str::to_string);
                self.read_usage(&message["usage"]);
            }
            "content_block_start" => {
                let block = v["content_block"].clone();
                match block["type"].as_str().unwrap_or_default() {
                    "tool_use" => {
                        let name = block["name"].as_str().unwrap_or_default().to_string();
                        let call = self.out.tool_calls.len();
                        on_event(StreamEvent::ToolCall {
                            index: call,
                            name: name.clone(),
                        });
                        self.out.tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or_default().to_string(),
                            name,
                            arguments: "{}".into(),
                        });
                        self.calls.insert(index, call);
                    }
                    // The model declined part way and another took over on the same stream: what
                    // was said stays, the calls the declined model began are void.
                    "fallback" => {
                        self.out.tool_calls.clear();
                        self.calls.clear();
                        let note = format!(
                            "\n\n{} declined; {} continued.\n\n",
                            block["from"]["model"].as_str().unwrap_or("The model"),
                            block["to"]["model"].as_str().unwrap_or("another model")
                        );
                        self.out.reasoning.push_str(&note);
                        on_event(StreamEvent::Reasoning(note));
                    }
                    "thinking" | "redacted_thinking" if self.thought && !self.out.reasoning.is_empty() => {
                        self.out.reasoning.push_str("\n\n");
                        on_event(StreamEvent::Reasoning("\n\n".into()));
                        self.thought = false;
                    }
                    _ => {}
                }
                self.blocks.insert(index, block);
            }
            "content_block_delta" => {
                let delta = &v["delta"];
                let Some(block) = self.blocks.get_mut(&index) else {
                    return Ok(());
                };
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        append(block, "text", text);
                        self.out.content.push_str(text);
                        self.thought = true;
                        if !text.is_empty() {
                            on_event(StreamEvent::Content(text.to_string()));
                        }
                    }
                    "thinking_delta" => {
                        let text = delta["thinking"].as_str().unwrap_or_default();
                        append(block, "thinking", text);
                        self.out.reasoning.push_str(text);
                        if !text.is_empty() {
                            on_event(StreamEvent::Reasoning(text.to_string()));
                        }
                    }
                    "signature_delta" => append(block, "signature", delta["signature"].as_str().unwrap_or_default()),
                    "input_json_delta" => self
                        .inputs
                        .entry(index)
                        .or_default()
                        .push_str(delta["partial_json"].as_str().unwrap_or_default()),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let raw = self.inputs.remove(&index).unwrap_or_default();
                if let Some(block) = self.blocks.get_mut(&index)
                    && block["type"] == "tool_use"
                {
                    let input = super::parse_arguments(&raw).unwrap_or_else(|_| json!({}));
                    block["input"] = input;
                    if let Some(call) = self.calls.get(&index).and_then(|c| self.out.tool_calls.get_mut(*c)) {
                        call.arguments = if raw.trim().is_empty() { "{}".into() } else { raw };
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = v["delta"]["stop_reason"].as_str() {
                    self.out.finish_reason = Some(reason.to_string());
                }
                if !v["delta"]["stop_details"].is_null() {
                    self.stop_details = Some(v["delta"]["stop_details"].clone());
                }
                self.read_usage(&v["usage"]);
            }
            "error" => {
                let err = &v["error"];
                let message = err["message"]
                    .as_str()
                    .unwrap_or("Claude returned an error")
                    .to_string();
                return Err(match err["type"].as_str().unwrap_or_default() {
                    "overloaded_error" | "rate_limit_error" | "api_error" => LlmError::Busy {
                        message,
                        retry_after: None,
                    },
                    _ if retry::sounds_busy(&message) => LlmError::Busy {
                        message,
                        retry_after: None,
                    },
                    _ => LlmError::Provider(message),
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Usage arrives in `message_start` and again, counted so far, in `message_delta`.
    fn read_usage(&mut self, u: &Value) {
        if !u.is_object() {
            return;
        }
        let n = |key: &str| u[key].as_u64();
        if let (Some(input), creation, read) = (
            n("input_tokens"),
            n("cache_creation_input_tokens").unwrap_or(0),
            n("cache_read_input_tokens").unwrap_or(0),
        ) {
            self.out.usage.prompt_tokens = input + creation + read;
            self.out.usage.cached_tokens = read;
        }
        if let Some(output) = n("output_tokens") {
            self.out.usage.completion_tokens = output;
        }
        if let Some(thinking) = u["output_tokens_details"]["thinking_tokens"].as_u64() {
            self.out.usage.reasoning_tokens = thinking;
        }
    }

    fn finish(mut self, model: &str) -> Result<Completion, LlmError> {
        match self.out.finish_reason.as_deref() {
            // A decline is final for this request: the partial answer is discarded.
            Some("refusal") => {
                let category = self
                    .stop_details
                    .as_ref()
                    .and_then(|d| d["category"].as_str())
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default();
                return Err(LlmError::Provider(format!(
                    "Claude declined to answer{category}. Rephrase the request or pick another model."
                )));
            }
            Some("model_context_window_exceeded") if self.out.content.is_empty() && self.out.tool_calls.is_empty() => {
                return Err(LlmError::Provider(
                    "The conversation is longer than the model's context window.".into(),
                ));
            }
            _ => {}
        }
        if self.out.content.is_empty() && self.out.tool_calls.is_empty() {
            let message = match self.out.finish_reason.as_deref() {
                None => "Claude's answer ended before it was complete. Try again.".to_string(),
                Some("end_turn") => "Claude returned an empty answer. Try again.".to_string(),
                Some(reason) => format!("Claude stopped without answering ({reason})"),
            };
            return Err(LlmError::Provider(message));
        }
        let blocks: Vec<Value> = std::mem::take(&mut self.blocks).into_values().collect();
        self.out.provider_meta = Some(json!({"anthropic": {"model": model, "content": blocks}}));
        Ok(self.out)
    }
}

fn append(block: &mut Value, key: &str, text: &str) {
    let joined = format!("{}{text}", block[key].as_str().unwrap_or_default());
    block[key] = json!(joined);
}

/// The error for a failed request: busy (rate limited or overloaded, worth another try), a model
/// this key cannot use, or anything else.
fn status_error(status: u16, message: String, asked: Option<Duration>) -> LlmError {
    match status {
        _ if retry::busy_status(status, false) => LlmError::Busy {
            message,
            retry_after: asked,
        },
        404 => LlmError::Unavailable(message),
        _ => LlmError::Provider(message),
    }
}

/// A tool's parameters as the API wants them: always an object schema.
fn input_schema(parameters: &Value) -> Value {
    match parameters {
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("object") => parameters.clone(),
        Value::Object(map) if map.is_empty() => json!({"type": "object", "properties": {}}),
        Value::Object(map) => {
            let mut map = map.clone();
            map.insert("type".into(), json!("object"));
            Value::Object(map)
        }
        _ => json!({"type": "object", "properties": {}}),
    }
}

/// Tool call ids as the API accepts them. Calls from other providers may use other characters.
fn tool_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() { "call".into() } else { clean }
}

/// Converts the neutral history into Messages API `messages`: roles alternate, tool results go
/// in the user turn after the calls, and Claude's own turns are replayed block for block.
fn messages(history: &[LlmMessage], model: &str) -> Vec<Value> {
    let mut out: Vec<(&'static str, Vec<Value>)> = Vec::new();
    let push = |role: &'static str, blocks: Vec<Value>, out: &mut Vec<(&'static str, Vec<Value>)>| {
        if blocks.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, existing)) if *last == role => existing.extend(blocks),
            _ => out.push((role, blocks)),
        }
    };
    for m in history {
        match m {
            LlmMessage::User { content, media } => {
                let mut blocks: Vec<Value> = media
                    .iter()
                    .filter(|m| IMAGE_TYPES.contains(&m.mime.as_str()))
                    .map(|m| json!({"type": "image", "source": {"type": "base64", "media_type": m.mime, "data": m.data}}))
                    .collect();
                if !content.trim().is_empty() {
                    blocks.push(json!({"type": "text", "text": content}));
                } else if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": "(empty message)"}));
                }
                push("user", blocks, &mut out);
            }
            LlmMessage::Assistant {
                content,
                tool_calls,
                provider_meta,
                ..
            } => {
                let blocks = provider_meta
                    .as_ref()
                    .and_then(|m| m.get("anthropic"))
                    .and_then(|m| replay(m, model))
                    .unwrap_or_else(|| {
                        let mut blocks = Vec::new();
                        if !content.trim().is_empty() {
                            blocks.push(json!({"type": "text", "text": content}));
                        }
                        for call in tool_calls {
                            let input = super::parse_arguments(&call.arguments).unwrap_or_else(|_| json!({}));
                            let input = if input.is_object() { input } else { json!({}) };
                            blocks.push(
                                json!({"type": "tool_use", "id": tool_id(&call.id), "name": call.name, "input": input}),
                            );
                        }
                        blocks
                    });
                push("assistant", blocks, &mut out);
            }
            LlmMessage::Tool { call_id, content, .. } => {
                let text = if content.is_empty() {
                    "(no output)"
                } else {
                    content.as_str()
                };
                let block = json!({"type": "tool_result", "tool_use_id": tool_id(call_id), "content": text});
                // Results go first in their user turn, before anything the person adds.
                match out.last_mut() {
                    Some(("user", existing)) => {
                        let at = existing.iter().take_while(|b| b["type"] == "tool_result").count();
                        existing.insert(at, block);
                    }
                    _ => out.push(("user", vec![block])),
                }
            }
        }
    }
    if out.first().is_some_and(|(role, _)| *role == "assistant") {
        out.insert(
            0,
            (
                "user",
                vec![json!({"type": "text", "text": "(conversation continues)"})],
            ),
        );
    }
    out.into_iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect()
}

/// Claude's own blocks for a turn it answered: thinking (only for the model that wrote it, since
/// thinking is bound to its model), text and tool calls. After a fallback the declined part keeps
/// only its text. `None` when nothing usable is left.
fn replay(meta: &Value, model: &str) -> Option<Vec<Value>> {
    let blocks = meta["content"].as_array()?;
    let same_model = meta["model"].as_str() == Some(model);
    let boundary = blocks.iter().rposition(|b| b["type"] == "fallback");
    let kept: Vec<Value> = blocks
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let before_fallback = boundary.is_some_and(|f| i < f);
            match b["type"].as_str()? {
                "text" if b["text"].as_str().is_some_and(|t| !t.trim().is_empty()) => {
                    Some(json!({"type": "text", "text": b["text"]}))
                }
                "thinking"
                    if same_model && !before_fallback && b["signature"].as_str().is_some_and(|s| !s.is_empty()) =>
                {
                    Some(json!({"type": "thinking", "thinking": b["thinking"], "signature": b["signature"]}))
                }
                "redacted_thinking" if same_model && !before_fallback => {
                    Some(json!({"type": "redacted_thinking", "data": b["data"]}))
                }
                "tool_use" if !before_fallback => {
                    let input = if b["input"].is_object() {
                        b["input"].clone()
                    } else {
                        json!({})
                    };
                    Some(json!({"type": "tool_use", "id": b["id"], "name": b["name"], "input": input}))
                }
                _ => None,
            }
        })
        .collect();
    let has_answer = kept.iter().any(|b| b["type"] == "text" || b["type"] == "tool_use");
    has_answer.then_some(kept)
}

/// Lists the models an API key can use. Also serves as the connection test.
pub async fn list_models(http: &reqwest::Client, base_url: &str, api_key: &str) -> Result<Vec<CloudModel>, LlmError> {
    let mut models = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut req = http
            .get(format!("{}/models", base_url.trim_end_matches('/')))
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .query(&[("limit", "1000")]);
        if let Some(id) = &after {
            req = req.query(&[("after_id", id.as_str())]);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        if !(200..300).contains(&status) {
            return Err(LlmError::Provider(error_message(status, &text)));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| LlmError::Provider(e.to_string()))?;
        models.extend(v["data"].as_array().into_iter().flatten().filter_map(parse_model));
        after = v["last_id"]
            .as_str()
            .filter(|_| v["has_more"].as_bool() == Some(true))
            .map(str::to_string);
        if after.is_none() {
            break;
        }
    }
    Ok(models)
}

fn parse_model(m: &Value) -> Option<CloudModel> {
    let id = m["id"].as_str()?;
    let caps = &m["capabilities"];
    let supported = |path: &str| caps.pointer(path).and_then(Value::as_bool).unwrap_or(false);
    let thinking = supported("/thinking/types/adaptive/supported");
    Some(CloudModel {
        id: id.to_string(),
        display_name: m["display_name"].as_str().unwrap_or(id).to_string(),
        description: String::new(),
        input_token_limit: m["max_input_tokens"].as_u64().unwrap_or(0),
        output_token_limit: m["max_tokens"].as_u64().unwrap_or(0),
        thinking,
        always_thinks: false,
        capabilities: Some(Capabilities {
            vision: Some(supported("/image_input/supported")),
            audio: Some(false),
            tools: Some(true),
            thinking: Some(thinking),
        }),
        free: false,
        effort: supported("/effort/supported"),
        efforts: ["low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|level| supported(&format!("/effort/{level}/supported")))
            .map(String::from)
            .collect(),
        default_effort: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{GenParams, Media, ToolSpec};

    fn model(id: &str, thinking: bool) -> CloudModel {
        CloudModel {
            id: id.into(),
            display_name: id.into(),
            description: String::new(),
            input_token_limit: 1_000_000,
            output_token_limit: 128_000,
            thinking,
            always_thinks: false,
            capabilities: None,
            free: false,
            effort: thinking,
            efforts: if thinking {
                ["low", "medium", "high", "max"].map(String::from).to_vec()
            } else {
                Vec::new()
            },
            default_effort: None,
        }
    }

    fn client(id: &str, thinking: bool, base: String) -> AnthropicClient {
        AnthropicClient::new(
            reqwest::Client::new(),
            base,
            "key".into(),
            id.into(),
            Some(&model(id, thinking)),
        )
    }

    fn request(thinking: Option<bool>) -> ChatRequest {
        ChatRequest {
            system: "sys".into(),
            messages: vec![LlmMessage::User {
                content: "hi".into(),
                media: vec![Media::new("image/png", &[1, 2, 3]), Media::new("audio/wav", &[4])],
            }],
            tools: vec![ToolSpec {
                name: "get_time".into(),
                description: "Now.".into(),
                parameters: json!({}),
            }],
            params: GenParams {
                thinking,
                temperature: Some(0.5),
                ..Default::default()
            },
        }
    }

    #[test]
    fn newest_models_think_adaptively_and_fall_back() {
        let c = client("claude-opus-5-5", true, String::new());
        let b = c.body(&request(Some(false)));
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["thinking"]["display"], "summarized");
        assert_eq!(b["thinking"]["block_binding"]["prefix_mismatch_behavior"], "drop_block");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["max_tokens"], 64_000);
        assert!(b.get("temperature").is_none());
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
        // The image goes, the sound does not.
        let content = b["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        assert_eq!(c.betas(), vec![BINDING_BETA, FALLBACK_BETA]);

        let haiku = client("claude-haiku-5-5", true, String::new());
        assert!(haiku.body(&request(None)).get("fallbacks").is_none());
        assert!(haiku.body(&request(None)).get("output_config").is_none());
        let mut req = request(None);
        req.params.effort = Some("max".into());
        assert_eq!(haiku.body(&req)["output_config"]["effort"], "max");
        assert_eq!(haiku.betas(), vec![BINDING_BETA]);

        let old = client("claude-3-5-haiku-20241022", false, String::new());
        let b = old.body(&request(None));
        assert!(b.get("thinking").is_none());
        assert_eq!(b["temperature"], 0.5);
        assert!(old.betas().is_empty());
    }

    #[test]
    fn history_alternates_with_results_first() {
        let history = vec![
            LlmMessage::User {
                content: "hi".into(),
                media: vec![],
            },
            LlmMessage::Assistant {
                content: "Let me check.".into(),
                reasoning: None,
                tool_calls: vec![
                    ToolCall {
                        id: "gemini_call:1".into(),
                        name: "one".into(),
                        arguments: "{\"x\":1}".into(),
                    },
                    ToolCall {
                        id: "b".into(),
                        name: "two".into(),
                        arguments: "".into(),
                    },
                ],
                provider_meta: None,
            },
            LlmMessage::Tool {
                call_id: "gemini_call:1".into(),
                name: "one".into(),
                content: "ok".into(),
            },
            LlmMessage::Tool {
                call_id: "b".into(),
                name: "two".into(),
                content: String::new(),
            },
            LlmMessage::User {
                content: "and now?".into(),
                media: vec![],
            },
        ];
        let m = messages(&history, "claude-opus-5-5");
        assert_eq!(m.len(), 3);
        assert_eq!(m[1]["content"][1]["id"], "gemini_call_1");
        assert_eq!(m[1]["content"][1]["input"]["x"], 1);
        assert_eq!(m[1]["content"][2]["input"], json!({}));
        let user = m[2]["content"].as_array().unwrap();
        assert_eq!(user[0]["tool_use_id"], "gemini_call_1");
        assert_eq!(user[1]["content"], "(no output)");
        assert_eq!(user[2]["text"], "and now?");
    }

    #[test]
    fn thinking_is_replayed_only_to_the_model_that_wrote_it() {
        let meta = json!({"anthropic": {"model": "claude-opus-5-5", "content": [
            {"type": "thinking", "thinking": "hmm", "signature": "sig"},
            {"type": "text", "text": "Hello"},
            {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}
        ]}});
        let turn = |model| {
            messages(
                &[LlmMessage::Assistant {
                    content: "Hello".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    provider_meta: Some(meta.clone()),
                }],
                model,
            )
        };
        let same = turn("claude-opus-5-5");
        assert_eq!(same[0]["role"], "user");
        assert_eq!(same[1]["content"][0]["signature"], "sig");
        let other = turn("claude-sonnet-5-5");
        assert_eq!(other[1]["content"].as_array().unwrap().len(), 2);
        assert_eq!(other[1]["content"][0]["type"], "text");
    }

    #[test]
    fn a_fallback_keeps_only_the_declined_text() {
        let meta = json!({"model": "claude-opus-5-5", "content": [
            {"type": "thinking", "thinking": "x", "signature": "s"},
            {"type": "text", "text": "Part one"},
            {"type": "tool_use", "id": "t0", "name": "f", "input": {}},
            {"type": "fallback", "from": {"model": "claude-opus-5-5"}, "to": {"model": "claude-opus-5"}},
            {"type": "text", "text": " and two"}
        ]});
        let kept = replay(&meta, "claude-opus-5-5").unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|b| b["type"] == "text"));
    }

    #[test]
    fn lists_models_with_what_they_can_do() {
        let m = parse_model(&json!({
            "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "max_input_tokens": 1000000,
            "max_tokens": 128000, "capabilities": {"image_input": {"supported": true},
            "thinking": {"supported": true, "types": {"adaptive": {"supported": true}}},
            "effort": {"supported": true}}
        }))
        .unwrap();
        assert!(m.thinking && m.effort);
        assert!(m.efforts.is_empty());
        assert_eq!(m.capabilities.unwrap().vision, Some(true));
        assert_eq!(m.output_token_limit, 128_000);
        let levels = parse_model(&json!({
            "id": "claude-opus-5-5", "capabilities": {"effort": {"supported": true,
            "low": {"supported": true}, "high": {"supported": true}, "xhigh": {"supported": false},
            "max": {"supported": true}}}
        }))
        .unwrap();
        assert_eq!(levels.efforts, ["low", "high", "max"]);
    }

    async fn sse_server(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
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
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        base
    }

    const STREAM: &str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":90,\"output_tokens\":1}}}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Plan.\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_time\",\"input\":{}}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"tz\\\":\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"UTC\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":42}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    #[tokio::test]
    async fn streams_thinking_text_and_tool_calls() {
        let base = sse_server(STREAM).await;
        let c = client("claude-opus-5-5", true, base);
        let mut events = Vec::new();
        let done = c
            .stream(&request(None), &CancellationToken::new(), |e| events.push(e))
            .await
            .unwrap();
        assert_eq!(done.reasoning, "Plan.");
        assert_eq!(done.content, "Checking");
        assert_eq!(done.tool_calls[0].arguments, "{\"tz\":\"UTC\"}");
        assert_eq!(done.usage.prompt_tokens, 100);
        assert_eq!(done.usage.cached_tokens, 90);
        assert_eq!(done.usage.completion_tokens, 42);
        assert_eq!(done.finish_reason.as_deref(), Some("tool_use"));
        let meta = &done.provider_meta.unwrap()["anthropic"];
        assert_eq!(meta["content"][0]["signature"], "sig");
        assert_eq!(meta["content"][2]["input"]["tz"], "UTC");
        assert!(events.contains(&StreamEvent::ToolCall {
            index: 0,
            name: "get_time".into()
        }));
    }

    #[tokio::test]
    async fn a_refusal_is_an_error_naming_its_category() {
        let base = sse_server(concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-fable-5-1\",\"usage\":{\"input_tokens\":5}}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":{\"category\":\"cyber\"}},\"usage\":{\"output_tokens\":0}}\n\n",
        ))
        .await;
        let c = client("claude-fable-5-1", true, base);
        let err = c
            .stream(&request(None), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(err.to_string().contains("declined to answer (cyber)"), "{err}");
    }

    #[tokio::test]
    async fn an_overload_mid_stream_is_busy() {
        let base = sse_server(
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        )
        .await;
        let c = client("claude-opus-5-5", true, base);
        let err = c
            .stream(&request(None), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::Busy { .. }));
    }
}
