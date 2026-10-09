//! How full a chat's context window is: the tokens the next request takes, part by part, and
//! how far that is from compaction. The composer shows it as a ring.
//!
//! It is measured as the turn measures it before compacting (see `run_turn`), so the ring fills
//! up exactly when the chat is about to be compacted.

use std::sync::Arc;

use serde::Serialize;

use super::{Frame, compact, frame, model_limit, prompt};
use crate::attachments::context::{self as files_context, ModelAccess};
use crate::db::{MessageStatus, Role};
use crate::error::CmdResult;
use crate::models::{ModelEntry, ModelRegistry, ModelSource};
use crate::runtime::RuntimeState;
use crate::state::AppState;
use crate::tools;

/// Tokens of each part of a request. They add up to [`ContextUsage::used`].
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parts {
    pub system: usize,
    pub tools: usize,
    /// The summary of the compacted part of the chat.
    pub summary: usize,
    /// Attached files, as much of them as the model is shown.
    pub files: usize,
    pub conversation: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Tokens the model reads at once.
    pub window: usize,
    /// A local model not loaded yet: its server may give it a smaller window when it loads.
    pub window_estimated: bool,
    /// Tokens of the window every request keeps for the answer.
    pub reserve: usize,
    /// Tokens the next request takes as the chat stands, before the message being written.
    pub used: usize,
    /// `used` comes from the model's own count of its latest answer, rather than an estimate.
    pub counted: bool,
    /// Tokens at which the chat is compacted; `None` with auto-compact off.
    pub threshold: Option<usize>,
    /// The chat has an older part worth summarizing. Without one, a chat past the window is
    /// shortened from its oldest messages instead.
    pub can_compact: bool,
    /// Summaries written in this chat so far.
    pub compactions: usize,
    pub parts: Parts,
}

/// The usage of `chat_id` (a new chat when `None`) with `model`.
pub fn usage(state: &Arc<AppState>, chat_id: Option<&str>, model: &ModelEntry) -> CmdResult<ContextUsage> {
    let settings = state.settings.get();
    let chat_id = chat_id.unwrap_or_default();
    let chat_messages = if chat_id.is_empty() {
        Vec::new()
    } else {
        state.db.list_messages(chat_id)?
    };
    let (window, window_estimated) = window_of(state, model);
    let reserve = compact::answer_reserve(&ModelRegistry::gen_params(model), window);
    let Frame {
        system_tokens,
        tool_tokens,
        ..
    } = frame(state, &settings, model, chat_id, &chat_messages);
    let fixed = system_tokens + tool_tokens;

    let messages = prompt::current_part(&chat_messages);
    let files = files_context::plan(
        &files_context::Inputs {
            db: &state.db,
            chat_id,
            access: ModelAccess::of(model),
            room_tokens: window.saturating_sub(reserve).saturating_sub(fixed).max(1024),
            tools: tools::usable_names(state, &settings),
            query_vector: None,
        },
        messages,
    );
    let estimated = fixed + prompt::estimate_history(&prompt::history(messages, usize::MAX, &files));
    let counted = compact::counted_tokens(messages, &files, 1.0).filter(|&c| c > estimated);
    let used = counted.unwrap_or(estimated);

    let summary = match messages.first() {
        Some(m) if m.role == Role::Summary => prompt::estimate_tokens(&prompt::summary_block(&m.content)),
        _ => 0,
    };
    let file_tokens = files.values().map(|x| prompt::estimate_tokens(&x.block)).sum();
    let parts = split(
        Parts {
            system: system_tokens,
            tools: tool_tokens,
            summary,
            files: file_tokens,
            conversation: estimated.saturating_sub(fixed + summary + file_tokens),
        },
        used,
    );

    // A turn compacts what came before its own message: during one, the turns before it; between
    // turns, the next message makes everything so far older.
    let running = state.agent.running_chats().iter().any(|id| id == chat_id);
    let can_compact = if running {
        compact::older_part(messages).is_some()
    } else {
        compact::worth_summarizing(messages)
    };

    Ok(ContextUsage {
        window,
        window_estimated,
        reserve,
        used,
        counted: counted.is_some(),
        threshold: settings
            .auto_compact
            .then(|| compact::threshold(window, reserve, settings.auto_compact_tokens)),
        can_compact,
        compactions: chat_messages
            .iter()
            .filter(|m| m.role == Role::Summary && m.status == MessageStatus::Done)
            .count(),
        parts,
    })
}

/// The window the next request has: what a local model's server gave it when that model is
/// loaded, else what its settings give it (and whether that is all it is).
fn window_of(state: &AppState, model: &ModelEntry) -> (usize, bool) {
    if model.source != ModelSource::Local {
        return (model_context(model), false);
    }
    let status = state.runtime.status();
    match status.context_length {
        Some(served) if status.state == RuntimeState::Ready && status.model_id.as_deref() == Some(&model.id) => {
            (served as usize, false)
        }
        _ => (model_context(model), true),
    }
}

/// `parts` (estimates) scaled to add up to `total`, which the model may have counted higher.
fn split(parts: Parts, total: usize) -> Parts {
    let sum = parts.system + parts.tools + parts.summary + parts.files + parts.conversation;
    if sum == 0 || sum == total {
        return parts;
    }
    let scale = |n: usize| (n as f64 * total as f64 / sum as f64).round() as usize;
    let mut scaled = Parts {
        system: scale(parts.system),
        tools: scale(parts.tools),
        summary: scale(parts.summary),
        files: scale(parts.files),
        conversation: 0,
    };
    // The conversation takes what rounding leaves, so the parts add up exactly.
    scaled.conversation = total.saturating_sub(scaled.system + scaled.tools + scaled.summary + scaled.files);
    scaled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_scaled_to_the_counted_total() {
        let parts = Parts {
            system: 1_000,
            tools: 500,
            summary: 0,
            files: 250,
            conversation: 250,
        };
        let scaled = split(parts, 3_001);
        assert_eq!(
            scaled,
            Parts {
                system: 1_501,
                tools: 750,
                summary: 0,
                files: 375,
                conversation: 375,
            }
        );
        let same = Parts {
            system: 10,
            ..Parts::default()
        };
        assert_eq!(split(same, 10).system, 10);
        assert_eq!(split(Parts::default(), 0), Parts::default());
    }
}
