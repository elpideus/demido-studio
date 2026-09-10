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
use std::sync::Mutex;

use demido_inference::{FinishReason, Message, Options, Request, Role, Usage};
use demido_prompts::Prompt;

use crate::event::{Body, Entry, Event, Filling, SessionId, Source, Weight};
use crate::journal::{Error, Journal, Result};
use crate::replay::Replay;
use crate::weight::{Estimate, Weigher};

/// One conversation, recording itself.
///
/// Holds the journal, the weigher, and the two pieces of bookkeeping that make
/// the log cheap: which turn is next, and which paragraph wordings have already
/// been written out in full this session.
pub struct Session<J: Journal> {
    id: SessionId,
    journal: J,
    weigher: Box<dyn Weigher>,
    versioned: Mutex<BTreeSet<String>>,
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
            journal,
            weigher: Box::new(weigher),
            versioned: Mutex::new(BTreeSet::new()),
            turns: AtomicU32::new(0),
        }
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
    ///   session.
    pub fn resume(&self) -> Result<()> {
        let events = self.journal.events()?;

        self.turns.store(
            events.iter().map(|event| event.turn).max().unwrap_or(0),
            Ordering::SeqCst,
        );

        let mut versioned = self
            .versioned
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        for event in &events {
            if let Body::Version { hash, .. } = &event.body {
                versioned.insert(hash.clone());
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

    /// Put one event on the log, stamped with this session.
    ///
    /// Every recording site goes through here, so the session id and the
    /// journal are named once. A `Turn` reaching for `session.journal.append`
    /// itself is how a second one gets stamped with something else.
    fn write(&self, turn: u32, source: Source, weight: Weight, body: Body) -> Result<Event> {
        self.journal
            .append(Entry::new(self.id.clone(), turn, source, weight, body))
    }

    /// Write a paragraph's full wording, at most once per session per hash.
    fn version(&self, turn: u32, prompt: &Prompt) -> Result<()> {
        let first = {
            let mut versioned = self
                .versioned
                .lock()
                .unwrap_or_else(|held| held.into_inner());
            versioned.insert(prompt.hash.clone())
        };
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
        } = self;

        let Some(parameters) = parameters else {
            return Err(Error::Unparameterised { turn: number });
        };
        let messages = messages(session, &blocks)?;

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
                blocks: blocks.iter().map(Block::seq).collect(),
            },
        )?;

        Ok(Sent {
            turn: number,
            seq: event.seq,
            request: Request {
                model: parameters.model,
                messages,
                options: parameters.options,
            },
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
}
