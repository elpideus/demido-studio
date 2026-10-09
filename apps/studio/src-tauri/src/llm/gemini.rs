//! Google Gemini through its native `generateContent` API.
//!
//! The native API (rather than Google's OpenAI-compatible layer) is used because function
//! calling on thinking models depends on replaying each model turn's parts together with their
//! thought signatures. Those parts are kept in the message's `provider_meta`.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::openai::error_message;
use super::sse::SseDecoder;
use super::{ChatRequest, CloudModel, Completion, LlmError, LlmMessage, StreamEvent, Usage};
use crate::db::ToolCall;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Google's documented placeholder for function calls that did not come from Gemini itself.
const FOREIGN_SIGNATURE: &str = "skip_thought_signature_validator";

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
    /// Model id without the `models/` prefix, e.g. `gemini-2.5-flash`.
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModel {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_token_limit: u64,
    pub output_token_limit: u64,
    pub thinking: bool,
}

impl GeminiClient {
    pub fn new(http: reqwest::Client, base_url: String, api_key: String, model: String) -> Self {
        Self {
            http,
            base_url,
            api_key,
            model,
        }
    }

    pub fn body(&self, req: &ChatRequest) -> Value {
        let mut body = json!({ "contents": contents(&req.messages) });
        if !req.system.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": req.system}]});
        }
        if !req.tools.is_empty() {
            let decls: Vec<Value> = req
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": sanitize_schema(&t.parameters),
                    })
                })
                .collect();
            body["tools"] = json!([{ "functionDeclarations": decls }]);
        }
        let p = &req.params;
        let mut config = Map::new();
        if let Some(v) = p.temperature {
            config.insert("temperature".into(), json!(v));
        }
        if let Some(v) = p.top_p {
            config.insert("topP".into(), json!(v));
        }
        if let Some(v) = p.top_k {
            config.insert("topK".into(), json!(v));
        }
        if let Some(v) = p.max_tokens {
            config.insert("maxOutputTokens".into(), json!(v));
        }
        if let Some(v) = p.seed {
            config.insert("seed".into(), json!(v));
        }
        if p.thinking != Some(false) {
            config.insert("thinkingConfig".into(), json!({"includeThoughts": true}));
        }
        if !config.is_empty() {
            body["generationConfig"] = Value::Object(config);
        }
        body
    }

    /// Streams one answer. When Gemini is busy or rate limited before anything was shown, the
    /// request is sent again after a short wait, a few times, before the error reaches the
    /// person.
    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let body = self.body(req);
        let mut attempt = 1;
        loop {
            let mut shown = false;
            let result = self
                .stream_once(&body, cancel, &mut |event| {
                    shown = true;
                    on_event(event);
                })
                .await;
            match result {
                Ok(completion) => return Ok(completion),
                Err(Failure::Busy(message)) if !shown && attempt < ATTEMPTS => {
                    let wait = RETRY_WAIT * 2u32.pow(attempt - 1);
                    tracing::warn!(model = %self.model, attempt, ?wait, "Gemini is busy, trying again: {message}");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(LlmError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                    attempt += 1;
                }
                Err(Failure::Busy(message)) => return Err(LlmError::Provider(message)),
                Err(Failure::Other(e)) => return Err(e),
            }
        }
    }

    async fn stream_once(
        &self,
        body: &Value,
        cancel: &CancellationToken,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, Failure> {
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let request = self.http.post(url).header("x-goog-api-key", &self.api_key).json(body);
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled.into()),
            r = request.send() => r.map_err(LlmError::from)?,
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(Failure::from_status(status, error_message(status, &text)));
        }

        let mut out = Completion {
            request_body: body.clone(),
            ..Default::default()
        };
        let mut parts: Vec<Value> = Vec::new();
        let mut decoder = SseDecoder::default();
        let mut stream = response.bytes_stream();
        let mut chunks = 0usize;
        let mut last = String::new();
        let mut blocked: Option<String> = None;
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(LlmError::Cancelled.into()),
                c = stream.next() => c,
            };
            let (events, ended) = match chunk {
                Some(Ok(bytes)) => (decoder.push(&bytes), false),
                Some(Err(e)) => return Err(LlmError::Network(e.to_string()).into()),
                None => (decoder.finish().into_iter().collect::<Vec<_>>(), true),
            };
            for data in events {
                chunks += 1;
                last = data.chars().take(2000).collect();
                let Ok(v) = serde_json::from_str::<Value>(&data) else {
                    tracing::warn!(model = %self.model, "Gemini sent a chunk that is not JSON: {last}");
                    continue;
                };
                if let Some(err) = v.get("error") {
                    let message = err["message"].as_str().unwrap_or("Gemini returned an error");
                    let code = err["code"].as_u64().unwrap_or_default() as u16;
                    return Err(Failure::from_status(code, message.to_string()));
                }
                apply_chunk(&v, &mut out, &mut parts, on_event);
                // A refused prompt arrives as feedback with no candidates at all.
                if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
                    blocked = Some(reason.to_string());
                }
            }
            if ended {
                break;
            }
        }
        if out.finish_reason.is_none() || (out.content.is_empty() && out.tool_calls.is_empty()) {
            tracing::warn!(
                model = %self.model,
                chunks,
                finish = ?out.finish_reason,
                ?blocked,
                "Gemini's answer has no text or tool call. Last chunk: {last}"
            );
        }
        // An answer with nothing in it would show as an empty message; say what happened instead.
        if out.content.is_empty() && out.tool_calls.is_empty() {
            let message = empty_answer_message(out.finish_reason.as_deref(), blocked.as_deref());
            return Err(LlmError::Provider(message).into());
        }
        out.provider_meta = Some(json!({ "gemini": { "parts": parts } }));
        Ok(out)
    }
}

/// Tries per answer while Gemini says it is busy.
const ATTEMPTS: u32 = 4;
/// Wait before the first retry; it doubles each time (2, 4, 8 seconds).
const RETRY_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_secs(2)
};

/// Why one attempt failed.
enum Failure {
    /// Rate limited or temporarily overloaded ("This model is currently experiencing high
    /// demand"): worth another try.
    Busy(String),
    Other(LlmError),
}

impl Failure {
    fn from_status(status: u16, message: String) -> Self {
        match status {
            429 | 500 | 502 | 503 | 504 => Failure::Busy(message),
            // Listed but not served to this key, e.g. "no longer available to new users".
            404 => Failure::Other(LlmError::Unavailable(message)),
            _ if message.contains("no longer available") => Failure::Other(LlmError::Unavailable(message)),
            _ => Failure::Other(LlmError::Provider(message)),
        }
    }
}

impl From<LlmError> for Failure {
    fn from(e: LlmError) -> Self {
        Failure::Other(e)
    }
}

/// Explains an answer with no text and no tool call. `blocked` is the prompt's block reason.
/// Trying again cannot get past a refusal, so those messages point at what can.
fn empty_answer_message(finish_reason: Option<&str>, blocked: Option<&str>) -> String {
    if let Some(reason) = blocked {
        return format!("Gemini refused this request ({reason}). Rephrase it or pick another model.");
    }
    match finish_reason {
        None => "Gemini's answer ended before it was complete. Try again.".to_string(),
        Some("STOP") => "Gemini returned an empty answer. Try again.".to_string(),
        Some(reason @ ("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII")) => {
            format!("Gemini refused to answer ({reason}). Rephrase the request or pick another model.")
        }
        Some(reason @ "RECITATION") => {
            format!("Gemini held back an answer that copied existing text ({reason}). Rephrase the request.")
        }
        Some(reason) => format!("Gemini stopped without answering ({reason})"),
    }
}

fn apply_chunk(v: &Value, out: &mut Completion, parts: &mut Vec<Value>, on_event: &mut dyn FnMut(StreamEvent)) {
    if out.model.is_none() {
        out.model = v["modelVersion"].as_str().map(str::to_string);
    }
    if let Some(candidate) = v["candidates"].get(0) {
        if let Some(list) = candidate["content"]["parts"].as_array() {
            for part in list {
                if let Some(call) = part.get("functionCall") {
                    let name = call["name"].as_str().unwrap_or_default().to_string();
                    let index = out.tool_calls.len();
                    on_event(StreamEvent::ToolCall {
                        index,
                        name: name.clone(),
                    });
                    out.tool_calls.push(ToolCall {
                        // Unique across the chat: results and the history match calls by id.
                        id: call["id"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("gemini_call_{index}_{}", crate::db::new_id().replace('-', ""))),
                        name,
                        arguments: call.get("args").map(|a| a.to_string()).unwrap_or_else(|| "{}".into()),
                    });
                } else if let Some(text) = part["text"].as_str() {
                    if part["thought"].as_bool().unwrap_or(false) {
                        out.reasoning.push_str(text);
                        if !text.is_empty() {
                            on_event(StreamEvent::Reasoning(text.to_string()));
                        }
                    } else {
                        out.content.push_str(text);
                        if !text.is_empty() {
                            on_event(StreamEvent::Content(text.to_string()));
                        }
                    }
                }
                push_part(parts, part.clone());
            }
        }
        if let Some(reason) = candidate["finishReason"].as_str() {
            out.finish_reason = Some(reason.to_string());
        }
    }
    if let Some(u) = v.get("usageMetadata") {
        out.usage = Usage {
            prompt_tokens: u["promptTokenCount"].as_u64().unwrap_or(0),
            completion_tokens: u["candidatesTokenCount"].as_u64().unwrap_or(0)
                + u["thoughtsTokenCount"].as_u64().unwrap_or(0),
            cached_tokens: u["cachedContentTokenCount"].as_u64().unwrap_or(0),
            reasoning_tokens: u["thoughtsTokenCount"].as_u64().unwrap_or(0),
        };
    }
}

/// Appends a streamed part, merging plain text into the previous part of the same kind so the
/// replayed history stays compact. Parts carrying a signature or a call are never merged.
fn push_part(parts: &mut Vec<Value>, part: Value) {
    let mergeable =
        |p: &Value| p.get("text").is_some() && p.get("thoughtSignature").is_none() && p.get("functionCall").is_none();
    if mergeable(&part)
        && let Some(last) = parts.last_mut()
        && mergeable(last)
        && last["thought"] == part["thought"]
    {
        let joined = format!(
            "{}{}",
            last["text"].as_str().unwrap_or_default(),
            part["text"].as_str().unwrap_or_default()
        );
        last["text"] = json!(joined);
        return;
    }
    parts.push(part);
}

/// Converts the neutral history into Gemini `contents`.
fn contents(messages: &[LlmMessage]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for m in messages {
        match m {
            LlmMessage::User { content, media } => {
                let mut parts: Vec<Value> = media
                    .iter()
                    .map(|m| json!({"inlineData": {"mimeType": m.mime, "data": m.data}}))
                    .collect();
                if !content.is_empty() || parts.is_empty() {
                    parts.push(json!({"text": content}));
                }
                out.push(json!({"role": "user", "parts": parts}));
            }
            LlmMessage::Assistant {
                content,
                tool_calls,
                provider_meta,
                ..
            } => {
                let replay = provider_meta
                    .as_ref()
                    .and_then(|m| m.pointer("/gemini/parts"))
                    .and_then(Value::as_array)
                    .filter(|p| !p.is_empty());
                let parts: Vec<Value> = match replay {
                    // Thought text is not needed on replay; signatures are.
                    Some(parts) => parts
                        .iter()
                        .filter(|p| !(p["thought"].as_bool().unwrap_or(false) && p.get("thoughtSignature").is_none()))
                        .cloned()
                        .collect(),
                    None => {
                        let mut parts = Vec::new();
                        if !content.is_empty() {
                            parts.push(json!({"text": content}));
                        }
                        for (i, call) in tool_calls.iter().enumerate() {
                            let args = super::parse_arguments(&call.arguments).unwrap_or(json!({}));
                            let mut part = json!({"functionCall": {"name": call.name, "args": args}});
                            if i == 0 {
                                part["thoughtSignature"] = json!(FOREIGN_SIGNATURE);
                            }
                            parts.push(part);
                        }
                        parts
                    }
                };
                if !parts.is_empty() {
                    out.push(json!({"role": "model", "parts": parts}));
                }
            }
            LlmMessage::Tool { name, content, .. } => {
                let result: Value = serde_json::from_str(content).unwrap_or_else(|_| json!(content));
                let part = json!({"functionResponse": {"name": name, "response": {"result": result}}});
                // Responses to calls made in the same turn travel together.
                let appended = out.last_mut().is_some_and(|last| {
                    let is_responses = last["role"] == "user"
                        && last["parts"]
                            .as_array()
                            .is_some_and(|p| p.iter().all(|x| x.get("functionResponse").is_some()));
                    if is_responses {
                        last["parts"].as_array_mut().unwrap().push(part.clone());
                    }
                    is_responses
                });
                if !appended {
                    out.push(json!({"role": "user", "parts": [part]}));
                }
            }
        }
    }
    out
}

/// Reduces a JSON Schema to the OpenAPI subset Gemini accepts.
pub fn sanitize_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                match k.as_str() {
                    "$schema" | "$id" | "additionalProperties" | "examples" | "default" | "const" | "title" => {}
                    "type" => match v {
                        Value::Array(types) => {
                            let non_null: Vec<&Value> = types.iter().filter(|t| t.as_str() != Some("null")).collect();
                            if let Some(first) = non_null.first() {
                                out.insert("type".into(), (*first).clone());
                            }
                            if non_null.len() != types.len() {
                                out.insert("nullable".into(), json!(true));
                            }
                        }
                        other => {
                            out.insert("type".into(), other.clone());
                        }
                    },
                    "properties" => {
                        let props: Map<String, Value> = v
                            .as_object()
                            .map(|o| o.iter().map(|(name, s)| (name.clone(), sanitize_schema(s))).collect())
                            .unwrap_or_default();
                        out.insert(k.clone(), Value::Object(props));
                    }
                    _ => {
                        out.insert(k.clone(), sanitize_schema(v));
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sanitize_schema).collect()),
        other => other.clone(),
    }
}

/// Lists the chat models an API key can use. Also serves as the connection test.
pub async fn list_models(http: &reqwest::Client, base_url: &str, api_key: &str) -> Result<Vec<CloudModel>, LlmError> {
    let mut models = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut req = http
            .get(format!("{}/models", base_url.trim_end_matches('/')))
            .header("x-goog-api-key", api_key)
            .query(&[("pageSize", "1000")]);
        if let Some(t) = &page_token {
            req = req.query(&[("pageToken", t.as_str())]);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        if !(200..300).contains(&status) {
            return Err(LlmError::Provider(error_message(status, &text)));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| LlmError::Provider(e.to_string()))?;
        for m in v["models"].as_array().into_iter().flatten() {
            let name = m["name"].as_str().unwrap_or_default();
            let id = name.trim_start_matches("models/").to_string();
            let methods: Vec<&str> = m["supportedGenerationMethods"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !methods.contains(&"generateContent") || !is_chat_model(&id) {
                continue;
            }
            models.push(CloudModel {
                display_name: m["displayName"].as_str().unwrap_or(&id).to_string(),
                description: m["description"].as_str().unwrap_or_default().to_string(),
                input_token_limit: m["inputTokenLimit"].as_u64().unwrap_or(0),
                output_token_limit: m["outputTokenLimit"].as_u64().unwrap_or(0),
                thinking: m["thinking"].as_bool().unwrap_or(false),
                always_thinks: false,
                capabilities: None,
                free: false,
                id,
            });
        }
        page_token = v["nextPageToken"].as_str().map(str::to_string);
        if page_token.is_none() {
            break;
        }
    }
    models.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(models)
}

fn is_chat_model(id: &str) -> bool {
    id.starts_with("gemini")
        && ![
            "embedding",
            "aqa",
            "imagen",
            "tts",
            "image",
            "native-audio",
            "live",
            "computer-use",
            "robotics",
        ]
        .iter()
        .any(|bad| id.contains(bad))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::GenParams;

    #[test]
    fn tool_results_of_one_turn_are_grouped() {
        let history = vec![
            LlmMessage::User {
                content: "hi".into(),
                media: Vec::new(),
            },
            LlmMessage::Assistant {
                content: String::new(),
                reasoning: None,
                tool_calls: vec![
                    ToolCall {
                        id: "a".into(),
                        name: "one".into(),
                        arguments: "{\"x\":1}".into(),
                    },
                    ToolCall {
                        id: "b".into(),
                        name: "two".into(),
                        arguments: "{}".into(),
                    },
                ],
                provider_meta: None,
            },
            LlmMessage::Tool {
                call_id: "a".into(),
                name: "one".into(),
                content: "{\"ok\":true}".into(),
            },
            LlmMessage::Tool {
                call_id: "b".into(),
                name: "two".into(),
                content: "plain text".into(),
            },
        ];
        let c = contents(&history);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1]["role"], "model");
        assert_eq!(c[1]["parts"][0]["thoughtSignature"], FOREIGN_SIGNATURE);
        assert_eq!(c[1]["parts"][0]["functionCall"]["args"]["x"], 1);
        assert_eq!(c[2]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(c[2]["parts"][1]["functionResponse"]["response"]["result"], "plain text");
    }

    #[test]
    fn replays_gemini_parts_without_bare_thoughts() {
        let meta = json!({"gemini": {"parts": [
            {"text": "pondering", "thought": true},
            {"functionCall": {"name": "f", "args": {}}, "thoughtSignature": "sig"}
        ]}});
        let c = contents(&[LlmMessage::Assistant {
            content: String::new(),
            reasoning: Some("pondering".into()),
            tool_calls: vec![],
            provider_meta: Some(meta),
        }]);
        let parts = c[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["thoughtSignature"], "sig");
    }

    #[test]
    fn streamed_text_parts_are_merged() {
        let mut parts = Vec::new();
        push_part(&mut parts, json!({"text": "Hel"}));
        push_part(&mut parts, json!({"text": "lo"}));
        push_part(&mut parts, json!({"text": "", "thoughtSignature": "s"}));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "Hello");
    }

    #[test]
    fn schema_is_reduced_to_the_supported_subset() {
        let s = sanitize_schema(&json!({
            "$schema": "x", "type": "object", "additionalProperties": false,
            "properties": {"a": {"type": ["string", "null"], "default": "q"}}
        }));
        assert_eq!(
            s,
            json!({"type": "object", "properties": {"a": {"type": "string", "nullable": true}}})
        );
    }

    #[test]
    fn only_chat_models_are_listed() {
        assert!(is_chat_model("gemini-2.5-flash"));
        assert!(!is_chat_model("gemini-embedding-001"));
        assert!(!is_chat_model("gemini-2.5-flash-preview-tts"));
        assert!(!is_chat_model("imagen-4.0"));
    }

    const BUSY: (&str, &str, &str) = (
        "503 Service Unavailable",
        "application/json",
        r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#,
    );
    const HELLO: (&str, &str, &str) = (
        "200 OK",
        "text/event-stream",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]},\"finishReason\":\"STOP\"}]}\r\n\r\n",
    );
    /// Thoughts, then the stream ends without the final chunk.
    const CUT_OFF: (&str, &str, &str) = (
        "200 OK",
        "text/event-stream",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Planning\",\"thought\":true}]}}]}\r\n\r\n",
    );
    /// The prompt itself is refused: feedback and no candidates.
    const PROMPT_BLOCKED: (&str, &str, &str) = (
        "200 OK",
        "text/event-stream",
        "data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"}}\r\n\r\n",
    );
    /// The answer is withheld: a candidate with a finish reason and no parts.
    const ANSWER_BLOCKED: (&str, &str, &str) = (
        "200 OK",
        "text/event-stream",
        "data: {\"candidates\":[{\"finishReason\":\"PROHIBITED_CONTENT\"}]}\r\n\r\n",
    );

    /// A local server that answers "busy" `busy` times, then streams one short answer.
    async fn flaky_server(busy: usize) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        scripted_server(move |n| if n < busy { BUSY } else { HELLO }).await
    }

    /// A local server that gives the `n`th request the answer `answer(n)`.
    async fn scripted_server(
        answer: impl Fn(usize) -> (&'static str, &'static str, &'static str) + Send + 'static,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                // Read the whole request before answering.
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
                let (status, kind, body) = answer(n);
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (base, hits)
    }

    fn hello_request() -> ChatRequest {
        ChatRequest {
            system: "sys".into(),
            messages: vec![LlmMessage::User {
                content: "hi".into(),
                media: Vec::new(),
            }],
            tools: vec![],
            params: GenParams::default(),
        }
    }

    #[tokio::test]
    async fn busy_answers_are_retried() {
        let (base, hits) = flaky_server(2).await;
        let client = GeminiClient::new(reqwest::Client::new(), base, "key".into(), "m".into());
        let mut text = String::new();
        let done = client
            .stream(&hello_request(), &CancellationToken::new(), |e| {
                if let StreamEvent::Content(t) = e {
                    text.push_str(&t);
                }
            })
            .await
            .expect("the third attempt answers");
        assert_eq!(done.content, "Hello");
        assert_eq!(text, "Hello");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_model_that_stays_busy_reports_why() {
        let (base, hits) = flaky_server(usize::MAX).await;
        let client = GeminiClient::new(reqwest::Client::new(), base, "key".into(), "m".into());
        let err = client
            .stream(&hello_request(), &CancellationToken::new(), |_| {})
            .await
            .expect_err("every attempt is busy");
        assert!(err.to_string().contains("high demand"), "{err}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), ATTEMPTS as usize);
    }

    #[tokio::test]
    async fn an_answer_cut_off_after_thinking_is_an_error() {
        let (base, _) = scripted_server(|_| CUT_OFF).await;
        let client = GeminiClient::new(reqwest::Client::new(), base, "key".into(), "m".into());
        let err = client
            .stream(&hello_request(), &CancellationToken::new(), |_| {})
            .await
            .expect_err("nothing but thoughts arrived");
        assert!(err.to_string().contains("ended before it was complete"), "{err}");
    }

    #[tokio::test]
    async fn a_blocked_prompt_says_it_was_refused() {
        let (base, hits) = scripted_server(|_| PROMPT_BLOCKED).await;
        let client = GeminiClient::new(reqwest::Client::new(), base, "key".into(), "m".into());
        let err = client
            .stream(&hello_request(), &CancellationToken::new(), |_| {})
            .await
            .expect_err("the prompt was refused");
        assert_eq!(
            err.to_string(),
            LlmError::Provider("Gemini refused this request (SAFETY). Rephrase it or pick another model.".into())
                .to_string()
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_withheld_answer_says_it_was_refused() {
        let (base, _) = scripted_server(|_| ANSWER_BLOCKED).await;
        let client = GeminiClient::new(reqwest::Client::new(), base, "key".into(), "m".into());
        let err = client
            .stream(&hello_request(), &CancellationToken::new(), |_| {})
            .await
            .expect_err("the answer was withheld");
        assert!(
            err.to_string().contains("refused to answer (PROHIBITED_CONTENT)"),
            "{err}"
        );
    }

    #[test]
    fn empty_answers_explain_why() {
        let refused = empty_answer_message(None, Some("PROHIBITED_CONTENT"));
        assert!(
            refused.contains("refused this request (PROHIBITED_CONTENT)"),
            "{refused}"
        );
        // The prompt's block reason wins over whatever the candidate said.
        assert!(empty_answer_message(Some("STOP"), Some("SAFETY")).contains("refused this request"));
        for reason in ["SAFETY", "PROHIBITED_CONTENT", "BLOCKLIST", "SPII"] {
            let message = empty_answer_message(Some(reason), None);
            assert!(message.contains(&format!("refused to answer ({reason})")), "{message}");
            assert!(!message.contains("Try again"), "{message}");
        }
        assert!(empty_answer_message(Some("RECITATION"), None).contains("copied existing text (RECITATION)"));
        assert!(empty_answer_message(None, None).contains("ended before it was complete"));
        assert!(empty_answer_message(Some("STOP"), None).contains("empty answer"));
        assert_eq!(
            empty_answer_message(Some("MAX_TOKENS"), None),
            "Gemini stopped without answering (MAX_TOKENS)"
        );
    }

    #[test]
    fn models_the_key_cannot_use_are_unavailable() {
        let retired = "This model models/gemini-2.5-flash is no longer available to new users.";
        for status in [404, 400] {
            assert!(matches!(
                Failure::from_status(status, retired.into()),
                Failure::Other(LlmError::Unavailable(_))
            ));
        }
        assert!(matches!(Failure::from_status(503, "busy".into()), Failure::Busy(_)));
        assert!(matches!(
            Failure::from_status(400, "bad request".into()),
            Failure::Other(LlmError::Provider(_))
        ));
    }
}
