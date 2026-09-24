use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub model_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Role {
        match s {
            "user" => Role::User,
            "tool" => Role::Tool,
            _ => Role::Assistant,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MessageStatus {
    /// The model is still writing this message.
    Streaming,
    /// A tool call is waiting for the person to allow or deny it.
    AwaitingApproval,
    /// A tool is running.
    Running,
    Done,
    Error,
    Cancelled,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageStatus::Streaming => "streaming",
            MessageStatus::AwaitingApproval => "awaitingApproval",
            MessageStatus::Running => "running",
            MessageStatus::Done => "done",
            MessageStatus::Error => "error",
            MessageStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> MessageStatus {
        match s {
            "streaming" => MessageStatus::Streaming,
            "awaitingApproval" => MessageStatus::AwaitingApproval,
            "running" => MessageStatus::Running,
            "error" => MessageStatus::Error,
            "cancelled" => MessageStatus::Cancelled,
            _ => MessageStatus::Done,
        }
    }
}

/// A tool call as the model requested it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments exactly as the model produced them.
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub seq: i64,
    pub role: Role,
    pub content: String,
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    /// What the UI shows for a tool message (arguments summary, files, previews).
    pub tool_result: Option<serde_json::Value>,
    pub model_id: Option<String>,
    pub status: MessageStatus,
    pub error: Option<String>,
    /// Token counts, speed and timings of the generation that produced this message.
    pub stats: Option<serde_json::Value>,
    /// Provider-specific data needed to replay the message (Gemini thought signatures).
    pub provider_meta: Option<serde_json::Value>,
    pub created_at: i64,
}

impl Message {
    pub fn new(chat_id: &str, seq: i64, role: Role, content: impl Into<String>) -> Self {
        Message {
            id: super::new_id(),
            chat_id: chat_id.to_string(),
            seq,
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            tool_result: None,
            model_id: None,
            status: MessageStatus::Done,
            error: None,
            stats: None,
            provider_meta: None,
            created_at: super::now_ms(),
        }
    }
}

/// Everything sent to and received from the model for one generation. This is the
/// transparency record: the Inspector window shows it verbatim.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub id: String,
    pub chat_id: String,
    pub message_id: Option<String>,
    pub created_at: i64,
    pub model_id: Option<String>,
    pub request: serde_json::Value,
    pub response: Option<serde_json::Value>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
}
