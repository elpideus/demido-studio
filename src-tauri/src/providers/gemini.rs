use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter};

use super::{ChatMessage, StreamOutput, ToolCall, ToolDef};

/// Recursively remove fields unsupported by Gemini's schema (e.g. `additionalProperties`).
fn strip_unsupported_schema_fields(v: Value) -> Value {
    match v {
        Value::Object(mut map) => {
            map.remove("additionalProperties");
            map.remove("$schema");
            // Gemini requires enum values to be strings — coerce numbers/bools.
            if let Some(Value::Array(enums)) = map.get_mut("enum") {
                *enums = enums
                    .iter()
                    .map(|e| match e {
                        Value::String(_) => e.clone(),
                        other => Value::String(other.to_string()),
                    })
                    .collect();
            }
            Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, strip_unsupported_schema_fields(v)))
                    .collect(),
            )
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(strip_unsupported_schema_fields)
                .collect(),
        ),
        other => other,
    }
}

async fn fetch_models_json(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Value> {
    let mut req = client.get(format!("{}/models", base_url));
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.query(&[("key", key)]);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("Gemini list_models error: {}", text));
    }
    Ok(resp.json().await?)
}

pub async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    let json = fetch_models_json(client, base_url, api_key).await?;
    let models = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|m| {
                    m["supportedGenerationMethods"]
                        .as_array()
                        .map(|methods| {
                            methods
                                .iter()
                                .any(|v| v.as_str() == Some("generateContent"))
                        })
                        .unwrap_or(false)
                })
                .filter_map(|m| {
                    m["name"]
                        .as_str()
                        .map(|s| s.trim_start_matches("models/").to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

pub async fn list_model_capabilities(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<HashMap<String, super::openai_compat::ModelCaps>> {
    let json = fetch_models_json(client, base_url, api_key).await?;
    let caps = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    // Only models that support generateContent
                    let supports_generate = m["supportedGenerationMethods"]
                        .as_array()
                        .map(|methods| {
                            methods
                                .iter()
                                .any(|v| v.as_str() == Some("generateContent"))
                        })
                        .unwrap_or(false);
                    if !supports_generate {
                        return None;
                    }

                    let id = m["name"]
                        .as_str()?
                        .trim_start_matches("models/")
                        .to_string();

                    // Reasoning: use the explicit API field — no heuristics needed.
                    let reasoning = m["thinking"].as_bool().unwrap_or(false);

                    // Audio/music generation models don't accept image input or function calls.
                    // Detect them by model id prefix/suffix rather than version number.
                    let is_audio_gen = id.contains("tts") || id.contains("lyria");
                    let is_embedding = id.contains("embedding");

                    // All non-audio, non-embedding generateContent models support multimodal vision.
                    let vision = !is_audio_gen && !is_embedding;

                    // Tool/function calling: same exclusions as vision.
                    let tools = !is_audio_gen && !is_embedding;

                    Some((
                        id,
                        super::openai_compat::ModelCaps {
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

/// Translate shared ChatMessage vec to Gemini "contents" array.
/// Maintains a tool_id→name/signature map so functionResponse parts can include the name and thought_signature.
fn to_gemini_contents(messages: Vec<ChatMessage>) -> Vec<Value> {
    // First pass: build a map of tool_call_id → function_name
    let mut id_to_name: HashMap<String, String> = HashMap::new();
    for m in &messages {
        if m.role == "assistant" {
            if let Some(tcs) = &m.tool_calls {
                if let Some(arr) = tcs.as_array() {
                    for tc in arr {
                        let id = tc["id"].as_str().unwrap_or("").to_string();
                        let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                        if !id.is_empty() {
                            id_to_name.insert(id, name);
                        }
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    for m in messages {
        match m.role.as_str() {
            "user" => {
                if let Value::Array(blocks) = &m.content {
                    let mut parts: Vec<Value> = blocks
                        .iter()
                        .filter_map(|b| {
                            if b.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                                let url = b["image_url"]["url"].as_str().unwrap_or("");
                                if let Some(rest) = url.strip_prefix("data:") {
                                    if let Some((mime, data)) = rest.split_once(";base64,") {
                                        return Some(json!({ "inlineData": { "mimeType": mime, "data": data } }));
                                    }
                                }
                                None
                            } else {
                                b["text"].as_str().map(|t| json!({ "text": t }))
                            }
                        })
                        .collect();
                    if parts.is_empty() {
                        parts.push(json!({ "text": "" }));
                    }
                    out.push(json!({ "role": "user", "parts": parts }));
                } else {
                    out.push(json!({ "role": "user", "parts": [{ "text": m.content }] }));
                }
            }
            "assistant" => {
                if let Some(tcs) = &m.tool_calls {
                    if let Some(arr) = tcs.as_array() {
                        let parts: Vec<Value> = arr
                            .iter()
                            .map(|tc| {
                                let name = tc["function"]["name"].as_str().unwrap_or("");
                                let args: Value = serde_json::from_str(
                                    tc["function"]["arguments"].as_str().unwrap_or("{}"),
                                )
                                .unwrap_or(json!({}));
                                let mut part =
                                    json!({ "functionCall": { "name": name, "args": args } });
                                // thoughtSignature must be at the part level, sibling of functionCall
                                if let Some(sig) = tc["thought_signature"].as_str() {
                                    part["thoughtSignature"] = json!(sig);
                                }
                                part
                            })
                            .collect();
                        out.push(json!({ "role": "model", "parts": parts }));
                        continue;
                    }
                }
                // Plain text assistant message
                let text = m.content.as_str().unwrap_or("");
                out.push(json!({ "role": "model", "parts": [{ "text": text }] }));
            }
            "tool" => {
                let call_id = m.tool_call_id.as_deref().unwrap_or("");
                let fn_name = id_to_name
                    .get(call_id)
                    .map(|s| s.as_str())
                    .unwrap_or(call_id);
                let result_text = m.content.as_str().unwrap_or("");
                out.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": fn_name,
                            "response": { "output": result_text }
                        }
                    }]
                }));
            }
            _ => {}
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat(
    client: &reqwest::Client,
    app: &AppHandle,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
    tools: &[ToolDef],
    reasoning_effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<StreamOutput> {
    let contents = to_gemini_contents(messages);

    let mut body = json!({ "contents": contents });

    if let Some(sp) = system_prompt {
        if !sp.is_empty() {
            body["systemInstruction"] = json!({ "parts": [{ "text": sp }] });
        }
    }

    if !tools.is_empty() {
        let fn_decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": strip_unsupported_schema_fields(t.input_schema.clone()),
                })
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": fn_decls }]);
    }

    if let Some(effort) = reasoning_effort {
        let budget: i64 = match effort {
            "low" => 1024,
            "medium" => 8192,
            "high" => 24576,
            _ => 0,
        };
        let include_thoughts = budget > 0;
        body["generationConfig"] = json!({
            "thinkingConfig": { "thinkingBudget": budget, "includeThoughts": include_thoughts }
        });
    }

    app.emit("stream_status", json!({ "label": "Waiting for response" }))?;

    let mut stream_req = client
        .post(format!(
            "{}/models/{}:streamGenerateContent",
            base_url, model
        ))
        .query(&[("alt", "sse")]);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        stream_req = stream_req.query(&[("key", key)]);
    }
    let resp = stream_req.json(&body).send().await?;

    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("Gemini error: {}", text));
    }

    app.emit("stream_status", json!({ "label": "Generating response" }))?;

    let mut reader = crate::streaming::SseLineReader::new(resp);
    let mut full_content = String::new();
    let mut full_thinking = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    while let Some(data) = reader.next_data().await? {
        if cancel.load(Ordering::Relaxed) {
            return Err(anyhow!("cancelled"));
        }
        if data == "[DONE]" {
            break;
        }

        let v: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Check for API error events in the stream
        if let Some(err) = v.get("error") {
            let msg = err["message"].as_str().unwrap_or("unknown error");
            return Err(anyhow!("Gemini stream error: {}", msg));
        }

        let parts = v["candidates"][0]["content"]["parts"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        for part in parts {
            if part
                .get("thought")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if let Some(t) = part["text"].as_str() {
                    full_thinking.push_str(t);
                    app.emit("stream_thinking", t)?;
                }
            } else if let Some(text) = part["text"].as_str() {
                full_content.push_str(text);
                app.emit("stream_token", text)?;
            } else if let Some(fc) = part.get("functionCall") {
                let name = fc["name"].as_str().unwrap_or("").to_string();
                let args = fc["args"].clone();
                // thoughtSignature is at the part level, not inside functionCall
                let thought_signature = part["thoughtSignature"].as_str().map(|s| s.to_string());
                let id = format!("gemini-{}-{}", name, tool_calls.len());
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments: args,
                    thought_signature,
                });
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
    })
}

pub async fn complete_chat(
    client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
) -> Result<String> {
    let contents = to_gemini_contents(messages);

    let mut body = json!({ "contents": contents });
    if let Some(sp) = system_prompt {
        if !sp.is_empty() {
            body["systemInstruction"] = json!({ "parts": [{ "text": sp }] });
        }
    }

    let model_path = model.strip_prefix("models/").unwrap_or(model);
    let url = format!(
        "{}/models/{}:generateContent",
        base_url.trim_end_matches('/'),
        model_path
    );
    let mut req = client.post(&url);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.query(&[("key", key)]);
    }
    let resp = req.json(&body).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await?;
        return Err(anyhow!("complete_chat error: {}", text));
    }
    let json: Value = resp.json().await?;
    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ChatMessage;

    #[test]
    fn to_gemini_contents_handles_array_user_content() {
        let msgs = vec![ChatMessage {
            role: "user".into(),
            content: serde_json::json!([
                {"type": "text", "text": "<file name=\"foo.txt\">\nhello\n</file>"},
                {"type": "text", "text": "summarise this"}
            ]),
            tool_call_id: None,
            tool_calls: None,
        }];
        let result = to_gemini_contents(msgs);
        assert_eq!(result.len(), 1);
        let parts = result[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[0]["text"].as_str().unwrap().contains("foo.txt"));
        assert_eq!(parts[1]["text"].as_str().unwrap(), "summarise this");
    }
}
