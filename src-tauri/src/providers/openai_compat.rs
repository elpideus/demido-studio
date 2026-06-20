use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter};

use super::{ChatMessage, StreamOutput, ToolCall, ToolDef};

/// Returns the largest byte index <= `index` that lies on a UTF-8 char boundary.
fn char_boundary_floor(s: &str, index: usize) -> usize {
    let index = index.min(s.len());
    (0..=index)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0)
}

/// Fetch models JSON. Tries provider-extended endpoint first (e.g. LM Studio /api/v0/models
/// which returns type/capabilities), falls back to standard /v1/models.
async fn fetch_models_json(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Value> {
    // Strip trailing /v1 or /v1/ so we can probe the extended endpoint
    let stripped = base_url.trim_end_matches('/').trim_end_matches("/v1");

    let mut auth_header: Option<String> = None;
    if let Some(key) = api_key {
        if !key.is_empty() {
            auth_header = Some(key.to_string());
        }
    }

    let add_auth = |req: reqwest::RequestBuilder| -> reqwest::RequestBuilder {
        if let Some(ref key) = auth_header {
            req.bearer_auth(key)
        } else {
            req
        }
    };

    // Try LM Studio extended endpoint first
    let extended_url = format!("{}/api/v0/models", stripped);
    let ext_resp = add_auth(client.get(&extended_url)).send().await;
    if let Ok(resp) = ext_resp {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<Value>().await {
                if json["data"].is_array() {
                    return Ok(json);
                }
            }
        }
    }

    // Fall back to standard OpenAI /v1/models
    let std_url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = add_auth(client.get(&std_url)).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("Provider list_models error: {}", text));
    }
    Ok(resp.json().await?)
}

pub async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    let json = fetch_models_json(client, base_url, api_key).await?;
    let models = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    // Skip embedding/reranker models from the selectable model list
                    let t = m["type"].as_str().unwrap_or("");
                    if t == "embeddings" || t == "reranker" {
                        return None;
                    }
                    m["id"].as_str().map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

/// Per-model capability flags returned from provider's /v1/models response.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ModelCaps {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
}

fn cap_array_has(caps_val: &Value, needle: &str) -> bool {
    caps_val
        .as_array()
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(needle)))
        .unwrap_or(false)
}

/// Returns map of model_id → ModelCaps.
///
/// Priority:
/// 1. LM Studio /api/v1/models — explicit `capabilities.vision`, `capabilities.trained_for_tool_use`,
///    `capabilities.reasoning` per-model object (richest source)
/// 2. LM Studio /api/v0/models (via fetch_models_json) — `type` field + capabilities array
/// 3. Standard /v1/models — OpenRouter modality, generic capabilities object
pub async fn list_model_capabilities(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<HashMap<String, ModelCaps>> {
    let auth_header: Option<String> = api_key.filter(|k| !k.is_empty()).map(|k| k.to_string());
    let add_auth = |req: reqwest::RequestBuilder| -> reqwest::RequestBuilder {
        if let Some(ref key) = auth_header {
            req.bearer_auth(key)
        } else {
            req
        }
    };

    // 1. Try LM Studio /api/v1/models — richest capability data
    let stripped = base_url.trim_end_matches('/').trim_end_matches("/v1");
    let lm_v1_url = format!("{}/api/v1/models", stripped);
    if let Ok(resp) = add_auth(client.get(&lm_v1_url)).send().await {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<Value>().await {
                if let Some(models) = json["models"].as_array() {
                    if !models.is_empty() {
                        let caps: HashMap<String, ModelCaps> = models
                            .iter()
                            .filter_map(|m| {
                                let id = m["key"].as_str()?.to_string();
                                let c = &m["capabilities"];
                                let mtype = m["type"].as_str().unwrap_or("");
                                // v0 uses "embeddings", v1 uses "embedding" — exclude both
                                if mtype == "embedding"
                                    || mtype == "embeddings"
                                    || mtype == "reranker"
                                {
                                    return None;
                                }
                                let vision = c["vision"].as_bool().unwrap_or(false);
                                let tools = c["trained_for_tool_use"].as_bool().unwrap_or(false);
                                // reasoning field is an object when present, absent when not supported
                                let reasoning =
                                    c.get("reasoning").map(|v| !v.is_null()).unwrap_or(false);
                                Some((
                                    id,
                                    ModelCaps {
                                        vision,
                                        tools,
                                        reasoning,
                                    },
                                ))
                            })
                            .collect();
                        if !caps.is_empty() {
                            return Ok(caps);
                        }
                    }
                }
            }
        }
    }

    // 2 & 3. Fall back to /api/v0/models (LM Studio) or standard /v1/models
    let json = fetch_models_json(client, base_url, api_key).await?;
    let caps = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let model_type = m["type"].as_str().unwrap_or("");

                    if model_type == "embeddings" || model_type == "reranker" {
                        return None;
                    }

                    let c = &m["capabilities"];
                    let modality = m["architecture"]["modality"].as_str().unwrap_or("");

                    let vision = model_type == "vlm"
                        || modality.contains("image")
                        || cap_array_has(c, "vision")
                        || c["vision"].as_bool().unwrap_or(false)
                        || c["completion_chat_multimodal"].as_bool().unwrap_or(false);

                    let tools = cap_array_has(c, "tool_use")
                        || cap_array_has(c, "tools")
                        || cap_array_has(c, "function_calling")
                        || c["tool_use"].as_bool().unwrap_or(false)
                        || c["tools"].as_bool().unwrap_or(false)
                        || c["function_calling"].as_bool().unwrap_or(false);

                    // For fallback path: infer reasoning from model id (no API field available)
                    let id_l = id.to_lowercase();
                    let reasoning = id_l.starts_with("o1")
                        || id_l.starts_with("o3")
                        || id_l.starts_with("o4")
                        || id_l.contains("-r1")
                        || id_l.starts_with("r1")
                        || id_l.contains("thinking")
                        || id_l.contains("-reason");

                    Some((
                        id,
                        ModelCaps {
                            vision,
                            tools,
                            reasoning,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(caps)
}

/// Returns raw JSON string from /v1/models for debugging capability detection.
pub async fn raw_models_json(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<String> {
    let json = fetch_models_json(client, base_url, api_key).await?;
    Ok(serde_json::to_string_pretty(&json)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat(
    client: &reqwest::Client,
    app: &AppHandle,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: &[ToolDef],
    reasoning_effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<StreamOutput> {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    // show_thinking: display the bubble unless user explicitly chose "off".
    // Models that always think (no capability reported) still get their bubble shown.
    let show_thinking = !matches!(reasoning_effort, Some("off"));
    // Only forward numeric effort values (low/medium/high/etc) to the API.
    // "on"/"off" are conceptual toggles — not valid LM Studio API values.
    // Omitting reasoning_effort lets the model use its default behavior.
    if let Some(effort) = reasoning_effort {
        if effort != "on" && effort != "off" {
            body["reasoning_effort"] = json!(effort);
        }
    }

    if !tools.is_empty() {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        body["tools"] = json!(tool_defs);
    }

    // Debug: log message structure when images are present (strips data URL content for brevity)
    #[cfg(debug_assertions)]
    {
        let has_image = messages.iter().any(|m| {
            m.content
                .as_array()
                .map(|arr| arr.iter().any(|b| b["type"] == "image_url"))
                .unwrap_or(false)
        });
        if has_image {
            eprintln!("[demido] stream_chat: sending {} messages with image attachment(s) to {}/chat/completions", messages.len(), base_url);
            for (i, msg) in messages.iter().enumerate() {
                if let Some(arr) = msg.content.as_array() {
                    let summary: Vec<String> = arr
                        .iter()
                        .map(|b| {
                            if b["type"] == "image_url" {
                                let url = b["image_url"]["url"].as_str().unwrap_or("");
                                format!(
                                    "image_url({}..., len={})",
                                    &url[..url.len().min(30)],
                                    url.len()
                                )
                            } else {
                                format!(
                                    "text({:?})",
                                    b["text"]
                                        .as_str()
                                        .unwrap_or("")
                                        .chars()
                                        .take(50)
                                        .collect::<String>()
                                )
                            }
                        })
                        .collect();
                    eprintln!(
                        "[demido]   msg[{}] role={} content={:?}",
                        i, msg.role, summary
                    );
                }
            }
        }
    }

    let mut req = client
        .post(format!("{}/chat/completions", base_url))
        .json(&body);
    if let Some(key) = api_key {
        if !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }

    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Waiting for response" }),
    )?;
    let response = req.send().await?;
    if !response.status().is_success() {
        let text = response.text().await?;
        return Err(anyhow!("Provider error: {}", text));
    }

    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Generating response" }),
    )?;
    let mut reader = crate::streaming::SseLineReader::new(response);
    let mut raw_content = String::new();
    let mut full_thinking = String::new();
    // tool_calls accumulator: index -> (id, name, arguments_str)
    let mut tool_acc: HashMap<usize, (String, String, String)> = HashMap::new();
    let mut finish_reason = String::new();
    // For <think> tag streaming: buffer content tokens until we know if we're in a think block
    let mut content_buf = String::new(); // pending tokens not yet emitted
    let mut in_think = false;

    while let Some(data) = reader.next_data().await? {
        if cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }
        if data == "[DONE]" {
            break;
        }

        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            let choice = &v["choices"][0];

            if let Some(fr) = choice["finish_reason"].as_str() {
                finish_reason = fr.to_string();
            }

            let delta = &choice["delta"];

            // reasoning_content: LM Studio 0.3.9+ / DeepSeek style
            if let Some(token) = delta["reasoning_content"].as_str() {
                if !token.is_empty() {
                    full_thinking.push_str(token);
                    if show_thinking {
                        app.emit("stream_thinking", token)?;
                    }
                }
            }

            // Stream content tokens, handling <think>...</think> tags inline.
            if let Some(token) = delta["content"].as_str() {
                if !token.is_empty() {
                    content_buf.push_str(token);
                }

                // Process content_buf: flush complete segments
                loop {
                    if in_think {
                        if let Some(end) = content_buf.find("</think>") {
                            let thought = &content_buf[..end];
                            full_thinking.push_str(thought);
                            if show_thinking {
                                app.emit("stream_thinking", thought)?;
                                app.emit("stream_thinking_end", ())?;
                            }
                            content_buf = content_buf[end + 8..].to_string();
                            in_think = false;
                        } else if content_buf.len() > 8 {
                            // Safely flush everything except the last 8 bytes (</think> length)
                            let safe = char_boundary_floor(&content_buf, content_buf.len() - 8);
                            let thought = content_buf[..safe].to_string();
                            full_thinking.push_str(&thought);
                            if show_thinking {
                                app.emit("stream_thinking", &thought)?;
                            }
                            content_buf = content_buf[safe..].to_string();
                            break;
                        } else {
                            break;
                        }
                    } else {
                        if let Some(start) = content_buf.find("<think>") {
                            let visible = content_buf[..start].to_string();
                            if !visible.is_empty() {
                                raw_content.push_str(&visible);
                                app.emit("stream_token", &visible)?;
                            }
                            content_buf = content_buf[start + 7..].to_string();
                            in_think = true;
                        } else if content_buf.len() > 7 {
                            // Flush everything except last 7 bytes (<think> length)
                            let safe = char_boundary_floor(&content_buf, content_buf.len() - 7);
                            let visible = content_buf[..safe].to_string();
                            raw_content.push_str(&visible);
                            app.emit("stream_token", &visible)?;
                            content_buf = content_buf[safe..].to_string();
                            break;
                        } else {
                            break;
                        }
                    }
                }
            }

            if let Some(tcs) = delta["tool_calls"].as_array() {
                for tc in tcs {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = tool_acc
                        .entry(idx)
                        .or_insert_with(|| (String::new(), String::new(), String::new()));
                    if let Some(id) = tc["id"].as_str() {
                        entry.0 = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        entry.1 = name.to_string();
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        entry.2.push_str(args);
                    }
                }
            }
        }
    }

    // Flush any remaining content_buf
    if !content_buf.is_empty() {
        if in_think {
            // Unclosed <think> tag — treat remainder as thinking
            full_thinking.push_str(&content_buf);
            if show_thinking {
                app.emit("stream_thinking", &content_buf)?;
                app.emit("stream_thinking_end", ())?;
            }
        } else {
            raw_content.push_str(&content_buf);
            app.emit("stream_token", &content_buf)?;
        }
    }

    let full_content = raw_content;

    // Build tool calls if any
    let mut tool_calls = Vec::new();
    if finish_reason == "tool_calls" || !tool_acc.is_empty() {
        let mut indices: Vec<usize> = tool_acc.keys().copied().collect();
        indices.sort();
        for idx in indices {
            if let Some((id, name, args_str)) = tool_acc.remove(&idx) {
                let arguments =
                    serde_json::from_str(&args_str).unwrap_or(Value::Object(Default::default()));
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                    thought_signature: None,
                });
            }
        }
    }

    Ok(StreamOutput {
        content: full_content,
        tool_calls,
        thinking: if show_thinking && !full_thinking.is_empty() {
            Some(full_thinking)
        } else {
            None
        },
    })
}

pub async fn complete_chat(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
) -> Result<String> {
    let mut req = client
        .post(format!("{}/chat/completions", base_url))
        .json(&json!({ "model": model, "messages": messages }));
    if let Some(key) = api_key {
        if !key.is_empty() {
            req = req.bearer_auth(key);
        }
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("complete_chat error: {}", text));
    }
    let json: Value = resp.json().await?;
    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(text)
}
