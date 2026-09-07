//! The append-only session log, and the assembly rebuilt from it.
//!
//! Everything that happens in a session is an event on a log that is only ever
//! appended to: what the person typed, every paragraph Demido composed into the
//! prompt, the parameters it was sent with, what came back, and what failed.
//! The brief asks for exactly this:
//!
//! > Everything the model sees is recorded in an append-only session log:
//! > system prompts, reasoning, tool calls and results, subagent scheduling,
//! > and every context injection.
//!
//! Two fields are on every event from the first line of code rather than added
//! when a screen needs them, because `design/windows.md` says a log without
//! them cannot serve the product's own thesis and because adding either later
//! means rewriting the slice that wrote them: its **source**, so what Demido
//! wrote can be told from what the user typed and from what the model produced,
//! and its **token weight**, because the monitor's second axis is cost.
//!
//! ## The log is the source of truth
//!
//! There is no chat table. A transcript is [`Replay::history`], an export is a
//! projection of the same events, and the Session Monitor will be a third. Two
//! stores over one conversation is two stores that can eventually disagree
//! about it, and not disagreeing is the whole product claim.
//!
//! ## Rebuilt, not described
//!
//! A fragment is recorded as the hash of the paragraph and the values that
//! filled it, never as the text it produced, so replaying it means filling the
//! paragraph again. [`Replay::assembly`] hands back the request that went to
//! the backend, character for character. That is the harder of the two
//! constraints `design/windows.md` puts on this crate, and it is what makes
//! "replay from here" and an editable prompt worth anything.
//!
//! ## What is here
//!
//! | Module | What |
//! |---|---|
//! | [`event`] | What one line is: its source, its weight, and what happened. |
//! | [`journal`] | The seam: append, and read back. |
//! | [`contract`] | What any journal must do. |
//! | [`jsonl`] | One event per line, on disk. |
//! | [`memory`] | The same, without a disk. |
//! | [`weight`] | What a piece of text costs, and who may say so. |
//! | [`record`] | The writing side: a turn recording itself. |
//! | [`replay`] | The reading side: history, the rebuild, the ledger. |
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod contract;
pub mod event;
pub mod journal;
pub mod jsonl;
pub mod memory;
pub mod record;
pub mod replay;
pub mod weight;

pub use event::{Basis, Body, Entry, Event, Filling, SessionId, Source, Weight};
pub use journal::{Error, Journal, Result};
pub use jsonl::JsonLines;
pub use memory::Memory;
pub use record::{Sent, Session, Turn};
pub use replay::{Exchange, Replay, Tally};
pub use weight::{estimate, Counting, Estimate, Weigher};
