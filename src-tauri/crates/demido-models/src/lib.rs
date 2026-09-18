//! The models on this machine: the library, its folders, and what each file is.
//!
//! Brief B22: "Models Browser & Downloader"
//!
//! | Module | What |
//! |---|---|
//! | [`folders`] | The download folder and the scan folders, resolved from the ladder. |
//! | [`sources`] | Where other tools keep models, and what a borrowed row is called. |
//! | [`library`] | The directory as the registry: scan, remove, and where a download lands. |
//! | [`parts`] | Which `.gguf` is weights, a projector, a draft or a piece of a split. |
//! | [`gguf`] | The header: the facts a file states, and whether it is whole. |
//!
//! **Nothing here downloads and nothing here talks to a server.** The index
//! (#70) and the queue (#73) are what fetch; this crate is what is already on
//! disk, which is the part a network failure must never take away.
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod folders;
pub mod gguf;
pub mod library;
pub mod parts;
pub mod sources;

use std::path::PathBuf;

pub use folders::Folders;
pub use gguf::Damage;
pub use library::{Capabilities, Damaged, Fact, Library, Local, Scan, LIMIT};
pub use parts::{Companion, Part, Shard};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A path the library was asked to delete that is not under Demido's own
    /// folder. The library lists borrowed models, and those are another
    /// tool's or the person's own.
    // not-a-prompt: a refusal the window shows the person who asked for the
    // delete. No model reads it.
    #[error("{path} is not in Demido's own models folder, so Demido will not delete it")]
    Outside { path: PathBuf },

    #[error("{path} is not a model file")]
    NotAModel { path: PathBuf },

    #[error("could not read or delete {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io { path, source } => demido_core::Error::io(path.display(), source),
            refused => demido_core::Error::invalid("a model", refused.to_string()),
        }
    }
}
