//! The seam: where settings are kept, and what an implementation owes.
//!
//! Two methods, and both can fail, which is the difference between this store
//! and [`demido_shell::Store`]. A desk arrangement that will not load is
//! discarded silently, because nobody can act on the report and remaking it
//! costs one gesture. Settings are typed by a person: a file that will not load
//! is their work, so it is reported, and an implementation that keeps files
//! moves the unreadable one aside rather than writing over it.

use crate::document::Document;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is there and is not settings, or is a shape this build does not
    /// know. What the store did about it is in the message, because "settings
    /// could not be read" without saying where they went is not a report.
    #[error("{context}: {detail}")]
    Unreadable { context: String, detail: String },

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

    pub fn unreadable(context: impl std::fmt::Display, detail: impl std::fmt::Display) -> Self {
        Self::Unreadable {
            context: context.to_string(),
            detail: detail.to_string(),
        }
    }
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io { context, source } => demido_core::Error::io(context, source),
            other => demido_core::Error::unavailable("the settings", other.to_string()),
        }
    }
}

/// Somewhere a settings document survives a restart.
///
/// The promises, which [`crate::contract`] is the executable statement of:
///
/// - A store nobody has written to reads as the default document, not as an
///   error. A profile that has never opened Settings is the ordinary case.
/// - What was written last is what comes back, exactly, including the tiers
///   this build does not use.
/// - A store opened again over the same storage reads back what the last one
///   wrote. This is the whole point, and it is in the contract rather than in
///   one implementation's tests because it is the reason the trait exists.
pub trait Store: Send + Sync {
    /// Everything that was last written, or a default document.
    fn read(&self) -> Result<Document>;

    /// Replace it. There is one settings document per profile, so this
    /// overwrites rather than merges: merging here would mean a value cleared
    /// in memory could never be cleared on disk.
    fn write(&self, document: &Document) -> Result<()>;
}
