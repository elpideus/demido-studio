//! The writing side: how a turn puts itself in the log.
//!
//! Everything that composes a prompt records it here, and the request it sends
//! is what [`Turn::send`] hands back. That ordering is the point: a caller
//! cannot send an assembly it did not record, because the assembly is the thing
//! recording produced. A composer that built a request and then described it to
//! a log is a composer that will one day describe it wrongly, and nothing would
//! notice until somebody replayed a session and got a different answer.
//!
//! What is recorded is deliberately not the finished prompt. A paragraph goes
//! in as its hash and the values that filled it, so the log rebuilds the text
//! rather than storing a copy of it (`design/windows.md`: "the event stream
//! must be sufficient to rebuild an assembly, not merely to describe one").

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use demido_inference::{FinishReason, Message, Options, Request, Role, ToolCall, ToolSpec, Usage};
use demido_prompts::{Document, Prompt};

use crate::event::{
    AgentId, Body, Decision, Entry, Event, Filling, Layer, Offer, SessionId, Source, Weight,
};
use crate::journal::{Error, Journal, Result};
use crate::replay::Replay;
use crate::weight::{Estimate, Weigher};

/// One conversation, recording itself.
///
/// Holds the journal, the weigher, and the bookkeeping that makes the log
/// cheap: which turn is next, which paragraph and tool wordings have already
/// been written out in full this session, and the tool set last recorded as
/// offered.
///
/// **All of that bookkeeping is per agent**, and a sub-agent starts with none
/// of it. A child that skipped a wording because its parent had already written
/// it would be a child whose assembly rebuilds only while the parent's half of
/// the log is there beside it, and the promise is that a child's assembly
/// rebuilds out of the child's own events.
pub struct Session<J: Journal> {
    id: SessionId,
    /// Whose events these are: the conversation, or one sub-agent of it.
    ///
    /// Every recording site goes through [`Session::write`], so this is stamped
    /// in one place and there is nowhere else to stamp it from. A sub-agent is a
    /// second `Session` over the same journal rather than a second journal,
    /// which is what makes the monitor's scope a filter over one stream
    /// (`docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md`).
    agent: AgentId,
    /// How far down the chain this agent is: zero for the conversation, and the
    /// indent for everything below it.
    depth: u32,
    journal: J,
    /// Shared rather than owned, because a child weighs its events the way its
    /// parent does: a sub-agent whose weights were estimates while its parent's
    /// were counted would put two bases in one ledger for no reason anybody
    /// could see.
    weigher: Arc<dyn Weigher>,
    versioned: Mutex<BTreeSet<String>>,
    /// Kept apart from `versioned` because a `tool/version` and a
    /// `prompt/version` are different events, and one register's hash being
    /// written must never excuse the other's.
    documented: Mutex<BTreeSet<String>>,
    /// The last `tools/offered`: where it was written, and the set it holds.
    offered: Mutex<Option<(u64, Vec<Offer>)>>,
    turns: AtomicU32,
}

impl<J: Journal> Session<J> {
    /// A session that weighs its events by estimate.
    pub fn new(id: impl Into<SessionId>, journal: J) -> Self {
        Self::weighed_by(id, journal, Estimate)
    }

    /// A session that weighs its events with something that can count them.
    ///
    /// **This is the wiring line for the cost axis.** The default is an
    /// estimate and says so on every event; handing this the running backend's
    /// tokeniser turns the whole log counted without any recording site
    /// changing.
    pub fn weighed_by(
        id: impl Into<SessionId>,
        journal: J,
        weigher: impl Weigher + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            agent: AgentId::main(),
            depth: 0,
            journal,
            weigher: Arc::new(weigher),
            versioned: Mutex::new(BTreeSet::new()),
            documented: Mutex::new(BTreeSet::new()),
            offered: Mutex::new(None),
            turns: AtomicU32::new(0),
        }
    }

    /// Whose events this records.
    pub fn agent(&self) -> &AgentId {
        &self.agent
    }

    /// How far down the chain of delegations this agent is, counting up from
    /// the conversation at zero.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// The log underneath, for replaying it.
    pub fn journal(&self) -> &J {
        &self.journal
    }

    /// Start recording the next exchange.
    pub fn begin(&self) -> Turn<'_, J> {
        Turn {
            session: self,
            number: self.turns.fetch_add(1, Ordering::SeqCst) + 1,
            blocks: Vec::new(),
            parameters: None,
            tools: Vec::new(),
            offered: None,
        }
    }

    /// Pick a session back up from the log it already has.
    ///
    /// Restores both pieces of bookkeeping, and both are **derived from the
    /// events** rather than remembered, because anything kept elsewhere is
    /// something that can disagree with the log:
    ///
    /// - the turn to number next, so a resumed session does not write turn 1
    ///   over the top of an old one;
    /// - which wordings have already been written out in full, so the first
    ///   fragment after a restart does not emit a second `prompt/version` for
    ///   a hash the log already holds. `docs/rules/prompts.md` says once per
    ///   session per hash, and a session that came back from disk is the same
    ///   session. Tool documents the same way;
    /// - the tool set last offered, so an unchanged set after a restart is not
    ///   recorded as a change nobody made.
    ///
    /// **Its own events, and no other agent's.** A conversation that took its
    /// turn number from a sub-agent's log would number its next exchange after
    /// somebody else's, and one that took a sub-agent's written wordings for
    /// its own would stop writing paragraphs its own assemblies name.
    pub fn resume(&self) -> Result<()> {
        let events: Vec<Event> = self
            .journal
            .events()?
            .into_iter()
            .filter(|event| event.agent == self.agent)
            .collect();

        self.turns.store(
            events.iter().map(|event| event.turn).max().unwrap_or(0),
            Ordering::SeqCst,
        );

        let mut versioned = held(&self.versioned);
        let mut documented = held(&self.documented);
        let mut offered = held(&self.offered);
        for event in &events {
            match &event.body {
                Body::Version { hash, .. } => {
                    versioned.insert(hash.clone());
                }
                Body::ToolVersion { hash, .. } => {
                    documented.insert(hash.clone());
                }
                Body::Offered { tools, .. } => *offered = Some((event.seq, tools.clone())),
                _ => {}
            }
        }

        Ok(())
    }

    /// What came back from the model.
    ///
    /// Weighed by the backend's own count rather than by the weigher: this is
    /// the one number nobody has to estimate.
    pub fn completed(
        &self,
        sent: &Sent,
        text: &str,
        thinking: &str,
        reason: FinishReason,
        usage: Usage,
    ) -> Result<u64> {
        let event = self.write(
            sent.turn,
            Source::Reasoning,
            Weight::counted(usage.completion_tokens),
            Body::Completion {
                text: text.to_owned(),
                thinking: thinking.to_owned(),
                reason,
                usage,
            },
        )?;
        Ok(event.seq)
    }

    /// One call an answer asked for. `completion` is that answer's position.
    ///
    /// Weighed as the name and the arguments, the text the model spent on it.
    pub fn called(&self, turn: u32, completion: u64, call: &ToolCall) -> Result<u64> {
        let weight = self
            .weigher
            .weigh(&call.name)
            .and(self.weigher.weigh(&call.arguments));
        let event = self.write(
            turn,
            Source::Tool,
            weight,
            Body::Call {
                completion,
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            },
        )?;
        Ok(event.seq)
    }

    /// What the person answered when asked about the call at `call`.
    ///
    /// Weighs nothing: a decision is never sent to the model. What the model is
    /// told about a denial is the refusal written after it.
    pub fn decided(&self, turn: u32, call: u64, decision: Decision) -> Result<u64> {
        let event = self.write(
            turn,
            Source::User,
            Weight::NOTHING,
            Body::Decided { call, decision },
        )?;
        Ok(event.seq)
    }

    /// What came back from the call at `call`, verbatim.
    pub fn returned(&self, turn: u32, call: u64, text: &str, failed: bool) -> Result<u64> {
        let event = self.write(
            turn,
            Source::Tool,
            self.weigher.weigh(text),
            Body::Result {
                call,
                text: text.to_owned(),
                failed,
            },
        )?;
        Ok(event.seq)
    }

    /// Demido's answer to a call it did not run, in a paragraph's wording.
    ///
    /// Recorded the way a fragment is: the wording once per session, then the
    /// hash and what filled it, so the rebuild fills it again.
    pub fn refused(
        &self,
        turn: u32,
        call: u64,
        prompt: &Prompt,
        values: &[(&str, &str)],
    ) -> Result<u64> {
        self.version(turn, prompt)?;
        let event = self.write(
            turn,
            Source::System,
            self.weigher.weigh(&prompt.fill(values)),
            Body::Refusal {
                call,
                hash: prompt.hash.clone(),
                values: values
                    .iter()
                    .map(|(name, value)| Filling::new(*name, *value))
                    .collect(),
            },
        )?;
        Ok(event.seq)
    }

    /// Send the same turn again, carrying what its last step produced.
    ///
    /// A turn that used a tool is sent once per step: the answer that asked for
    /// the calls and what came back from each go on the end of the assembly it
    /// was sent with, under the same parameters and the same offered set, and
    /// the new assembly is recorded before it is handed back to send. `blocks`
    /// are positions this session already wrote, in the order the model should
    /// read them.
    pub fn step(&self, sent: &Sent, blocks: &[u64], offering: NextStep) -> Result<Sent> {
        let replay = Replay::of(&self.journal)?;
        let mut request = sent.request.clone();
        for seq in blocks {
            request.messages.push(replay.block(*seq)?);
        }
        let mut named = sent.blocks.clone();
        named.extend_from_slice(blocks);

        let offered = match offering {
            NextStep::Offering => sent.offered,
            NextStep::Withholding => {
                request.tools.clear();
                Some(self.withhold(sent.turn)?)
            }
        };

        let event = self.write(
            sent.turn,
            Source::System,
            // As for the first assembly: its blocks carry their own weights.
            Weight::NOTHING,
            Body::Assembly {
                parameters: sent.parameters,
                blocks: named.clone(),
                tools: offered,
            },
        )?;

        Ok(Sent {
            turn: sent.turn,
            seq: event.seq,
            request,
            parameters: sent.parameters,
            blocks: named,
            offered,
        })
    }

    /// Record that this turn is offering nothing from here on, and say who
    /// decided it.
    ///
    /// An empty set under [`Layer::Withheld`], which is a set like any other:
    /// the assembly names it, the rebuild reads it back, and the monitor can
    /// tell it from an empty registry and from a picker with everything
    /// switched off. It also replaces what [`Turn::offer`] compares against, so
    /// the next turn's ordinary set is a change again and is written rather
    /// than deduplicated against a set that is two events old.
    fn withhold(&self, turn: u32) -> Result<u64> {
        let mut last = held(&self.offered);
        let event = self.write(
            turn,
            Source::System,
            Weight::NOTHING,
            Body::Offered {
                tools: Vec::new(),
                layer: Layer::Withheld,
            },
        )?;
        *last = Some((event.seq, Vec::new()));
        Ok(event.seq)
    }

    /// Something went wrong, in this turn or beside it.
    ///
    /// Weighed like anything else, because a failure the model is shown costs
    /// context exactly like a tool result does, and one it is not shown costs
    /// nothing and reads as zero.
    pub fn failed(&self, turn: u32, kind: &str, detail: &str) -> Result<u64> {
        let event = self.write(
            turn,
            Source::Error,
            self.weigher.weigh(detail),
            Body::Failure {
                kind: kind.to_owned(),
                detail: detail.to_owned(),
            },
        )?;
        Ok(event.seq)
    }

    /// Open a sub-agent, and record that this one opened it.
    ///
    /// The event and the recorder come out together, so there is no way to get
    /// a child that is not in the log and no way to write a delegation that
    /// opened nothing. `call` is the `tool/call` that asked for it, which is
    /// also what the child is named after.
    ///
    /// The child is the same conversation, the same journal and the same
    /// weigher, with its own agent, its own depth and its own bookkeeping. What
    /// it may offer and the mode it runs under are not here: that is the
    /// inheritance rule (`demido_permission::inherit`), a pure function over
    /// what the parent resolved with no log in it.
    pub fn delegate(&self, turn: u32, call: u64) -> Result<Self>
    where
        J: Clone,
    {
        let agent = AgentId::delegated(call);
        let depth = self.depth.saturating_add(1);
        self.write(
            turn,
            // A delegation is what a tool call did. It is not a ninth source,
            // and the colour delegated work is drawn in comes from the agent on
            // the event rather than from a source of its own (`design/surfaces.md`:
            // state is not elevation).
            Source::Tool,
            // The task went to the model in the call's own arguments and the
            // answer comes back as its result, both already weighed. This
            // occupies nothing.
            Weight::NOTHING,
            Body::Delegated {
                call,
                agent: agent.clone(),
                depth,
            },
        )?;

        Ok(Self {
            id: self.id.clone(),
            agent,
            depth,
            journal: self.journal.clone(),
            weigher: Arc::clone(&self.weigher),
            versioned: Mutex::new(BTreeSet::new()),
            documented: Mutex::new(BTreeSet::new()),
            offered: Mutex::new(None),
            turns: AtomicU32::new(0),
        })
    }

    /// A background delegation's answer, folded into the turn that asked for
    /// it.
    ///
    /// Written at a step boundary and nowhere else, and only above the default
    /// parallelism ([#66](https://github.com/elpideus/demido-studio/issues/66)):
    /// a delegation that blocked was answered as its call's own result, and a
    /// second record of one answer is a log that can disagree with itself about
    /// what came back.
    ///
    /// `answer` is the child's completion by position rather than its text,
    /// which is the rule every reference on this log keeps. It weighs nothing
    /// here: what the parent's model is shown is a framed message, and that is
    /// a block with a weight of its own.
    pub fn folded_in(&self, turn: u32, call: u64, agent: &AgentId, answer: u64) -> Result<u64> {
        let event = self.write(
            turn,
            Source::Tool,
            Weight::NOTHING,
            Body::Returned {
                call,
                agent: agent.clone(),
                answer,
            },
        )?;
        Ok(event.seq)
    }

    /// A host paragraph placed in this agent's context between two steps.
    ///
    /// The step-boundary sibling of [`Turn::fragment`], and it exists for the
    /// same reason [`Session::refused`] does: a background delegation's answer
    /// arrives when no `Turn` is open, and the block it becomes has to be an
    /// event before [`Session::step`] can name it.
    ///
    /// Recorded the way every host wording is, as the hash and what filled it,
    /// so the rebuild fills the paragraph again rather than reading a copy.
    pub fn framed(
        &self,
        turn: u32,
        source: Source,
        role: Role,
        prompt: &Prompt,
        values: &[(&str, &str)],
    ) -> Result<u64> {
        self.version(turn, prompt)?;
        let event = self.write(
            turn,
            source,
            self.weigher.weigh(&prompt.fill(values)),
            Body::Fragment {
                role,
                hash: prompt.hash.clone(),
                values: values
                    .iter()
                    .map(|(name, value)| Filling::new(*name, *value))
                    .collect(),
            },
        )?;
        Ok(event.seq)
    }

    /// Put one event on the log, stamped with this session.
    ///
    /// Every recording site goes through here, so the session id and the
    /// journal are named once. A `Turn` reaching for `session.journal.append`
    /// itself is how a second one gets stamped with something else.
    fn write(&self, turn: u32, source: Source, weight: Weight, body: Body) -> Result<Event> {
        self.journal.append(Entry::new(
            self.id.clone(),
            self.agent.clone(),
            turn,
            source,
            weight,
            body,
        ))
    }

    /// Write a tool document's full wording, at most once per session per hash.
    fn document(&self, turn: u32, document: &Document) -> Result<()> {
        if !held(&self.documented).insert(document.hash.clone()) {
            return Ok(());
        }

        self.write(
            turn,
            Source::System,
            self.weigher.weigh(&document.text),
            Body::ToolVersion {
                name: document.tool.name.to_owned(),
                hash: document.hash.clone(),
                text: document.text.clone(),
            },
        )?;
        Ok(())
    }

    /// Write a paragraph's full wording, at most once per session per hash.
    fn version(&self, turn: u32, prompt: &Prompt) -> Result<()> {
        let first = held(&self.versioned).insert(prompt.hash.clone());
        if !first {
            return Ok(());
        }

        self.write(
            turn,
            Source::System,
            self.weigher.weigh(&prompt.text),
            Body::Version {
                id: prompt.paragraph.id.to_owned(),
                hash: prompt.hash.clone(),
                text: prompt.text.clone(),
            },
        )?;
        Ok(())
    }
}

/// A lock whose holder panicked is still the bookkeeping it was: nothing in
/// it is left half written by an insert or an assignment.
fn held<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|held| held.into_inner())
}

/// What the next step of a turn is offered: the set the turn started with, or
/// nothing.
///
/// [`NextStep::Withholding`] is the turn loop's answer to a model going round in
/// a circle, and the turn loop is where the rule that decides it lives
/// (`docs/rules/tools.md`). What is here is the recording: an empty set under
/// [`Layer::Withheld`], so the rebuild of such a step is the assembly that step
/// was really sent with and a reader can tell it from a picker with everything
/// switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// The set the turn was sent with, unchanged. Every ordinary step.
    Offering,
    /// Nothing, for this step and the rest of the turn.
    Withholding,
}

/// One exchange, being assembled.
///
/// Every method both records an event and adds its block to the assembly, in
/// the order they were called. Dropping a turn without sending it records
/// everything that was said and no assembly, which is exactly what happened.
pub struct Turn<'a, J: Journal> {
    session: &'a Session<J>,
    number: u32,
    blocks: Vec<Block>,
    parameters: Option<Parameters>,
    /// What the request offers, merged from what [`Turn::offer`] recorded.
    tools: Vec<ToolSpec>,
    /// The `tools/offered` event in force, which the assembly names.
    offered: Option<u64>,
}

/// One block of an assembly being built.
///
/// A block this turn wrote keeps the message it contributed, so [`Turn::send`]
/// builds the request out of what it recorded rather than by reading the log
/// back, which would make the rebuild compare the log against itself. A block
/// carried from earlier keeps **only its position**: the text is the log's, and
/// asking the caller for it would let a caller send something the log does not
/// name.
enum Block {
    Written { seq: u64, message: Message },
    Carried { seq: u64 },
}

impl Block {
    fn seq(&self) -> u64 {
        match self {
            Block::Written { seq, .. } | Block::Carried { seq } => *seq,
        }
    }
}

/// The parameter set of a turn, and where it was written.
struct Parameters {
    seq: u64,
    model: String,
    options: Options,
}

impl<J: Journal> Turn<'_, J> {
    /// Which exchange this is, counting from one.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// Place a host paragraph in the assembly.
    ///
    /// Records the wording once per session, then a fragment naming it by hash
    /// with the values that filled it.
    pub fn fragment(
        &mut self,
        source: Source,
        role: Role,
        prompt: &Prompt,
        values: &[(&str, &str)],
    ) -> Result<u64> {
        self.session.version(self.number, prompt)?;

        let text = prompt.fill(values);
        self.wrote(
            source,
            Message::new(role, text),
            Body::Fragment {
                role,
                hash: prompt.hash.clone(),
                values: values
                    .iter()
                    .map(|(name, value)| Filling::new(*name, *value))
                    .collect(),
            },
        )
    }

    /// Record the tools this turn offers, in the order it offers them, and
    /// what decided that set.
    ///
    /// Each document's text is written once per session per hash, and the set
    /// itself only when it differs from the last one recorded: the set in
    /// force at any event is the last `tools/offered` before it, which costs
    /// one event per change rather than a field on every turn. Answers with
    /// the event's position when one was written.
    ///
    /// A change is a different list of names and hashes. The same list decided
    /// by a different layer is not one, because the set did not change: a chat
    /// that switches on exactly the tools global already offered has changed
    /// who says so, and nothing about what the model was shown.
    ///
    /// Each tool is its document and its schema's shape. It adds no block:
    /// the tools go in the request's own `tools`, each described by merging
    /// the document's prose onto the shape, and the assembly names the set in
    /// force so that the rebuild merges the same two things the same way.
    ///
    /// Nothing offered in a session that has never offered anything records
    /// nothing, because nothing changed.
    pub fn offer(
        &mut self,
        layer: Layer,
        tools: &[(Document, serde_json::Value)],
    ) -> Result<Option<u64>> {
        for (document, _) in tools {
            self.session.document(self.number, document)?;
        }

        self.tools = tools
            .iter()
            .map(|(document, shape)| ToolSpec {
                name: document.tool.name.to_owned(),
                description: document.description().to_owned(),
                parameters: document.describe(shape.clone()),
            })
            .collect();
        let tools: Vec<Offer> = tools
            .iter()
            .map(|(document, shape)| Offer {
                name: document.tool.name.to_owned(),
                hash: document.hash.clone(),
                shape: shape.clone(),
            })
            .collect();

        // Held across the write, so two turns racing cannot both decide the
        // set changed and record it twice.
        let mut last = held(&self.session.offered);
        match last.as_ref() {
            Some((seq, set)) if *set == tools => {
                self.offered = Some(*seq);
                return Ok(None);
            }
            None if tools.is_empty() => {
                self.offered = None;
                return Ok(None);
            }
            _ => {}
        }

        let event = self.session.write(
            self.number,
            Source::System,
            // The documents are already weighed, once each, as they were
            // written. Weighing the set again would count them twice.
            Weight::NOTHING,
            Body::Offered {
                tools: tools.clone(),
                layer,
            },
        )?;
        *last = Some((event.seq, tools));
        self.offered = Some(event.seq);
        Ok(Some(event.seq))
    }

    /// Place something somebody said in the assembly, verbatim.
    pub fn message(&mut self, source: Source, role: Role, text: &str) -> Result<u64> {
        self.wrote(
            source,
            Message::new(role, text),
            Body::Message {
                role,
                text: text.to_owned(),
            },
        )
    }

    /// Record one block and add it to the assembly, in that order.
    ///
    /// The message is what the block contributes, weighed as the text it puts
    /// in the window.
    fn wrote(&mut self, source: Source, message: Message, body: Body) -> Result<u64> {
        let weight = self.session.weigher.weigh(&message.content);
        let event = self.session.write(self.number, source, weight, body)?;
        self.blocks.push(Block::Written {
            seq: event.seq,
            message,
        });
        Ok(event.seq)
    }

    /// What the person typed. The common case of [`Turn::message`].
    pub fn user(&mut self, text: &str) -> Result<u64> {
        self.message(Source::User, Role::User, text)
    }

    /// Put an earlier block back in the assembly, by its position in the log.
    ///
    /// This is how history reaches the model, and it writes nothing: the block
    /// is already an event, and the assembly refers to it. A turn that carried
    /// copies of what came before would put a second copy of the conversation
    /// in the log on every exchange, and the log would grow as the square of
    /// the session.
    ///
    /// **A position, never text.** [`Turn::send`] resolves it from the log, so
    /// what goes to the model is what the log names. A `carry` that took the
    /// text as well would be the one way left to send an assembly this session
    /// did not record, and it would look exactly like a correct call.
    ///
    /// The positions come from [`crate::Replay::history`], where every
    /// [`Exchange`] carries its own.
    pub fn carry(&mut self, seq: u64) {
        self.blocks.push(Block::Carried { seq });
    }

    /// The parameter set this turn is sent with.
    pub fn parameters(&mut self, model: &str, options: Options) -> Result<u64> {
        let event = self.session.write(
            self.number,
            Source::System,
            // Settings occupy no context. They shape what does.
            Weight::NOTHING,
            Body::Parameters {
                model: model.to_owned(),
                options: options.clone(),
            },
        )?;

        self.parameters = Some(Parameters {
            seq: event.seq,
            model: model.to_owned(),
            options,
        });
        Ok(event.seq)
    }

    /// Close the assembly and hand back the request to send.
    ///
    /// The assembly event is written **before** the request is sent, so a crash
    /// between the two leaves a log that says what was about to happen rather
    /// than a log with a completion in it and nothing that produced it.
    pub fn send(self) -> Result<Sent> {
        let Turn {
            session,
            number,
            blocks,
            parameters,
            tools,
            offered,
        } = self;

        let Some(parameters) = parameters else {
            return Err(Error::Unparameterised { turn: number });
        };
        let messages = messages(session, &blocks)?;
        let blocks: Vec<u64> = blocks.iter().map(Block::seq).collect();

        let event = session.write(
            number,
            Source::System,
            // The assembly's own weight is the sum of its blocks, and every one
            // of those is already an event with a weight of its own. Adding
            // them again here would double every row in the ledger, so the
            // assembly weighs nothing and the occupancy of a turn is computed
            // from the blocks it names (`Replay::occupancy`).
            Weight::NOTHING,
            Body::Assembly {
                parameters: parameters.seq,
                blocks: blocks.clone(),
                tools: offered,
            },
        )?;

        Ok(Sent {
            turn: number,
            seq: event.seq,
            request: Request {
                model: parameters.model,
                messages,
                tools,
                options: parameters.options,
            },
            parameters: parameters.seq,
            blocks,
            offered,
        })
    }
}

/// The blocks as the messages they contribute.
///
/// The log is read once, and only when something was carried. A turn that
/// carried nothing needs no log to know what it just wrote.
fn messages<J: Journal>(session: &Session<J>, blocks: &[Block]) -> Result<Vec<Message>> {
    let carried = blocks
        .iter()
        .any(|block| matches!(block, Block::Carried { .. }));
    let earlier = if carried {
        Some(Replay::of(&session.journal)?)
    } else {
        None
    };

    blocks
        .iter()
        .map(|block| match block {
            Block::Written { message, .. } => Ok(message.clone()),
            Block::Carried { seq } => match &earlier {
                Some(replay) => replay.block(*seq),
                None => Err(Error::Dangling { seq: *seq }),
            },
        })
        .collect()
}

/// An assembly that has been recorded, and the request that goes on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub turn: u32,
    /// The sequence number of the assembly event, which is what a replay from
    /// here is replayed from.
    pub seq: u64,
    pub request: Request,
    /// What [`Session::step`] sends the next step with: the same parameter
    /// set, these blocks and more, and the same offered set. Private, so a
    /// step can only follow an assembly this session recorded.
    parameters: u64,
    blocks: Vec<u64>,
    offered: Option<u64>,
}
