//! `downloads.json`, one per profile: what the queue was asked to fetch.
//!
//! **The file records intent; the bytes on disk record progress.** An entry is
//! an item and whether somebody paused it or it failed, and nothing about how
//! far it got: that is the length of its partial files, read again on the next
//! launch. A count written here would be a second record of the same fact, and
//! the one on disk would be right whenever the two disagreed.
//!
//! A finished item and a cancelled one leave the file. The library is the
//! record of a finished model, and a cancel is somebody saying they do not
//! want it.
//!
//! Written the way `runtimes.json` is: a staged file and a rename, so a process
//! killed mid write leaves the previous queue rather than half of the next, and
//! a file this build cannot read is moved aside rather than overwritten.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Failure, Item};

pub const FILE_NAME: &str = "downloads.json";
pub const KEPT_ASIDE: &str = "downloads.json.unreadable";

/// Bumped when the shape changes. A file of another generation is kept aside
/// rather than read as something it is not.
const GENERATION: u32 = 1;

/// One item the profile asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    pub item: Item,
    /// Why it is not running. Absent means it should be: a restart picks it
    /// up where its bytes on disk say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "held", rename_all = "kebab-case")]
pub enum Held {
    /// Somebody paused it. A restart does not undo that.
    Paused,
    /// It failed, and the row keeps saying why until somebody retries.
    Failed { failure: Failure },
}

#[derive(Serialize, Deserialize)]
struct Queue {
    generation: u32,
    items: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Files {
    path: PathBuf,
}

impl Files {
    /// The queue of the profile whose data directory this is. Nothing is
    /// created until something is queued.
    pub fn in_profile(directory: impl AsRef<Path>) -> Self {
        Self::at(directory.as_ref().join(FILE_NAME))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What was queued, in the order it was queued.
    ///
    /// **Never fails.** A missing file is an empty queue, and a file this
    /// build cannot read is kept aside and reported in the log: startup never
    /// blocks, and a queue that could not be read is not a window that cannot
    /// open.
    pub fn read(&self) -> Vec<Entry> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "the download queue could not be read");
                return Vec::new();
            }
        };
        match serde_json::from_slice::<Queue>(&bytes) {
            Ok(queue) if queue.generation == GENERATION => queue.items,
            Ok(queue) => {
                self.keep_aside(format!("generation {}", queue.generation));
                Vec::new()
            }
            Err(error) => {
                self.keep_aside(error);
                Vec::new()
            }
        }
    }

    pub fn write(&self, items: &[Entry]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_vec_pretty(&Queue {
            generation: GENERATION,
            items: items.to_vec(),
        })
        .map_err(std::io::Error::other)?;
        let staged = self.path.with_extension("json.writing");
        std::fs::write(&staged, &json)?;
        std::fs::rename(&staged, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&staged);
        })
    }

    fn keep_aside(&self, detail: impl std::fmt::Display) {
        let aside = self.path.with_file_name(KEPT_ASIDE);
        let moved = std::fs::rename(&self.path, &aside);
        tracing::warn!(
            path = %self.path.display(),
            %detail,
            kept = moved.is_ok(),
            "the download queue is not one this build reads, and was kept aside"
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::Piece;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-download-file")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn entry(held: Option<Held>) -> Entry {
        Entry {
            id: 3,
            item: Item {
                repo: "a/b".into(),
                name: "b-Q4_K_M".into(),
                files: vec![Piece {
                    url: "http://x/a/b/resolve/main/b-Q4_K_M.gguf".into(),
                    destination: PathBuf::from("models/a/b/b-Q4_K_M.gguf"),
                    bytes: 10,
                    sha256: None,
                }],
            },
            held,
        }
    }

    #[test]
    fn an_absent_file_is_an_empty_queue_and_reading_it_creates_nothing() {
        let dir = scratch("absent");
        assert!(Files::in_profile(&dir).read().is_empty());
        assert!(!dir.exists());
    }

    #[test]
    fn what_was_asked_for_reads_back_including_why_it_is_held() {
        let dir = scratch("round");
        let files = Files::in_profile(&dir);
        let held = vec![
            entry(None),
            entry(Some(Held::Paused)),
            entry(Some(Held::Failed {
                failure: Failure::Reset { received: 4 },
            })),
        ];
        files.write(&held).expect("wrote");
        assert_eq!(files.read(), held);
    }

    #[test]
    fn an_unreadable_file_is_kept_aside_and_read_as_empty() {
        let dir = scratch("unreadable");
        std::fs::create_dir_all(&dir).expect("made");
        let files = Files::in_profile(&dir);
        std::fs::write(files.path(), "not a queue").expect("wrote");
        assert!(files.read().is_empty());
        assert!(dir.join(KEPT_ASIDE).exists());
    }
}
