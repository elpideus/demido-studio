//! OpenAI-compatible chat completions, as served by llama.cpp's `llama-server` and by OpenRouter.

use std::collections::BTreeMap;

use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::sse::SseDecoder;
use super::{ChatRequest, Completion, LlmError, LlmMessage, Media, StreamEvent, Usage, openrouter, retry};
use crate::db::ToolCall;

/// The server on the other end: they differ in a few fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// llama.cpp's `llama-server`, running a local model.
    LlamaCpp,
    /// OpenRouter. `always_thinks` for a model that refuses to have thinking turned off.
    OpenRouter { always_thinks: bool },
}

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    /// e.g. `http://127.0.0.1:52100/v1`
    pub base_url: String,
    /// Sent as the `model` field (llama-server serves one model and ignores it).
    pub model: String,
    pub api_key: Option<String>,
    pub dialect: Dialect,
}

impl OpenAiClient {
    pub fn new(http: reqwest::Client, base_url: String, model: String) -> Self {
        Self {
            http,
            base_url,
            model,
            api_key: None,
            dialect: Dialect::LlamaCpp,
        }
    }

    pub fn openrouter(
        http: reqwest::Client,
        base_url: String,
        api_key: String,
        model: String,
        always_thinks: bool,
    ) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            api_key: Some(api_key),
            dialect: Dialect::OpenRouter { always_thinks },
        }
    }

    pub fn body(&self, req: &ChatRequest) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            messages.push(json!({"role": "system", "content": req.system}));
        }
        for m in &req.messages {
            messages.push(match m {
                LlmMessage::User { content, media } if media.is_empty() => {
                    json!({"role": "user", "content": content})
                }
                // Media first, then the text, as the chat templates of vision models expect.
                LlmMessage::User { content, media } => {
                    let mut parts: Vec<Value> = media.iter().map(media_part).collect();
                    if !content.is_empty() {
                        parts.push(json!({"type": "text", "text": content}));
                    }
                    json!({"role": "user", "content": parts})
                }
                LlmMessage::Assistant {
                    content,
                    reasoning,
                    tool_calls,
                    provider_meta,
                } => {
                    let mut msg = json!({"role": "assistant", "content": content});
                    match self.dialect {
                        Dialect::LlamaCpp => {
                            if let Some(r) = reasoning.as_ref().filter(|r| !r.is_empty()) {
                                msg["reasoning_content"] = json!(r);
                            }
                        }
                        // Thinking models that call tools want their reasoning back exactly as
                        // they gave it; another model's would not match.
                        Dialect::OpenRouter { .. } => {
                            if let Some(details) = provider_meta
                                .as_ref()
                                .and_then(|m| m.get("openrouter"))
                                .filter(|m| m["model"].as_str() == Some(self.model.as_str()))
                                .and_then(|m| m.get("reasoningDetails"))
                            {
                                msg["reasoning_details"] = details.clone();
                            }
                        }
                    }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = Value::Array(
                            tool_calls
                                .iter()
                                .map(|c| {
                                    json!({
                                        "id": c.id,
                                        "type": "function",
                                        "function": {"name": c.name, "arguments": c.arguments}
                                    })
                                })
                                .collect(),
                        );
                    }
                    msg
                }
                LlmMessage::Tool { call_id, content, .. } => {
                    json!({"role": "tool", "tool_call_id": call_id, "content": content})
                }
            });
        }

        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !req.tools.is_empty() {
            body["tools"] = Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.parameters,
                            }
                        })
                    })
                    .collect(),
            );
        }
        let p = &req.params;
        let set = |body: &mut Value, key: &str, v: Option<Value>| {
            if let Some(v) = v {
                body[key] = v;
            }
        };
        set(&mut body, "temperature", p.temperature.map(|v| json!(v)));
        set(&mut body, "top_p", p.top_p.map(|v| json!(v)));
        set(&mut body, "top_k", p.top_k.map(|v| json!(v)));
        set(&mut body, "min_p", p.min_p.map(|v| json!(v)));
        set(&mut body, "max_tokens", p.max_tokens.map(|v| json!(v)));
        set(&mut body, "seed", p.seed.map(|v| json!(v)));
        match self.dialect {
            Dialect::LlamaCpp => {
                body["cache_prompt"] = json!(true);
                set(&mut body, "repeat_penalty", p.repeat_penalty.map(|v| json!(v)));
                if let Some(thinking) = p.thinking {
                    body["chat_template_kwargs"] = json!({"enable_thinking": thinking});
                }
            }
            Dialect::OpenRouter { always_thinks } => {
                set(&mut body, "repetition_penalty", p.repeat_penalty.map(|v| json!(v)));
                match p.thinking {
                    Some(true) => body["reasoning"] = json!({"enabled": true}),
                    Some(false) if !always_thinks => body["reasoning"] = json!({"effort": "none"}),
                    _ => {}
                }
            }
        }
        body
    }

    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let body = self.body(req);
        let mut request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled),
            r = request.send() => r?,
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(failure(status.as_u16(), &text));
        }

        let mut out = Completion {
            request_body: body,
            ..Default::default()
        };
        let mut calls: BTreeMap<usize, ToolCall> = BTreeMap::new();
        let mut details: Vec<Value> = Vec::new();
        let mut decoder = SseDecoder::default();
        let mut stream = response.bytes_stream();
        'read: loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    finish_calls(&mut out, calls);
                    return Err(LlmError::Cancelled);
                }
                c = stream.next() => c,
            };
            let (events, ended) = match chunk {
                Some(Ok(bytes)) => (decoder.push(&bytes), false),
                Some(Err(e)) => return Err(LlmError::Network(e.to_string())),
                None => (decoder.finish().into_iter().collect::<Vec<_>>(), true),
            };
            for data in events {
                if data.trim() == "[DONE]" {
                    break 'read;
                }
                let Ok(v) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                if let Some(err) = v.get("error") {
                    return Err(LlmError::Provider(
                        err.get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("the model returned an error")
                            .to_string(),
                    ));
                }
                apply_chunk(&v, &mut out, &mut calls, &mut details, &mut on_event);
            }
            if ended {
                break;
            }
        }
        finish_calls(&mut out, calls);
        // A router may answer with another model next time, so its reasoning is not replayed.
        let details = merge_details(details);
        if matches!(self.dialect, Dialect::OpenRouter { .. })
            && !details.is_empty()
            && !openrouter::is_router(&self.model)
        {
            out.provider_meta = Some(json!({"openrouter": {"model": self.model, "reasoningDetails": details}}));
        }
        Ok(out)
    }
}

/// An image as a data URL, sound as `input_audio` (llama.cpp reads WAV and MP3).
fn media_part(m: &Media) -> Value {
    if m.is_image() {
        json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", m.mime, m.data)}})
    } else {
        let format = if m.mime.contains("mpeg") || m.mime.contains("mp3") {
            "mp3"
        } else {
            "wav"
        };
        json!({"type": "input_audio", "input_audio": {"data": m.data, "format": format}})
    }
}

fn apply_chunk(
    v: &Value,
    out: &mut Completion,
    calls: &mut BTreeMap<usize, ToolCall>,
    details: &mut Vec<Value>,
    on_event: &mut impl FnMut(StreamEvent),
) {
    if out.model.is_none() {
        out.model = v.get("model").and_then(Value::as_str).map(str::to_string);
    }
    if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
        let delta = &choice["delta"];
        // llama.cpp says `reasoning_content`; OpenRouter says `reasoning`, along with the
        // `reasoning_details` it wants back (see `merge_details`).
        let pieces = delta.get("reasoning_details").and_then(Value::as_array);
        details.extend(pieces.into_iter().flatten().cloned());
        let said = ["reasoning_content", "reasoning"]
            .iter()
            .find_map(|k| delta.get(*k).and_then(Value::as_str));
        let reasoning: String = match said {
            Some(r) => r.to_string(),
            None => pieces
                .into_iter()
                .flatten()
                .filter_map(|d| d.get("text").or_else(|| d.get("summary")).and_then(Value::as_str))
                .collect(),
        };
        if !reasoning.is_empty() {
            out.reasoning.push_str(&reasoning);
            on_event(StreamEvent::Reasoning(reasoning));
        }
        if let Some(c) = delta.get("content").and_then(Value::as_str)
            && !c.is_empty()
        {
            out.content.push_str(c);
            on_event(StreamEvent::Content(c.to_string()));
        }
        if let Some(list) = delta.get("tool_calls").and_then(Value::as_array) {
            for tc in list {
                let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let entry = calls.entry(index).or_insert_with(|| ToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
                if let Some(id) = tc.get("id").and_then(Value::as_str)
                    && !id.is_empty()
                {
                    entry.id = id.to_string();
                }
                if let Some(f) = tc.get("function") {
                    if let Some(name) = f.get("name").and_then(Value::as_str)
                        && !name.is_empty()
                        && entry.name.is_empty()
                    {
                        entry.name = name.to_string();
                        on_event(StreamEvent::ToolCall {
                            index,
                            name: name.to_string(),
                        });
                    }
                    if let Some(args) = f.get("arguments").and_then(Value::as_str) {
                        entry.arguments.push_str(args);
                    }
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            out.finish_reason = Some(reason.to_string());
        }
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        out.usage = Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            cached_tokens: u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
            reasoning_tokens: u["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0),
        };
    }
    if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
        out.timings = Some(t.clone());
    }
}

fn finish_calls(out: &mut Completion, calls: BTreeMap<usize, ToolCall>) {
    out.tool_calls = calls
        .into_values()
        .enumerate()
        .filter(|(_, c)| !c.name.is_empty())
        .map(|(i, mut c)| {
            if c.id.is_empty() {
                c.id = format!("call_{}_{}", i, crate::db::new_id().replace('-', ""));
            }
            c
        })
        .collect();
}

/// OpenRouter's streamed reasoning details as the list it takes back: the pieces of one entry
/// (the same `type` and `index`) joined in order, text to text, up to the signature that ends it.
/// Encrypted reasoning without an index comes whole.
fn merge_details(pieces: Vec<Value>) -> Vec<Value> {
    fn field(m: &Map<String, Value>, key: &str) -> Option<Value> {
        m.get(key).filter(|v| !v.is_null()).cloned()
    }
    let mut merged: Vec<Value> = Vec::new();
    for piece in pieces {
        let Value::Object(piece) = piece else { continue };
        let joins = |last: &Map<String, Value>| {
            let (id, last_id) = (field(&piece, "id"), field(last, "id"));
            last.get("type") == piece.get("type")
                && field(last, "index") == field(&piece, "index")
                && (field(&piece, "index").is_some()
                    || piece.get("type").and_then(Value::as_str) != Some("reasoning.encrypted"))
                && field(last, "signature").is_none()
                && (id.is_none() || last_id.is_none() || id == last_id)
        };
        match merged
            .last_mut()
            .and_then(Value::as_object_mut)
            .filter(|last| joins(last))
        {
            Some(last) => {
                for (key, value) in piece {
                    let text = matches!(key.as_str(), "text" | "summary" | "data");
                    match (last.get_mut(&key), &value) {
                        (Some(Value::String(acc)), Value::String(more)) if text => acc.push_str(more),
                        (_, Value::Null) => {}
                        _ => {
                            last.insert(key, value);
                        }
                    }
                }
            }
            None => merged.push(Value::Object(piece)),
        }
    }
    merged
}

/// The error for a request the server refused, telling one llama-server refused because it does
/// not fit in the context apart.
fn failure(status: u16, body: &str) -> LlmError {
    let message = error_message(status, body);
    let error = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").cloned());
    let count = |key: &str| error.as_ref()?.get(key)?.as_u64().map(|n| n as usize);
    match (
        error.as_ref().and_then(|e| e.get("type")).and_then(Value::as_str),
        count("n_prompt_tokens"),
        count("n_ctx"),
    ) {
        (Some("exceed_context_size_error"), Some(prompt_tokens), Some(context_tokens)) => LlmError::ContextFull {
            message,
            prompt_tokens,
            context_tokens,
        },
        _ => LlmError::Provider(message),
    }
}

pub fn error_message(status: u16, body: &str) -> String {
    let parsed: Option<String> = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.get("error")
            .and_then(describe_error)
            .or_else(|| v.get("message").and_then(Value::as_str).map(str::to_string))
    });
    match parsed {
        Some(m) => m,
        None if body.trim().is_empty() => format!("the model server answered {status}"),
        None => format!("the model server answered {status}: {}", body.trim()),
    }
}

/// Longest provider's reason passed on; Google lists every field it rejects.
const MAX_REASON_CHARS: usize = 600;

/// What an `error` object says went wrong. OpenRouter reports every refusal by the provider it
/// sent the request to as "Provider returned error", with the provider's own words (often JSON
/// themselves) in `metadata.raw`, and a moderation refusal's reasons in `metadata.reasons`.
fn describe_error(error: &Value) -> Option<String> {
    fn said(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
            Value::Array(items) => items.first().and_then(said),
            Value::Object(o) => ["error", "message"].iter().find_map(|k| o.get(*k)).and_then(said),
            _ => None,
        }
    }
    let message = error.get("message").and_then(Value::as_str)?;
    let meta = &error["metadata"];
    let reason = match &meta["raw"] {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|v| said(&v))
            .or_else(|| said(&meta["raw"])),
        Value::Null => meta["reasons"].as_array().map(|reasons| {
            let reasons: Vec<&str> = reasons.iter().filter_map(Value::as_str).collect();
            reasons.join(", ")
        }),
        raw => said(raw),
    }
    .filter(|r| !r.is_empty())
    .map(|r| match r.char_indices().nth(MAX_REASON_CHARS) {
        Some((cut, _)) => format!("{}…", &r[..cut]),
        None => r,
    });
    let provider = meta["provider_name"].as_str();
    Some(match (reason, provider) {
        (Some(reason), Some(provider)) => format!("{provider}: {reason}"),
        (Some(reason), None) => format!("{message}: {reason}"),
        (None, Some(provider)) => format!("{message} ({provider})"),
        (None, None) => message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{GenParams, ToolSpec};

    #[test]
    fn body_carries_tools_params_and_history() {
        let client = OpenAiClient::new(reqwest::Client::new(), "http://x/v1".into(), "m".into());
        let req = ChatRequest {
            system: "sys".into(),
            messages: vec![
                LlmMessage::User {
                    content: "hi".into(),
                    media: vec![Media::new("image/png", &[1, 2, 3])],
                },
                LlmMessage::Assistant {
                    content: String::new(),
                    reasoning: Some("thinking".into()),
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        name: "t".into(),
                        arguments: "{}".into(),
                    }],
                    provider_meta: None,
                },
                LlmMessage::Tool {
                    call_id: "c1".into(),
                    name: "t".into(),
                    content: "ok".into(),
                },
            ],
            tools: vec![ToolSpec {
                name: "t".into(),
                description: "d".into(),
                parameters: json!({"type": "object"}),
            }],
            params: GenParams {
                temperature: Some(0.5),
                top_k: Some(20),
                thinking: Some(false),
                ..Default::default()
            },
        };
        let body = client.body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(
            body["messages"][1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
        assert_eq!(body["messages"][1]["content"][1]["text"], "hi");
        assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "c1");
        assert_eq!(body["messages"][3]["tool_call_id"], "c1");
        assert_eq!(body["tools"][0]["function"]["name"], "t");
        assert_eq!(body["top_k"], 20);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn assembles_streamed_tool_calls() {
        let mut out = Completion::default();
        let mut calls = BTreeMap::new();
        let mut details = Vec::new();
        let mut events = Vec::new();
        for chunk in [
            json!({"choices":[{"delta":{"reasoning_content":"hmm"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"abc","type":"function","function":{"name":"market_quote","arguments":"{"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"symbols\":[\"FX:EURUSD\"]}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":4}},"timings":{"predicted_per_second":50.0}}),
        ] {
            apply_chunk(&chunk, &mut out, &mut calls, &mut details, &mut |e| events.push(e));
        }
        finish_calls(&mut out, calls);
        assert_eq!(out.reasoning, "hmm");
        assert_eq!(out.tool_calls.len(), 1);
        assert_eq!(out.tool_calls[0].id, "abc");
        assert_eq!(out.tool_calls[0].arguments, "{\"symbols\":[\"FX:EURUSD\"]}");
        assert_eq!(out.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(out.usage.cached_tokens, 4);
        assert!(out.timings.is_some());
        assert!(events.contains(&StreamEvent::ToolCall {
            index: 0,
            name: "market_quote".into()
        }));
    }

    fn history_with_reasoning(model: &str) -> ChatRequest {
        ChatRequest {
            system: String::new(),
            messages: vec![LlmMessage::Assistant {
                content: String::new(),
                reasoning: Some("thinking".into()),
                tool_calls: vec![],
                provider_meta: Some(json!({"openrouter": {
                    "model": model,
                    "reasoningDetails": [{"type": "reasoning.text", "text": "thinking", "signature": "sig"}]
                }})),
            }],
            tools: vec![],
            params: GenParams {
                repeat_penalty: Some(1.5),
                thinking: Some(false),
                ..Default::default()
            },
        }
    }

    #[test]
    fn openrouter_gets_its_own_fields() {
        let client = |always_thinks| {
            OpenAiClient::openrouter(
                reqwest::Client::new(),
                "https://openrouter.ai/api/v1/".into(),
                "key".into(),
                "anthropic/claude-x".into(),
                always_thinks,
            )
        };
        let body = client(false).body(&history_with_reasoning("anthropic/claude-x"));
        assert_eq!(client(false).base_url, "https://openrouter.ai/api/v1");
        assert!(body.get("cache_prompt").is_none() && body.get("chat_template_kwargs").is_none());
        assert_eq!(body["repetition_penalty"], 1.5);
        assert_eq!(body["reasoning"]["effort"], "none");
        assert_eq!(body["messages"][0]["reasoning_details"][0]["signature"], "sig");
        assert!(body["messages"][0].get("reasoning_content").is_none());
        // A model that always thinks refuses to be told not to.
        assert!(
            client(true)
                .body(&history_with_reasoning("x"))
                .get("reasoning")
                .is_none()
        );
        // Another model's reasoning is not sent back.
        let other = client(false).body(&history_with_reasoning("google/gemini-x"));
        assert!(other["messages"][0].get("reasoning_details").is_none());
    }

    #[test]
    fn openrouter_reasoning_streams_and_is_kept_whole() {
        let mut out = Completion::default();
        let mut calls = BTreeMap::new();
        let mut details = Vec::new();
        let mut events = Vec::new();
        for chunk in [
            json!({"choices":[{"delta":{"reasoning":"Let me ","reasoning_details":[{"type":"reasoning.text","text":"Let me ","format":"anthropic-claude-v1","index":0}]}}]}),
            json!({"choices":[{"delta":{"reasoning":"look.","reasoning_details":[{"type":"reasoning.text","text":"look.","format":"anthropic-claude-v1","index":0}]}}]}),
            json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.text","text":"","signature":"abc","format":"anthropic-claude-v1","index":0}]}}]}),
            json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"e1","id":"call_1","format":"google-gemini-v1"}]}}]}),
            json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.encrypted","data":"e2","id":"call_2","format":"google-gemini-v1"}]}}]}),
            json!({"choices":[{"delta":{"content":"Done."}}]}),
        ] {
            apply_chunk(&chunk, &mut out, &mut calls, &mut details, &mut |e| events.push(e));
        }
        assert_eq!(out.reasoning, "Let me look.");
        assert_eq!(out.content, "Done.");
        let merged = merge_details(details);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0]["text"], "Let me look.");
        assert_eq!(merged[0]["signature"], "abc");
        assert_eq!(merged[1]["data"], "e1");
        assert_eq!(merged[2]["id"], "call_2");
    }

    #[test]
    fn reasoning_text_comes_from_details_when_alone() {
        let mut out = Completion::default();
        let chunk =
            json!({"choices":[{"delta":{"reasoning_details":[{"type":"reasoning.summary","summary":"Plan."}]}}]});
        apply_chunk(&chunk, &mut out, &mut BTreeMap::new(), &mut Vec::new(), &mut |_| {});
        assert_eq!(out.reasoning, "Plan.");
    }

    #[test]
    fn provider_errors_are_readable() {
        assert_eq!(
            error_message(400, r#"{"error":{"message":"context too long"}}"#),
            "context too long"
        );
        assert_eq!(error_message(503, ""), "the model server answered 503");
    }

    #[test]
    fn openrouter_errors_say_what_the_provider_said() {
        // The provider's answer, JSON inside a string, as OpenRouter passes it on.
        let raw = r#"{"error":{"code":400,"message":"Function calling is not enabled for this model.","status":"INVALID_ARGUMENT"}}"#;
        let body = json!({"error": {"code": 400, "message": "Provider returned error",
            "metadata": {"raw": raw, "provider_name": "Google AI Studio"}}});
        assert_eq!(
            error_message(400, &body.to_string()),
            "Google AI Studio: Function calling is not enabled for this model."
        );
        // Plain text, and the same error arriving inside the stream.
        let busy = json!({"message": "Provider returned error", "code": 429,
            "metadata": {"raw": "m:free is temporarily rate-limited upstream.", "provider_name": "Venice"}});
        assert_eq!(
            describe_error(&busy).as_deref(),
            Some("Venice: m:free is temporarily rate-limited upstream.")
        );
        let flagged = json!({"message": "Input was flagged", "metadata": {"reasons": ["violence", "hate"]}});
        assert_eq!(
            describe_error(&flagged).as_deref(),
            Some("Input was flagged: violence, hate")
        );
        let bare = json!({"message": "Provider returned error", "metadata": {"provider_name": "X"}});
        assert_eq!(describe_error(&bare).as_deref(), Some("Provider returned error (X)"));
    }

    #[test]
    fn a_full_context_is_told_apart() {
        // As llama-server b11146 answers.
        let body = r#"{"error":{"code":400,"message":"request (18188 tokens) exceeds the available context size (16384 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":18188,"n_ctx":16384}}"#;
        assert!(matches!(
            failure(400, body),
            LlmError::ContextFull { prompt_tokens: 18188, context_tokens: 16384, message }
                if message.starts_with("request (18188 tokens)")
        ));
        assert!(matches!(
            failure(400, r#"{"error":{"message":"context too long"}}"#),
            LlmError::Provider(_)
        ));
    }
}
