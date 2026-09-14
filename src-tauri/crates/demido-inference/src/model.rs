//! What a request to a model looks like, and what comes back.
//!
//! Deliberately smaller than any provider's API, and smaller again than v2's,
//! which carried grammars and twenty four samplers. Tool calling arrived with
//! S2 ([#54](https://github.com/elpideus/demido-studio/issues/54)) the way this
//! file said it would, additively: [`Chunk::Call`], [`Request::tools`], a
//! [`Role::Tool`] message answering a call, and a contract case saying what a
//! backend must do with them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who said it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    /// What came back from a call, going back to the model. A message in this
    /// role names the call it answers ([`Message::answers`]).
    Tool,
}

/// One call the model asked for, whole.
///
/// The arguments are the text the model produced rather than a parsed value:
/// a call that never parsed still has to be answered, and what it needs is an
/// objection naming what the parser found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// What the backend called this call. A result is tied to it by this, so a
    /// backend never hands one out blank.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One tool a request offers: its name, what it is for, and the JSON Schema of
/// its arguments with their prose on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// One turn of the conversation, as the model will see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// The calls an assistant message asked for, in the order it asked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<ToolCall>,
    /// For a [`Role::Tool`] message, the id of the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<String>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            calls: Vec::new(),
            answers: None,
        }
    }

    /// What the model said, and the calls it asked for with it.
    pub fn calling(content: impl Into<String>, calls: Vec<ToolCall>) -> Self {
        Self {
            calls,
            ..Self::assistant(content)
        }
    }

    /// What came back from the call with this id.
    pub fn result(call: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            answers: Some(call.into()),
            ..Self::new(Role::Tool, content)
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }
}

/// Everything that shapes one generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// What the backend calls the model. A backend that serves one model
    /// refuses any other name rather than answering with what it has.
    pub model: String,
    pub messages: Vec<Message>,
    /// What the model may call. Empty is a request that offers nothing, which
    /// is a conversation with nothing to do but answer rather than a different
    /// kind of request (`docs/rules/tools.md`: there is no Chat mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    pub options: Options,
}

/// Sampling settings, resolved through the settings ladder before they get
/// here.
///
/// **Every sampler is an `Option`, and `None` means the request does not carry
/// it.** That is not the same as zero. v2 conflated the two, with zero meaning
/// "do not send", so the settings page could not ask for a literal zero and
/// could not ask for a sampler to be left alone at all. On llama.cpp those are
/// different generations.
///
/// Only the settings S1 exposes are here, plus `seed`, which the contract suite
/// needs to ask the same question twice, and `max_tokens`, which is how a test
/// makes a stream long enough to cancel in the middle of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// How far the model strays from its likeliest next token.
    ///
    /// Written even when it is `None`, unlike the rest, because its default is
    /// a value rather than absence: `#[serde(default)]` filling in a missing
    /// key is right for an old log and wrong for a new one that meant "off",
    /// and the session log is replayed.
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Fix the sampler, so the same request answers the same way. `None` is
    /// random, which is the normal case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            temperature: Some(0.7),
            max_tokens: None,
            seed: None,
        }
    }
}

/// One piece of a streamed response.
///
/// Every chunk is recorded in the session log, which is what makes replay
/// possible, and that is why this enum stays narrow: each variant is a thing
/// the log must be able to reproduce exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Chunk {
    /// Visible output.
    Text { text: String },
    /// Reasoning, where the model separates it from its answer.
    Thinking { text: String },
    /// A call, whole. A backend that receives one in pieces assembles it
    /// before handing it on, and a call cut off by a cancel is never handed on:
    /// half an argument list is not something anybody can run or answer.
    Call { call: ToolCall },
    /// The generation finished. Always the last chunk of a successful stream.
    Done { reason: FinishReason, usage: Usage },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model stopped on its own.
    Stop,
    /// It hit the token ceiling, so the answer is cut off.
    Length,
    /// It stopped to have its calls run. Said whenever a stream carried a
    /// [`Chunk::Call`] and was not cancelled, so a caller never has to count
    /// calls to learn that the turn is not over.
    ToolCalls,
    /// The caller cancelled. The partial answer before it is real output and is
    /// kept: `done.md`'s window gate is a person pressing stop, and a
    /// transcript that discards what was already on screen does not match what
    /// happened.
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// The model a backend is serving, as it can describe it without asking anyone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loaded {
    /// What the caller must put in `Request::model`.
    pub id: String,
    /// Bytes on disk, where the backend loaded a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn a_sampler_nobody_set_is_absent_rather_than_zero() {
        let options = Options {
            temperature: None,
            ..Options::default()
        };
        let wire = serde_json::to_value(&options).expect("a value");

        assert_eq!(
            wire.get("temperature"),
            Some(&serde_json::Value::Null),
            "temperature is written even when absent, so a replayed log can tell \
             'leave it alone' from 'the key predates the field'"
        );
        assert!(
            wire.get("seed").is_none(),
            "a sampler the request does not carry is not in the request"
        );
    }

    #[test]
    fn a_sampler_set_to_zero_is_sent_as_zero() {
        let options = Options {
            temperature: Some(0.0),
            ..Options::default()
        };
        let wire = serde_json::to_value(&options).expect("a value");
        assert_eq!(wire["temperature"], serde_json::json!(0.0));
    }
}
