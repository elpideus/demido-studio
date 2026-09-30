//! The agent loop: one turn is the model answering, calling tools, reading their results and
//! answering again, until it replies without calling anything.
//!
//! Every step is persisted as it happens (so a crash loses nothing the person saw), streamed to
//! the UI as [`ChatEvent`]s, and recorded as a [`Trace`] holding the exact request and response.

pub mod events;
pub mod prompt;
pub mod row;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::Emitter;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub use events::{CHAT_EVENT, ChatEvent};
pub use row::{Cancelled, ToolRow};

use crate::bail_msg;
use crate::db::{Chat, Message, MessageStatus, Role, Trace, new_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::llm::gemini::GeminiClient;
use crate::llm::openai::OpenAiClient;
use crate::llm::{ChatRequest, Client, LlmError, StreamEvent};
use crate::models::{ModelEntry, ModelRegistry, ModelSource};
use crate::state::AppState;
use crate::tools::{self, ToolContext};

/// A turn stops after this many model calls, so a confused model cannot loop forever.
const MAX_STEPS: usize = 16;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Approval {
    Once,
    Always,
    Deny,
}

#[derive(Default)]
pub struct Agent {
    turns: Mutex<HashMap<String, CancellationToken>>,
    approvals: Mutex<HashMap<String, oneshot::Sender<Approval>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub chat: Chat,
    pub message: Message,
}

impl Agent {
    pub fn running_chats(&self) -> Vec<String> {
        self.turns.lock().keys().cloned().collect()
    }

    /// Sends a message, creating the chat when `chat_id` is `None`, and starts the turn.
    pub fn send(
        state: &Arc<AppState>,
        chat_id: Option<String>,
        text: String,
        model_id: String,
    ) -> CmdResult<SendResult> {
        let text = text.trim().to_string();
        if text.is_empty() {
            bail_msg!("Write a message first.");
        }
        let chat = match chat_id {
            Some(id) => state
                .db
                .get_chat(&id)?
                .ok_or_else(|| AppError::msg("That chat no longer exists."))?,
            None => {
                let chat = state.db.create_chat(&title_from(&text), Some(&model_id))?;
                state.emit_chat(ChatEvent::Chat { chat: chat.clone() });
                chat
            }
        };
        if state.agent.turns.lock().contains_key(&chat.id) {
            bail_msg!("Wait for the current answer to finish, or stop it.");
        }
        let seq = state.db.next_seq(&chat.id)?;
        let message = Message::new(&chat.id, seq, Role::User, text);
        state.db.save_message(&message)?;
        state.emit_chat(ChatEvent::Message {
            chat_id: chat.id.clone(),
            message: message.clone(),
        });
        Self::start(state, &chat.id, model_id);
        let chat = state.db.get_chat(&chat.id)?.unwrap_or(chat);
        Ok(SendResult { chat, message })
    }

    /// Answers the last user message again, discarding the previous answer.
    pub fn regenerate(state: &Arc<AppState>, chat_id: &str, model_id: String) -> CmdResult<()> {
        if state.agent.turns.lock().contains_key(chat_id) {
            bail_msg!("Wait for the current answer to finish, or stop it.");
        }
        let messages = state.db.list_messages(chat_id)?;
        let Some(last_user) = messages.iter().rev().find(|m| m.role == Role::User) else {
            bail_msg!("There is nothing to regenerate.");
        };
        state.db.truncate_messages(chat_id, last_user.seq + 1)?;
        state.emit_chat(ChatEvent::Truncated {
            chat_id: chat_id.to_string(),
            from_seq: last_user.seq + 1,
        });
        Self::start(state, chat_id, model_id);
        Ok(())
    }

    /// Replaces a user message with new text and answers from there.
    pub fn edit(
        state: &Arc<AppState>,
        chat_id: &str,
        message_id: &str,
        text: String,
        model_id: String,
    ) -> CmdResult<Message> {
        if state.agent.turns.lock().contains_key(chat_id) {
            bail_msg!("Wait for the current answer to finish, or stop it.");
        }
        let Some(original) = state.db.get_message(message_id)? else {
            bail_msg!("That message no longer exists.");
        };
        if original.role != Role::User || original.chat_id != chat_id {
            bail_msg!("Only your own messages can be edited.");
        }
        state.db.truncate_messages(chat_id, original.seq)?;
        state.emit_chat(ChatEvent::Truncated {
            chat_id: chat_id.to_string(),
            from_seq: original.seq,
        });
        let message = Message::new(chat_id, original.seq, Role::User, text.trim());
        state.db.save_message(&message)?;
        state.emit_chat(ChatEvent::Message {
            chat_id: chat_id.to_string(),
            message: message.clone(),
        });
        Self::start(state, chat_id, model_id);
        Ok(message)
    }

    pub fn stop(&self, chat_id: &str) {
        if let Some(token) = self.turns.lock().get(chat_id) {
            token.cancel();
        }
    }

    pub fn stop_all(&self) {
        for token in self.turns.lock().values() {
            token.cancel();
        }
    }

    pub fn resolve_approval(&self, message_id: &str, decision: Approval) -> bool {
        match self.approvals.lock().remove(message_id) {
            Some(tx) => tx.send(decision).is_ok(),
            None => false,
        }
    }

    /// Waits for the person to answer the approval shown on row `row_id`.
    pub(crate) async fn wait_approval(&self, row_id: &str, cancel: &CancellationToken) -> Result<Approval, Cancelled> {
        let (tx, rx) = oneshot::channel();
        self.approvals.lock().insert(row_id.to_string(), tx);
        tokio::select! {
            d = rx => Ok(d.unwrap_or(Approval::Deny)),
            _ = cancel.cancelled() => {
                self.approvals.lock().remove(row_id);
                Err(Cancelled)
            }
        }
    }

    fn start(state: &Arc<AppState>, chat_id: &str, model_id: String) {
        let cancel = CancellationToken::new();
        state.agent.turns.lock().insert(chat_id.to_string(), cancel.clone());
        let state = state.clone();
        let chat_id = chat_id.to_string();
        tauri::async_runtime::spawn(async move {
            state.emit_chat(ChatEvent::TurnStarted {
                chat_id: chat_id.clone(),
            });
            let _ = state.db.touch_chat(&chat_id, Some(&model_id));
            if let Ok(Some(chat)) = state.db.get_chat(&chat_id) {
                state.emit_chat(ChatEvent::Chat { chat });
            }
            let result = run_turn(&state, &chat_id, &model_id, &cancel).await;
            state.agent.turns.lock().remove(&chat_id);
            let error = result.err().map(|e| e.to_string());
            if let Some(e) = &error {
                tracing::warn!(chat = %chat_id, "turn ended with an error: {e}");
            }
            state.emit_chat(ChatEvent::TurnFinished { chat_id, error });
        });
    }
}

async fn client_for(state: &Arc<AppState>, model: &ModelEntry) -> CmdResult<Client> {
    match model.source {
        ModelSource::Local => {
            let spec = state.models.launch_spec(&model.id)?;
            let base = state.runtime.ensure(&spec).await.map_err(AppError::msg)?;
            Ok(Client::OpenAi(OpenAiClient::new(
                state.local_http.clone(),
                base,
                model.id.clone(),
            )))
        }
        ModelSource::Gemini => {
            let Some((_, provider_id, remote)) = ModelRegistry::parse_cloud_id(&model.id) else {
                bail_msg!("Unknown model id {}.", model.id);
            };
            let Some(provider) = state.providers.get(provider_id) else {
                bail_msg!("The provider of {} was removed.", model.name);
            };
            if !provider.enabled {
                bail_msg!("{} is disabled in Settings, Providers.", provider.name);
            }
            let Some(key) = state.providers.api_key(provider_id) else {
                bail_msg!("{} has no API key. Add one in Settings, Providers.", provider.name);
            };
            Ok(Client::Gemini(GeminiClient::new(
                state.http.clone(),
                provider.base_url(),
                key,
                remote.to_string(),
            )))
        }
    }
}

async fn run_turn(state: &Arc<AppState>, chat_id: &str, model_id: &str, cancel: &CancellationToken) -> CmdResult<()> {
    let Some(model) = state.models.get(model_id) else {
        bail_msg!("The selected model is no longer available. Pick another one.");
    };
    if !model.enabled {
        bail_msg!(
            "{} is disabled. Enable it in Settings, Models, or pick another model.",
            model.name
        );
    }
    let workspace = state.paths.workspace(chat_id);
    let context_tokens = model
        .effective
        .context_length
        .map(|c| c as usize)
        .or(model.max_context.map(|c| c as usize))
        .unwrap_or(32_768)
        .min(1_000_000);
    let params = ModelRegistry::gen_params(&model);

    for step in 0..MAX_STEPS {
        if cancel.is_cancelled() {
            return Ok(());
        }
        // Resolved every step: a long tool call or approval wait may outlive the local runtime
        // (unloaded, restarted on another port), and this brings it back.
        let client = client_for(state, &model).await?;
        let settings = state.settings.get();
        let tools = tools::specs(state, &settings);
        let system = prompt::system_prompt(&prompt::PromptInputs {
            model: &model,
            tools: &tools,
            skills: &state.skills,
            context_tokens,
        });
        let fixed = prompt::estimate_tokens(&system)
            + tools
                .iter()
                .map(|t| prompt::estimate_tokens(&t.description) + prompt::estimate_tokens(&t.parameters.to_string()))
                .sum::<usize>();
        let reserve = params
            .max_tokens
            .map(|m| m as usize)
            .unwrap_or(context_tokens / 4)
            .max(1024);
        let budget = context_tokens.saturating_sub(fixed + reserve).max(1024);
        let history = prompt::history(&state.db.list_messages(chat_id)?, budget);
        let request = ChatRequest {
            system,
            messages: history,
            tools,
            params: params.clone(),
        };

        let mut reply = Message::new(chat_id, state.db.next_seq(chat_id)?, Role::Assistant, "");
        reply.status = MessageStatus::Streaming;
        reply.model_id = Some(model.id.clone());
        state.db.save_message(&reply)?;
        state.emit_chat(ChatEvent::Message {
            chat_id: chat_id.to_string(),
            message: reply.clone(),
        });

        let started = Instant::now();
        let mut first_token: Option<Instant> = None;
        let mut pending_content = String::new();
        let mut pending_reasoning = String::new();
        let mut streamed_content = String::new();
        let mut streamed_reasoning = String::new();
        let mut last_flush = Instant::now();
        let emit_state = state.clone();
        let flush = |content: &mut String, reasoning: &mut String| {
            if content.is_empty() && reasoning.is_empty() {
                return;
            }
            emit_state.emit_chat(ChatEvent::Delta {
                chat_id: chat_id.to_string(),
                message_id: reply.id.clone(),
                content: std::mem::take(content),
                reasoning: std::mem::take(reasoning),
            });
        };
        let result = client
            .stream(&request, cancel, |event| {
                first_token.get_or_insert_with(Instant::now);
                match event {
                    StreamEvent::Content(t) => {
                        streamed_content.push_str(&t);
                        pending_content.push_str(&t);
                    }
                    StreamEvent::Reasoning(t) => {
                        streamed_reasoning.push_str(&t);
                        pending_reasoning.push_str(&t);
                    }
                    StreamEvent::ToolCall { name, .. } => {
                        flush(&mut pending_content, &mut pending_reasoning);
                        emit_state.emit_chat(ChatEvent::ToolCall {
                            chat_id: chat_id.to_string(),
                            message_id: reply.id.clone(),
                            name,
                        });
                    }
                }
                if last_flush.elapsed().as_millis() >= 40 {
                    last_flush = Instant::now();
                    flush(&mut pending_content, &mut pending_reasoning);
                }
            })
            .await;
        flush(&mut pending_content, &mut pending_reasoning);
        let duration_ms = started.elapsed().as_millis() as i64;
        let ttft_ms = first_token.map(|t| (t - started).as_millis() as i64);

        let completion = match result {
            Ok(c) => c,
            Err(err) => {
                let cancelled = matches!(err, LlmError::Cancelled);
                let mut message = err.to_string();
                if matches!(err, LlmError::Unavailable(_)) {
                    // Keep a model the account cannot use out of the model picker.
                    let mut settings = state.models.settings_of(&model.id);
                    settings.enabled = Some(false);
                    if state.models.set_settings(&model.id, settings).is_ok() {
                        let _ = state.app.emit(crate::models::CHANGED_EVENT, state.models.list());
                        message = format!(
                            "{message}\n\n{} is now turned off. You can turn it back on in Settings, Models.",
                            model.name
                        );
                    }
                }
                reply.content = streamed_content;
                reply.reasoning = Some(streamed_reasoning).filter(|r| !r.is_empty());
                reply.status = if cancelled {
                    MessageStatus::Cancelled
                } else {
                    MessageStatus::Error
                };
                reply.error = (!cancelled).then(|| message.clone());
                reply.stats = Some(json!({"durationMs": duration_ms, "model": model.name}));
                state.db.save_message(&reply)?;
                state.emit_chat(ChatEvent::Message {
                    chat_id: chat_id.to_string(),
                    message: reply.clone(),
                });
                save_trace(
                    state,
                    chat_id,
                    &reply,
                    &model,
                    request_snapshot(&client, &request),
                    None,
                    duration_ms,
                    Some(err.to_string()),
                );
                return if cancelled { Ok(()) } else { Err(AppError::msg(message)) };
            }
        };

        reply.content = completion.content.clone();
        reply.reasoning = Some(completion.reasoning.clone()).filter(|r| !r.trim().is_empty());
        reply.tool_calls = completion.tool_calls.clone();
        reply.provider_meta = completion.provider_meta.clone();
        reply.status = MessageStatus::Done;
        reply.stats = Some(stats(&completion, &model, duration_ms, ttft_ms));
        state.db.save_message(&reply)?;
        state.emit_chat(ChatEvent::Message {
            chat_id: chat_id.to_string(),
            message: reply.clone(),
        });
        let response = json!({
            "model": completion.model,
            "content": completion.content,
            "reasoning": completion.reasoning,
            "toolCalls": completion.tool_calls,
            "finishReason": completion.finish_reason,
            "usage": completion.usage,
            "timings": completion.timings,
        });
        save_trace(
            state,
            chat_id,
            &reply,
            &model,
            completion.request_body.clone(),
            Some(response),
            duration_ms,
            None,
        );

        if reply.tool_calls.is_empty() {
            return Ok(());
        }
        for call in reply.tool_calls.clone() {
            if cancel.is_cancelled() {
                return Ok(());
            }
            run_tool_call(state, chat_id, &model, &workspace, cancel, &call).await?;
        }
        if step + 1 == MAX_STEPS {
            bail_msg!("Stopped after {MAX_STEPS} steps without a final answer.");
        }
    }
    Ok(())
}

async fn run_tool_call(
    state: &Arc<AppState>,
    chat_id: &str,
    model: &ModelEntry,
    workspace: &std::path::Path,
    cancel: &CancellationToken,
    call: &crate::db::ToolCall,
) -> CmdResult<()> {
    let mut first = Message::new(chat_id, state.db.next_seq(chat_id)?, Role::Tool, "");
    first.tool_call_id = Some(call.id.clone());
    first.tool_name = Some(call.name.clone());
    first.model_id = Some(model.id.clone());
    first.status = MessageStatus::Running;

    let args = match crate::llm::parse_arguments(&call.arguments) {
        Ok(v) => v,
        Err(e) => {
            let row = ToolRow::new(state.clone(), first);
            return row.finish(false, json!({"error": e}).to_string(), json!({"error": e}), 0);
        }
    };
    first.tool_result = Some(json!({"label": tools::describe(&call.name, &args), "args": args}));
    let row = ToolRow::new(state.clone(), first);

    if !tools::exists(&call.name) {
        let msg = format!(
            "There is no tool called {}. Use only the tools you were given.",
            call.name
        );
        return row.finish(false, json!({"error": msg}).to_string(), json!({"error": msg}), 0);
    }

    let settings = state.settings.get();
    if tools::needs_approval(&call.name, &settings) {
        let decision = match row.request_approval(None, cancel).await {
            Ok(d) => d,
            Err(Cancelled) => return row.cancel(),
        };
        match decision {
            Approval::Deny => {
                let msg = "The user declined to run this. Do not retry it; continue without it or ask the user how to proceed.";
                return row.finish(false, json!({"error": msg}).to_string(), json!({"denied": true}), 0);
            }
            Approval::Always => {
                let name = call.name.clone();
                let _ = state.settings.update(|s| {
                    s.always_allowed_tools.insert(name);
                });
            }
            Approval::Once => {}
        }
    }

    row.update(|m| m.status = MessageStatus::Running)?;
    let ctx = ToolContext {
        state: state.clone(),
        chat_id: chat_id.to_string(),
        workspace: workspace.to_path_buf(),
        cancel: cancel.clone(),
        row: row.clone(),
    };
    let started = Instant::now();
    let output = tools::run(&call.name, args, &ctx).await;
    let elapsed = started.elapsed().as_millis() as i64;
    if cancel.is_cancelled() && !output.ok {
        return row.cancel();
    }
    row.finish(output.ok, output.content, output.display, elapsed)
}

fn stats(c: &crate::llm::Completion, model: &ModelEntry, duration_ms: i64, ttft_ms: Option<i64>) -> Value {
    let timings = c.timings.clone().unwrap_or(Value::Null);
    let generation_ms = ttft_ms.map(|t| (duration_ms - t).max(1)).unwrap_or(duration_ms.max(1));
    let tokens_per_second = timings["predicted_per_second"]
        .as_f64()
        .unwrap_or(c.usage.completion_tokens as f64 * 1000.0 / generation_ms as f64);
    json!({
        "model": model.name,
        "providerModel": c.model,
        "promptTokens": c.usage.prompt_tokens,
        "completionTokens": c.usage.completion_tokens,
        "cachedTokens": timings["cache_n"].as_u64().unwrap_or(c.usage.cached_tokens),
        "reasoningTokens": c.usage.reasoning_tokens,
        "tokensPerSecond": (tokens_per_second * 10.0).round() / 10.0,
        "promptPerSecond": timings["prompt_per_second"].as_f64().map(|v| v.round()),
        "durationMs": duration_ms,
        "ttftMs": ttft_ms,
        "finishReason": c.finish_reason,
    })
}

fn request_snapshot(client: &Client, request: &ChatRequest) -> Value {
    match client {
        Client::OpenAi(c) => c.body(request),
        Client::Gemini(c) => c.body(request),
    }
}

#[allow(clippy::too_many_arguments)]
fn save_trace(
    state: &Arc<AppState>,
    chat_id: &str,
    reply: &Message,
    model: &ModelEntry,
    request: Value,
    response: Option<Value>,
    duration_ms: i64,
    error: Option<String>,
) {
    let trace = Trace {
        id: new_id(),
        chat_id: chat_id.to_string(),
        message_id: Some(reply.id.clone()),
        created_at: now_ms(),
        model_id: Some(model.id.clone()),
        request,
        response,
        duration_ms: Some(duration_ms),
        error,
    };
    if let Err(e) = state.db.save_trace(&trace) {
        tracing::warn!("could not save a trace: {e}");
    }
}

/// A chat title from the first message: its first line, trimmed to a few words.
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("New chat");
    let cleaned: String = line
        .trim_start_matches(|c: char| c == '#' || c == '>' || c == '-' || c.is_whitespace())
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '_'))
        .collect();
    let mut title = String::new();
    for word in cleaned.split_whitespace() {
        if title.chars().count() + word.chars().count() > 48 {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if title.is_empty() {
        title = cleaned.chars().take(48).collect();
    }
    // A cut-off title should not end on a joining word.
    const DANGLING: &[&str] = &[
        "a", "an", "and", "the", "of", "for", "to", "in", "on", "with", "or", "at", "by",
    ];
    while let Some((head, last)) = title.rsplit_once(' ')
        && DANGLING.contains(&last.to_ascii_lowercase().as_str())
    {
        title = head.to_string();
    }
    let title = title.trim_end_matches(['.', ',', ':', ';', '?', '!']).to_string();
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "New chat".into(),
    }
}

impl AppState {
    pub fn emit_chat(&self, event: ChatEvent) {
        let _ = self.app.emit(CHAT_EVENT, event);
    }

    pub fn notice(&self, chat_id: &str, kind: &str, text: &str) {
        self.emit_chat(ChatEvent::Notice {
            chat_id: chat_id.to_string(),
            kind: kind.to_string(),
            text: text.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approvals_reach_the_waiting_tool() {
        let agent = Arc::new(Agent::default());
        let cancel = CancellationToken::new();
        let waiter = {
            let (agent, cancel) = (agent.clone(), cancel.clone());
            tokio::spawn(async move { agent.wait_approval("row-1", &cancel).await })
        };
        while !agent.resolve_approval("row-1", Approval::Once) {
            tokio::task::yield_now().await;
        }
        assert_eq!(waiter.await.unwrap(), Ok(Approval::Once));
        assert!(
            !agent.resolve_approval("row-1", Approval::Once),
            "an answered approval is gone"
        );
    }

    #[tokio::test]
    async fn stopping_the_turn_ends_an_approval_wait() {
        let agent = Agent::default();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(agent.wait_approval("row-2", &cancel).await, Err(Cancelled));
        assert!(agent.approvals.lock().is_empty());
    }

    #[test]
    fn approvals_are_spelled_in_camel_case() {
        assert_eq!(serde_json::to_value(Approval::Always).unwrap(), "always");
        assert_eq!(
            serde_json::from_value::<Approval>(json!("once")).unwrap(),
            Approval::Once
        );
        // The smaller "only this timeframe" download is gone: every download is 1-minute candles.
        assert!(serde_json::from_value::<Approval>(json!("minimal")).is_err());
    }

    #[test]
    fn titles_are_short_and_clean() {
        assert_eq!(
            title_from("what's the price of **EURUSD** right now?"),
            "What's the price of EURUSD right now"
        );
        assert_eq!(title_from("\n\n# Plan\nmore"), "Plan");
        assert_eq!(
            title_from("Fetch the last three months of hourly candles for gold and compute realised volatility"),
            "Fetch the last three months of hourly candles"
        );
        assert_eq!(title_from("   "), "New chat");
    }
}
