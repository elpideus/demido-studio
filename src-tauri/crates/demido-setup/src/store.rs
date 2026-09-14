//! The seam: where the answers are kept, and what an implementation owes.

use crate::answers::Answers;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is there and is not an answers file this build can read.
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
            other => demido_core::Error::unavailable("the set-up answers", other.to_string()),
        }
    }
}

/// Somewhere the answers survive a restart.
///
/// The promises, executed by [`crate::contract`]:
///
/// - A store nobody has written to reads as the default answers, not an
///   error. A profile that has never opened the wizard is the ordinary case.
/// - What was written last is what comes back, exactly.
/// - A store opened again over the same storage reads back what the last one
///   wrote.
pub trait Store: Send + Sync {
    fn read(&self) -> Result<Answers>;
    fn write(&self, answers: &Answers) -> Result<()>;
}
