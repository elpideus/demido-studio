//! What one line of the log is.
//!
//! An event is the smallest thing that happened, and it carries two fields
//! `design/windows.md` requires the data to have before any screen reads it:
//! its [`Source`], because "inspect these records by source" is the axis the
//! brief asks for, and its [`Weight`], because the monitor's second axis is
//! cost and "token weight must be recorded per event, not per request".
//!
//! Both are here rather than added later on purpose. Adding either afterwards
//! means rewriting the slice that wrote the events without them, and a log
//! whose older half cannot answer the question is a log the monitor has to
//! apologise for.

use serde::{Deserialize, Serialize};

use demido_inference::{FinishReason, Options, Role, Usage};

/// Which session a line belongs to.
///
/// Carried on every event even though a journal holds one session, because an
/// export is a projection of these lines and a line that has left its file
/// still has to say what it is part of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

impl From<&str> for SessionId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

/// Who put this in front of the model.
///
/// Exactly the eight `--src-*` tokens `design/tokens.css` declares, with the
/// same names, so the ledger in the monitor and this enum cannot come to
/// disagree about how many there are. A ninth source is a ninth colour, and
/// that is a change to a frozen board rather than a change here.
///
/// [`Source::Reasoning`] is the model's own output, its reply as well as its
/// thinking. There is no separate model source because the monitor puts model
/// output in a lane, and violet is already the colour this design system gives
/// to work the model did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Text Demido wrote: a host paragraph, a tool document.
    System,
    /// Text a skill brought with it.
    Skill,
    /// What the person typed.
    User,
    /// Something Demido put in the window without being asked: a project tree,
    /// a lesson, a file a project attached.
    Inject,
    /// What the model produced.
    Reasoning,
    /// A tool call and what came back from it.
    Tool,
    /// An artifact's content.
    Artifact,
    /// A failure, whether or not the model was shown it.
    Error,
}

/// Where a token count came from.
///
/// The distinction is the whole reason the count is trustworthy. An occupancy
/// bar built out of guesses and an occupancy bar built out of the backend's own
/// numbers look identical on screen, so the difference is carried in the data
/// and the monitor can say which it is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Basis {
    /// A tokeniser counted it: the backend's `usage`, or a weigher that asked
    /// the model's own tokeniser.
    Counted,
    /// Nobody counted it. See [`crate::weight::Estimate`].
    Estimated,
}

/// What one event cost, and how well that is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weight {
    pub tokens: u32,
    pub basis: Basis,
}

impl Weight {
    /// Nothing, counted. What an event that occupies no context weighs.
    pub const NOTHING: Weight = Weight {
        tokens: 0,
        basis: Basis::Counted,
    };

    pub const fn counted(tokens: u32) -> Self {
        Self {
            tokens,
            basis: Basis::Counted,
        }
    }

    pub const fn estimated(tokens: u32) -> Self {
        Self {
            tokens,
            basis: Basis::Estimated,
        }
    }

    /// Two weights added, and an estimate anywhere in a sum makes the sum an
    /// estimate. A total that is mostly counted is still not a count, and
    /// rounding that up to `Counted` is how a bar starts claiming more than it
    /// knows.
    pub fn and(self, other: Weight) -> Self {
        Self {
            tokens: self.tokens.saturating_add(other.tokens),
            basis: match (self.basis, other.basis) {
                (Basis::Counted, Basis::Counted) => Basis::Counted,
                _ => Basis::Estimated,
            },
        }
    }
}

impl std::iter::Sum for Weight {
    fn sum<I: Iterator<Item = Weight>>(iter: I) -> Self {
        iter.fold(Weight::NOTHING, Weight::and)
    }
}

/// One value that filled one placeholder.
///
/// Named rather than a pair, because these are read by a person in the raw JSON
/// tab and a two element array is a riddle where a named object is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filling {
    pub name: String,
    pub value: String,
}

impl Filling {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// What happened.
///
/// The variant names are the event names the rules already use:
/// `docs/rules/prompts.md` fixes `prompt/version` in writing, and the rest
/// follow its shape.
///
/// The one thing to notice is what [`Body::Fragment`] does **not** hold: the
/// text it contributed. It holds the hash of the wording and the values that
/// filled it, so replaying it means filling the paragraph again. A fragment
/// that stored its own output would make the rebuild a copy and prove nothing,
/// which is exactly the difference between rebuilding an assembly and
/// describing one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum Body {
    /// The full text of one host paragraph, written once per session per hash.
    ///
    /// A forty turn session holds one copy of a paragraph; an edit mid session
    /// writes a second entry, and a reply from before the edit stays
    /// explainable (`docs/rules/prompts.md`).
    #[serde(rename = "prompt/version")]
    Version {
        /// The catalog id, so a person reading the log knows what it is.
        id: String,
        /// The digest of the wording, placeholders standing.
        hash: String,
        text: String,
    },

    /// A paragraph placed in an assembly, by hash and by what filled it.
    #[serde(rename = "prompt/fragment")]
    Fragment {
        role: Role,
        hash: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        values: Vec<Filling>,
    },

    /// Something a person or a model said, verbatim.
    #[serde(rename = "chat/message")]
    Message { role: Role, text: String },

    /// The parameter set one turn was sent with.
    #[serde(rename = "turn/parameters")]
    Parameters { model: String, options: Options },

    /// The assembly that went to the backend: the parameters, and the blocks in
    /// the order they were sent.
    ///
    /// Blocks are sequence numbers rather than copies, which is what makes this
    /// log append-only without a second copy of the conversation in it. A block
    /// dropped from the next turn's assembly is an eviction, and the two
    /// `blocks` lists are the diff `design/windows.md` renders. See
    /// `docs/decisions/0009-an-assembly-refers-to-its-blocks.md`.
    #[serde(rename = "turn/assembly")]
    Assembly { parameters: u64, blocks: Vec<u64> },

    /// What came back.
    #[serde(rename = "turn/completion")]
    Completion {
        text: String,
        /// The reasoning, where the model separated it. Recorded because the
        /// monitor shows it, and left out of a rebuilt assembly because it was
        /// never sent back.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        thinking: String,
        reason: FinishReason,
        usage: Usage,
    },

    /// Something failed. The turn it belongs to is on the event.
    #[serde(rename = "turn/failure")]
    Failure {
        /// The machine readable half, from `demido_core::Error::kind` or from
        /// whatever named the failure.
        kind: String,
        detail: String,
    },
}

/// An event with everything but the two fields the journal owns.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub session: SessionId,
    /// Which exchange this belongs to, counting from one. Zero is for what
    /// belongs to the session rather than to a turn.
    pub turn: u32,
    pub source: Source,
    pub weight: Weight,
    pub body: Body,
}

impl Entry {
    pub fn new(session: SessionId, turn: u32, source: Source, weight: Weight, body: Body) -> Self {
        Self {
            session,
            turn,
            source,
            weight,
            body,
        }
    }
}

/// One line of the log.
///
/// `seq` and `at` are the journal's to assign: a writer that chose its own
/// sequence number could write two, and a writer that chose its own clock could
/// write a log that goes backwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Position in the log, from one. Also how a block is referred to.
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    pub session: SessionId,
    pub turn: u32,
    pub source: Source,
    pub weight: Weight,
    #[serde(flatten)]
    pub body: Body,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    fn event(body: Body) -> Event {
        Event {
            seq: 1,
            at: 0,
            session: SessionId::new("s"),
            turn: 1,
            source: Source::User,
            weight: Weight::estimated(3),
            body,
        }
    }

    #[test]
    fn a_line_names_its_event_kind_flat() {
        // The raw JSON tab is the last thing a person reads when the monitor
        // disagrees with them, so a line says what it is on the line rather
        // than inside a nested object.
        let line = serde_json::to_value(event(Body::Message {
            role: Role::User,
            text: "hello".into(),
        }))
        .expect("a value");

        assert_eq!(line["event"], serde_json::json!("chat/message"));
        assert_eq!(line["source"], serde_json::json!("user"));
        assert_eq!(line["weight"]["tokens"], serde_json::json!(3));
        assert_eq!(line["weight"]["basis"], serde_json::json!("estimated"));
    }

    #[test]
    fn a_fragment_records_the_hash_and_the_values_and_not_the_text() {
        // The whole claim of this ticket. A fragment that carried its own
        // output would make the rebuild a copy of what was sent, and a copy
        // proves nothing about whether the log can produce it.
        let line = serde_json::to_value(event(Body::Fragment {
            role: Role::System,
            hash: "sha256:abc".into(),
            values: vec![Filling::new("target", "your reply")],
        }))
        .expect("a value");

        assert_eq!(line["hash"], serde_json::json!("sha256:abc"));
        assert_eq!(line["values"][0]["name"], serde_json::json!("target"));
        assert!(
            line.get("text").is_none(),
            "a fragment that stores its own text is a second copy of the assembly"
        );
    }

    #[test]
    fn an_estimate_anywhere_in_a_sum_makes_the_sum_an_estimate() {
        let total: Weight = [Weight::counted(10), Weight::estimated(5)]
            .into_iter()
            .sum();
        assert_eq!(total.tokens, 15);
        assert_eq!(
            total.basis,
            Basis::Estimated,
            "a total that is mostly counted is still not a count"
        );
    }

    #[test]
    fn nothing_summed_weighs_nothing_and_is_counted() {
        let total: Weight = std::iter::empty().sum();
        assert_eq!(total, Weight::NOTHING);
    }

    #[test]
    fn a_round_trip_through_json_changes_nothing() {
        let original = event(Body::Completion {
            text: "Paris.".into(),
            thinking: String::new(),
            reason: FinishReason::Stop,
            usage: Usage {
                prompt_tokens: 12,
                completion_tokens: 3,
            },
        });
        let line = serde_json::to_string(&original).expect("a line");
        let back: Event = serde_json::from_str(&line).expect("an event");
        assert_eq!(back, original);
    }
}
