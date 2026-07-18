use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, Arc};
use tauri::{AppHandle, Emitter};

use super::{ChatMessage, StreamOutput, ToolCall, ToolDef};
use crate::streaming::Chunk;

pub async fn list_models() -> Result<Vec<String>> {
    Ok(vec![
        "claude-opus-4-8".into(),
        "claude-sonnet-4-6".into(),
        "claude-haiku-4-5-20251001".into(),
    ])
}

pub async fn list_model_capabilities(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<HashMap<String, crate::caps::PartialCaps>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    // models-2025-02-19 beta header enables the `capabilities` object in each model entry.
    let mut req = client
        .get(&url)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "models-2025-02-19");
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.header("x-api-key", key);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("Anthropic list_models error: {}", text));
    }
    let json: Value = resp.json().await?;
    let caps = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let c = &m["capabilities"];
                    // The beta capabilities object is authoritative when present. Without the
                    // beta header it's absent — leave those None so models.dev answers rather
                    // than us assuming every listed model does vision/tools.
                    Some((
                        id,
                        crate::caps::PartialCaps {
                            vision: c["image_input"]["supported"].as_bool(),
                            tools: c["tool_use"]["supported"].as_bool(),
                            reasoning: c["extended_thinking"]["supported"].as_bool(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(caps)
}

/// Convert our generic message format to Anthropic format.
/// Anthropic uses tool_result content blocks inside user messages, not separate "tool" role messages.
fn to_anthropic_messages(messages: Vec<ChatMessage>) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let m = &messages[i];
        if m.role == "tool" {
            // Collect consecutive tool results and merge into one user message
            let mut tool_results = Vec::new();
            while i < messages.len() && messages[i].role == "tool" {
                let tm = &messages[i];
                tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": tm.tool_call_id,
                    "content": tm.content,
                }));
                i += 1;
            }
            out.push(json!({ "role": "user", "content": tool_results }));
            continue;
        }
        if m.role == "assistant" {
            if let Some(tool_calls) = &m.tool_calls {
                // Reconstruct Anthropic tool_use blocks
                let content: Vec<serde_json::Value> = if let Some(arr) = tool_calls.as_array() {
                    arr.iter()
                        .map(|tc| {
                            json!({
                                "type": "tool_use",
                                "id": tc["id"],
                                "name": tc["function"]["name"],
                                "input": serde_json::from_str::<serde_json::Value>(
                                    tc["function"]["arguments"].as_str().unwrap_or("{}")
                                ).unwrap_or(json!({})),
                            })
                        })
                        .collect()
                } else {
                    vec![]
                };
                out.push(json!({ "role": "assistant", "content": content }));
                i += 1;
                continue;
            }
        }
        // Translate OpenAI-format image_url blocks → Anthropic image blocks
        let content = match &m.content {
            Value::Array(blocks) => {
                let translated: Vec<Value> = blocks.iter().map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                        let url = b["image_url"]["url"].as_str().unwrap_or("");
                        if let Some(rest) = url.strip_prefix("data:") {
                            if let Some((mime, data)) = rest.split_once(";base64,") {
                                return json!({
                                    "type": "image",
                                    "source": { "type": "base64", "media_type": mime, "data": data }
                                });
                            }
                        }
                    }
                    b.clone()
                }).collect();
                Value::Array(translated)
            }
            other => other.clone(),
        };
        out.push(json!({ "role": m.role, "content": content }));
        i += 1;
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat(
    client: &reqwest::Client,
    app: &AppHandle,
    api_key: &str,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
    tools: &[ToolDef],
    reasoning_effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<StreamOutput> {
    let thinking_enabled = reasoning_effort.map(|e| e != "off").unwrap_or(false);
    let anthropic_messages = to_anthropic_messages(messages);
    let mut body = json!({
        "model": model,
        "max_tokens": 8192,
        "messages": anthropic_messages,
        "stream": true,
    });

    if let Some(sp) = system_prompt {
        if !sp.is_empty() {
            body["system"] = json!(sp);
        }
    }

    if !tools.is_empty() {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        body["tools"] = json!(tool_defs);
    }

    if thinking_enabled {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": 10000 });
        body["max_tokens"] = json!(16000);
    }

    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Waiting for response" }),
    )?;

    let mut req = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01");

    if thinking_enabled {
        req = req.header("anthropic-betas", "interleaved-thinking-2025-05-14");
    }

    let response = req.json(&body).send().await?;

    if !response.status().is_success() {
        let text = response.text().await?;
        return Err(anyhow!("Anthropic error: {}", text));
    }

    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Generating response" }),
    )?;

    let mut reader = crate::streaming::SseLineReader::new(response);
    let mut full_content = String::new();

    // Tool call state
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_args = String::new();
    let mut in_tool_use = false;
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut current_thinking = String::new();
    let mut in_thinking = false;
    let mut full_thinking = String::new();

    let mut cancelled = false;

    loop {
        let data = match reader.next_data_cancellable(cancel).await? {
            Chunk::Data(d) => d,
            Chunk::End => break,
            Chunk::Cancelled => {
                cancelled = true;
                break;
            }
        };
        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            match v["type"].as_str() {
                Some("content_block_start") => {
                    let block = &v["content_block"];
                    if block["type"].as_str() == Some("tool_use") {
                        in_tool_use = true;
                        current_tool_id = block["id"].as_str().unwrap_or("").to_string();
                        current_tool_name = block["name"].as_str().unwrap_or("").to_string();
                        current_tool_args.clear();
                    }
                    if block["type"].as_str() == Some("thinking") {
                        in_thinking = true;
                        current_thinking.clear();
                    }
                }
                Some("content_block_delta") => {
                    let delta = &v["delta"];
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            if let Some(token) = delta["text"].as_str() {
                                full_content.push_str(token);
                                app.emit("stream_token", token)?;
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) = delta["partial_json"].as_str() {
                                current_tool_args.push_str(partial);
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(t) = delta["thinking"].as_str() {
                                current_thinking.push_str(t);
                                app.emit("stream_thinking", t)?;
                            }
                        }
                        _ => {}
                    }
                }
                Some("content_block_stop") => {
                    if in_tool_use {
                        let arguments = serde_json::from_str(&current_tool_args)
                            .unwrap_or(Value::Object(Default::default()));
                        tool_calls.push(ToolCall {
                            id: current_tool_id.clone(),
                            name: current_tool_name.clone(),
                            arguments,
                            thought_signature: None,
                        });
                        in_tool_use = false;
                    }
                    if in_thinking {
                        full_thinking = std::mem::take(&mut current_thinking);
                        app.emit("stream_thinking_end", ())?;
                        in_thinking = false;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(StreamOutput {
        content: full_content,
        tool_calls,
        thinking: if full_thinking.is_empty() {
            None
        } else {
            Some(full_thinking)
        },
        cancelled,
    })
}

pub async fn complete_chat(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
) -> Result<String> {
    let anthropic_msgs = to_anthropic_messages(messages);
    let mut body = json!({
        "model": model,
        "max_tokens": 64,
        "messages": anthropic_msgs,
    });
    if let Some(sp) = system_prompt {
        if !sp.is_empty() {
            body["system"] = json!(sp);
        }
    }
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("complete_chat error: {}", text));
    }
    let json: Value = resp.json().await?;
    let text = json["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(text)
}
