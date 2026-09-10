//! The append-only log itself: the seam, and what an implementation owes.
//!
//! Two methods, because there are only two things anyone does to an append-only
//! log. Nothing here can rewrite a line, delete one, or hand out a writable
//! handle to one: append-only is the type, not a convention the callers keep.
//!
//! **The methods are synchronous, and that is a decision.** An append is a few
//! hundred bytes to a handle that is already open, and every place a turn is
//! assembled would otherwise have to be async in order to write a line about
//! itself, including inside the decoder that is pulling chunks off a stream.
//! What the caller gets in exchange is that recording is never a reason for a
//! function to change colour. The cost is a lock held for the length of a
//! `write_all`, which is the same lock a queue behind an async API would take
//! anyway.

use crate::event::{Entry, Event};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The log could not be read or written. The context says which file and
    /// what was being attempted, because "access is denied" alone is not a
    /// report.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// A line in the middle of the log is not an event.
    ///
    /// A truncated **last** line is not this: a process killed mid write leaves
    /// one, and refusing to open the log over it would turn a lost event into a
    /// lost session. See [`crate::jsonl::JsonLines::open`].
    #[error("line {line}: {detail}")]
    Corrupt { line: usize, detail: String },

    /// An assembly refers to a block that is not in the log. A log that cannot
    /// resolve its own references cannot rebuild what was sent, which is the
    /// one thing it exists to do.
    #[error("the assembly refers to event {seq}, which is not in the log")]
    Dangling { seq: u64 },

    /// A fragment names a wording the log never recorded.
    #[error("no prompt version {hash} was recorded before it was used")]
    Unversioned { hash: String },

    /// An event was asked to be a block of an assembly and cannot be one.
    #[error("event {seq} is not something an assembly can be built out of")]
    NotABlock { seq: u64 },

    /// Nothing was sent for that turn.
    #[error("turn {turn} has no assembly: nothing was sent")]
    NotSent { turn: u32 },

    /// A turn was sent without saying what it was sent with. The parameter set
    /// is half of what a rebuild has to produce, so a turn missing one is
    /// refused at the point of sending rather than found at replay.
    #[error("turn {turn} has no parameter set")]
    Unparameterised { turn: u32 },
}

impl Error {
    pub fn io(context: impl std::fmt::Display, source: std::io::Error) -> Self {
        Self::Io {
            context: context.to_string(),
            source,
        }
    }
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io { context, source } => demido_core::Error::io(context, source),
            other => demido_core::Error::invalid("session log", other.to_string()),
        }
    }
}

/// A log you can add to and read back, and do nothing else to.
///
/// The promises, which [`crate::contract`] is the executable statement of:
///
/// - `append` returns the event it wrote, with the sequence number and the
///   timestamp it was given.
/// - Sequence numbers start at one and go up by one, with no gaps and no
///   repeats, however many threads are appending.
/// - `events` returns everything appended so far, in sequence order, byte for
///   byte what went in.
/// - A journal opened again over the same storage replays all of it and carries
///   on numbering where the last one stopped. This is the restart promise, and
///   it is in the contract rather than in one implementation's tests because it
///   is the reason the log exists.
pub trait Journal: Send + Sync {
    /// Write one event. The sequence number and the clock are the journal's.
    fn append(&self, entry: Entry) -> Result<Event>;

    /// Everything, in order.
    ///
    /// The whole log, because every consumer of it is a projection over the
    /// whole log: history, the rebuild, the ledger. A session that outgrows
    /// memory is a real problem and it is a later one, and the shape that
    /// solves it is a cursor on this trait rather than a second source of
    /// truth beside it.
    fn events(&self) -> Result<Vec<Event>>;
}

/// Milliseconds since the Unix epoch, for a journal stamping an event.
///
/// A clock before the epoch reads as zero rather than as a panic. A desk with a
/// wrong clock should still get a log.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}
