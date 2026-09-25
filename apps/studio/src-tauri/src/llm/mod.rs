//! Talking to models.
//!
//! [`ChatRequest`] is provider-neutral. [`Client`] turns it into the wire format of the model's
//! provider (llama.cpp's OpenAI-compatible server, or Gemini), streams the answer back as
//! [`StreamEvent`]s and returns the assembled [`Completion`], including the exact request body
//! for the trace.

pub mod gemini;
pub mod openai;
mod sse;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::db::ToolCall;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum LlmMessage {
    User {
        content: String,
    },
    Assistant {
        content: String,
        reasoning: Option<String>,
        tool_calls: Vec<ToolCall>,
        /// Provider-specific replay data (Gemini parts with thought signatures).
        provider_meta: Option<serde_json::Value>,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
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
    /// Model name the provider reported.
    pub model: Option<String>,
}

/// A model endpoint ready to take requests.
#[derive(Clone)]
pub enum Client {
    OpenAi(openai::OpenAiClient),
    Gemini(gemini::GeminiClient),
}

impl Client {
    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        match self {
            Client::OpenAi(c) => c.stream(req, cancel, on_event).await,
            Client::Gemini(c) => c.stream(req, cancel, on_event).await,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("stopped")]
    Cancelled,
    #[error("{0}")]
    Provider(String),
    /// The provider lists the model but will not run it for this account (for example a
    /// model retired for new API keys).
    #[error("{0}")]
    Unavailable(String),
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
    use super::parse_arguments;
    use serde_json::json;

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
