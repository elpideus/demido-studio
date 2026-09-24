//! Google Gemini through its native `generateContent` API.
//!
//! The native API (rather than Google's OpenAI-compatible layer) is used because function
//! calling on thinking models depends on replaying each model turn's parts together with their
//! thought signatures. Those parts are kept in the message's `provider_meta`.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::openai::error_message;
use super::sse::SseDecoder;
use super::{ChatRequest, Completion, LlmError, LlmMessage, StreamEvent, Usage};
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

    pub async fn stream(
        &self,
        req: &ChatRequest,
        cancel: &CancellationToken,
        mut on_event: impl FnMut(StreamEvent) + Send,
    ) -> Result<Completion, LlmError> {
        let body = self.body(req);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let request = self
            .http
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body);
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LlmError::Cancelled),
            r = request.send() => r?,
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::Provider(error_message(status, &text)));
        }

        let mut out = Completion {
            request_body: body,
            ..Default::default()
        };
        let mut parts: Vec<Value> = Vec::new();
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
                    continue;
                };
                if let Some(err) = v.get("error") {
                    return Err(LlmError::Provider(
                        err["message"]
                            .as_str()
                            .unwrap_or("Gemini returned an error")
                            .to_string(),
                    ));
                }
                apply_chunk(&v, &mut out, &mut parts, &mut on_event);
            }
            if ended {
                break;
            }
        }
        if out.content.is_empty() && out.tool_calls.is_empty() {
            if let Some(reason) = out.finish_reason.as_deref() {
                if reason != "STOP" {
                    return Err(LlmError::Provider(format!(
                        "Gemini stopped without answering ({reason})"
                    )));
                }
            }
        }
        out.provider_meta = Some(json!({ "gemini": { "parts": parts } }));
        Ok(out)
    }
}

fn apply_chunk(
    v: &Value,
    out: &mut Completion,
    parts: &mut Vec<Value>,
    on_event: &mut impl FnMut(StreamEvent),
) {
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
                        id: call["id"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("gemini_call_{index}")),
                        name,
                        arguments: call
                            .get("args")
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "{}".into()),
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
    let mergeable = |p: &Value| {
        p.get("text").is_some() && p.get("thoughtSignature").is_none() && p.get("functionCall").is_none()
    };
    if mergeable(&part) {
        if let Some(last) = parts.last_mut() {
            if mergeable(last) && last["thought"] == part["thought"] {
                let joined = format!(
                    "{}{}",
                    last["text"].as_str().unwrap_or_default(),
                    part["text"].as_str().unwrap_or_default()
                );
                last["text"] = json!(joined);
                return;
            }
        }
    }
    parts.push(part);
}

/// Converts the neutral history into Gemini `contents`.
fn contents(messages: &[LlmMessage]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for m in messages {
        match m {
            LlmMessage::User { content } => {
                out.push(json!({"role": "user", "parts": [{"text": content}]}));
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
                        .filter(|p| {
                            !(p["thought"].as_bool().unwrap_or(false)
                                && p.get("thoughtSignature").is_none())
                        })
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
                let result: Value =
                    serde_json::from_str(content).unwrap_or_else(|_| json!(content));
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
                    "$schema" | "$id" | "additionalProperties" | "examples" | "default" | "const"
                    | "title" => {}
                    "type" => match v {
                        Value::Array(types) => {
                            let non_null: Vec<&Value> =
                                types.iter().filter(|t| t.as_str() != Some("null")).collect();
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
                            .map(|o| {
                                o.iter()
                                    .map(|(name, s)| (name.clone(), sanitize_schema(s)))
                                    .collect()
                            })
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
pub async fn list_models(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<GeminiModel>, LlmError> {
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
        let v: Value =
            serde_json::from_str(&text).map_err(|e| LlmError::Provider(e.to_string()))?;
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
            models.push(GeminiModel {
                display_name: m["displayName"].as_str().unwrap_or(&id).to_string(),
                description: m["description"].as_str().unwrap_or_default().to_string(),
                input_token_limit: m["inputTokenLimit"].as_u64().unwrap_or(0),
                output_token_limit: m["outputTokenLimit"].as_u64().unwrap_or(0),
                thinking: m["thinking"].as_bool().unwrap_or(false),
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
            "embedding", "aqa", "imagen", "tts", "image", "native-audio", "live", "computer-use",
            "robotics",
        ]
        .iter()
        .any(|bad| id.contains(bad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_results_of_one_turn_are_grouped() {
        let history = vec![
            LlmMessage::User {
                content: "hi".into(),
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
        assert_eq!(s, json!({"type": "object", "properties": {"a": {"type": "string", "nullable": true}}}));
    }

    #[test]
    fn only_chat_models_are_listed() {
        assert!(is_chat_model("gemini-2.5-flash"));
        assert!(!is_chat_model("gemini-embedding-001"));
        assert!(!is_chat_model("gemini-2.5-flash-preview-tts"));
        assert!(!is_chat_model("imagen-4.0"));
    }
}
