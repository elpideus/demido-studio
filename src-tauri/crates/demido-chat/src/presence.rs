//! Whether there is anything to talk to.
//!
//! The composer is disabled rather than hidden, and it states why
//! (`design/shell.md`). Which sentence it states is the window's to choose, so
//! this carries the fact and never the wording: a model that is still loading
//! and a model that never started are different situations, and a UI that
//! cannot tell them apart shows a person an idle composer while their weights
//! are coming off a disk.
//!
//! [`Presence::Failed`] is the exception, and it carries the backend's own
//! sentence, because "the model file is for a newer GGUF version" is not
//! something the frontend can derive from a tag. That is the string
//! [`demido_inference::LlamaCpp`] went to the trouble of keeping, and dropping
//! it here would turn a fix into a bug report.

use serde::Serialize;

/// What the desk can say about the model right now.
///
/// Serialised tagged, so the window branches on `state` and reads the fields
/// that variant carries rather than sniffing for a null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Presence {
    /// Nothing is configured to answer. Not a failure: it is a fresh install
    /// before the set-up wizard has run, which is the normal first launch.
    Absent,
    /// A backend is starting. Several gigabytes off a cold disk is genuinely
    /// slow, and a person who is not told that reads it as a broken app.
    Loading { model: String },
    /// It is answering.
    Ready { model: String },
    /// It did not start, or it died. The desk stays usable: startup never
    /// blocks and a subsystem that fails is reported and skipped (`AGENTS.md`).
    Failed { detail: String },
}

impl Presence {
    /// Whether a turn can be sent right now.
    ///
    /// One place, because the composer, the send command and the turn loop all
    /// ask it, and three implementations of "can I talk yet" is how a disabled
    /// button and a refused command come to disagree.
    pub fn is_ready(&self) -> bool {
        matches!(self, Presence::Ready { .. })
    }

    /// The model that is loaded or loading, where there is one.
    pub fn model(&self) -> Option<&str> {
        match self {
            Presence::Loading { model } | Presence::Ready { model } => Some(model),
            Presence::Absent | Presence::Failed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn only_a_loaded_model_is_ready() {
        assert!(Presence::Ready {
            model: "tiny".into()
        }
        .is_ready());
        assert!(!Presence::Loading {
            model: "tiny".into()
        }
        .is_ready());
        assert!(!Presence::Absent.is_ready());
        assert!(!Presence::Failed {
            detail: "it does not fit".into()
        }
        .is_ready());
    }

    /// The window branches on the tag and reads the fields beside it. A shape
    /// that made the frontend sniff for a null would put the state machine in
    /// two places.
    #[test]
    fn a_state_is_a_tag_and_the_fields_that_state_carries() {
        let loading = serde_json::to_value(Presence::Loading {
            model: "gemma".into(),
        })
        .expect("a value");
        assert_eq!(loading["state"], serde_json::json!("loading"));
        assert_eq!(loading["model"], serde_json::json!("gemma"));

        let absent = serde_json::to_value(Presence::Absent).expect("a value");
        assert_eq!(absent["state"], serde_json::json!("absent"));
        assert!(absent.get("model").is_none());
    }

    /// A failure keeps the backend's own sentence. It is the difference between
    /// "inference did not start" and a fix.
    #[test]
    fn a_failure_carries_what_the_backend_said() {
        let failed = Presence::Failed {
            detail: "the model file is for a newer GGUF version".into(),
        };
        assert_eq!(
            failed.model(),
            None,
            "nothing is loaded, so nothing is named"
        );
        let value = serde_json::to_value(&failed).expect("a value");
        assert_eq!(
            value["detail"],
            serde_json::json!("the model file is for a newer GGUF version")
        );
    }
}
