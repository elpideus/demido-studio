use serde::Serialize;

use crate::db::{Chat, Message};

pub const CHAT_EVENT: &str = "chat://event";

/// Everything the chat UI needs to follow a conversation live.
///
/// Events are serialized the moment they are created and never stored, so the size of the
/// `Message` variant costs nothing worth boxing it for.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    /// A message was created or changed; the full row replaces the UI's copy.
    #[serde(rename_all = "camelCase")]
    Message { chat_id: String, message: Message },
    /// Text streamed into a message since the previous delta.
    #[serde(rename_all = "camelCase")]
    Delta {
        chat_id: String,
        message_id: String,
        content: String,
        reasoning: String,
    },
    /// The model started writing a tool call.
    #[serde(rename_all = "camelCase")]
    ToolCall {
        chat_id: String,
        message_id: String,
        name: String,
    },
    /// The model was busy: what streamed into the message is void, and the request goes again in
    /// `wait_ms`, as retry `attempt` of `attempts`.
    #[serde(rename_all = "camelCase")]
    Retrying {
        chat_id: String,
        message_id: String,
        attempt: u32,
        attempts: u32,
        wait_ms: u64,
        reason: String,
    },
    /// Messages from `from_seq` on were removed (regenerate, edit).
    #[serde(rename_all = "camelCase")]
    Truncated { chat_id: String, from_seq: i64 },
    #[serde(rename_all = "camelCase")]
    TurnStarted { chat_id: String },
    #[serde(rename_all = "camelCase")]
    TurnFinished { chat_id: String, error: Option<String> },
    /// Chat metadata changed (title, model, recency).
    Chat { chat: Chat },
    /// Something the person should notice (for example: sign in to TradingView).
    #[serde(rename_all = "camelCase")]
    Notice {
        chat_id: String,
        kind: String,
        text: String,
    },
}
