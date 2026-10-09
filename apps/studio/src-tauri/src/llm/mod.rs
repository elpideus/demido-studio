//! Talking to models.
//!
//! [`ChatRequest`] is provider-neutral. [`Client`] turns it into the wire format of the model's
//! provider (OpenAI-compatible for llama.cpp's server and OpenRouter, or Gemini's own), streams
//! the answer back as [`StreamEvent`]s and returns the assembled [`Completion`], including the
//! exact request body for the trace. A model that is busy for a moment is asked again (see
//! [`retry`]).

pub mod gemini;
pub mod openai;
pub mod openrouter;
pub mod retry;
pub mod sse;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::db::ToolCall;
use crate::models::capabilities::Capabilities;

/// A model a cloud provider offers, as its list of models describes it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloudModel {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_token_limit: u64,
    pub output_token_limit: u64,
    /// Can think before answering.
    pub thinking: bool,
    /// Thinks whatever it is asked: the provider refuses a request to answer directly.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub always_thinks: bool,
    /// What it can do, when the provider's list says (OpenRouter's does). Gemini's list says
    /// only which models think; models.dev tells the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    /// Costs nothing to use.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free: bool,
}

/// Part of what a provider lets a key use, as the provider itself reports it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Allowance {
    pub kind: AllowanceKind,
    pub remaining: f64,
    pub limit: f64,
    /// When it is full again (ms since the epoch); `None` when it never refills.
    pub resets_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AllowanceKind {
    /// Requests to free models in the current UTC day.
    FreeRequests,
    /// US dollars of credit the key may still spend.
    KeyCredit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum LlmMessage {
    User {
        content: String,
        /// Images and sound attached to the message, sent before its text.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<Media>,
    },
    Assistant {
        content: String,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCall>,
        /// Provider-specific replay data (Gemini parts with thought signatures, OpenRouter
        /// reasoning details).
        provider_meta: Option<serde_json::Value>,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
}

/// An image or a sound a model reads with a message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Media {
    /// `image/jpeg`, `image/png`, `audio/wav`, `audio/mpeg`...
    pub mime: String,
    /// The bytes, base64-encoded.
    pub data: String,
}

impl Media {
    pub fn new(mime: impl Into<String>, bytes: &[u8]) -> Self {
        use base64::Engine;
        Self {
            mime: mime.into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }
}

/// A request body as the trace keeps it: every base64 image or sound is replaced by a note of its
/// type and size, so a chat with pictures does not store them again for every model call.
pub fn redact_media(body: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    fn note(mime: &str, base64_len: usize) -> String {
        let kb = (base64_len * 3 / 4).div_ceil(1024);
        format!("[{mime}, {kb} KB: left out of the trace]")
    }
    match body {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let redacted = match (k.as_str(), v) {
                    // OpenAI-compatible images: "url": "data:image/png;base64,..."
                    ("url", Value::String(s)) if s.starts_with("data:") && s.len() > 256 => {
                        let (head, data) = s.split_once(',').unwrap_or((s, ""));
                        let mime = head.trim_start_matches("data:").split(';').next().unwrap_or("");
                        Value::String(note(mime, data.len()))
                    }
                    // OpenAI-compatible sound, and Gemini's inline data.
                    ("input_audio" | "inlineData", Value::Object(inner)) => {
                        let mut inner = inner.clone();
                        let mime = inner
                            .get("mimeType")
                            .or_else(|| inner.get("format"))
                            .and_then(Value::as_str)
                            .unwrap_or("data")
                            .to_string();
                        if let Some(Value::String(data)) = inner.get("data") {
                            let len = data.len();
                            inner.insert("data".into(), Value::String(note(&mime, len)));
                        }
                        Value::Object(inner)
                    }
                    _ => redact_media(v),
                };
                out.insert(k.clone(), redacted);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_media).collect()),
        other => other.clone(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: serde_json::Value,
}

/// Sampling and length settings. `None` leaves the provider's default.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenParams {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<i64>,
    /// Ask thinking models to think (true) or answer directly (false).
    pub thinking: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolSpec>,
    pub params: GenParams,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Content(String),
    Reasoning(String),
    /// A tool call started streaming; its arguments follow.
    ToolCall {
        index: usize,
        name: String,
    },
    /// The model was busy (`reason`): the request goes again after `wait`, as retry `attempt` of
    /// `attempts`. What streamed of the failed attempt is void; the next one starts afresh.
    Retrying {
        attempt: u32,
        attempts: u32,
        wait: Duration,
        reason: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Completion {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Usage,
    /// Server-side timings (llama.cpp), passed through for the stats line and the trace.
    pub timings: Option<serde_json::Value>,
    pub provider_meta: Option<serde_json::Value>,
    /// The request body exactly as sent (secrets are never part of the body).
    pub request_body: serde_json::Value,
    /// Model name the provider reported: for a router, the model it picked.
    pub model: Option<String>,
    /// Who ran the model, when the API says: OpenRouter names the provider it sent the request to.
    pub provider: Option<String>,
}

/// A model endpoint ready to take requests.
#[derive(Clone)]
pub enum Client {
    OpenAi(openai::OpenAiClient),
    Gemini(gemini::GeminiClient),
}

impl Client {
    /// Streams one answer. While the model is busy (overloaded, rate limited for a moment, or out
    /// of reach for a cloud model), the request is sent again after a growing wait, up to
    /// [`retry::RETRIES`] times; [`StreamEvent::Retrying`] says so before each wait.
    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let mut retries = 0;
        loop {
            let result = match self {
                Client::OpenAi(c) => c.stream(req, cancel, &mut on_event).await,
                Client::Gemini(c) => c.stream(req, cancel, &mut on_event).await,
            };
            let err = match result {
                Ok(completion) => return Ok(completion),
                Err(err) => err,
            };
            let (reason, asked) = match &err {
                LlmError::Busy { message, retry_after } => (message.clone(), *retry_after),
                // A local server that cannot be reached is down, not busy: the runtime restarts it.
                LlmError::Network(message) if !self.is_local() => {
                    (format!("could not reach the model: {message}"), None)
                }
                _ => return Err(err),
            };
            let wait = (retries < retry::RETRIES && !retry::lasting(&reason))
                .then(|| retry::wait_before(retries + 1, asked))
                .flatten();
            let Some(wait) = wait else {
                return Err(gave_up(err, retries, asked));
            };
            retries += 1;
            tracing::warn!(attempt = retries, ?wait, "the model is busy, trying again: {reason}");
            on_event(StreamEvent::Retrying {
                attempt: retries,
                attempts: retry::RETRIES,
                wait,
                reason,
            });
            tokio::select! {
                _ = cancel.cancelled() => return Err(LlmError::Cancelled),
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }

    fn is_local(&self) -> bool {
        matches!(self, Client::OpenAi(c) if c.dialect == openai::Dialect::LlamaCpp)
    }

    /// Whether the model is one of OpenRouter's routers (the Free Models Router, say), which
    /// picks another model for each request: [`Completion::model`] says which.
    pub fn is_router(&self) -> bool {
        matches!(self, Client::OpenAi(c)
            if matches!(c.dialect, openai::Dialect::OpenRouter { .. }) && openrouter::is_router(&c.model))
    }
}

/// The error of the last attempt, saying how often the request was tried or when it may work.
fn gave_up(err: LlmError, retries: u32, asked: Option<Duration>) -> LlmError {
    let note = match asked {
        Some(wait) if retry::too_long(wait) => {
            format!(" Try again in {}.", retry::rough(wait))
        }
        _ if retries > 0 => format!(" It was tried {} times.", retries + 1),
        _ => String::new(),
    };
    match err {
        LlmError::Busy { message, retry_after } => LlmError::Busy {
            message: format!("{}{note}", message.trim_end()),
            retry_after,
        },
        LlmError::Network(message) => LlmError::Network(format!("{}{note}", message.trim_end())),
        other => other,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("stopped")]
    Cancelled,
    #[error("{0}")]
    Provider(String),
    /// Overloaded or rate limited for now: worth asking again, after `retry_after` when the
    /// provider said how long.
    #[error("{message}")]
    Busy {
        message: String,
        retry_after: Option<Duration>,
    },
    /// The provider lists the model but will not run it for this account (for example a
    /// model retired for new API keys).
    #[error("{0}")]
    Unavailable(String),
    /// The request does not fit in the local model's context: `prompt_tokens` of it, counted by
    /// the server, against `context_tokens`.
    #[error("{message}")]
    ContextFull {
        message: String,
        prompt_tokens: usize,
        context_tokens: usize,
    },
    #[error("could not reach the model: {0}")]
    Network(String),
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        LlmError::Network(e.to_string())
    }
}

/// Parses tool call arguments, tolerating the small mistakes small models make: trailing
/// text after the object, code fences, or an empty string for "no arguments".
pub fn parse_arguments(raw: &str) -> Result<serde_json::Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return match v {
            // Some models double-encode the object as a JSON string.
            serde_json::Value::String(s) => parse_arguments(&s),
            other => Ok(other),
        };
    }
    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Some(start) = unfenced.find('{') {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escaped = false;
        for (i, ch) in unfenced[start..].char_indices() {
            match ch {
                _ if escaped => escaped = false,
                '\\' if in_str => escaped = true,
                '"' => in_str = !in_str,
                '{' if !in_str => depth += 1,
                '}' if !in_str => {
                    depth -= 1;
                    if depth == 0 {
                        let candidate = &unfenced[start..start + i + 1];
                        if let Ok(v) = serde_json::from_str(candidate) {
                            return Ok(v);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    Err(format!("the arguments were not valid JSON: {trimmed}"))
}

#[cfg(test)]
mod tests {
    use super::{parse_arguments, redact_media};
    use serde_json::json;

    #[test]
    fn traces_leave_out_media_bytes() {
        let image = format!("data:image/jpeg;base64,{}", "A".repeat(4096));
        let body = json!({
            "messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": image}},
                {"type": "input_audio", "input_audio": {"data": "B".repeat(2048), "format": "wav"}},
                {"type": "text", "text": "what is this?"}
            ]}],
            "contents": [{"parts": [{"inlineData": {"mimeType": "image/png", "data": "C".repeat(8192)}}]}],
            "short": {"url": "data:,x"}
        });
        let r = redact_media(&body);
        assert_eq!(
            r["messages"][0]["content"][0]["image_url"]["url"],
            "[image/jpeg, 3 KB: left out of the trace]"
        );
        assert_eq!(
            r["messages"][0]["content"][1]["input_audio"]["data"],
            "[wav, 2 KB: left out of the trace]"
        );
        assert_eq!(r["messages"][0]["content"][1]["input_audio"]["format"], "wav");
        assert_eq!(r["messages"][0]["content"][2]["text"], "what is this?");
        assert_eq!(
            r["contents"][0]["parts"][0]["inlineData"]["data"],
            "[image/png, 6 KB: left out of the trace]"
        );
        assert_eq!(r["short"]["url"], "data:,x");
    }

    #[test]
    fn lenient_argument_parsing() {
        assert_eq!(parse_arguments("").unwrap(), json!({}));
        assert_eq!(parse_arguments(r#"{"a":1}"#).unwrap(), json!({"a": 1}));
        assert_eq!(parse_arguments(r#""{\"a\":1}""#).unwrap(), json!({"a": 1}));
        assert_eq!(
            parse_arguments("```json\n{\"a\": \"}\"}\n```").unwrap(),
            json!({"a": "}"})
        );
        assert_eq!(parse_arguments(r#"{"a":1} trailing"#).unwrap(), json!({"a": 1}));
        assert!(parse_arguments("not json").is_err());
    }
}
