//! What can go wrong, said in words a person can act on.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Storage failed: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("A file could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("That is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("There is no setting called \"{0}\".")]
    UnknownSetting(String),
    /// Something by that name does not exist. The text says what was looked for.
    #[error("{0}")]
    NotFound(String),
    /// It cannot be done right now because something else is under way. The text says what.
    #[error("{0}")]
    Busy(String),
    /// Another computer did not answer, or answered with a refusal. The text says which and what to try.
    #[error("{0}")]
    Net(String),
    /// The person stopped it.
    #[error("{0}")]
    Stopped(String),
    /// The caller asked for something that cannot be done as asked. The text says what to change.
    #[error("{0}")]
    Invalid(String),
    /// What was to be changed is no longer what the change was made to: a file written since it was opened.
    #[error("{0}")]
    Changed(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
