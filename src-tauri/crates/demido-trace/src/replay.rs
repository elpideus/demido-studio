//! The reading side: everything else in the app, as a projection of the log.
//!
//! There is no chat table, no message store and no second copy of a session
//! anywhere. [`Replay::history`] is what a transcript is made of,
//! [`Replay::assembly`] is what was sent, and both are computed from the same
//! events. That is the brief's own claim ("Resume, fork, search, and replay all
//! operate on the same event stream") and it is only true while nothing else
//! keeps its own copy, which is why the projections live here rather than being
//! written again by each caller.
//!
//! [`Replay::assembly`] is the one that has to be exact. It rebuilds the
//! request that went to the backend, refilling every paragraph from the wording
//! the log stored, in the order the assembly names. A rebuild that comes out
//! different from what was sent is a defect in this function or a gap in what
//! was recorded, and the live suite is where those meet a real model.

use std::collections::BTreeMap;

use demido_inference::{Message, Request, Role};

use crate::event::{Basis, Body, Event, Source, Weight};
use crate::journal::{Error, Journal, Result};

/// One thing said, as a transcript shows it.
///
/// Carries its own position, because that is what a later turn refers to when
/// it carries this block back into an assembly, and what "replay from here"
/// starts at.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub seq: u64,
    pub turn: u32,
    pub role: Role,
    pub text: String,
    pub source: Source,
    pub weight: Weight,
}

/// What one source cost over a session: the ledger in the monitor's left
/// column, "count and tokens per source, not a legend beside a search box".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub events: u32,
    pub tokens: u32,
    /// [`Basis::Estimated`] if any event in the tally was estimated.
    pub basis: Option<Basis>,
}

/// A log, read.
///
/// Built once and asked many questions. Every projection is a scan of the same
/// vector, which is what keeps them from being able to disagree.
#[derive(Debug, Clone)]
pub struct Replay {
    events: Vec<Event>,
}

impl Replay {
    /// Read a journal.
    pub fn of(journal: &impl Journal) -> Result<Self> {
        Ok(Self::over(journal.events()?))
    }

    /// Replay events somebody already has, such as a fixture read off disk.
    pub fn over(events: Vec<Event>) -> Self {
        Self { events }
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The turns that were sent, in order.
    pub fn turns(&self) -> Vec<u32> {
        let mut turns: Vec<u32> = self
            .events
            .iter()
            .filter(|event| matches!(event.body, Body::Assembly { .. }))
            .map(|event| event.turn)
            .collect();
        turns.dedup();
        turns
    }

    /// The transcript: what a person typed and what the model answered, in the
    /// order it happened.
    ///
    /// Deliberately not every block. A system paragraph is in the log and is in
    /// the assembly, and it is not part of the conversation; the monitor shows
    /// it and the chat bubble does not. Both read the same events.
    pub fn history(&self) -> Vec<Exchange> {
        self.events
            .iter()
            .filter_map(|event| match &event.body {
                Body::Message {
                    role: role @ (Role::User | Role::Assistant),
                    text,
                } => Some(self.exchange(event, *role, text.clone())),
                // The answer is the completion. There is no assistant message
                // written beside it: a second event saying the same thing is
                // the parallel table this log exists to not have.
                Body::Completion { text, .. } => {
                    Some(self.exchange(event, Role::Assistant, text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn exchange(&self, event: &Event, role: Role, text: String) -> Exchange {
        Exchange {
            seq: event.seq,
            turn: event.turn,
            role,
            text,
            source: event.source,
            weight: event.weight,
        }
    }

    /// Rebuild the request one turn was sent with.
    ///
    /// Every block is resolved from the log: a fragment by refilling the
    /// wording recorded under its hash, a message by its own text, a completion
    /// by the answer it carried. Nothing is read from anywhere but the events.
    ///
    /// A turn sent more than once, which is what a retry is, rebuilds from the
    /// last assembly it recorded.
    pub fn assembly(&self, turn: u32) -> Result<Request> {
        let (parameters, blocks) = self.assembled(turn)?;

        let Body::Parameters { model, options } = &self.at(parameters)?.body else {
            return Err(Error::NotABlock { seq: parameters });
        };

        let messages = blocks
            .iter()
            .map(|seq| self.block(*seq))
            .collect::<Result<Vec<Message>>>()?;

        Ok(Request {
            model: model.clone(),
            messages,
            options: options.clone(),
        })
    }

    /// The last assembly a turn recorded: its parameter set, and its blocks.
    ///
    /// The **last**, because a turn sent more than once is what a retry is, and
    /// what it was finally sent with is what happened.
    fn assembled(&self, turn: u32) -> Result<(u64, &[u64])> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.body {
                Body::Assembly { parameters, blocks } if event.turn == turn => {
                    Some((*parameters, blocks.as_slice()))
                }
                _ => None,
            })
            .ok_or(Error::NotSent { turn })
    }

    /// What one turn occupied, as the sum of the blocks its assembly named.
    ///
    /// The assembly event itself weighs nothing, so this is the number the
    /// occupancy bar shows and it double counts nothing.
    pub fn occupancy(&self, turn: u32) -> Result<Weight> {
        let (_, blocks) = self.assembled(turn)?;

        blocks
            .iter()
            .map(|seq| self.at(*seq).map(|event| event.weight))
            .collect::<Result<Vec<Weight>>>()
            .map(|weights| weights.into_iter().sum())
    }

    /// Count and tokens per source, over the whole session.
    ///
    /// Sources with nothing in them are absent rather than zero, so the ledger
    /// lists what a session actually contains.
    pub fn ledger(&self) -> BTreeMap<Source, Tally> {
        let mut ledger: BTreeMap<Source, Tally> = BTreeMap::new();
        for event in &self.events {
            let tally = ledger.entry(event.source).or_default();
            tally.events = tally.events.saturating_add(1);
            tally.tokens = tally.tokens.saturating_add(event.weight.tokens);
            tally.basis = Some(match (tally.basis, event.weight.basis) {
                (Some(Basis::Estimated), _) | (_, Basis::Estimated) => Basis::Estimated,
                _ => Basis::Counted,
            });
        }
        ledger
    }

    fn at(&self, seq: u64) -> Result<&Event> {
        // The log is in sequence order and numbering has no gaps, so position
        // is arithmetic. Checked rather than assumed, because a pruned fixture
        // is a legitimate log with gaps in it.
        let guess = seq
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok());
        if let Some(event) = guess.and_then(|index| self.events.get(index)) {
            if event.seq == seq {
                return Ok(event);
            }
        }

        self.events
            .iter()
            .find(|event| event.seq == seq)
            .ok_or(Error::Dangling { seq })
    }

    /// One block of an assembly, as the message it contributes.
    ///
    /// Public because a turn carrying an earlier block resolves it through
    /// here rather than being handed the text ([`crate::Turn::carry`]), which
    /// is what keeps the log the only place a carried message can come from.
    pub fn block(&self, seq: u64) -> Result<Message> {
        let event = self.at(seq)?;
        match &event.body {
            Body::Message { role, text } => Ok(Message::new(*role, text.clone())),
            Body::Fragment { role, hash, values } => {
                let wording = self.wording(hash, seq)?;
                let filled = demido_prompts::fill(
                    wording,
                    &values
                        .iter()
                        .map(|filling| (filling.name.as_str(), filling.value.as_str()))
                        .collect::<Vec<_>>(),
                );
                Ok(Message::new(*role, filled))
            }
            // What the model said, going back in as its own turn. The reasoning
            // is not carried: it was never sent back, and a rebuild that added
            // it would produce an assembly the backend never saw.
            Body::Completion { text, .. } => Ok(Message::new(Role::Assistant, text.clone())),
            _ => Err(Error::NotABlock { seq }),
        }
    }

    /// The wording recorded under a hash, as it stood when the fragment was
    /// written.
    ///
    /// Searched backwards from the fragment rather than forwards from the
    /// start, because an edit mid session writes a second version under a new
    /// hash and a session can hold both. Two versions never share a hash, so
    /// this is exact rather than nearest.
    fn wording(&self, hash: &str, before: u64) -> Result<&str> {
        self.events
            .iter()
            .rev()
            .skip_while(|event| event.seq >= before)
            .find_map(|event| match &event.body {
                Body::Version {
                    hash: recorded,
                    text,
                    ..
                } if recorded == hash => Some(text.as_str()),
                _ => None,
            })
            .ok_or_else(|| Error::Unversioned {
                hash: hash.to_owned(),
            })
    }
}
