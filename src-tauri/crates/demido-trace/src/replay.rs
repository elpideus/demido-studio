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

use serde::Serialize;

use demido_inference::{FinishReason, Message, Request, Role, ToolCall, ToolSpec};

use crate::event::{Basis, Body, Event, Layer, Source, Weight};
use crate::journal::{Error, Journal, Result};

/// The tools on offer at some moment, in the wording they were offered in.
#[derive(Debug, Clone, PartialEq)]
pub struct Offering {
    /// Where the set was recorded.
    pub seq: u64,
    pub layer: Layer,
    pub tools: Vec<OfferedTool>,
}

/// One tool of an [`Offering`]: its name, its hash, the document recorded
/// under that hash, and the shape its prose goes on.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferedTool {
    pub name: String,
    pub hash: String,
    pub text: String,
    pub shape: serde_json::Value,
}

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

/// One moment in a transcript, in the order it happened.
///
/// A transcript is not only what was said. `design/system.md` gives the chat a
/// **tool call row** beside its messages, and
/// [#55](https://github.com/elpideus/demido-studio/issues/55) puts it where the
/// call happened rather than in a window somebody has to go and open. So the
/// projection a bubble list is drawn from carries both.
#[derive(Debug, Clone, PartialEq)]
pub enum Moment {
    Said(Exchange),
    Called(Called),
}

/// One call, with what came back from it.
///
/// **The pairing is the transcript's, never the log's.** A call and its result
/// are two events each carrying their own source and weight, which is what
/// `demido-chat/AGENTS.md` fixes and what the session monitor reads. A row on
/// screen is the other question: what one call did, from asking to answered,
/// and a reader who has to match two rows by a sequence number is a reader
/// doing a join by hand.
///
/// Serializable because the window draws it as it stands. There is nothing to
/// narrow on the way: unlike an [`Exchange`], this carries no source and no
/// weight to keep off a chat bubble, so a second copy of it in `demido-chat`
/// would be a rename and two `From` impls.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Called {
    /// The call's own position on the log.
    pub seq: u64,
    pub turn: u32,
    pub name: String,
    /// The model's own text, whether or not it parses. Shown as written: a row
    /// that pretty-printed what the model actually sent would be the one place
    /// in this application where the record is tidied before it is read.
    pub arguments: String,
    /// What came back, or nothing while the call is still waiting or running.
    pub outcome: Option<Outcome>,
}

/// What came back from one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Outcome {
    /// The tool was attempted. `failed` is its own outcome, so a broken tool
    /// and a model paraphrasing one do not read alike.
    Returned { text: String, failed: bool },
    /// Demido's own answer to a call it did not run: declined, stopped, or past
    /// the step limit. Filled from the paragraph the log recorded, like any
    /// other refusal, rather than written again here.
    Refused { text: String },
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
                //
                // An answer that said nothing and only asked for calls is not
                // a bubble: what it did is the calls, and the transcript draws
                // those from their own events.
                Body::Completion {
                    text,
                    reason: FinishReason::ToolCalls,
                    ..
                } if text.is_empty() => None,
                Body::Completion { text, .. } => {
                    Some(self.exchange(event, Role::Assistant, text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// The transcript: what was said, and every call, in the order it happened.
    ///
    /// **Literally** [`Replay::history`] with the tool calls put back in. The
    /// messages are that function's own answer rather than the same filter
    /// written again, so the two projections cannot come to disagree about what
    /// counts as a bubble: a change to one is a change to both.
    ///
    /// Ordered by position, which is what puts a call between the answer that
    /// asked for it and whatever the model said next. Sequence numbers are
    /// unique, so the order is total and the sort decides nothing.
    ///
    /// Fallible where `history` is not, because a refusal is stored as a hash
    /// and its values: reading one means filling the paragraph the log recorded
    /// under that hash, which is the same rebuild [`Replay::block`] does and can
    /// fail the same way, on a log that lost the version event.
    pub fn transcript(&self) -> Result<Vec<Moment>> {
        // One pass for what answered what, rather than a search of the whole
        // log per call. The transcript is re-read every time a call is
        // recorded, so a scan per call is a scan per call per call, and a long
        // session is where somebody would notice.
        let mut answers: BTreeMap<u64, u64> = BTreeMap::new();
        for event in &self.events {
            if let Body::Result { call, .. } | Body::Refusal { call, .. } = &event.body {
                answers.entry(*call).or_insert(event.seq);
            }
        }

        let mut moments: Vec<Moment> = self.history().into_iter().map(Moment::Said).collect();
        for event in &self.events {
            let Body::Call {
                name, arguments, ..
            } = &event.body
            else {
                continue;
            };
            moments.push(Moment::Called(Called {
                seq: event.seq,
                turn: event.turn,
                name: name.clone(),
                arguments: arguments.clone(),
                outcome: answers
                    .get(&event.seq)
                    .map(|seq| self.outcome(*seq))
                    .transpose()?,
            }));
        }

        moments.sort_by_key(|moment| match moment {
            Moment::Said(exchange) => exchange.seq,
            Moment::Called(called) => called.seq,
        });
        Ok(moments)
    }

    /// The answer at `seq`, read as what a row shows.
    fn outcome(&self, seq: u64) -> Result<Outcome> {
        match &self.at(seq)?.body {
            Body::Result { text, failed, .. } => Ok(Outcome::Returned {
                text: text.clone(),
                failed: *failed,
            }),
            // Filled rather than copied, which is the whole shape of a refusal
            // on this log: the paragraph is held once per session and this puts
            // the values back into it.
            Body::Refusal { .. } => Ok(Outcome::Refused {
                text: self.block(seq)?.content,
            }),
            _ => Err(Error::NotABlock { seq }),
        }
    }

    /// Every block a later turn carries, in the order it happened: what the
    /// person typed, every answer including one that only asked for calls, and
    /// what came back for each call, whether a result or a refusal.
    ///
    /// Wider than [`Replay::history`] on purpose. A model that is not shown its
    /// own calls and their results next turn is a model that calls again for
    /// what it already has, and a request carrying an answer's calls without
    /// what came back for them is one a server refuses.
    pub fn conversation(&self) -> Vec<u64> {
        self.events
            .iter()
            .filter(|event| {
                matches!(
                    event.body,
                    Body::Message {
                        role: Role::User | Role::Assistant,
                        ..
                    } | Body::Completion { .. }
                        | Body::Result { .. }
                        | Body::Refusal { .. }
                )
            })
            .map(|event| event.seq)
            .collect()
    }

    // What the person has answered *always for this tool* about used to be read
    // back from here, off the `tool/decision` events. It is a **setting** now,
    // `tools.always` on the ladder's chat tier
    // ([#55](https://github.com/elpideus/demido-studio/issues/55)), resolved
    // once per message with the mode and the step limit. The log still records
    // which of the three was answered, because that is what happened; a second
    // way to ask what is in force would be a second thing to keep in step.

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
    ///
    /// A turn that used a tool is sent once per step, and rebuilds here as its
    /// last step. Each earlier one is [`Replay::request`] at its own assembly.
    pub fn assembly(&self, turn: u32) -> Result<Request> {
        let seq = self
            .events
            .iter()
            .rev()
            .find(|event| event.turn == turn && matches!(event.body, Body::Assembly { .. }))
            .map(|event| event.seq)
            .ok_or(Error::NotSent { turn })?;
        self.request(seq)
    }

    /// Rebuild the request the assembly at `seq` was sent as: its parameters,
    /// its blocks, and the tools of the offered set it names, each described
    /// again from the shape and the wording that set recorded.
    pub fn request(&self, seq: u64) -> Result<Request> {
        let Body::Assembly {
            parameters,
            blocks,
            tools,
        } = &self.at(seq)?.body
        else {
            return Err(Error::NotABlock { seq });
        };

        let Body::Parameters { model, options } = &self.at(*parameters)?.body else {
            return Err(Error::NotABlock { seq: *parameters });
        };

        let messages = blocks
            .iter()
            .map(|seq| self.block(*seq))
            .collect::<Result<Vec<Message>>>()?;

        let tools = match tools {
            None => Vec::new(),
            Some(offered) => self
                .offered(*offered)?
                .map(|offering| offering.tools)
                .unwrap_or_default()
                .into_iter()
                .map(|tool| ToolSpec {
                    description: demido_prompts::sections(&tool.text).description.to_owned(),
                    parameters: demido_prompts::describe(tool.shape, &tool.text),
                    name: tool.name,
                })
                .collect(),
        };

        Ok(Request {
            model: model.clone(),
            messages,
            tools,
            options: options.clone(),
        })
    }

    /// The last assembly a turn recorded: its parameter set, and its blocks.
    ///
    /// The **last**, because a turn sent more than once is what a retry or a
    /// step is, and what it was finally sent with is what happened.
    fn assembled(&self, turn: u32) -> Result<(u64, &[u64])> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.body {
                Body::Assembly {
                    parameters, blocks, ..
                } if event.turn == turn => Some((*parameters, blocks.as_slice())),
                _ => None,
            })
            .ok_or(Error::NotSent { turn })
    }

    /// The tools on offer as of event `at`, in the wording they were offered
    /// in, or `None` when nothing had been offered by then.
    ///
    /// The set is the last `tools/offered` at or before `at`, and each tool's
    /// text is the `tool/version` recorded under its hash, looked for backwards
    /// from the set exactly as a fragment's wording is. That is what keeps a
    /// document edited after the fact from rewriting the record of a reply
    /// from before it.
    pub fn offered(&self, at: u64) -> Result<Option<Offering>> {
        let Some((seq, layer, tools)) =
            self.events
                .iter()
                .rev()
                .find_map(|event| match &event.body {
                    Body::Offered { tools, layer } if event.seq <= at => {
                        Some((event.seq, *layer, tools))
                    }
                    _ => None,
                })
        else {
            return Ok(None);
        };

        let tools = tools
            .iter()
            .map(|offer| {
                Ok(OfferedTool {
                    name: offer.name.clone(),
                    hash: offer.hash.clone(),
                    text: self.document(&offer.hash, seq)?.to_owned(),
                    shape: offer.shape.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Some(Offering { seq, layer, tools }))
    }

    /// The tool document recorded under a hash, before event `before`.
    fn document(&self, hash: &str, before: u64) -> Result<&str> {
        self.recorded(hash, before, |body| match body {
            Body::ToolVersion { hash, text, .. } => Some((hash.as_str(), text.as_str())),
            _ => None,
        })
    }

    /// The text some version event recorded under `hash`, searched backwards
    /// from event `before`.
    ///
    /// One search for both registers, told apart only by which event counts as
    /// a version of it, so a paragraph's wording and a tool's document are
    /// found by exactly the same rule. Backwards, because an edit mid session
    /// writes a second version under a new hash and a session can hold both.
    /// Two versions never share a hash, so this is exact rather than nearest.
    fn recorded<'a>(
        &'a self,
        hash: &str,
        before: u64,
        version: fn(&'a Body) -> Option<(&'a str, &'a str)>,
    ) -> Result<&'a str> {
        self.events
            .iter()
            .rev()
            .skip_while(|event| event.seq >= before)
            .find_map(|event| match version(&event.body) {
                Some((recorded, text)) if recorded == hash => Some(text),
                _ => None,
            })
            .ok_or_else(|| Error::Unversioned {
                hash: hash.to_owned(),
            })
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
            // What the model said, going back in as its own turn, with the
            // calls it asked for on it. The reasoning is not carried: it was
            // never sent back, and a rebuild that added it would produce an
            // assembly the backend never saw.
            Body::Completion { text, .. } => Ok(Message::calling(text.clone(), self.calls(seq))),
            Body::Result { call, text, .. } => {
                Ok(Message::result(self.call_id(*call)?, text.clone()))
            }
            Body::Refusal { call, hash, values } => {
                let wording = self.wording(hash, seq)?;
                let filled = demido_prompts::fill(
                    wording,
                    &values
                        .iter()
                        .map(|filling| (filling.name.as_str(), filling.value.as_str()))
                        .collect::<Vec<_>>(),
                );
                Ok(Message::result(self.call_id(*call)?, filled))
            }
            _ => Err(Error::NotABlock { seq }),
        }
    }

    /// The calls the answer at `completion` asked for, in the order written.
    fn calls(&self, completion: u64) -> Vec<ToolCall> {
        self.events
            .iter()
            .filter_map(|event| match &event.body {
                Body::Call {
                    completion: of,
                    id,
                    name,
                    arguments,
                } if *of == completion => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The id the backend gave the call at `call`, which is what a result is
    /// tied to on the wire.
    fn call_id(&self, call: u64) -> Result<&str> {
        match &self.at(call)?.body {
            Body::Call { id, .. } => Ok(id),
            _ => Err(Error::NotABlock { seq: call }),
        }
    }

    /// The wording recorded under a hash, as it stood when the fragment was
    /// written.
    ///
    /// Searched backwards from the fragment rather than forwards from the
    /// start ([`Replay::recorded`] says why).
    fn wording(&self, hash: &str, before: u64) -> Result<&str> {
        self.recorded(hash, before, |body| match body {
            Body::Version { hash, text, .. } => Some((hash.as_str(), text.as_str())),
            _ => None,
        })
    }
}
