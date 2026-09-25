//! OpenAI-compatible chat completions, as served by llama.cpp's `llama-server`.

use std::collections::BTreeMap;

use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::sse::SseDecoder;
use super::{ChatRequest, Completion, LlmError, LlmMessage, StreamEvent, Usage};
use crate::db::ToolCall;

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
    /// e.g. `http://127.0.0.1:52100/v1`
    pub base_url: String,
    /// Sent as the `model` field (llama-server serves one model and ignores it).
    pub model: String,
    pub api_key: Option<String>,
}

impl OpenAiClient {
    pub fn new(http: reqwest::Client, base_url: String, model: String) -> Self {
        Self {
            http,
            base_url,
            model,
            api_key: None,
        }
    }

    pub fn body(&self, req: &ChatRequest) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            messages.push(json!({"role": "system", "content": req.system}));
        }
        for m in &req.messages {
            messages.push(match m {
                LlmMessage::User { content } => json!({"role": "user", "content": content}),
                LlmMessage::Assistant {
                    content,
                    reasoning,
                    tool_calls,
                    ..
                } => {
                    let mut msg = json!({"role": "assistant", "content": content});
                    if let Some(r) = reasoning.as_ref().filter(|r| !r.is_empty()) {
                        msg["reasoning_content"] = json!(r);
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
            "cache_prompt": true,
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
        set(&mut body, "repeat_penalty", p.repeat_penalty.map(|v| json!(v)));
        set(&mut body, "max_tokens", p.max_tokens.map(|v| json!(v)));
        set(&mut body, "seed", p.seed.map(|v| json!(v)));
        if let Some(thinking) = p.thinking {
            body["chat_template_kwargs"] = json!({"enable_thinking": thinking});
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
            return Err(LlmError::Provider(error_message(status.as_u16(), &text)));
        }

        let mut out = Completion {
            request_body: body,
            ..Default::default()
        };
        let mut calls: BTreeMap<usize, ToolCall> = BTreeMap::new();
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
                apply_chunk(&v, &mut out, &mut calls, &mut on_event);
            }
            if ended {
                break;
            }
        }
        finish_calls(&mut out, calls);
        Ok(out)
    }
}

fn apply_chunk(
    v: &Value,
    out: &mut Completion,
    calls: &mut BTreeMap<usize, ToolCall>,
    on_event: &mut impl FnMut(StreamEvent),
) {
    if out.model.is_none() {
        out.model = v.get("model").and_then(Value::as_str).map(str::to_string);
    }
    if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
        let delta = &choice["delta"];
        if let Some(r) = delta.get("reasoning_content").and_then(Value::as_str)
            && !r.is_empty()
        {
            out.reasoning.push_str(r);
            on_event(StreamEvent::Reasoning(r.to_string()));
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

pub fn error_message(status: u16, body: &str) -> String {
    let parsed: Option<String> = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.pointer("/error/message")
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    match parsed {
        Some(m) => m,
        None if body.trim().is_empty() => format!("the model server answered {status}"),
        None => format!("the model server answered {status}: {}", body.trim()),
    }
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
                LlmMessage::User { content: "hi".into() },
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
        let mut events = Vec::new();
        for chunk in [
            json!({"choices":[{"delta":{"reasoning_content":"hmm"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"abc","type":"function","function":{"name":"market_quote","arguments":"{"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"symbols\":[\"FX:EURUSD\"]}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":4}},"timings":{"predicted_per_second":50.0}}),
        ] {
            apply_chunk(&chunk, &mut out, &mut calls, &mut |e| events.push(e));
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

    #[test]
    fn provider_errors_are_readable() {
        assert_eq!(
            error_message(400, r#"{"error":{"message":"context too long"}}"#),
            "context too long"
        );
        assert_eq!(error_message(503, ""), "the model server answered 503");
    }
}
