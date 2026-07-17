pub mod anthropic;
pub mod gemini;
pub mod reasoning_channel;
pub mod openai_compat;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::{atomic::AtomicBool, Arc};
use tauri::AppHandle;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
}

impl ChatMessage {
    pub fn text(role: &str, content: &str) -> Self {
        Self {
            role: role.into(),
            content: serde_json::Value::String(content.into()),
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

pub struct StreamOutput {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub thinking: Option<String>,
    /// The user pressed stop mid-stream. The other fields hold whatever had arrived by then and
    /// are kept: a stop must not throw away generated output. `tool_calls` may be incomplete, so
    /// callers must not execute them — see `run_generation_loop`.
    pub cancelled: bool,
}

/// What the provider's own API says about each of its models. Silent on a capability
/// means `None` — see `crate::caps`.
pub async fn list_model_capabilities(
    client: &reqwest::Client,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<std::collections::HashMap<String, crate::caps::PartialCaps>> {
    match provider_type {
        "anthropic" => anthropic::list_model_capabilities(client, base_url, api_key).await,
        "gemini" => gemini::list_model_capabilities(client, base_url, api_key).await,
        _ => openai_compat::list_model_capabilities(client, base_url, api_key).await,
    }
}

pub async fn raw_models_json(
    client: &reqwest::Client,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<String> {
    match provider_type {
        "anthropic" | "gemini" => Ok("{}".into()),
        _ => openai_compat::raw_models_json(client, base_url, api_key).await,
    }
}

pub async fn list_models(
    client: &reqwest::Client,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    match provider_type {
        "anthropic" => anthropic::list_models().await,
        "gemini" => gemini::list_models(client, base_url, api_key).await,
        // All non-anthropic, non-gemini providers (including openai_compat, openai, groq,
        // lmstudio, ollama) use the OpenAI-compatible chat completions API.
        _ => openai_compat::list_models(client, base_url, api_key).await,
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_chat(
    client: &reqwest::Client,
    app: &AppHandle,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
    tools: &[ToolDef],
    reasoning_effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
) -> Result<StreamOutput> {
    match provider_type {
        "anthropic" => {
            anthropic::stream_chat(
                client,
                app,
                api_key.unwrap_or(""),
                model,
                messages,
                system_prompt,
                tools,
                reasoning_effort,
                cancel,
            )
            .await
        }
        "gemini" => {
            gemini::stream_chat(
                client,
                app,
                base_url,
                api_key,
                model,
                messages,
                system_prompt,
                tools,
                reasoning_effort,
                cancel,
            )
            .await
        }
        _ => {
            // All non-anthropic, non-gemini providers (including openai_compat, openai, groq,
            // lmstudio, ollama) use the OpenAI-compatible chat completions API.
            let mut msgs = messages.clone();
            if let Some(sp) = system_prompt {
                if !sp.is_empty() {
                    msgs.insert(0, ChatMessage::text("system", sp));
                }
            }
            openai_compat::stream_chat(
                client,
                app,
                base_url,
                api_key,
                model,
                msgs,
                tools,
                reasoning_effort,
                cancel,
            )
            .await
        }
    }
}

pub async fn complete_chat(
    client: &reqwest::Client,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: Vec<ChatMessage>,
    system_prompt: Option<&str>,
) -> Result<String> {
    match provider_type {
        "anthropic" => {
            anthropic::complete_chat(
                client,
                api_key.unwrap_or(""),
                model,
                messages,
                system_prompt,
            )
            .await
        }
        "gemini" => {
            gemini::complete_chat(client, base_url, api_key, model, messages, system_prompt).await
        }
        _ => {
            let mut msgs = messages;
            if let Some(sp) = system_prompt {
                if !sp.is_empty() {
                    msgs.insert(0, ChatMessage::text("system", sp));
                }
            }
            openai_compat::complete_chat(client, base_url, api_key, model, msgs).await
        }
    }
}
