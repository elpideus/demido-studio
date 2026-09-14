//! What the window is told while a turn runs.
//!
//! One enum, emitted as it happens, and deliberately **not** the whole answer
//! so far. A stream that re-sent the accumulated text on every token would make
//! the window's cost quadratic in the length of an answer, and it would make
//! the frontend's job "replace" rather than "append", which is the job that
//! cannot be done without re-rendering everything.
//!
//! [`Update::Done`] carries the finished text all the same, and that is not a
//! contradiction. It is what the log recorded, for whoever is listening; the
//! desk reads the log back instead, because its own bubble needs a sequence
//! number as well. Either way what the window accumulated is a draft, per
//! `docs/decisions/0011-the-window-draws-the-log-not-its-draft.md`.

use serde::Serialize;

use demido_inference::FinishReason;

/// One thing that happened during a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "update", rename_all = "kebab-case")]
pub enum Update {
    /// Visible output, as it arrives.
    Text { text: String },
    /// Reasoning, where the model separates it. Visible by default
    /// (`design/system.md`), and it is the window that decides that.
    Thinking { text: String },
    /// The turn is over, however it ended. A stop arrives here too, carrying
    /// [`FinishReason::Cancelled`] and whatever was generated before it.
    Done {
        turn: u32,
        /// Where the answer sits on the log, which is what the transcript keys
        /// a bubble by.
        seq: u64,
        text: String,
        thinking: String,
        reason: FinishReason,
    },
    /// The turn did not produce an answer. The sentence is the backend's own.
    Failed { turn: u32, detail: String },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    /// A token carries the token and nothing else. The window appends; it is
    /// never handed the answer so far and asked to spot the difference.
    #[test]
    fn a_token_is_a_token_rather_than_the_answer_so_far() {
        let value = serde_json::to_value(Update::Text {
            text: " Paris".into(),
        })
        .expect("a value");
        assert_eq!(value["update"], serde_json::json!("text"));
        assert_eq!(value["text"], serde_json::json!(" Paris"));
        assert_eq!(
            value.as_object().map(|fields| fields.len()),
            Some(2),
            "an update that carried anything else would be state the window \
             could disagree with the log about"
        );
    }

    #[test]
    fn a_stop_is_a_done_that_says_it_was_stopped() {
        let value = serde_json::to_value(Update::Done {
            turn: 1,
            seq: 4,
            text: "Par".into(),
            thinking: String::new(),
            reason: FinishReason::Cancelled,
        })
        .expect("a value");
        assert_eq!(value["update"], serde_json::json!("done"));
        assert_eq!(value["reason"], serde_json::json!("cancelled"));
        assert_eq!(
            value["text"],
            serde_json::json!("Par"),
            "what was generated before the stop is real output and is kept"
        );
    }
}
