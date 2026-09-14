//! The seam: where a layout is kept, and what an implementation owes.
//!
//! Two methods, and the asymmetry between them is the whole rule this crate
//! exists to hold.
//!
//! **Reading cannot fail.** [`Store::read`] returns an `Option`, not a
//! `Result`, because there is no failure a caller could usefully do anything
//! about: a missing file, an unreadable one, a file of the wrong generation and
//! a file full of something else all mean the same thing to a desk, which is
//! "draw the default". Handing back a `Result` would make it possible to write
//! a caller that reports a bad layout, and a layout must never be a reason the
//! window does not open. The rule is in the type rather than in a comment
//! asking every caller to keep it.
//!
//! **Writing can fail**, and the caller that fails is [`crate::Debounced`],
//! which logs it and carries on. A disk that will not take a layout is a desk
//! that forgets, not a session that stops.

use crate::layout::Shell;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The layout could not be written. The context says which file and what
    /// was being attempted, because "access is denied" alone is not a report.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
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
        }
    }
}

/// Somewhere a desk arrangement survives a restart.
///
/// The promises, which [`crate::contract`] is the executable statement of:
///
/// - A store nobody has written to reads as `None`, not as an error.
/// - What was written last is what comes back, exactly.
/// - A store opened again over the same storage reads back what the last one
///   wrote. This is the whole point, and it is in the contract rather than in
///   one implementation's tests because it is the reason the trait exists.
/// - Anything else that comes back reads as `None`: a store never hands out a
///   layout it is not sure of, and never says why.
pub trait Store: Send + Sync {
    /// The arrangement that was last written, if there is one this build can
    /// still read.
    fn read(&self) -> Option<Shell>;

    /// Replace the arrangement. There is exactly one layout per profile, so
    /// this overwrites rather than appends.
    fn write(&self, shell: &Shell) -> Result<()>;
}
