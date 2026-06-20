use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{
    conversations, mcp_servers as db_mcp, messages, model_overrides, providers, settings,
};

use crate::mcp::{types::McpServer, types::McpTool, McpManager};
use crate::providers as prov;
use crate::providers::ToolDef;
use crate::secrets::Secrets;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttachment {
    pub name: String,
    pub content: String,
    pub mime_type: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub conversation_id: String,
    pub content: String,
    pub disabled_tools: Option<Vec<String>>,
    pub reasoning_effort: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub attachments: Option<Vec<FileAttachment>>,
    pub skills_context: Option<String>,
    pub historical_attachments: Option<Vec<FileAttachment>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningInfo {
    pub allowed_options: Vec<String>,
    pub default: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub description: String,
}

pub struct AppState {
    pub conn: Mutex<rusqlite::Connection>,
    pub secrets: Secrets,
    pub mcp: Mutex<McpManager>,
    /// Holds the cancel flag for the currently running stream. None when idle.
    pub active_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub http_client: reqwest::Client,
    pub pending_permission: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
}

#[tauri::command]
pub fn list_conversations(
    state: State<AppState>,
) -> Result<Vec<conversations::Conversation>, String> {
    let conn = state.conn.lock().unwrap();
    conversations::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_conversation(
    state: State<AppState>,
    provider_id: String,
    model_id: String,
) -> Result<conversations::Conversation, String> {
    let conn = state.conn.lock().unwrap();
    conversations::create(&conn, &provider_id, &model_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_conversation(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    conversations::delete(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_conversation_title(
    state: State<AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    conversations::update_title(&conn, &id, &title).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_messages(
    state: State<AppState>,
    conversation_id: String,
) -> Result<Vec<messages::Message>, String> {
    let conn = state.conn.lock().unwrap();
    messages::list(&conn, &conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_providers(state: State<AppState>) -> Result<Vec<providers::Provider>, String> {
    let conn = state.conn.lock().unwrap();
    providers::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_provider(
    state: State<AppState>,
    provider: providers::Provider,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    providers::upsert(&conn, &provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_provider(state: State<AppState>, id: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    let provider = providers::find_by_id(&conn, &id).map_err(|e| e.to_string())?;
    providers::delete(&conn, &id).map_err(|e| e.to_string())?;
    drop(conn);
    if let Some(p) = provider {
        if let Some(key_ref) = p.api_key_ref {
            let _ = state.secrets.delete(&key_ref);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<settings::AppSettings, String> {
    let conn = state.conn.lock().unwrap();
    settings::get_all(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    // value arrives JSON-encoded from the frontend (e.g. `"\"hello\""` for the string `hello`).
    let decoded = serde_json::from_str::<serde_json::Value>(&value)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or(value);
    settings::set(&conn, &key, &decoded).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_secret(state: State<AppState>, key: String) -> Result<Option<String>, String> {
    state.secrets.get(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_secret(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    state.secrets.set(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_conversations(
    state: State<AppState>,
    query: String,
) -> Result<Vec<messages::SearchResult>, String> {
    let conn = state.conn.lock().unwrap();
    messages::search(&conn, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let (provider_type, base_url, api_key_ref) = {
        let conn = state.conn.lock().unwrap();
        let p = providers::find_by_id(&conn, &provider_id)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        (p.r#type, p.base_url, p.api_key_ref)
    };
    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());
    prov::list_models(
        &state.http_client,
        &provider_type,
        &base_url,
        api_key.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn raw_provider_models_json(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<String, String> {
    let (provider_type, base_url, api_key_ref) = {
        let conn = state.conn.lock().unwrap();
        let p = providers::find_by_id(&conn, &provider_id)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        (p.r#type, p.base_url, p.api_key_ref)
    };
    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());
    prov::raw_models_json(
        &state.http_client,
        &provider_type,
        &base_url,
        api_key.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_model_capabilities(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<std::collections::HashMap<String, prov::openai_compat::ModelCaps>, String> {
    let (provider_type, base_url, api_key_ref) = {
        let conn = state.conn.lock().unwrap();
        let p = providers::find_by_id(&conn, &provider_id)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        (p.r#type, p.base_url, p.api_key_ref)
    };
    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());
    prov::list_model_capabilities(
        &state.http_client,
        &provider_type,
        &base_url,
        api_key.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_model_reasoning(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
) -> Result<Option<ReasoningInfo>, String> {
    let (provider_type, base_url, api_key_ref) = {
        let conn = state.conn.lock().unwrap();
        let p = providers::find_by_id(&conn, &provider_id)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        (p.r#type, p.base_url, p.api_key_ref)
    };

    match provider_type.as_str() {
        "anthropic" => Ok(Some(ReasoningInfo {
            allowed_options: vec!["off".into(), "on".into()],
            default: "off".into(),
        })),
        "gemini" => {
            // Only 2.5+ models support thinking; older models silently ignore thinkingConfig
            let supports_thinking = model_id.contains("2.5") || model_id.contains("2-5");
            if supports_thinking {
                Ok(Some(ReasoningInfo {
                    allowed_options: vec![
                        "off".into(),
                        "low".into(),
                        "medium".into(),
                        "high".into(),
                    ],
                    default: "off".into(),
                }))
            } else {
                Ok(None)
            }
        }
        "openai_compat" => {
            let api_key = api_key_ref
                .as_deref()
                .and_then(|r| state.secrets.get(r).ok().flatten());
            // Try LM Studio native API first — it reports per-model reasoning capabilities
            let native_url = base_url
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string();
            let mut req = state
                .http_client
                .get(format!("{}/api/v1/models", native_url));
            if let Some(key) = api_key.as_deref() {
                if !key.is_empty() {
                    req = req.bearer_auth(key);
                }
            }
            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        // LM Studio native API: "models" array, each entry has "key" and capabilities.reasoning
                        if let Some(models) = json["models"].as_array() {
                            let model_entry =
                                models.iter().find(|m| m["key"].as_str() == Some(&model_id));
                            if let Some(reasoning) =
                                model_entry.and_then(|m| m["capabilities"].get("reasoning"))
                            {
                                let allowed: Vec<String> = reasoning["allowed_options"]
                                    .as_array()
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let default =
                                    reasoning["default"].as_str().unwrap_or("off").to_string();
                                if !allowed.is_empty() {
                                    return Ok(Some(ReasoningInfo {
                                        allowed_options: allowed,
                                        default,
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            // Fallback: always offer on/off for openai_compat so users can try thinking on any model
            Ok(Some(ReasoningInfo {
                allowed_options: vec!["on".into(), "off".into()],
                default: "off".into(),
            }))
        }
        _ => Ok(None),
    }
}

/// Build the API message array from DB messages, reconstructing tool call/result pairs.
///
/// NOTE: This function currently returns ALL messages without trimming.
/// The `context_window_limit` setting is stored in AppSettings but not yet enforced here.
/// For long conversations, the model API will return a context-length error.
/// Future fix: trim oldest messages (keeping at minimum the last user+assistant pair)
/// until the estimated token count fits within the configured limit.
fn extract_artifact_identifiers(content: &str) -> Vec<(String, String)> {
    use std::sync::OnceLock;
    static RE_ARTIFACT: OnceLock<regex::Regex> = OnceLock::new();
    static RE_ID: OnceLock<regex::Regex> = OnceLock::new();
    static RE_TITLE: OnceLock<regex::Regex> = OnceLock::new();
    let re_artifact = RE_ARTIFACT
        .get_or_init(|| regex::Regex::new(r#"(?s)<artifact\s([^>]*)>.*?</artifact>"#).unwrap());
    let re_id = RE_ID.get_or_init(|| regex::Regex::new(r#"identifier="([^"]*)""#).unwrap());
    let re_title = RE_TITLE.get_or_init(|| regex::Regex::new(r#"title="([^"]*)""#).unwrap());
    let mut results = Vec::new();
    for cap in re_artifact.captures_iter(content) {
        let full = cap[0].to_string();
        let attrs = &cap[1];
        let key = if let Some(m) = re_id.captures(attrs) {
            m[1].to_string()
        } else if let Some(m) = re_title.captures(attrs) {
            m[1].to_lowercase()
        } else {
            continue;
        };
        results.push((key, full));
    }
    results
}

fn collect_active_artifact_ids(db_msgs: &[messages::Message]) -> Vec<(String, String)> {
    use std::sync::OnceLock;
    static RE_ARTIFACT: OnceLock<regex::Regex> = OnceLock::new();
    static RE_ID: OnceLock<regex::Regex> = OnceLock::new();
    static RE_TITLE: OnceLock<regex::Regex> = OnceLock::new();
    let re_artifact =
        RE_ARTIFACT.get_or_init(|| regex::Regex::new(r#"(?s)<artifact\s([^>]*)>"#).unwrap());
    let re_id = RE_ID.get_or_init(|| regex::Regex::new(r#"identifier="([^"]*)""#).unwrap());
    let re_title = RE_TITLE.get_or_init(|| regex::Regex::new(r#"title="([^"]*)""#).unwrap());

    let mut latest: HashMap<String, (usize, String, String)> = HashMap::new();
    for (i, m) in db_msgs.iter().enumerate() {
        if m.role != "assistant" {
            continue;
        }
        for cap in re_artifact.captures_iter(&m.content) {
            let attrs = &cap[1];
            if let (Some(id_m), Some(title_m)) = (re_id.captures(attrs), re_title.captures(attrs)) {
                let id = id_m[1].to_string();
                let title = title_m[1].to_string();
                latest.insert(id.clone(), (i, title, id));
            }
        }
    }
    latest
        .into_values()
        .map(|(_, title, id)| (title, id))
        .collect()
}

fn build_api_messages(db_msgs: &[messages::Message]) -> Vec<prov::ChatMessage> {
    // First pass: find the index of the last message that contains each artifact identifier.
    let mut latest_msg_for: HashMap<String, usize> = HashMap::new();
    for (i, m) in db_msgs.iter().enumerate() {
        if m.role != "assistant" {
            continue;
        }
        for (key, _) in extract_artifact_identifiers(&m.content) {
            latest_msg_for.insert(key, i);
        }
    }

    let mut out = Vec::new();
    for (i, m) in db_msgs.iter().enumerate() {
        match m.role.as_str() {
            "user" => out.push(prov::ChatMessage::text("user", &m.content)),
            "assistant" => {
                if let Ok(v) = serde_json::from_str::<Value>(&m.content) {
                    if let Some(tool_calls) = v.get("__tool_calls__") {
                        out.push(prov::ChatMessage {
                            role: "assistant".into(),
                            content: Value::Null,
                            tool_calls: Some(tool_calls.clone()),
                            tool_call_id: None,
                        });
                        continue;
                    }
                }
                // Strip artifact blocks superseded by a later message.
                let mut content = m.content.clone();
                for (key, full_match) in extract_artifact_identifiers(&m.content) {
                    if latest_msg_for.get(&key).copied().unwrap_or(i) != i {
                        content = content.replace(&full_match, "");
                    }
                }
                let content = content.trim().to_string();
                if !content.is_empty() {
                    out.push(prov::ChatMessage::text("assistant", &content));
                }
            }
            "tool" => {
                out.push(prov::ChatMessage {
                    role: "tool".into(),
                    content: Value::String(m.content.clone()),
                    tool_call_id: m.tool_call_id.clone(),
                    tool_calls: None,
                });
            }
            _ => {}
        }
    }
    out
}

#[tauri::command]
pub fn cancel_stream(state: State<AppState>) {
    if let Some(flag) = state.active_cancel.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    if let Some(tx) = state.pending_permission.lock().unwrap().take() {
        let _ = tx.send(false);
    }
}

#[tauri::command]
pub fn respond_to_permission(state: State<AppState>, approved: bool) {
    if let Some(tx) = state.pending_permission.lock().unwrap().take() {
        let _ = tx.send(approved);
    }
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SendMessageRequest,
) -> Result<(), String> {
    let SendMessageRequest {
        conversation_id,
        content,
        disabled_tools,
        reasoning_effort,
        provider_id: req_provider_id,
        model_id: req_model_id,
        attachments,
        skills_context,
        historical_attachments,
    } = req;

    // Build content-block override for the provider call when files are attached.
    // The DB always stores plain text; file content is ephemeral.
    #[cfg(debug_assertions)]
    if let Some(ref atts) = attachments {
        eprintln!("[demido] send_message: {} attachment(s)", atts.len());
        for a in atts {
            eprintln!(
                "[demido]   attachment: name={} mime={:?} content_len={}",
                a.name,
                a.mime_type,
                a.content.len()
            );
        }
    }
    let first_user_content: Option<serde_json::Value> = attachments
        .filter(|a| !a.is_empty())
        .map(|atts| {
            let mut blocks: Vec<serde_json::Value> = atts
                .iter()
                .map(|a| {
                    if a.mime_type.as_deref().map(|m| m.starts_with("image/")).unwrap_or(false) {
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": { "url": &a.content }
                        })
                    } else {
                        serde_json::json!({
                            "type": "text",
                            "text": format!("<file name=\"{}\">\n{}\n</file>", xml_escape(&a.name), xml_escape(&a.content))
                        })
                    }
                })
                .collect();
            blocks.push(serde_json::json!({ "type": "text", "text": &content }));
            serde_json::Value::Array(blocks)
        });

    // Emit stream_status FIRST so frontend opens the stream gate
    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Processing prompt" }),
    )
    .map_err(|e| e.to_string())?;

    // Persist user message and emit it
    let saved_user_msg = {
        let conn = state.conn.lock().unwrap();
        let msg = messages::insert(&conn, &conversation_id, "user", &content, None, None)
            .map_err(|e| e.to_string())?;
        conversations::touch(&conn, &conversation_id).map_err(|e| e.to_string())?;
        msg
    };
    app.emit("user_message", &saved_user_msg)
        .map_err(|e| e.to_string())?;

    // Collect everything needed before any async work (drop the lock)
    let (sys_prompt, provider_type, base_url, api_key_ref, model_id, agent_mode, working_directory) = {
        let conn = state.conn.lock().unwrap();
        let s = settings::get_all(&conn).map_err(|e| e.to_string())?;
        let conv = conversations::find_by_id(&conn, &conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or("Conversation not found")?;
        let provider_id = req_provider_id.unwrap_or(conv.provider_id);
        let model_id = req_model_id.unwrap_or(conv.model_id);
        let provider = providers::find_by_id(&conn, &provider_id)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        (
            s.system_prompt,
            provider.r#type,
            provider.base_url,
            provider.api_key_ref,
            model_id,
            conv.agent_mode,
            conv.working_directory,
        )
    };

    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());

    let disabled = disabled_tools.unwrap_or_default();
    let tools: Vec<ToolDef> = {
        let mcp = state.mcp.lock().unwrap();
        mcp.list_tools()
            .into_iter()
            .filter(|t| {
                let key = format!("{}:{}", t.server_id, t.name);
                !disabled.contains(&key)
            })
            .map(|t| ToolDef {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect()
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));
    *state.active_cancel.lock().unwrap() = Some(Arc::clone(&cancel_flag));

    let effective_prompt = match skills_context.filter(|s| !s.is_empty()) {
        Some(ctx) if !sys_prompt.is_empty() => format!("{}\n\n{}", sys_prompt, ctx),
        Some(ctx) => ctx,
        None => sys_prompt,
    };

    // Build image blocks for the FIRST user message in history (from a prior turn's attachment).
    // This keeps images in context across follow-up messages without re-attaching.
    let historical_first_content: Option<serde_json::Value> = historical_attachments
        .filter(|a| !a.is_empty())
        .map(|atts| {
            // placeholder — actual text is injected per-iteration from DB content
            let blocks: Vec<serde_json::Value> = atts.iter().filter_map(|a| {
                if a.mime_type.as_deref().map(|m| m.starts_with("image/")).unwrap_or(false) {
                    Some(serde_json::json!({ "type": "image_url", "image_url": { "url": &a.content } }))
                } else { None }
            }).collect();
            serde_json::Value::Array(blocks)
        });

    let result = run_generation_loop(
        &state.http_client,
        &app,
        &state,
        &conversation_id,
        &provider_type,
        &base_url,
        api_key.as_deref(),
        &model_id,
        &effective_prompt,
        &tools,
        reasoning_effort.as_deref(),
        &cancel_flag,
        &agent_mode,
        working_directory.as_deref(),
        first_user_content,
        historical_first_content,
        None,
    )
    .await;
    *state.active_cancel.lock().unwrap() = None;
    result
}

async fn maybe_generate_title(
    client: &reqwest::Client,
    state: &AppState,
    conversation_id: &str,
    app: &AppHandle,
) {
    let (provider_type, base_url, api_key_ref, model_id, db_msgs) = {
        let conn = state.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?1 AND role = 'assistant' AND content NOT LIKE '{\"__tool_calls__%'",
            [conversation_id],
            |r| r.get(0),
        ).unwrap_or(0);
        let s = match settings::get_all(&conn) {
            Ok(s) => s,
            Err(_) => return,
        };
        let n: i64 = s.title_every_n_messages;
        let should_run = count == 1 || (n > 0 && count % n == 0);
        if !should_run {
            return;
        }
        let conv = match conversations::find_by_id(&conn, conversation_id) {
            Ok(Some(c)) => c,
            _ => return,
        };
        let (pid, mid) = if !s.task_provider_id.is_empty() && !s.task_model_id.is_empty() {
            (s.task_provider_id, s.task_model_id)
        } else {
            (conv.provider_id, conv.model_id)
        };
        let provider = match providers::find_by_id(&conn, &pid) {
            Ok(Some(p)) => p,
            _ => return,
        };
        let db_msgs = match messages::list(&conn, conversation_id) {
            Ok(m) => m,
            Err(_) => return,
        };
        (
            provider.r#type,
            provider.base_url,
            provider.api_key_ref,
            mid,
            db_msgs,
        )
    };

    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());

    let mut chat_messages = build_api_messages(&db_msgs);
    chat_messages.push(prov::ChatMessage::text(
        "user",
        "Summarise this conversation as a short title (5 words or fewer). \
         Output only the title text — no markdown, no bullet points, no bold, no italics, \
         no hyphens, no dashes, no punctuation, no quotes. Plain words only.",
    ));

    match prov::complete_chat(
        client,
        &provider_type,
        &base_url,
        api_key.as_deref(),
        &model_id,
        chat_messages,
        None,
    )
    .await
    {
        Ok(raw) if !raw.is_empty() => {
            // Strip markdown defensively: remove leading #/*/- chars and trim whitespace.
            let title = raw
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or(&raw)
                .trim_matches(|c: char| {
                    c == '#' || c == '*' || c == '_' || c == '-' || c == '`' || c.is_whitespace()
                })
                .to_string();
            if title.is_empty() {
                return;
            }
            let conn = state.conn.lock().unwrap();
            let _ = conversations::update_title(&conn, conversation_id, &title);
            drop(conn);
            let _ = app.emit(
                "conversation_title_updated",
                serde_json::json!({
                    "id": conversation_id,
                    "title": title,
                }),
            );
        }
        _ => {}
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn format_tool_description(name: &str, args: &Value) -> String {
    match name {
        "read_file" => format!("Read file: {}", args["path"].as_str().unwrap_or("?")),
        "write_file" => format!("Write file: {}", args["path"].as_str().unwrap_or("?")),
        "edit_file" => format!("Edit file: {}", args["path"].as_str().unwrap_or("?")),
        "list_dir" => format!("List directory: {}", args["path"].as_str().unwrap_or(".")),
        "run_command" => format!("Run command: {}", args["command"].as_str().unwrap_or("?")),
        "search_files" => format!(
            "Search files: {} in {}",
            args["pattern"].as_str().unwrap_or("?"),
            args["path"].as_str().unwrap_or(".")
        ),
        _ => name.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_generation_loop(
    client: &reqwest::Client,
    app: &AppHandle,
    state: &AppState,
    conversation_id: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
    model_id: &str,
    sys_prompt: &str,
    mcp_tools: &[ToolDef],
    reasoning_effort: Option<&str>,
    cancel: &Arc<AtomicBool>,
    agent_mode: &str,
    working_directory: Option<&str>,
    first_user_content: Option<serde_json::Value>,
    historical_first_content: Option<serde_json::Value>,
    continuation_hint: Option<&str>,
) -> Result<(), String> {
    let mut first_iteration = true; // tracks first vs subsequent agent loop iterations

    loop {
        let db_msgs = {
            let conn = state.conn.lock().unwrap();
            messages::list(&conn, conversation_id).map_err(|e| e.to_string())?
        };
        let mut chat_messages = build_api_messages(&db_msgs);

        // Append active artifact identifiers to system prompt so weak models can't miss them.
        let loop_sys_prompt: String = {
            let active = collect_active_artifact_ids(&db_msgs);
            if active.is_empty() {
                sys_prompt.to_string()
            } else {
                let list = active
                    .iter()
                    .map(|(title, id)| format!("  - title=\"{}\" identifier=\"{}\"", title, id))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n\nACTIVE ARTIFACTS IN THIS CONVERSATION — when modifying any of these, use the EXACT same identifier and title:\n{}", sys_prompt, list)
            }
        };

        // Inject historical image blocks into the FIRST user message so images stay in
        // context across follow-up turns (image is ephemeral, not stored in DB).
        if let Some(ref hist_blocks) = historical_first_content {
            if let Some(pos) = chat_messages.iter().position(|m| m.role == "user") {
                let text = chat_messages[pos]
                    .content
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let mut blocks = hist_blocks.as_array().cloned().unwrap_or_default();
                blocks.push(serde_json::json!({ "type": "text", "text": text }));
                chat_messages[pos].content = serde_json::Value::Array(blocks);
            }
        }

        // Re-apply attachment content blocks on every iteration so images survive
        // multi-turn tool-calling loops (the image is ephemeral and not stored in DB).
        if let Some(ref ov) = first_user_content {
            if let Some(pos) = chat_messages.iter().rposition(|m| m.role == "user") {
                chat_messages[pos].content = ov.clone();
            }
        }
        if first_iteration {
            first_iteration = false;
            if let Some(hint) = continuation_hint {
                chat_messages.push(prov::ChatMessage::text("user", hint));
            }
        }

        // Only emit the "tool results" label when the current turn has pending tool results.
        // Checking after the last user message avoids false-positives for conversations that
        // previously used tools.
        let last_user_pos = chat_messages.iter().rposition(|m| m.role == "user");
        let has_pending_tool_results = last_user_pos
            .map(|pos| chat_messages[pos..].iter().any(|m| m.role == "tool"))
            .unwrap_or(false);
        if has_pending_tool_results {
            app.emit(
                "stream_status",
                serde_json::json!({ "label": "Processing tool results" }),
            )
            .map_err(|e| e.to_string())?;
        }

        let all_tools: Vec<ToolDef> = if agent_mode != "off" {
            let mut v = mcp_tools.to_vec();
            v.extend(crate::agent::builtin_tool_defs());
            v
        } else {
            mcp_tools.to_vec()
        };

        #[cfg(debug_assertions)]
        {
            eprintln!(
                "[demido:context] === LLM CONTEXT ({} messages) ===",
                chat_messages.len()
            );
            eprintln!(
                "[demido:context] sys_prompt: {}",
                if sys_prompt.is_empty() {
                    "(none)"
                } else {
                    sys_prompt
                }
            );
            for (i, msg) in chat_messages.iter().enumerate() {
                let preview = match msg.content.as_str() {
                    Some(s) => {
                        let trimmed = s.trim();
                        if trimmed.len() > 300 {
                            format!("{}…", &trimmed[..300])
                        } else {
                            trimmed.to_string()
                        }
                    }
                    None => serde_json::to_string(&msg.content).unwrap_or_default(),
                };
                eprintln!(
                    "[demido:context] [{}] role={} content={:?}",
                    i, msg.role, preview
                );
            }
            eprintln!("[demido:context] ==================");
        }

        let output = prov::stream_chat(
            client,
            app,
            provider_type,
            base_url,
            api_key,
            model_id,
            chat_messages,
            Some(&loop_sys_prompt),
            &all_tools,
            reasoning_effort,
            cancel,
        )
        .await;

        let output = match output {
            Ok(o) => o,
            Err(e) if e.to_string() == "cancelled" => {
                let _ = app.emit("stream_status", serde_json::json!({ "label": null }));
                let _ = app.emit("stream_cancelled", ());
                return Ok(());
            }
            Err(e) => {
                let _ = app.emit("stream_status", serde_json::json!({ "label": null }));
                return Err(e.to_string());
            }
        };

        if output.tool_calls.is_empty() {
            if continuation_hint.is_some() {
                // Find the last assistant message and append to it instead of inserting.
                let updated = {
                    let conn = state.conn.lock().unwrap();
                    let msgs = messages::list(&conn, conversation_id).map_err(|e| e.to_string())?;
                    let last = msgs.into_iter().rev().find(|m| m.role == "assistant");
                    match last {
                        Some(mut msg) if !output.content.trim().is_empty() => {
                            msg.content = format!("{}{}", msg.content, output.content);
                            messages::update_content(&conn, &msg.id, &msg.content)
                                .map_err(|e| e.to_string())?;
                            Some(msg)
                        }
                        Some(msg) => Some(msg), // empty response — return unchanged
                        None => None,
                    }
                };
                if let Some(msg) = updated {
                    app.emit("stream_continue_done", &msg)
                        .map_err(|e| e.to_string())?;
                } else {
                    let _ = app.emit("stream_cancelled", ());
                }
                return Ok(());
            }
            let saved = {
                let conn = state.conn.lock().unwrap();
                let msg = messages::insert(
                    &conn,
                    conversation_id,
                    "assistant",
                    &output.content,
                    None,
                    output.thinking.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                msg
            };
            app.emit("stream_done", &saved).map_err(|e| e.to_string())?;
            maybe_generate_title(client, state, conversation_id, app).await;
            return Ok(());
        }

        // Model made tool calls — execute each one and loop back
        let tool_calls_json: Vec<Value> = output
            .tool_calls
            .iter()
            .map(|tc| {
                let mut obj = json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments.to_string() }
                });
                if let Some(sig) = &tc.thought_signature {
                    obj["thought_signature"] = json!(sig);
                }
                obj
            })
            .collect();
        {
            let conn = state.conn.lock().unwrap();
            // The "__tool_calls__" key is a sentinel: frontend filters out messages
            // starting with this prefix to hide them from the chat UI.
            // See src/lib/constants.ts — keep these in sync.
            let content = json!({ "__tool_calls__": tool_calls_json }).to_string();
            messages::insert(&conn, conversation_id, "assistant", &content, None, None)
                .map_err(|e| e.to_string())?;
        }

        for tc in &output.tool_calls {
            app.emit(
                "tool_call",
                json!({ "name": tc.name, "id": tc.id, "args": tc.arguments }),
            )
            .map_err(|e| e.to_string())?;

            let result_content = if crate::agent::is_builtin(&tc.name) {
                use crate::agent::permissions::{is_permitted, PermissionResult};
                const MUTATING_TOOLS: &[&str] = &["write_file", "edit_file", "run_command"];
                if working_directory.is_none() && MUTATING_TOOLS.contains(&tc.name.as_str()) {
                    "No working folder set. Set a working directory before using file or command tools.".to_string()
                } else {
                    let approved = match is_permitted(agent_mode, &tc.name, &tc.arguments) {
                        PermissionResult::Allow => true,
                        PermissionResult::Ask => {
                            let req = PermissionRequest {
                                tool_name: tc.name.clone(),
                                args: tc.arguments.clone(),
                                description: format_tool_description(&tc.name, &tc.arguments),
                            };
                            app.emit("tool_permission_request", &req)
                                .map_err(|e| e.to_string())?;
                            let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
                            *state.pending_permission.lock().unwrap() = Some(tx);
                            rx.await.unwrap_or(false)
                        }
                    };
                    if cancel.load(Ordering::Relaxed) {
                        let _ = app.emit("stream_status", serde_json::json!({ "label": null }));
                        let _ = app.emit("stream_cancelled", ());
                        return Ok(());
                    }
                    if approved {
                        let tool_name = tc.name.clone();
                        let tool_args = tc.arguments.clone();
                        let wd = working_directory.map(String::from);
                        tokio::task::spawn_blocking(move || {
                            crate::agent::executor::execute_tool(
                                &tool_name,
                                &tool_args,
                                wd.as_deref(),
                            )
                        })
                        .await
                        .unwrap_or_else(|e| format!("Tool panic: {}", e))
                    } else {
                        "Permission denied by user. Do not retry this action using alternative commands or methods.".to_string()
                    }
                } // end else (working_directory is some)
            } else {
                let server_id = {
                    let mcp = state.mcp.lock().unwrap();
                    mcp.list_tools()
                        .into_iter()
                        .find(|t| t.name == tc.name)
                        .map(|t| t.server_id)
                        .unwrap_or_default()
                };
                if server_id.is_empty() {
                    format!("Error: no MCP server found for tool '{}'", tc.name)
                } else {
                    let client_arc = {
                        let mcp = state.mcp.lock().unwrap();
                        mcp.get_stdio_client(&server_id)
                    };
                    match client_arc {
                        Some(client) => {
                            let tool_name = tc.name.clone();
                            let tool_args = tc.arguments.clone();
                            tokio::task::spawn_blocking(move || {
                                client.call_tool(&tool_name, tool_args)
                            })
                            .await
                            .map_err(|e| e.to_string())?
                            .map(|v| v.to_string())
                            .unwrap_or_else(|e| format!("Tool error: {}", e))
                        }
                        None => format!("Error: MCP server '{}' not connected", server_id),
                    }
                }
            };

            app.emit(
                "tool_call_result",
                json!({ "id": tc.id, "name": tc.name, "result": result_content }),
            )
            .map_err(|e| e.to_string())?;
            {
                let conn = state.conn.lock().unwrap();
                messages::insert(
                    &conn,
                    conversation_id,
                    "tool",
                    &result_content,
                    Some(&tc.id),
                    None,
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn continue_generation(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    provider_id: Option<String>,
    model_id: Option<String>,
    disabled_tools: Option<Vec<String>>,
    reasoning_effort: Option<String>,
    skills_context: Option<String>,
) -> Result<(), String> {
    let (
        sys_prompt,
        provider_type,
        base_url,
        api_key_ref,
        resolved_model_id,
        agent_mode,
        working_directory,
        last_assistant_tail,
    ) = {
        let conn = state.conn.lock().unwrap();
        let s = settings::get_all(&conn).map_err(|e| e.to_string())?;
        let conv = conversations::find_by_id(&conn, &conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or("Conversation not found")?;
        let pid = provider_id.unwrap_or(conv.provider_id);
        let mid = model_id.unwrap_or(conv.model_id);
        let provider = providers::find_by_id(&conn, &pid)
            .map_err(|e| e.to_string())?
            .ok_or("Provider not found")?;
        let msgs = messages::list(&conn, &conversation_id).map_err(|e| e.to_string())?;
        let tail = msgs
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .map(|m| {
                let content = m.content.trim();
                // Take last 200 chars as the anchor so the model knows where to continue from
                let start = content
                    .char_indices()
                    .rev()
                    .nth(199)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                content[start..].trim().to_string()
            })
            .unwrap_or_default();
        (
            s.system_prompt,
            provider.r#type,
            provider.base_url,
            provider.api_key_ref,
            mid,
            conv.agent_mode,
            conv.working_directory,
            tail,
        )
    };

    let api_key = api_key_ref
        .as_deref()
        .and_then(|r| state.secrets.get(r).ok().flatten());

    app.emit(
        "stream_status",
        serde_json::json!({ "label": "Continuing response" }),
    )
    .map_err(|e| e.to_string())?;

    let disabled = disabled_tools.unwrap_or_default();
    let tools: Vec<ToolDef> = {
        let mcp = state.mcp.lock().unwrap();
        mcp.list_tools()
            .into_iter()
            .filter(|t| {
                let key = format!("{}:{}", t.server_id, t.name);
                !disabled.contains(&key)
            })
            .map(|t| ToolDef {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect()
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));
    *state.active_cancel.lock().unwrap() = Some(Arc::clone(&cancel_flag));

    let effective_prompt = match skills_context.filter(|s| !s.is_empty()) {
        Some(ctx) if !sys_prompt.is_empty() => format!("{}\n\n{}", sys_prompt, ctx),
        Some(ctx) => ctx,
        None => sys_prompt,
    };

    let continuation_hint = if last_assistant_tail.is_empty() {
        "Please continue your previous response.".to_string()
    } else {
        format!(
            "Please continue your previous response. Your last words were: \"{}\". \
             Output only new content that naturally follows, without repeating anything already written.",
            last_assistant_tail
        )
    };

    let result = run_generation_loop(
        &state.http_client,
        &app,
        &state,
        &conversation_id,
        &provider_type,
        &base_url,
        api_key.as_deref(),
        &resolved_model_id,
        &effective_prompt,
        &tools,
        reasoning_effort.as_deref(),
        &cancel_flag,
        &agent_mode,
        working_directory.as_deref(),
        None,
        None,
        Some(&continuation_hint),
    )
    .await;
    *state.active_cancel.lock().unwrap() = None;
    if result.is_ok() {
        let conn = state.conn.lock().unwrap();
        let _ = conversations::touch(&conn, &conversation_id);
    }
    result
}

#[tauri::command]
pub fn list_mcp_servers(state: State<AppState>) -> Result<Vec<McpServer>, String> {
    let conn = state.conn.lock().unwrap();
    db_mcp::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_mcp_servers(state: State<AppState>, servers: Vec<McpServer>) -> Result<(), String> {
    {
        let conn = state.conn.lock().unwrap();
        db_mcp::save_all(&conn, &servers).map_err(|e| e.to_string())?;
    }
    let mut mcp = state.mcp.lock().unwrap();
    mcp.load_servers(servers).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_mcp_tools(state: State<AppState>) -> Result<Vec<McpTool>, String> {
    let mcp = state.mcp.lock().unwrap();
    Ok(mcp.list_tools())
}

#[tauri::command]
pub fn test_mcp_server(server: McpServer) -> Result<usize, String> {
    if server.transport != "stdio" {
        return Err("Only stdio transport is supported for testing".into());
    }
    let cmd = server.command.as_deref().ok_or("Missing command")?;
    let args = server.args.as_deref().unwrap_or(&[]);
    let client = crate::mcp::stdio::StdioClient::spawn(cmd, args, server.env.as_ref())
        .map_err(|e| e.to_string())?;
    client.initialize().map_err(|e| e.to_string())?;
    let tools = client.list_tools().map_err(|e| e.to_string())?;
    Ok(tools.len())
}

#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    provider_type: String,
    base_url: String,
    api_key_ref: Option<String>,
    api_key_override: Option<String>,
) -> Result<usize, String> {
    let api_key = if api_key_override
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        api_key_override
    } else {
        api_key_ref
            .as_deref()
            .and_then(|r| state.secrets.get(r).ok().flatten())
    };
    let models = prov::list_models(
        &state.http_client,
        &provider_type,
        &base_url,
        api_key.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(models.len())
}

#[tauri::command]
pub fn list_model_overrides(
    state: State<AppState>,
    provider_id: String,
) -> Result<Vec<model_overrides::ModelOverride>, String> {
    let conn = state.conn.lock().unwrap();
    model_overrides::list(&conn, &provider_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_model_override(
    state: State<AppState>,
    override_entry: model_overrides::ModelOverride,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    model_overrides::upsert(&conn, &override_entry).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn batch_upsert_model_overrides(
    state: State<AppState>,
    overrides: Vec<model_overrides::ModelOverride>,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    model_overrides::batch_upsert(&conn, &overrides).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_messages_after(state: State<AppState>, message_id: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    messages::delete_after(&conn, &message_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_message_content(
    state: State<AppState>,
    message_id: String,
    content: String,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    messages::update_content(&conn, &message_id, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_messages_from(state: State<AppState>, message_id: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    messages::delete_from(&conn, &message_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_message(state: State<AppState>, message_id: String) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    messages::delete_one(&conn, &message_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_agent_mode(
    state: State<AppState>,
    conversation_id: String,
    mode: String,
) -> Result<(), String> {
    let valid = ["off", "cautious", "balanced", "autonomous"];
    if !valid.contains(&mode.as_str()) {
        return Err(format!("Invalid agent mode: {}", mode));
    }
    let conn = state.conn.lock().unwrap();
    conversations::set_agent_mode(&conn, &conversation_id, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_working_directory(
    state: State<AppState>,
    conversation_id: String,
    path: Option<String>,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();
    conversations::set_working_directory(&conn, &conversation_id, path.as_deref())
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ExportToolCall>>,
    created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationExport {
    version: String,
    exported_at: String,
    conversation: conversations::Conversation,
    provider_name: String,
    messages: Vec<ExportMessage>,
}

#[tauri::command]
pub fn export_conversation(
    state: State<AppState>,
    conversation_id: String,
    file_path: String,
) -> Result<(), String> {
    let conn = state.conn.lock().unwrap();

    let conv = conversations::find_by_id(&conn, &conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or("Conversation not found")?;

    let provider_name = providers::find_by_id(&conn, &conv.provider_id)
        .ok()
        .flatten()
        .map(|p| p.name)
        .unwrap_or_default();

    let db_msgs = messages::list(&conn, &conversation_id).map_err(|e| e.to_string())?;

    // Build tool results lookup
    let mut tool_results: HashMap<String, String> = HashMap::new();
    for m in &db_msgs {
        if m.role == "tool" {
            if let Some(ref id) = m.tool_call_id {
                tool_results.insert(id.clone(), m.content.clone());
            }
        }
    }

    // Build export messages, reconstructing tool calls into their parent assistant message
    let mut export_msgs: Vec<ExportMessage> = Vec::new();
    for m in &db_msgs {
        match m.role.as_str() {
            "assistant" => {
                if let Ok(v) = serde_json::from_str::<Value>(&m.content) {
                    if let Some(tc_array) = v.get("__tool_calls__").and_then(|v| v.as_array()) {
                        let tool_calls: Vec<ExportToolCall> = tc_array
                            .iter()
                            .filter_map(|tc| {
                                let id = tc["id"].as_str()?.to_string();
                                let name = tc["function"]["name"].as_str()?.to_string();
                                let args_str = tc["function"]["arguments"].as_str()?;
                                let arguments: Value = serde_json::from_str(args_str)
                                    .unwrap_or(Value::String(args_str.to_string()));
                                let result = tool_results.remove(&id);
                                Some(ExportToolCall {
                                    id,
                                    name,
                                    arguments,
                                    result,
                                })
                            })
                            .collect();
                        export_msgs.push(ExportMessage {
                            role: "assistant".into(),
                            content: String::new(),
                            thinking: None,
                            tool_calls: Some(tool_calls),
                            created_at: m.created_at,
                        });
                        continue;
                    }
                }
                export_msgs.push(ExportMessage {
                    role: "assistant".into(),
                    content: m.content.clone(),
                    thinking: m.thinking.clone(),
                    tool_calls: None,
                    created_at: m.created_at,
                });
            }
            "tool" => {
                // Embedded in the tool_calls of the parent assistant message
                continue;
            }
            _ => {
                export_msgs.push(ExportMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    thinking: None,
                    tool_calls: None,
                    created_at: m.created_at,
                });
            }
        }
    }

    let export = ConversationExport {
        version: "1.0".into(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        conversation: conv,
        provider_name,
        messages: export_msgs,
    };

    let json = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(&file_path, &json).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn open_devtools(app: AppHandle) {
    if let Some(webview) = app.get_webview_window("main") {
        webview.open_devtools();
    }
}

// ─── File-system browse commands (used by the sidebar file explorer) ──────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Strip the `\\?\` extended-length prefix that `canonicalize` adds on Windows.
fn strip_verbatim(p: std::path::PathBuf) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(rest.to_owned())
    } else {
        p
    }
}

/// Resolve `requested` path and verify it lives inside `root`.
/// Uses canonicalize to resolve symlinks and `..` segments.
fn fs_check_within_root(root: &str, requested: &str) -> Result<std::path::PathBuf, String> {
    let canon_root = std::fs::canonicalize(root)
        .map_err(|e| format!("Cannot resolve working directory: {}", e))?;
    let canon_req =
        std::fs::canonicalize(requested).map_err(|e| format!("Cannot resolve path: {}", e))?;
    if !canon_req.starts_with(&canon_root) {
        return Err("Path is outside the working directory".into());
    }
    Ok(strip_verbatim(canon_req))
}

fn fs_get_working_dir(state: &AppState, conversation_id: &str) -> Result<String, String> {
    let conn = state.conn.lock().unwrap();
    let conv = conversations::find_by_id(&conn, conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or("Conversation not found")?;
    conv.working_directory
        .ok_or("No working directory set for this conversation".into())
}

#[tauri::command]
pub fn fs_list_dir(
    state: State<AppState>,
    conversation_id: String,
    path: String,
) -> Result<Vec<FsEntry>, String> {
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_path = fs_check_within_root(&root, &path)?;
    let entries = std::fs::read_dir(&safe_path).map_err(|e| e.to_string())?;
    let mut result: Vec<FsEntry> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            Some(FsEntry {
                name,
                path: e.path().to_string_lossy().into_owned(),
                is_dir,
            })
        })
        .collect();
    result.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(result)
}

#[tauri::command]
pub fn fs_read_file_base64(
    state: State<AppState>,
    conversation_id: String,
    path: String,
) -> Result<String, String> {
    use base64::Engine;
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_path = fs_check_within_root(&root, &path)?;
    let meta = std::fs::metadata(&safe_path).map_err(|e| e.to_string())?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err(format!("File too large ({} bytes); max 20 MB", meta.len()));
    }
    let bytes = std::fs::read(&safe_path).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

#[tauri::command]
pub fn copy_file_to_clipboard(data: String, filename: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| e.to_string())?;
    let safe_name = std::path::Path::new(&filename)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid filename")?;
    let temp_path = std::env::temp_dir().join(safe_name);
    std::fs::write(&temp_path, &bytes).map_err(|e| e.to_string())?;
    let path_str = temp_path.to_str().ok_or("invalid path")?;
    // PowerShell Set-Clipboard -Path copies files to Windows clipboard (CF_HDROP)
    std::process::Command::new("powershell")
        .env("CLIP_PATH", path_str)
        .args([
            "-NoProfile",
            "-Command",
            "Set-Clipboard -Path $env:CLIP_PATH",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn save_file_base64(
    app: AppHandle,
    filename: String,
    data: String,
) -> Result<(), String> {
    use base64::Engine;
    use tauri_plugin_dialog::DialogExt;
    let path = app
        .dialog()
        .file()
        .set_file_name(&filename)
        .blocking_save_file()
        .ok_or("cancelled")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| e.to_string())?;
    std::fs::write(path.as_path().ok_or("invalid path")?, &bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fs_read_file(
    state: State<AppState>,
    conversation_id: String,
    path: String,
) -> Result<String, String> {
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_path = fs_check_within_root(&root, &path)?;
    let meta = std::fs::metadata(&safe_path).map_err(|e| e.to_string())?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err(format!("File too large ({} bytes); max 2 MB", meta.len()));
    }
    std::fs::read_to_string(&safe_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fs_walk(state: State<AppState>, conversation_id: String) -> Result<Vec<FsEntry>, String> {
    let root = fs_get_working_dir(&state, &conversation_id)?;
    // Walk from the working directory root — no caller-supplied path needed
    let entries: Vec<FsEntry> = walkdir::WalkDir::new(&root)
        .max_depth(6)
        .into_iter()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .skip(1)
        .take(500)
        .map(|e| FsEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().to_string_lossy().into_owned(),
            is_dir: e.file_type().is_dir(),
        })
        .collect();
    Ok(entries)
}

#[tauri::command]
pub fn fs_rename(
    state: State<AppState>,
    conversation_id: String,
    path: String,
    new_name: String,
) -> Result<(), String> {
    if new_name.is_empty()
        || new_name.contains('/')
        || new_name.contains('\\')
        || new_name.contains('\0')
    {
        return Err("Invalid name".into());
    }
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_path = fs_check_within_root(&root, &path)?;
    let new_path = safe_path
        .parent()
        .ok_or("No parent directory")?
        .join(&new_name);
    std::fs::rename(&safe_path, &new_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fs_delete(
    state: State<AppState>,
    conversation_id: String,
    path: String,
) -> Result<(), String> {
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_path = fs_check_within_root(&root, &path)?;
    if safe_path == std::path::Path::new(&root) {
        return Err("Cannot delete working directory root".into());
    }
    let meta = std::fs::metadata(&safe_path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&safe_path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(&safe_path).map_err(|e| e.to_string())
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name())).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn fs_copy_dir(
    state: State<AppState>,
    conversation_id: String,
    src_path: String,
    dest_dir: String,
) -> Result<(), String> {
    let root = fs_get_working_dir(&state, &conversation_id)?;
    let safe_src = fs_check_within_root(&root, &src_path)?;
    let safe_dest = fs_check_within_root(&root, &dest_dir)?;
    if !std::fs::metadata(&safe_dest)
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Err("Destination must be a directory".into());
    }
    let dir_name = safe_src.file_name().ok_or("Invalid source name")?;
    let dest = safe_dest.join(dir_name);
    if dest.exists() {
        return Err(format!(
            "{} already exists in destination",
            dir_name.to_string_lossy()
        ));
    }
    copy_dir_recursive(&safe_src, &dest)
}
