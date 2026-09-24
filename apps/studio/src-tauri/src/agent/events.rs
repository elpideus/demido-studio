use serde::Serialize;

use crate::db::{Chat, Message};

pub const CHAT_EVENT: &str = "chat://event";

/// Everything the chat UI needs to follow a conversation live.
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
    /// Messages from `from_seq` on were removed (regenerate, edit).
    #[serde(rename_all = "camelCase")]
    Truncated { chat_id: String, from_seq: i64 },
    #[serde(rename_all = "camelCase")]
    TurnStarted { chat_id: String },
    #[serde(rename_all = "camelCase")]
    TurnFinished {
        chat_id: String,
        error: Option<String>,
    },
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
