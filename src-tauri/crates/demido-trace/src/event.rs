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

/// Which agent produced a line: the conversation itself, or one of its
/// sub-agents.
///
/// On **every** event from the first line of code that records one, for the
/// same reason [`Source`] and [`Weight`] are: the monitor's agent scope is a
/// filter over one stream, and a field the older half of a log does not carry
/// is a filter that has to apologise. A sub-agent is a scope on this log rather
/// than a log of its own
/// (`docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    /// What the conversation's own agent is called, which the monitor's way
    /// back out of a scope is named after.
    pub const MAIN: &'static str = "main";

    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The conversation itself: what every event of the main session carries.
    pub fn main() -> Self {
        Self::new(Self::MAIN)
    }

    /// The agent a delegation opens, named after the call that opened it.
    ///
    /// Derived rather than minted, because a call is one event and two
    /// delegations are two calls: there is no counter to get wrong and no way
    /// for two children of one session to answer to the same name. A reader who
    /// has an agent has the call without a projection, and the projection
    /// ([`crate::Replay::agents`]) exists for the other direction.
    pub fn delegated(call: u64) -> Self {
        Self(format!("agent-{call}"))
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

impl From<&str> for AgentId {
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

/// One tool in an offered set: which tool, and which wording of it.
///
/// Named rather than a pair for the same reason [`Filling`] is: the raw JSON
/// tab is read by a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub name: String,
    /// The hash of the tool's document, description and parameter prose
    /// together. Its text is the `tool/version` recorded under it.
    pub hash: String,
    /// The tool's JSON Schema with no prose on it: the half of what was offered
    /// that is a contract with the parser rather than wording. The rebuild puts
    /// the document's prose back on it ([`demido_prompts::describe`]), so the
    /// wording is still held once per session rather than once per change.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub shape: serde_json::Value,
}

/// What the person said about one call they were asked about.
///
/// Three, and each is its own answer rather than a flag on another: the log has
/// to say which of the three happened, because *always* changes what the next
/// call is asked and the other two do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Decision {
    /// Run this one.
    Allow,
    /// Do not run it. The model is told, and gets to do something else.
    Deny,
    /// Run this one, and do not ask again about this tool in this chat. Never
    /// covers a destructive call (`docs/rules/tools.md`).
    Always,
}

/// What decided the offered set, so the monitor can tell a deliberate absence
/// from a dropped one (`docs/rules/tools.md`).
///
/// The registry, and the settings ladder's four tiers, which is where the
/// picker writes ([#56](https://github.com/elpideus/demido-studio/issues/56)).
/// A skill's switch, an account an endpoint may not receive and a sub-agent
/// narrowing its parent each add a variant with the code that decides, rather
/// than being declared here as a shape nothing can yet be held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// What this build registers, and whether a workspace is set for it to
    /// act in, with nobody on the ladder having named a set. No workspace
    /// offers nothing.
    Registry,
    /// A set named for every conversation.
    Global,
    /// A set named for one model. Stored, and nothing names a model in v0.1.
    Model,
    /// A set named by a character. Stored, and there are no characters in v0.1.
    Character,
    /// A set named for this conversation, from the picker. The last word.
    Chat,
    /// Nobody named this one. Demido withheld every tool for the rest of a turn,
    /// because the step before it ran nothing: every call it made came back
    /// refused, and another step with the same options would come back refused
    /// the same way.
    ///
    /// It is a layer rather than an empty set with somebody else's name on it
    /// because the monitor has to be able to say so. A reader who finds no tools
    /// in the fourth step of a turn that had six in the first has a right to
    /// know which of the three reasons it was, and the other two are
    /// [`Layer::Registry`] and the ladder.
    Withheld,
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

    /// The full text of one host tool's document, written once per session per
    /// hash, exactly as [`Body::Version`] is for a paragraph.
    ///
    /// A separate event rather than a `prompt/version` with a tool name in its
    /// `id`, because a tool name and a paragraph id are two namespaces
    /// (`docs/rules/prompts.md`).
    #[serde(rename = "tool/version")]
    ToolVersion {
        name: String,
        hash: String,
        text: String,
    },

    /// The set of tools on offer changed, and what changed it.
    ///
    /// Written when the set changes, never on every turn: the set in force at
    /// any event is the last one of these before it. A set is a name and a
    /// hash per tool, in the order they are offered, so the same names in new
    /// wording is a change too.
    #[serde(rename = "tools/offered")]
    Offered { tools: Vec<Offer>, layer: Layer },

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
    ///
    /// `tools` is the `tools/offered` event in force when it was sent, named
    /// the way `parameters` names its parameter set, so the tools a request
    /// carried are rebuilt from the set it was sent with rather than from
    /// whichever set happens to come before it. `None` offered nothing.
    #[serde(rename = "turn/assembly")]
    Assembly {
        parameters: u64,
        blocks: Vec<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tools: Option<u64>,
    },

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

    /// One call the model asked for, as it asked.
    ///
    /// Its own event rather than a field on the completion, so a call and its
    /// result are two lines each with a source and a weight. `completion` is
    /// the answer that asked for it, which is where the rebuild puts it back:
    /// on that assistant message, in the order the calls were written.
    #[serde(rename = "tool/call")]
    Call {
        completion: u64,
        id: String,
        name: String,
        /// The model's own text, whether or not it parses.
        arguments: String,
    },

    /// The person was asked about a call, and answered. `call` is the call's
    /// event. A call nobody was asked about has none of these.
    #[serde(rename = "tool/decision")]
    Decided { call: u64, decision: Decision },

    /// What came back from a call that was attempted, verbatim.
    ///
    /// `failed` is the tool's own outcome: a command that exited non-zero, a
    /// path that was refused, a call that named no tool. Never a non-empty
    /// stderr (`docs/rules/lessons.md`), and never a denial, which is a
    /// [`Body::Refusal`] because nothing was attempted.
    #[serde(rename = "tool/result")]
    Result {
        call: u64,
        text: String,
        failed: bool,
    },

    /// Demido's own answer to a call it did not run: declined, stopped, past
    /// the step limit, or past the delegation depth.
    ///
    /// Recorded like a fragment, by the hash of the paragraph and what filled
    /// it, because it is host prompt text the model reads and the rebuild fills
    /// it again rather than copying it.
    #[serde(rename = "tool/refusal")]
    Refusal {
        call: u64,
        hash: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        values: Vec<Filling>,
    },

    /// A sub-agent was opened, by the call at `call`.
    ///
    /// Written by the **parent**, because what happened in the parent's stream
    /// is that it delegated; everything the child then does carries the child's
    /// own agent and is read by scoping to it. The parent is therefore implicit
    /// and is not a field: it is the agent of this event.
    ///
    /// `depth` counts **up** from the main session at zero, because it is the
    /// indent `design/windows.md` renders the chain as. The number
    /// `demido_permission::Resolution` carries is the other one: how many
    /// levels remain below, counting down to the limit. Two directions, two
    /// jobs, and neither is derivable from the other without the setting that
    /// was in force at dispatch.
    #[serde(rename = "agent/delegated")]
    Delegated {
        call: u64,
        /// The child. **`child` on the wire**, because a body is flattened onto
        /// the line and the line already has an `agent`, which is the parent
        /// that wrote it. Two fields of one name is a line that writes and will
        /// not read back, which is what a session log that had ever carried a
        /// delegation did before
        /// [#67](https://github.com/elpideus/demido-studio/issues/67) drove one
        /// through a window. The in-memory name stays `agent` because that is
        /// what it is, and the rename is where the collision is.
        #[serde(rename = "child")]
        agent: AgentId,
        depth: u32,
    },

    /// A background delegation's answer was folded into the turn that asked for
    /// it, at a step boundary.
    ///
    /// **Only** where a background answer is folded in. At the default
    /// parallelism the call blocks and the answer is the tool's own result, and
    /// a second record of one answer is a log that can disagree with itself
    /// about what came back.
    ///
    /// `answer` is the child's completion, by position. The text is the
    /// child's event and this names it, for the reason an assembly names its
    /// blocks rather than copying them
    /// (`docs/decisions/0009-an-assembly-refers-to-its-blocks.md`). What the
    /// parent's model is then shown is a framed message, which is a fragment
    /// like any other ([#66](https://github.com/elpideus/demido-studio/issues/66)).
    #[serde(rename = "agent/returned")]
    Returned {
        call: u64,
        /// The child whose answer this is, `child` on the wire for the reason
        /// [`Body::Delegated`]'s is.
        #[serde(rename = "child")]
        agent: AgentId,
        answer: u64,
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
    /// Who produced it: the conversation, or one of its sub-agents.
    pub agent: AgentId,
    /// Which exchange this belongs to, counting from one. Zero is for what
    /// belongs to the session rather than to a turn.
    ///
    /// **An agent's own count.** A sub-agent runs the agent loop again, so its
    /// exchanges are its own and start at one; two events with the same turn
    /// number and different agents are two different exchanges.
    pub turn: u32,
    pub source: Source,
    pub weight: Weight,
    pub body: Body,
}

impl Entry {
    pub fn new(
        session: SessionId,
        agent: AgentId,
        turn: u32,
        source: Source,
        weight: Weight,
        body: Body,
    ) -> Self {
        Self {
            session,
            agent,
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
    pub agent: AgentId,
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
            agent: AgentId::main(),
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
        assert_eq!(line["agent"], serde_json::json!("main"));
        assert_eq!(line["weight"]["tokens"], serde_json::json!(3));
        assert_eq!(line["weight"]["basis"], serde_json::json!("estimated"));
    }

    #[test]
    fn a_delegation_names_the_parent_and_the_child_without_colliding() {
        // The line carries two agents: the one that wrote it, which is the
        // parent, and the one it opened. They are one field name apart, and
        // before they were, a log that had ever carried a delegation wrote
        // `agent` twice and refused to read itself back, with the whole desk
        // reporting an invalid session log (#67).
        let original = event(Body::Delegated {
            call: 42,
            agent: AgentId::delegated(42),
            depth: 1,
        });
        let line = serde_json::to_string(&original).expect("a line");
        let value: serde_json::Value = serde_json::from_str(&line).expect("a value");
        assert_eq!(value["agent"], serde_json::json!("main"), "who wrote it");
        assert_eq!(
            value["child"],
            serde_json::json!("agent-42"),
            "and who it opened"
        );

        let back: Event = serde_json::from_str(&line).expect("an event");
        assert_eq!(back, original, "a delegation reads back off the line");
    }

    #[test]
    fn an_answer_folded_in_reads_back_off_the_line_too() {
        let original = event(Body::Returned {
            call: 42,
            agent: AgentId::delegated(42),
            answer: 51,
        });
        let line = serde_json::to_string(&original).expect("a line");
        let back: Event = serde_json::from_str(&line).expect("an event");
        assert_eq!(back, original);
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
