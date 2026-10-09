//! The agent loop: one turn is the model answering, calling tools, reading their results and
//! answering again, until it replies without calling anything.
//!
//! Every step is persisted as it happens (so a crash loses nothing the person saw), streamed to
//! the UI as [`ChatEvent`]s, and recorded as a [`Trace`] holding the exact request and response.

pub mod compact;
pub mod events;
pub mod prompt;
pub mod row;
pub mod usage;

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

use crate::attachments;
use crate::attachments::meaning::QueryVector;
use crate::bail_msg;
use crate::db::{Chat, CommandUse, Message, MessageStatus, Role, Trace, new_id, now_ms};
use crate::error::{AppError, CmdResult};
use crate::llm::gemini::GeminiClient;
use crate::llm::openai::OpenAiClient;
use crate::llm::{ChatRequest, Client, LlmError, StreamEvent, ToolSpec};
use crate::models::{ModelEntry, ModelRegistry, ModelSource};
use crate::providers::ProviderKind;
use crate::settings::Settings;
use crate::state::AppState;
use crate::tools::{self, ToolContext};

/// A turn stops after this many model calls, so a confused model cannot loop forever.
const MAX_STEPS: usize = 16;
/// How often one call (the same tool with the same arguments) may fail in a turn before the turn
/// stops: a small model otherwise repeats a call that cannot work until the steps run out.
const MAX_SAME_FAILURES: usize = 3;

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
    /// Running tool calls by row id, for ending one early while its turn goes on.
    tool_stops: Mutex<HashMap<String, CancellationToken>>,
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

    /// Sends a message with the files staged as `attachments`, creating the chat when `chat_id`
    /// is `None`, and starts the turn. `command` is the slash command that wrote `text`.
    pub fn send(
        state: &Arc<AppState>,
        chat_id: Option<String>,
        text: String,
        model_id: String,
        attachment_ids: Vec<String>,
        command: Option<CommandUse>,
    ) -> CmdResult<SendResult> {
        let text = text.trim().to_string();
        if text.is_empty() && attachment_ids.is_empty() {
            bail_msg!("Write a message first.");
        }
        if let Some(id) = &chat_id
            && state.agent.turns.lock().contains_key(id)
        {
            bail_msg!("Wait for the current answer to finish, or stop it.");
        }
        // Every file is checked before a chat is made for the message.
        attachments::check_staged(&state.db, &attachment_ids)?;
        let (chat, created) = match chat_id {
            Some(id) => (
                state
                    .db
                    .get_chat(&id)?
                    .ok_or_else(|| AppError::msg("That chat no longer exists."))?,
                false,
            ),
            None => {
                let title = if let Some(c) = &command {
                    title_from(&format!("{} {}", c.name, c.args))
                } else if text.is_empty() {
                    first_file_name(state, &attachment_ids)
                } else {
                    title_from(&text)
                };
                (state.db.create_chat(&title, Some(&model_id))?, true)
            }
        };
        let seq = state.db.next_seq(&chat.id)?;
        let mut message = Message::new(&chat.id, seq, Role::User, text);
        message.command = command;
        let saved = attachments::take_for_message(state, &chat.id, &message.id, &attachment_ids).and_then(|files| {
            message.attachments = files;
            state.db.save_message(&message).map_err(AppError::from)
        });
        if let Err(e) = saved {
            // A chat made for this message goes with it. Files that could not all be moved went
            // back to the composer (`take_for_message`), so its workspace holds nothing of value.
            if created {
                let _ = state.db.delete_chat(&chat.id);
                let _ = std::fs::remove_dir_all(state.paths.workspace(&chat.id));
            }
            return Err(e);
        }
        if created {
            state.emit_chat(ChatEvent::Chat { chat: chat.clone() });
        }
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
        if text.trim().is_empty() && original.attachments.is_empty() {
            bail_msg!("Write a message first.");
        }
        state.db.truncate_messages(chat_id, original.seq)?;
        state.emit_chat(ChatEvent::Truncated {
            chat_id: chat_id.to_string(),
            from_seq: original.seq,
        });
        let mut message = Message::new(chat_id, original.seq, Role::User, text.trim());
        state.db.save_message(&message)?;
        // The edited message keeps its files; files of the messages after it are forgotten.
        state.db.relink_attachments(chat_id, &original.id, &message.id)?;
        state.db.forget_removed_messages(chat_id)?;
        if let Some(saved) = state.db.get_message(&message.id)? {
            message.attachments = saved.attachments;
        }
        crate::attachments::resolve_messages(&state.paths, std::slice::from_mut(&mut message));
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

    /// Ends a running tool call early (a command's Stop button); the tool hands the model what
    /// it has so far and the turn goes on. False when the call is not running.
    pub fn stop_tool(&self, row_id: &str) -> bool {
        match self.tool_stops.lock().get(row_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Summarizes the chat so far (`/compact`), the summary keeping what `focus` says. Runs like
    /// a turn: the chat is busy and Stop ends it.
    pub fn compact(state: &Arc<AppState>, chat_id: &str, model_id: String, focus: Option<String>) -> CmdResult<()> {
        let Some(model) = state.models.get(&model_id) else {
            bail_msg!("The selected model is no longer available. Pick another one.");
        };
        if !model.enabled {
            bail_msg!(
                "{} is disabled. Enable it in Settings, Models, or pick another model.",
                model.name
            );
        }
        let all = state.db.list_messages(chat_id)?;
        let messages = prompt::current_part(&all).to_vec();
        if !compact::has_news(&messages) {
            bail_msg!("There is nothing to compact yet.");
        }
        let cancel = CancellationToken::new();
        {
            let mut turns = state.agent.turns.lock();
            if turns.contains_key(chat_id) {
                bail_msg!("Wait for the current answer to finish, or stop it.");
            }
            turns.insert(chat_id.to_string(), cancel.clone());
        }
        let state = state.clone();
        let chat_id = chat_id.to_string();
        tauri::async_runtime::spawn(async move {
            state.emit_chat(ChatEvent::TurnStarted {
                chat_id: chat_id.clone(),
            });
            let result = async {
                let (client, served_context) = client_for(&state, &model)
                    .await
                    .map_err(|e| AppError::msg(format!("Could not compact the chat: {e}")))?;
                let job = compact::Job {
                    chat_id: &chat_id,
                    model: &model,
                    client: &client,
                    context_tokens: window(&model, served_context),
                    messages: &messages,
                    at: compact::Place::End,
                    focus: focus.as_deref(),
                    ratio: 1.0,
                    auto: false,
                };
                // A failed summary stays in the chat with its error, which says enough.
                if let compact::Outcome::Failed(e) = compact::run(&state, job, &cancel).await {
                    tracing::warn!(chat = %chat_id, "could not compact the conversation: {e}");
                }
                Ok::<(), AppError>(())
            }
            .await;
            state.agent.turns.lock().remove(&chat_id);
            let error = result.err().map(|e| e.to_string());
            state.emit_chat(ChatEvent::TurnFinished { chat_id, error });
        });
        Ok(())
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

/// The most context `model` is made for: what a local model was trained on (its GGUF says), what
/// a cloud model's provider takes. `None` when that is unknown.
pub(crate) fn model_limit(model: &ModelEntry) -> Option<usize> {
    model.max_context.filter(|&c| c > 0).map(|c| c as usize)
}

/// A model's context window as its settings give it, never more than [`model_limit`]; a local
/// model's server may give it less (see [`client_for`]).
pub(crate) fn model_context(model: &ModelEntry) -> usize {
    let limit = model_limit(model);
    model
        .effective
        .context_length
        .map(|c| c as usize)
        .or(limit)
        .unwrap_or(32_768)
        .min(limit.unwrap_or(usize::MAX))
        .min(1_000_000)
}

/// The window a request with `model` has: what a local model's server took (`served`, as much as
/// the memory free allows), else what its settings give it. Never more than the model is made
/// for, however much memory there is: past its limit, a model loses track of what it reads.
pub(crate) fn window(model: &ModelEntry, served: Option<usize>) -> usize {
    match served {
        Some(served) => served.min(model_limit(model).unwrap_or(usize::MAX)),
        None => model_context(model),
    }
}

/// What every request of a turn starts with: the system prompt and the tools.
pub(crate) struct Frame {
    pub system: String,
    pub tools: Vec<ToolSpec>,
    /// Estimated tokens of the system prompt.
    pub system_tokens: usize,
    /// Estimated tokens of the tools' descriptions and parameters.
    pub tool_tokens: usize,
}

/// The [`Frame`] of the next request in `chat_id`, whose messages are `chat_messages`.
pub(crate) fn frame(
    state: &Arc<AppState>,
    settings: &Settings,
    model: &ModelEntry,
    chat_id: &str,
    chat_messages: &[Message],
) -> Frame {
    let attached = state.db.chat_attachments(chat_id).is_ok_and(|a| !a.is_empty());
    // Tool groups load on demand (see `tools`); attached files come with the tools to search
    // and read them.
    let mut loaded = tools::loaded_groups(chat_messages);
    if attached {
        loaded.insert("files");
    }
    let tools = tools::specs(state, settings, &loaded);
    let loadable = tools::loadable(state, settings, &loaded);
    let mail_accounts = state.mail.account_list();
    let system = prompt::system_prompt(&prompt::PromptInputs {
        model,
        tools: &tools,
        skills: &state.skills,
        attachments: attached,
        shell: crate::shell::current().map(|s| s.name.as_str()),
        mail_accounts: &mail_accounts,
        loadable: &loadable,
    });
    let system_tokens = prompt::estimate_tokens(&system);
    let tool_tokens = tools
        .iter()
        .map(|t| prompt::estimate_tokens(&t.description) + prompt::estimate_tokens(&t.parameters.to_string()))
        .sum();
    Frame {
        system,
        tools,
        system_tokens,
        tool_tokens,
    }
}

/// The client for `model`, and for a local model the context its server gave it.
async fn client_for(state: &Arc<AppState>, model: &ModelEntry) -> CmdResult<(Client, Option<usize>)> {
    match model.source {
        ModelSource::Local => {
            let spec = state.models.launch_spec(&model.id)?;
            let served = state.runtime.ensure(&spec).await.map_err(AppError::msg)?;
            let client = OpenAiClient::new(state.local_http.clone(), served.base_url, model.id.clone());
            Ok((Client::OpenAi(client), Some(served.context_length as usize)))
        }
        ModelSource::Gemini | ModelSource::OpenRouter => {
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
            let client = match provider.kind {
                ProviderKind::Gemini => Client::Gemini(GeminiClient::new(
                    state.http.clone(),
                    provider.base_url(),
                    key,
                    remote.to_string(),
                )),
                ProviderKind::OpenRouter => {
                    let always_thinks = provider.models.iter().any(|m| m.id == remote && m.always_thinks);
                    Client::OpenAi(OpenAiClient::openrouter(
                        state.http.clone(),
                        provider.base_url(),
                        key,
                        remote.to_string(),
                        always_thinks,
                    ))
                }
            };
            Ok((client, None))
        }
    }
}

/// The last user message as the search model puts it, for choosing the passages of long files
/// it is shown by meaning (see `attachments::context`). `None` when they were chosen already,
/// when every file fits whole, and without a search model.
async fn question_vector(
    state: &Arc<AppState>,
    chat_id: &str,
    messages: &[Message],
    room_tokens: usize,
    cancel: &CancellationToken,
) -> Option<(String, QueryVector)> {
    let last = messages.iter().rev().find(|m| m.role == Role::User)?;
    if !attachments::context::may_have_long_files(room_tokens, messages)
        || state.db.shown_passages(&last.id).ok()?.is_some()
    {
        return None;
    }
    let ids: Vec<String> = state
        .db
        .chat_attachments(chat_id)
        .ok()?
        .into_iter()
        .filter(|a| a.tokens.is_some_and(|t| t > 0))
        .map(|a| a.id)
        .collect();
    let vector = state.embedder.query(&ids, &last.content, cancel).await?;
    Some((last.id.clone(), vector))
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
    let params = ModelRegistry::gen_params(&model);
    tools::prepare(&state.settings.get()).await;
    // The message this turn answers as the search model put it, with its id.
    let mut question: Option<(String, QueryVector)> = None;
    // Failed calls of this turn, by `call_key`.
    let mut failures: HashMap<String, usize> = HashMap::new();
    // Tokens per estimated token: raised when the local server counts a request that does not fit.
    let mut token_ratio = 1.0f64;
    // A turn compacts the chat once at most: when no summary could be written, the history is
    // shortened to fit as it would be without compaction.
    let mut compact_tried = false;

    for step in 0..MAX_STEPS {
        if cancel.is_cancelled() {
            return Ok(());
        }
        // Resolved every step: a long tool call or approval wait may outlive the local runtime
        // (unloaded, restarted on another port, with another context), and this brings it back.
        let (client, served_context) = client_for(state, &model).await?;
        let context_tokens = window(&model, served_context);
        let settings = state.settings.get();
        let chat_messages = state.db.list_messages(chat_id)?;
        let Frame {
            system,
            tools,
            system_tokens,
            tool_tokens,
        } = frame(state, &settings, &model, chat_id, &chat_messages);
        let fixed = system_tokens + tool_tokens;
        let reserve = compact::answer_reserve(&params, context_tokens);
        // Estimated tokens the history may take.
        let room = |token_ratio: f64| {
            ((context_tokens.saturating_sub(reserve) as f64 / token_ratio) as usize)
                .saturating_sub(fixed)
                .max(1024)
        };
        let mut budget = room(token_ratio);
        let mut messages = prompt::current_part(&chat_messages).to_vec();
        if step == 0 {
            question = question_vector(state, chat_id, &messages, budget, cancel).await;
        }
        // The hints in attached files name the tools the model has or can load: they stay the same
        // when a group is loaded.
        let tool_names = tools::usable_names(state, &settings);
        let plan = |budget: usize, messages: &[Message]| {
            attachments::context::plan(
                &attachments::context::Inputs {
                    db: &state.db,
                    chat_id,
                    access: attachments::context::ModelAccess::of(&model),
                    room_tokens: budget,
                    tools: tool_names.clone(),
                    query_vector: question.as_ref().map(|(id, q)| (id.as_str(), q)),
                },
                messages,
            )
        };
        let mut files = plan(budget, &messages);

        // Near the end of the context window, the turns before this one become a summary.
        if settings.auto_compact && !compact_tried {
            let custom = compact::custom(&settings, &model).map(|c| c.tokens);
            let limit = compact::threshold(context_tokens, reserve, custom);
            let whole = fixed + prompt::estimate_history(&prompt::history(&messages, usize::MAX, &files));
            let used = compact::request_tokens(&messages, &files, whole, token_ratio);
            if used >= limit
                && let Some(split) = compact::older_part(&messages)
            {
                compact_tried = true;
                tracing::info!(chat = %chat_id, used, limit, "compacting the conversation");
                let job = compact::Job {
                    chat_id,
                    model: &model,
                    client: &client,
                    context_tokens,
                    messages: &messages[..split],
                    at: compact::Place::Before(messages[split].seq),
                    focus: None,
                    ratio: token_ratio,
                    auto: true,
                };
                match compact::run(state, job, cancel).await {
                    compact::Outcome::Done => {
                        messages = prompt::current_part(&state.db.list_messages(chat_id)?).to_vec();
                        files = plan(budget, &messages);
                    }
                    compact::Outcome::Cancelled => return Ok(()),
                    compact::Outcome::Failed(e) => {
                        tracing::warn!(chat = %chat_id, "could not compact the conversation: {e}");
                    }
                }
            }
        }

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
        let mut request = ChatRequest {
            system,
            messages: Vec::new(),
            tools,
            params: params.clone(),
        };
        let mut retried = false;
        // Times the model was busy and the request went again.
        let mut busy_retries = 0u32;
        let (trace_keep, result) = loop {
            request.messages = prompt::history(&messages, budget, &files);
            // The files of the message this turn answers are kept whole in its first call's trace only.
            let trace_keep = (step == 0)
                .then(|| {
                    messages
                        .iter()
                        .rev()
                        .find(|m| m.role == Role::User)
                        .and_then(|m| files.get(&m.id))
                        .map(|x| x.block.clone())
                })
                .flatten();
            let result = client
                .stream(&request, cancel, |event| {
                    if let StreamEvent::Retrying {
                        attempt,
                        attempts,
                        wait,
                        reason,
                    } = event
                    {
                        // The next attempt starts the answer afresh.
                        busy_retries += 1;
                        first_token = None;
                        for text in [
                            &mut pending_content,
                            &mut pending_reasoning,
                            &mut streamed_content,
                            &mut streamed_reasoning,
                        ] {
                            text.clear();
                        }
                        emit_state.emit_chat(ChatEvent::Retrying {
                            chat_id: chat_id.to_string(),
                            message_id: reply.id.clone(),
                            attempt,
                            attempts,
                            wait_ms: wait.as_millis() as u64,
                            reason,
                        });
                        return;
                    }
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
                        StreamEvent::Retrying { .. } => {}
                    }
                    if last_flush.elapsed().as_millis() >= 40 {
                        last_flush = Instant::now();
                        flush(&mut pending_content, &mut pending_reasoning);
                    }
                })
                .await;
            // The history was fitted by estimate, and the local server counts exactly: a request
            // that does not fit after all is fitted once more, at the ratio the count showed. The
            // ratio holds for the rest of the turn.
            if let Err(LlmError::ContextFull { prompt_tokens, .. }) = &result
                && !retried
            {
                let sent = fixed + prompt::estimate_history(&request.messages);
                let ratio = *prompt_tokens as f64 / sent as f64 * 1.05;
                if room(ratio) < budget {
                    tracing::info!(
                        prompt_tokens,
                        sent,
                        "the request did not fit in the context; fitting it again"
                    );
                    token_ratio = ratio;
                    budget = room(ratio);
                    files = plan(budget, &messages);
                    retried = true;
                    continue;
                }
            }
            break (trace_keep, result);
        };
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
                reply.stats = Some(json!({
                    "durationMs": duration_ms,
                    "model": model.name,
                    "retries": (busy_retries > 0).then_some(busy_retries),
                }));
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
                    trace_keep.as_deref(),
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
        reply.stats = Some(stats(
            &completion,
            &model,
            duration_ms,
            ttft_ms,
            busy_retries,
            client.is_router(),
        ));
        state.db.save_message(&reply)?;
        state.emit_chat(ChatEvent::Message {
            chat_id: chat_id.to_string(),
            message: reply.clone(),
        });
        let response = json!({
            "model": completion.model,
            "provider": completion.provider,
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
            trace_keep.as_deref(),
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
            let key = call_key(&call);
            let before = failures.get(&key).copied().unwrap_or(0);
            if !run_tool_call(state, chat_id, &model, &workspace, cancel, &call, before).await? {
                let times = failures.entry(key).or_default();
                *times += 1;
                if *times >= MAX_SAME_FAILURES {
                    bail_msg!(
                        "Stopped: the model made the same {} call {} times, and it failed each time.",
                        call.name,
                        *times
                    );
                }
            }
        }
        if step + 1 == MAX_STEPS {
            bail_msg!("Stopped after {MAX_STEPS} steps without a final answer.");
        }
    }
    Ok(())
}

/// A call by its tool and arguments, whatever their spacing or key order.
fn call_key(call: &crate::db::ToolCall) -> String {
    let args = serde_json::from_str::<Value>(&call.arguments)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| call.arguments.clone());
    format!("{}\u{0}{args}", call.name)
}

/// A failed call's answer when the model already made this same call and saw it fail.
fn with_repeat_note(content: &str, times: usize) -> String {
    let note = format!(
        "You already made this exact call {} and it failed the same way, so repeating it cannot work. Change the arguments as the error says, use another tool, or answer the user.",
        if times == 1 { "once".to_string() } else { format!("{times} times") }
    );
    match serde_json::from_str::<Value>(content) {
        Ok(Value::Object(mut o)) => {
            o.insert("repeated".into(), note.into());
            Value::Object(o).to_string()
        }
        _ => format!("{content}\n\n{note}"),
    }
}

/// Runs one call and records its result; `false` when it failed. `failed_before` counts the
/// times this same call already failed in the turn.
async fn run_tool_call(
    state: &Arc<AppState>,
    chat_id: &str,
    model: &ModelEntry,
    workspace: &std::path::Path,
    cancel: &CancellationToken,
    call: &crate::db::ToolCall,
    failed_before: usize,
) -> CmdResult<bool> {
    let mut first = Message::new(chat_id, state.db.next_seq(chat_id)?, Role::Tool, "");
    first.tool_call_id = Some(call.id.clone());
    first.tool_name = Some(call.name.clone());
    first.model_id = Some(model.id.clone());
    first.status = MessageStatus::Running;

    let args = match crate::llm::parse_arguments(&call.arguments) {
        Ok(v) => v,
        Err(e) => {
            let row = ToolRow::new(state.clone(), first);
            return row
                .finish(false, json!({"error": e}).to_string(), json!({"error": e}), 0)
                .map(|()| false);
        }
    };
    first.tool_result = Some(json!({"label": tools::describe(&call.name, &args), "args": args}));
    let row = ToolRow::new(state.clone(), first);

    let settings = state.settings.get();
    if let Some(msg) = tools::refusal(state, &settings, &call.name) {
        return row
            .finish(false, json!({"error": msg}).to_string(), json!({"error": msg}), 0)
            .map(|()| false);
    }

    if tools::needs_approval(&call.name, &settings) {
        let decision = match row.request_approval(None, cancel).await {
            Ok(d) => d,
            Err(Cancelled) => return row.cancel().map(|()| true),
        };
        match decision {
            Approval::Deny => {
                let msg = "The user declined to run this. Do not retry it; continue without it or ask the user how to proceed.";
                return row
                    .finish(false, json!({"error": msg}).to_string(), json!({"denied": true}), 0)
                    .map(|()| false);
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
    let stop = CancellationToken::new();
    state.agent.tool_stops.lock().insert(row.id(), stop.clone());
    let ctx = ToolContext {
        state: state.clone(),
        chat_id: chat_id.to_string(),
        workspace: workspace.to_path_buf(),
        cancel: cancel.clone(),
        stop,
        row: row.clone(),
    };
    let started = Instant::now();
    let output = tools::run(&call.name, args, &ctx).await;
    state.agent.tool_stops.lock().remove(&row.id());
    let elapsed = started.elapsed().as_millis() as i64;
    if cancel.is_cancelled() && !output.ok {
        return row.cancel().map(|()| true);
    }
    let content = if !output.ok && failed_before > 0 {
        with_repeat_note(&output.content, failed_before)
    } else {
        output.content
    };
    row.finish(output.ok, content, output.display, elapsed)?;
    Ok(output.ok)
}

/// The stats line of an answer. `retries` is how often the model was busy and asked again;
/// `ttft_ms` counts from the first attempt, waits included. `routed` says the model is a router,
/// so `providerModel` is the model it picked (shown beside the stats).
fn stats(
    c: &crate::llm::Completion,
    model: &ModelEntry,
    duration_ms: i64,
    ttft_ms: Option<i64>,
    retries: u32,
    routed: bool,
) -> Value {
    let timings = c.timings.clone().unwrap_or(Value::Null);
    let generation_ms = ttft_ms.map(|t| (duration_ms - t).max(1)).unwrap_or(duration_ms.max(1));
    let tokens_per_second = timings["predicted_per_second"]
        .as_f64()
        .unwrap_or(c.usage.completion_tokens as f64 * 1000.0 / generation_ms as f64);
    json!({
        "model": model.name,
        "providerModel": c.model,
        "provider": c.provider,
        "routed": routed.then_some(true),
        "promptTokens": c.usage.prompt_tokens,
        "completionTokens": c.usage.completion_tokens,
        "cachedTokens": timings["cache_n"].as_u64().unwrap_or(c.usage.cached_tokens),
        "reasoningTokens": c.usage.reasoning_tokens,
        "tokensPerSecond": (tokens_per_second * 10.0).round() / 10.0,
        "promptPerSecond": timings["prompt_per_second"].as_f64().map(|v| v.round()),
        "durationMs": duration_ms,
        "ttftMs": ttft_ms,
        "finishReason": c.finish_reason,
        "retries": (retries > 0).then_some(retries),
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
    keep: Option<&str>,
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
        request: attachments::context::for_trace(&crate::llm::redact_media(&request), keep),
        response,
        duration_ms: Some(duration_ms),
        error,
    };
    if let Err(e) = state.db.save_trace(&trace) {
        tracing::warn!("could not save a trace: {e}");
    }
}

/// A chat title for a message with files and no text: the first file's name.
fn first_file_name(state: &AppState, attachments: &[String]) -> String {
    attachments
        .first()
        .and_then(|id| state.db.get_attachment(id).ok().flatten())
        .map(|a| a.name)
        .unwrap_or_else(|| "New chat".into())
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

    #[test]
    fn the_same_call_is_known_whatever_its_spacing() {
        let call = |args: &str| crate::db::ToolCall { id: "c".into(), name: "pine_test".into(), arguments: args.into() };
        assert_eq!(
            call_key(&call(r#"{"symbol": "FX:EURUSD", "timeframe": "1d"}"#)),
            call_key(&call(r#"{"timeframe":"1d","symbol":"FX:EURUSD"}"#))
        );
        assert_ne!(call_key(&call(r#"{"id": "a"}"#)), call_key(&call(r#"{"id": "b"}"#)));
        let noted = with_repeat_note(r#"{"error":"Give the id."}"#, 1);
        let v: Value = serde_json::from_str(&noted).unwrap();
        assert_eq!(v["error"], "Give the id.");
        assert!(v["repeated"].as_str().unwrap().contains("exact call once"));
        assert!(with_repeat_note("plain", 2).ends_with("Change the arguments as the error says, use another tool, or answer the user."));
    }

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
