//! The download queue: a model's files onto this machine, resumable, verified,
//! and a failure that says what happened.
//!
//! Brief B22:
//!
//! > UI should also have a download status indicator which also allows to pause, resume or cancel model downloads (especially useful when adding multiple models to the download queue).
//!
//! | Module | What |
//! |---|---|
//! | [`queue`] | The pool, the rows, pause, resume, cancel and `pause_all`. |
//! | [`transfer`] | One file: resume, range negotiation, write, verify. |
//! | [`file`] | `downloads.json`: what the profile asked for, and nothing about progress. |
//! | [`room`] | Free space on the volume a download lands on. |
//!
//! Three properties are the whole point, and each is tested against a server
//! that misbehaves on purpose (`tests/over_http.rs`):
//!
//! - **The library never holds a partial file.** Bytes accumulate in
//!   `<name>.gguf.part`, which the library does not list, and every file of an
//!   item is renamed into place only once all of them are whole and verified.
//! - **The bytes on disk are the progress.** A retry, a resume and a restart
//!   all ask for the bytes still missing, and nothing records how far a
//!   transfer got except the partial file itself.
//! - **A failure is a value with a cause.** A reset connection, a full disk and
//!   a gated file are three [`Failure`]s, and one of them is one row.
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod file;
pub mod queue;
pub mod room;
pub mod transfer;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use demido_models::{Choice, Damage, Library};

pub use file::Files;
pub use queue::{Event, Id, Queue, Row, State};

/// Where the files are fetched from when nobody says otherwise.
pub const HOST: &str = "https://huggingface.co";

/// One thing in the queue: one model a person chose, which is one or more
/// files. A split model's shards and the projector it needs are pieces of one
/// item, so a model published in four pieces is one row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `unsloth/Qwen3-8B-GGUF`.
    pub repo: String,
    /// What the row is called: the choice's name, never a URL.
    pub name: String,
    /// Every file, weights first to last, then the projector.
    pub files: Vec<Piece>,
}

/// One file of an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub url: String,
    /// Where the finished file goes, which is always under the download
    /// folder ([`Library::destination`]).
    pub destination: PathBuf,
    /// The size the index stated. A transfer that says otherwise is refused,
    /// and a file of any other length is not finished.
    pub bytes: u64,
    /// SHA-256 as Git LFS names the object, when the index sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl Piece {
    /// The sidecar bytes accumulate in.
    ///
    /// Appended rather than swapped for the extension, so `model.gguf` and
    /// `model.bin` could never share one, and so the library, which lists
    /// `.gguf` files only, never lists a file still arriving.
    pub fn partial(&self) -> PathBuf {
        let mut name = self.destination.clone().into_os_string();
        name.push(".part");
        PathBuf::from(name)
    }
}

impl Item {
    /// What a person chose in a repository, as files to fetch from `host`
    /// into `library`'s download folder.
    pub fn chosen(host: &str, repo: &str, choice: &Choice, library: &Library) -> Item {
        let files = choice
            .pieces
            .iter()
            .chain(choice.projector.as_ref())
            .map(|file| Piece {
                url: url(host, repo, &file.path),
                destination: library.destination(repo, &file.path),
                bytes: file.bytes,
                sha256: file.sha256.clone(),
            })
            .collect();
        Item {
            repo: repo.to_owned(),
            name: choice.name.clone(),
            files,
        }
    }

    /// Every byte the item costs, as the index stated it.
    pub fn bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0u64, |total, piece| total.saturating_add(piece.bytes))
    }

    /// The file a backend is handed: the first piece of the weights. Two
    /// requests for the same model are the same item because they land here.
    pub fn target(&self) -> Option<&std::path::Path> {
        self.files.first().map(|piece| piece.destination.as_path())
    }
}

/// `{host}/{repo}/resolve/main/{path}`, each segment escaped, so a filename
/// with a space in it is a filename rather than a malformed request.
fn url(host: &str, repo: &str, path: &str) -> String {
    let segments = repo
        .split('/')
        .chain(["resolve", "main"])
        .chain(path.split('/'));
    match reqwest::Url::parse(host) {
        Ok(mut url) => {
            if let Ok(mut path) = url.path_segments_mut() {
                path.pop_if_empty().extend(segments);
            }
            url.to_string()
        }
        Err(_) => format!(
            "{}/{}",
            host.trim_end_matches('/'),
            segments.collect::<Vec<_>>().join("/")
        ),
    }
}

/// Why an item stopped, as data. The window writes the sentence; each of
/// these is a different thing for a person to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Failure {
    /// Refused before a byte was fetched: the volume has less free space than
    /// what is still to come.
    #[error("{needs} bytes are needed and {free} are free")]
    NoRoom { needs: u64, free: u64 },
    /// A write failed because the volume filled up while it was running.
    #[error("the disk is full")]
    DiskFull,
    /// `401` or `403`: the file needs an account, or accepted terms, and this
    /// fetch is keyless.
    #[error("the file needs an account ({status})")]
    Gated { status: u16 },
    /// `404`: the file is not in the repository any more.
    #[error("the file is not there any more")]
    Missing,
    /// `429`: the host is limiting this address. It passes.
    #[error("the host is limiting requests")]
    RateLimited,
    /// Any other refusal, with its status.
    #[error("the host refused with {status}")]
    Refused { status: u16 },
    /// Nothing answered: no connection, DNS, a handshake.
    #[error("the host could not be reached: {detail}")]
    Unreachable { detail: String },
    /// The connection was reset in the middle of the body.
    #[error("the connection was reset at {received} bytes")]
    Reset { received: u64 },
    /// The body ended before the file did.
    #[error("the transfer ended at {received} of {expected} bytes")]
    EndedEarly { received: u64, expected: u64 },
    /// No bytes for long enough that the connection is not coming back.
    #[error("nothing arrived for {seconds}s")]
    Stalled { seconds: u64 },
    /// The server's length is not the length the index listed, so what it is
    /// sending is not the file that was chosen.
    #[error("the host is sending {stated} bytes where {expected} were listed")]
    WrongSize { stated: u64, expected: u64 },
    /// A web page where a file was expected: what a login redirect answers
    /// with.
    #[error("the host sent a web page ({content_type}), not the file")]
    NotTheFile { content_type: String },
    /// The bytes do not hash to the digest the index published.
    #[error("the file does not match its published digest")]
    Corrupt,
    /// The bytes are the right length and are not a model.
    #[error("the file is not a model: {damage}")]
    Damaged { damage: Damage },
    /// Any other disk failure: permissions, a folder that cannot be made.
    #[error("the disk refused: {detail}")]
    Disk { detail: String },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_partial_keeps_the_real_extension() {
        let piece = Piece {
            url: String::new(),
            destination: PathBuf::from("models/a/model.gguf"),
            bytes: 0,
            sha256: None,
        };
        assert!(piece.partial().ends_with("model.gguf.part"));
    }

    #[test]
    fn a_url_escapes_each_segment_and_keeps_the_directory() {
        assert_eq!(
            url("https://huggingface.co", "a/b", "Q4_K_M/my model.gguf"),
            "https://huggingface.co/a/b/resolve/main/Q4_K_M/my%20model.gguf"
        );
        assert_eq!(
            url("http://127.0.0.1:9/", "a/b", "m.gguf"),
            "http://127.0.0.1:9/a/b/resolve/main/m.gguf"
        );
    }

    #[test]
    fn a_failure_crosses_to_the_window_as_a_kind() {
        let json = serde_json::to_value(Failure::Reset { received: 20 }).expect("serialised");
        assert_eq!(json["kind"], "reset");
        assert_eq!(json["received"], 20);
    }
}
