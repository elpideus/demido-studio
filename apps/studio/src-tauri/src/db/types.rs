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
    /// A summary of the conversation before it, which the model reads instead (see
    /// `agent::compact`).
    Summary,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
            Role::Summary => "summary",
        }
    }

    pub fn parse(s: &str) -> Role {
        match s {
            "user" => Role::User,
            "tool" => Role::Tool,
            "summary" => Role::Summary,
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
    /// Files sent with a user message; empty for every other message. Stored in their own table.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// The slash command that wrote a user message: the person typed it, the model reads
    /// `content`, what the command expanded to.
    #[serde(default)]
    pub command: Option<CommandUse>,
}

/// A slash command as the person typed it (see `slash`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandUse {
    pub name: String,
    pub args: String,
    /// The skill that provides the command.
    pub skill: Option<String>,
}

/// A file the person attached to a message. Staged while it waits in the composer, then moved
/// into the chat's workspace when the message is sent. Its text and its model-ready image are
/// stored beside it in the database and read only when the prompt needs them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    /// `None` while staged.
    #[serde(skip)]
    pub chat_id: Option<String>,
    #[serde(skip)]
    pub message_id: Option<String>,
    /// File name as the person sees it.
    pub name: String,
    /// Where the file is: relative to the staging folder while staged, to the chat's workspace
    /// once sent.
    #[serde(skip)]
    pub stored: String,
    /// Path inside the chat's workspace once sent (`uploads/report.pdf`).
    pub file: Option<String>,
    /// Absolute path on disk, filled in for the UI (see `attachments::resolve`).
    #[serde(default)]
    pub path: String,
    pub mime: String,
    pub kind: demido_extract::Kind,
    pub size: u64,
    /// PDF pages, slides, or spreadsheet sheets.
    pub pages: Option<u32>,
    /// Estimated tokens of the text the model reads.
    pub tokens: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Something the person should know, shown on the file's chip.
    pub note: Option<String>,
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
            attachments: Vec::new(),
            command: None,
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
