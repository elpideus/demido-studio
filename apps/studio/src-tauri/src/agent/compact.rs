//! Compaction: a chat that nears the model's context window has its older part summarized into
//! one message, which the model reads from then on instead of what it stands in for.
//!
//! The summary is a message of its own ([`Role::Summary`]) placed where the part it summarizes
//! ends, so the chat still shows everything while the history (`prompt::history`) starts from
//! the latest summary. A turn compacts on its own before a model call once the request would
//! take [`threshold`] tokens; `/compact` does it on demand.
//!
//! The summarizer reads the conversation as plain text in a single message, without tools or
//! thinking, which small models handle best. A conversation longer than the model can read at
//! once is summarized part by part, each part together with the summary of the ones before it.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{ChatEvent, prompt, request_snapshot, save_trace};
use crate::attachments::context::Extras;
use crate::db::{Message, MessageStatus, Role};
use crate::llm::openai::Dialect;
use crate::llm::{ChatRequest, Client, GenParams, LlmError, LlmMessage, StreamEvent};
use crate::models::{ModelEntry, ModelRegistry};
use crate::settings::Settings;
use crate::state::AppState;
use crate::tools::clip;

/// Share of the room a request has (the context window less what is kept for the answer) at
/// which a chat is compacted, unless the person set a lower threshold.
pub const DEFAULT_SHARE: f64 = 0.85;
/// The lowest threshold there is, whatever is set.
pub const MIN_TOKENS: usize = 1_000;
/// Older messages smaller than this are left alone: a summary would not save much.
const MIN_OLDER_TOKENS: usize = 400;

/// Characters of each kind of message the summarizer reads at most.
const USER_CHARS: usize = 6_000;
const ASSISTANT_CHARS: usize = 4_000;
const ARGS_CHARS: usize = 300;
const RESULT_CHARS: usize = 800;

/// The room every request keeps for the model's answer.
pub fn answer_reserve(params: &GenParams, context_tokens: usize) -> usize {
    params
        .max_tokens
        .map(|m| m as usize)
        .unwrap_or(context_tokens / 4)
        .max(1024)
}

/// Tokens a request may take before the chat is compacted: [`DEFAULT_SHARE`] of the room a
/// request has, or `custom` when that is lower. A higher one would never be reached: the history
/// is shortened to fit before.
pub fn threshold(context_tokens: usize, reserve: usize, custom: Option<u32>) -> usize {
    let limit = ((context_tokens.saturating_sub(reserve) as f64 * DEFAULT_SHARE) as usize).max(MIN_TOKENS);
    custom.map_or(limit, |t| (t as usize).clamp(MIN_TOKENS, limit))
}

/// Whom a threshold the person set is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// This model, in its settings.
    Model,
    /// Every model without one of its own: Settings, General, or `/autocompact`.
    All,
}

/// A threshold the person set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Custom {
    pub tokens: u32,
    pub scope: Scope,
}

/// The threshold the person set for `model`: its own, else the one for every model.
pub fn custom(settings: &Settings, model: &ModelEntry) -> Option<Custom> {
    custom_of(model.settings.auto_compact_tokens, settings.auto_compact_tokens)
}

fn custom_of(own: Option<u32>, all: Option<u32>) -> Option<Custom> {
    own.map(|tokens| Custom {
        tokens,
        scope: Scope::Model,
    })
    .or(all.map(|tokens| Custom {
        tokens,
        scope: Scope::All,
    }))
}

/// Estimated tokens of one message as the history shows it, `files` holding the files of user
/// messages (see `attachments::context`).
fn message_tokens(m: &Message, files: &HashMap<String, Extras>) -> usize {
    match m.role {
        Role::User => prompt::estimate_tokens(&m.content) + files.get(&m.id).map_or(0, |x| x.tokens),
        Role::Assistant => {
            prompt::estimate_tokens(&m.content)
                + m.tool_calls
                    .iter()
                    .map(|c| prompt::estimate_tokens(&c.arguments) + 8)
                    .sum::<usize>()
        }
        // The history clips older results to fewer characters than this.
        Role::Tool => prompt::estimate_tokens(&m.content).min(4_000),
        Role::Summary => prompt::estimate_tokens(&m.content),
    }
}

/// Tokens the next request would take. `estimated` is the estimate of all of it (system prompt,
/// tools and the history unshortened); the model's own count of its latest request and answer,
/// with the estimate of what came after, is taken when larger, since estimates run low on code
/// and numbers. `ratio` is the tokens per estimated token the turn found.
pub fn request_tokens(messages: &[Message], files: &HashMap<String, Extras>, estimated: usize, ratio: f64) -> usize {
    counted_tokens(messages, files, ratio)
        .unwrap_or(0)
        .max((estimated as f64 * ratio) as usize)
}

/// The model's own count of its latest request and answer in `messages`, with the estimate of
/// what came after; `None` before it answered.
pub fn counted_tokens(messages: &[Message], files: &HashMap<String, Extras>, ratio: f64) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.role == Role::Assistant && m.status == MessageStatus::Done)
        .find_map(|(i, m)| {
            let stats = m.stats.as_ref()?;
            let prompt = stats["promptTokens"].as_u64().filter(|&t| t > 0)? as usize;
            let completion = stats["completionTokens"].as_u64().unwrap_or(0) as usize;
            let after: usize = messages[i + 1..].iter().map(|m| message_tokens(m, files)).sum();
            Some(prompt + completion + (after as f64 * ratio) as usize)
        })
}

/// Where the latest turn starts in `messages` (the chat from its latest summary on), when what
/// comes before it is worth summarizing.
pub fn older_part(messages: &[Message]) -> Option<usize> {
    let split = messages.iter().rposition(|m| m.role == Role::User)?;
    worth_summarizing(&messages[..split]).then_some(split)
}

/// Whether a summary of `messages` would save enough to be written.
pub fn worth_summarizing(messages: &[Message]) -> bool {
    let tokens: usize = messages
        .iter()
        .filter(|m| m.role != Role::Summary)
        .map(|m| message_tokens(m, &HashMap::new()))
        .sum();
    tokens >= MIN_OLDER_TOKENS
}

/// Whether `messages` (the chat from its latest summary on) hold anything a new summary would add.
pub fn has_news(messages: &[Message]) -> bool {
    messages.iter().any(|m| m.role != Role::Summary)
}

/// Where a summary goes.
pub enum Place {
    /// Before the message with this seq, which moves down with every one after it.
    Before(i64),
    /// After the last message.
    End,
}

pub struct Job<'a> {
    pub chat_id: &'a str,
    pub model: &'a ModelEntry,
    pub client: &'a Client,
    pub context_tokens: usize,
    /// What to summarize: the chat from its latest summary on, up to where the new one goes.
    pub messages: &'a [Message],
    pub at: Place,
    /// What the person asked the summary to keep (`/compact <focus>`).
    pub focus: Option<&'a str>,
    /// Tokens per estimated token, as the turn found it.
    pub ratio: f64,
    /// Started by the turn rather than by the person.
    pub auto: bool,
}

pub enum Outcome {
    Done,
    Cancelled,
    Failed(String),
}

enum Failure {
    Cancelled,
    Error(String),
}

/// Writes the summary of `job.messages` into a new summary message, streamed to the UI as it is
/// written. A summary that fails or is stopped stays in the chat as such, and the model never
/// reads it.
pub async fn run(state: &Arc<AppState>, job: Job<'_>, cancel: &CancellationToken) -> Outcome {
    let summarized = job.messages.iter().filter(|m| m.role != Role::Summary).count();
    let tokens_before = (job
        .messages
        .iter()
        .map(|m| message_tokens(m, &HashMap::new()))
        .sum::<usize>() as f64
        * job.ratio) as usize;
    let seq = match job.at {
        Place::Before(seq) => seq,
        Place::End => match state.db.next_seq(job.chat_id) {
            Ok(seq) => seq,
            Err(e) => return Outcome::Failed(e.to_string()),
        },
    };
    let mut summary = Message::new(job.chat_id, seq, Role::Summary, "");
    summary.status = MessageStatus::Streaming;
    summary.model_id = Some(job.model.id.clone());
    summary.stats = Some(json!({
        "auto": job.auto,
        "messages": summarized,
        "tokensBefore": tokens_before,
        "model": job.model.name,
    }));
    let saved = match job.at {
        Place::Before(_) => state.db.insert_message_at(&summary).map(|moved| {
            for message in moved {
                state.emit_chat(ChatEvent::Message {
                    chat_id: job.chat_id.to_string(),
                    message,
                });
            }
        }),
        Place::End => state.db.save_message(&summary),
    };
    if let Err(e) = saved {
        return Outcome::Failed(e.to_string());
    }
    emit(state, &summary);

    let started = Instant::now();
    let (previous, entries) = transcript(job.messages);
    let result = summarize(state, &job, &mut summary, previous, entries, cancel).await;
    let stats = summary.stats.get_or_insert_with(|| json!({}));
    stats["durationMs"] = json!(started.elapsed().as_millis() as i64);
    if let Some(o) = stats.as_object_mut() {
        o.remove("part");
        o.remove("parts");
    }
    let outcome = match result {
        Ok(text) => {
            stats["tokensAfter"] = json!((prompt::estimate_tokens(&text) as f64 * job.ratio) as usize);
            summary.content = text;
            summary.status = MessageStatus::Done;
            Outcome::Done
        }
        Err(Failure::Cancelled) => {
            summary.status = MessageStatus::Cancelled;
            Outcome::Cancelled
        }
        Err(Failure::Error(e)) => {
            summary.status = MessageStatus::Error;
            summary.error = Some(e.clone());
            Outcome::Failed(e)
        }
    };
    if let Err(e) = state.db.save_message(&summary) {
        return Outcome::Failed(e.to_string());
    }
    emit(state, &summary);
    outcome
}

fn emit(state: &AppState, message: &Message) {
    state.emit_chat(ChatEvent::Message {
        chat_id: message.chat_id.clone(),
        message: message.clone(),
    });
}

/// Summarizes `entries` part by part, each part with the summary so far (`previous`, at first
/// the summary the chat had). Streams each part's summary into `summary`.
async fn summarize(
    state: &Arc<AppState>,
    job: &Job<'_>,
    summary: &mut Message,
    mut previous: Option<String>,
    entries: Vec<String>,
    cancel: &CancellationToken,
) -> Result<String, Failure> {
    let target = summary_tokens(job.context_tokens);
    let words = target * 3 / 4;
    let system = system_prompt(words);
    let mut params = ModelRegistry::gen_params(job.model);
    params.thinking = (job.model.capabilities.thinking != Some(false)).then_some(false);
    params.temperature = Some(params.temperature.unwrap_or(0.3).min(0.3));
    let answer = match job.client {
        // The answer room of a local model comes out of its small window: ask for what is needed.
        Client::OpenAi(c) if c.dialect == Dialect::LlamaCpp => {
            let tokens = (target * 2).max(1_024).min(job.context_tokens / 2);
            params.max_tokens = Some(tokens as u32);
            tokens
        }
        // Thinking counts against a cloud model's answer, which is left as configured.
        _ => answer_reserve(&params, job.context_tokens),
    };
    // Estimated tokens of conversation one request can carry.
    let room = |ratio: f64, previous: &Option<String>| {
        ((job.context_tokens.saturating_sub(answer) as f64 / ratio) as usize)
            .saturating_sub(
                prompt::estimate_tokens(&system) + 120 + previous.as_deref().map_or(0, prompt::estimate_tokens),
            )
            .max(512)
    };

    let mut ratio = job.ratio;
    let mut retried = false;
    let mut queue: VecDeque<String> = entries.into();
    let mut done = 0usize;
    while !queue.is_empty() {
        let fits = room(ratio, &previous);
        let parts = done + count_parts(&queue, fits);
        let part = take_part(&mut queue, fits);
        // The part being written is shown on its own: earlier parts are in the summary it extends.
        summary.content.clear();
        if let Some(stats) = summary.stats.as_mut() {
            stats["part"] = json!(done + 1);
            stats["parts"] = json!(parts);
        }
        emit(state, summary);

        let request = ChatRequest {
            system: system.clone(),
            messages: vec![LlmMessage::User {
                content: request_text(previous.as_deref(), &part, job.focus, words),
                media: Vec::new(),
            }],
            tools: Vec::new(),
            params: params.clone(),
        };
        let started = Instant::now();
        let mut pending = String::new();
        let mut last_flush = Instant::now();
        let flush = |pending: &mut String| {
            if !pending.is_empty() {
                state.emit_chat(ChatEvent::Delta {
                    chat_id: summary.chat_id.clone(),
                    message_id: summary.id.clone(),
                    content: std::mem::take(pending),
                    reasoning: String::new(),
                });
            }
        };
        let result = job
            .client
            .stream(&request, cancel, |event| {
                match event {
                    StreamEvent::Content(t) => pending.push_str(&t),
                    // The next attempt writes the part afresh.
                    StreamEvent::Retrying {
                        attempt,
                        attempts,
                        wait,
                        reason,
                    } => {
                        pending.clear();
                        state.emit_chat(ChatEvent::Retrying {
                            chat_id: summary.chat_id.clone(),
                            message_id: summary.id.clone(),
                            attempt,
                            attempts,
                            wait_ms: wait.as_millis() as u64,
                            reason,
                        });
                    }
                    _ => {}
                }
                if last_flush.elapsed().as_millis() >= 40 {
                    last_flush = Instant::now();
                    flush(&mut pending);
                }
            })
            .await;
        flush(&mut pending);
        let duration_ms = started.elapsed().as_millis() as i64;

        match result {
            Ok(completion) => {
                let response = json!({
                    "model": completion.model,
                    "content": completion.content,
                    "finishReason": completion.finish_reason,
                    "usage": completion.usage,
                    "timings": completion.timings,
                });
                save_trace(
                    state,
                    job.chat_id,
                    summary,
                    job.model,
                    completion.request_body.clone(),
                    None,
                    Some(response),
                    duration_ms,
                    None,
                );
                let text = clean(&completion.content);
                if text.is_empty() {
                    return Err(Failure::Error("The model wrote an empty summary.".into()));
                }
                summary.content = text.clone();
                previous = Some(text);
                done += 1;
            }
            Err(err) => {
                save_trace(
                    state,
                    job.chat_id,
                    summary,
                    job.model,
                    request_snapshot(job.client, &request),
                    None,
                    None,
                    duration_ms,
                    Some(err.to_string()),
                );
                match err {
                    LlmError::Cancelled => return Err(Failure::Cancelled),
                    // Fitted by estimate, counted exactly: take less at the ratio the count showed.
                    LlmError::ContextFull { prompt_tokens, .. } if !retried => {
                        let sent =
                            prompt::estimate_tokens(&request.system) + prompt::estimate_history(&request.messages);
                        ratio = (prompt_tokens as f64 / sent as f64 * 1.05).max(ratio * 1.1);
                        retried = true;
                        for entry in part.into_iter().rev() {
                            queue.push_front(entry);
                        }
                    }
                    err => return Err(Failure::Error(err.to_string())),
                }
            }
        }
    }
    previous.ok_or_else(|| Failure::Error("There was nothing to summarize.".into()))
}

/// Tokens of summary to ask for: about a tenth of the context window.
fn summary_tokens(context_tokens: usize) -> usize {
    (context_tokens / 10).clamp(300, 2_000)
}

fn system_prompt(words: usize) -> String {
    format!(
        "You write the summary an assistant reads in place of an earlier conversation, so that it can carry on without it.\n\
         Write only the summary, in the language of the conversation, under these headings:\n\
         ## Goal\n\
         ## Facts and decisions\n\
         ## Files and data\n\
         ## Done so far\n\
         ## Open tasks and next step\n\
         Keep exact values: numbers, symbols, dates, file names and paths, code names and settings. \
         Keep what the user asked for and how they want it, in their words when short. \
         Leave out greetings, attempts that led nowhere and tool output nobody used. \
         Under a heading with nothing to say, write \"None.\" Use at most {words} words."
    )
}

/// The one message the summarizer reads: the summary so far, the part to add, and the task last,
/// where a small model looks.
fn request_text(previous: Option<&str>, part: &[String], focus: Option<&str>, words: usize) -> String {
    let mut s = String::new();
    if let Some(p) = previous {
        s.push_str("<previous_summary>\n");
        s.push_str(p);
        s.push_str("\n</previous_summary>\n\n");
    }
    s.push_str("<conversation>\n");
    s.push_str(&part.join("\n\n"));
    s.push_str("\n</conversation>\n\n");
    s.push_str(if previous.is_some() {
        "Write one summary of the previous summary and the conversation after it."
    } else {
        "Summarize the conversation."
    });
    if let Some(f) = focus.map(str::trim).filter(|f| !f.is_empty()) {
        s.push_str(&format!(" The user wants it to focus on: {f}."));
    }
    s.push_str(&format!(
        " Use the headings from your instructions and at most {words} words."
    ));
    s
}

/// The conversation as the summarizer reads it: the summary it carries on from, if any, and one
/// entry per message, long ones clipped.
fn transcript(messages: &[Message]) -> (Option<String>, Vec<String>) {
    let mut previous = None;
    let mut entries = Vec::new();
    for m in messages {
        match m.role {
            Role::Summary => {
                // A summary stands in for everything before it.
                if m.status == MessageStatus::Done {
                    previous = Some(m.content.trim().to_string());
                    entries.clear();
                }
            }
            Role::User => {
                let text = m.content.trim();
                let mut entry = if text.is_empty() {
                    "User sent files.".to_string()
                } else {
                    format!("User: {}", clip(text, USER_CHARS))
                };
                if !m.attachments.is_empty() {
                    let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
                    entry.push_str(&format!("\n[attached: {}]", names.join(", ")));
                }
                entries.push(entry);
            }
            Role::Assistant => {
                if matches!(m.status, MessageStatus::Error | MessageStatus::Streaming) {
                    continue;
                }
                let mut entry = String::new();
                if !m.content.trim().is_empty() {
                    entry = format!("Assistant: {}", clip(m.content.trim(), ASSISTANT_CHARS));
                }
                for call in &m.tool_calls {
                    if !entry.is_empty() {
                        entry.push('\n');
                    }
                    entry.push_str(&format!(
                        "[Assistant called {} {}]",
                        call.name,
                        clip(call.arguments.trim(), ARGS_CHARS)
                    ));
                }
                if !entry.is_empty() {
                    entries.push(entry);
                }
            }
            Role::Tool => {
                let name = m.tool_name.as_deref().unwrap_or("tool");
                let result = match m.content.trim() {
                    "" => "(no result)".to_string(),
                    text => clip(text, RESULT_CHARS),
                };
                entries.push(format!("[{name} returned: {result}]"));
            }
        }
    }
    (previous, entries)
}

/// Takes from `queue` the entries that fit in `room` estimated tokens: at least one, clipped
/// when it alone is too long.
fn take_part(queue: &mut VecDeque<String>, room: usize) -> Vec<String> {
    let mut part = Vec::new();
    let mut used = 0usize;
    while let Some(next) = queue.front() {
        let tokens = prompt::estimate_tokens(next) + 1;
        if !part.is_empty() && used + tokens > room {
            break;
        }
        let Some(mut entry) = queue.pop_front() else { break };
        if tokens > room {
            entry = clip(&entry, room * 3);
        }
        used += prompt::estimate_tokens(&entry) + 1;
        part.push(entry);
    }
    part
}

/// How many parts [`take_part`] would make of `queue`.
fn count_parts(queue: &VecDeque<String>, room: usize) -> usize {
    let mut copy = queue.clone();
    let mut n = 0;
    while !copy.is_empty() {
        take_part(&mut copy, room);
        n += 1;
    }
    n
}

/// The summary without what models wrap it in: their thinking, a `<summary>` tag.
fn clean(text: &str) -> String {
    let mut t = text.trim();
    if t.starts_with("<think>")
        && let Some(end) = t.find("</think>")
    {
        t = t[end + "</think>".len()..].trim();
    }
    if let Some(inner) = t.strip_prefix("<summary>").and_then(|s| s.strip_suffix("</summary>")) {
        t = inner.trim();
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ToolCall;

    fn msg(role: Role, content: &str) -> Message {
        Message::new("c", 0, role, content)
    }

    #[test]
    fn the_threshold_is_near_the_limit_and_can_only_be_lowered() {
        // 32k window, 8k kept for the answer: 85% of the 24k left.
        assert_eq!(threshold(32_768, 8_192, None), 20_889);
        assert_eq!(threshold(32_768, 8_192, Some(12_000)), 12_000);
        assert_eq!(
            threshold(32_768, 8_192, Some(100_000)),
            20_889,
            "above the default is never reached"
        );
        assert_eq!(threshold(32_768, 8_192, Some(10)), MIN_TOKENS);
        assert_eq!(
            threshold(2_048, 1_024, None),
            MIN_TOKENS,
            "a tiny window still has a floor"
        );
    }

    #[test]
    fn a_models_own_threshold_wins_over_the_one_for_every_model() {
        let own = |tokens| Custom {
            tokens,
            scope: Scope::Model,
        };
        let all = |tokens| Custom {
            tokens,
            scope: Scope::All,
        };
        assert_eq!(custom_of(Some(50_000), Some(12_000)), Some(own(50_000)));
        assert_eq!(custom_of(None, Some(12_000)), Some(all(12_000)));
        assert_eq!(custom_of(Some(8_000), None), Some(own(8_000)));
        assert_eq!(custom_of(None, None), None);
    }

    #[test]
    fn the_model_count_wins_when_estimates_run_low() {
        let mut answer = msg(Role::Assistant, "done");
        answer.stats = Some(json!({"promptTokens": 9_000, "completionTokens": 500}));
        let messages = vec![msg(Role::User, "q"), answer, msg(Role::User, &"x".repeat(300))];
        // 101 estimated tokens after the answer.
        assert_eq!(request_tokens(&messages, &HashMap::new(), 2_000, 1.0), 9_601);
        assert_eq!(request_tokens(&messages, &HashMap::new(), 20_000, 1.0), 20_000);
        assert_eq!(request_tokens(&messages[..1], &HashMap::new(), 2_000, 1.5), 3_000);
    }

    #[test]
    fn only_turns_before_the_latest_are_compacted() {
        let big = "x".repeat(3_000);
        let messages = vec![
            msg(Role::User, &big),
            msg(Role::Assistant, &big),
            msg(Role::User, "next"),
            msg(Role::Assistant, "step"),
        ];
        assert_eq!(older_part(&messages), Some(2));
        assert_eq!(older_part(&messages[2..]), None, "a single turn has nothing older");
        let small = vec![
            msg(Role::User, "hi"),
            msg(Role::Assistant, "hello"),
            msg(Role::User, "next"),
        ];
        assert_eq!(older_part(&small), None, "too little to be worth a summary");
        let summarized = vec![msg(Role::Summary, &big), msg(Role::User, "next")];
        assert_eq!(
            older_part(&summarized),
            None,
            "the summary alone is not summarized again"
        );
        assert!(!has_news(&summarized[..1]));
        assert!(has_news(&summarized));
    }

    #[test]
    fn the_transcript_is_plain_text_and_continues_the_last_summary() {
        let mut call = msg(Role::Assistant, "Let me check.");
        call.tool_calls.push(ToolCall {
            id: "1".into(),
            name: "market_quote".into(),
            arguments: r#"{"symbol":"FX:EURUSD"}"#.into(),
        });
        let mut result = msg(
            Role::Tool,
            &format!(r#"{{"price":1.0834,"rows":"{}"}}"#, "y".repeat(5_000)),
        );
        result.tool_name = Some("market_quote".into());
        let mut failed = msg(Role::Assistant, "half an answer");
        failed.status = MessageStatus::Error;
        let messages = vec![
            msg(Role::User, "forgotten"),
            msg(Role::Summary, "The gist."),
            msg(Role::User, "EURUSD?"),
            call,
            result,
            failed,
            msg(Role::Assistant, "1.0834."),
        ];
        let (previous, entries) = transcript(&messages);
        assert_eq!(previous.as_deref(), Some("The gist."));
        assert_eq!(entries.len(), 4, "{entries:?}");
        assert_eq!(entries[0], "User: EURUSD?");
        assert_eq!(
            entries[1],
            "Assistant: Let me check.\n[Assistant called market_quote {\"symbol\":\"FX:EURUSD\"}]"
        );
        assert!(entries[2].starts_with("[market_quote returned: {\"price\":1.0834") && entries[2].len() < 1_000);
        assert_eq!(entries[3], "Assistant: 1.0834.");

        let text = request_text(previous.as_deref(), &entries, Some("the prices"), 300);
        assert!(text.starts_with("<previous_summary>\nThe gist.\n</previous_summary>"));
        assert!(text.ends_with(
            "The user wants it to focus on: the prices. Use the headings from your instructions and at most 300 words."
        ));
    }

    #[test]
    fn long_conversations_are_split_into_parts_that_fit() {
        let entries: VecDeque<String> = (0..10).map(|i| format!("{i}{}", "z".repeat(299))).collect();
        // 101 tokens each: three fit in 310.
        assert_eq!(count_parts(&entries, 310), 4);
        let mut queue = entries.clone();
        assert_eq!(take_part(&mut queue, 310).len(), 3);
        assert_eq!(queue.len(), 7);
        // One entry larger than the room is clipped rather than left behind.
        let mut queue: VecDeque<String> = VecDeque::from(["w".repeat(10_000)]);
        let part = take_part(&mut queue, 600);
        assert!(queue.is_empty() && part[0].len() < 2_000);
    }

    #[test]
    fn summaries_lose_their_wrapping() {
        assert_eq!(clean("  <summary>\n## Goal\nX\n</summary> "), "## Goal\nX");
        assert_eq!(clean("<think>hmm</think>\n## Goal"), "## Goal");
        assert_eq!(clean("## Goal"), "## Goal");
    }
}
