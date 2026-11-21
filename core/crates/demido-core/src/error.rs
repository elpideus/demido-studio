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
    /// The caller asked for something that cannot be done as asked. The text says what to change.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
