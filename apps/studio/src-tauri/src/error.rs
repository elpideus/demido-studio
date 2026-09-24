//! The error type commands return. It serializes to a plain message, which is what the UI shows.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("{0:#}")]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn msg(text: impl Into<String>) -> Self {
        AppError::Message(text.into())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Other(e.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type CmdResult<T> = Result<T, AppError>;

/// Returns early with a user-facing message.
#[macro_export]
macro_rules! bail_msg {
    ($($arg:tt)*) => {
        return Err($crate::error::AppError::msg(format!($($arg)*)))
    };
}
