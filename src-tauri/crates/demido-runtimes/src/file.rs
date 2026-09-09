//! `runtimes.json`, one per profile, beside `settings.json`.
//!
//! Same shape as `demido_settings::file::Files`: a temporary file and a
//! rename, so a process killed mid write leaves the previous ledger rather
//! than half of the new one, and a file this build cannot read is moved aside
//! rather than overwritten on the next write.

use std::path::{Path, PathBuf};

use crate::state::{Ledger, GENERATION};
use crate::store::{Error, Result, Store};

pub const FILE_NAME: &str = "runtimes.json";
pub const KEPT_ASIDE: &str = "runtimes.json.unreadable";

#[derive(Debug, Clone)]
pub struct Files {
    path: PathBuf,
}

impl Files {
    /// The ledger of the profile whose data directory this is.
    pub fn in_profile(directory: impl AsRef<Path>) -> Self {
        Self::at(directory.as_ref().join(FILE_NAME))
    }

    /// A ledger at an exact path. Nothing is created here: a store that made a
    /// directory when constructed would put a folder on disk for a profile
    /// that has fetched nothing.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The profile's runtimes directory: where a managed row's bytes live,
    /// sibling to the ledger rather than under it.
    pub fn runtimes_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|parent| parent.join("runtimes"))
            .unwrap_or_else(|| PathBuf::from("runtimes"))
    }

    fn keep_aside(&self, detail: impl std::fmt::Display) -> Error {
        let aside = self.path.with_file_name(KEPT_ASIDE);
        match std::fs::rename(&self.path, &aside) {
            Ok(()) => Error::unreadable(
                format!("{} was kept at {}", self.path.display(), aside.display()),
                detail,
            ),
            Err(error) => Error::unreadable(
                format!("{} could not be moved aside ({error})", self.path.display()),
                detail,
            ),
        }
    }
}

impl Store for Files {
    fn read(&self) -> Result<Ledger> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Ledger::default())
            }
            Err(error) => return Err(Error::io(format!("reading {}", self.path.display()), error)),
        };

        let ledger: Ledger = match serde_json::from_slice(&bytes) {
            Ok(ledger) => ledger,
            Err(error) => return Err(self.keep_aside(error)),
        };

        if ledger.generation != GENERATION {
            return Err(self.keep_aside(format!(
                "generation {} is not {GENERATION}",
                ledger.generation
            )));
        }

        Ok(ledger)
    }

    fn write(&self, ledger: &Ledger) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
            }
        }

        let json = serde_json::to_vec_pretty(ledger)
            .map_err(|error| Error::io("writing the ledger", std::io::Error::other(error)))?;

        let staged = self.path.with_extension("json.writing");
        std::fs::write(&staged, &json)
            .map_err(|error| Error::io(format!("writing {}", staged.display()), error))?;
        std::fs::rename(&staged, &self.path).map_err(|error| {
            let _ = std::fs::remove_file(&staged);
            Error::io(format!("replacing {}", self.path.display()), error)
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-runtimes-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_profile_with_no_fetch_reads_as_an_empty_ledger() {
        let dir = scratch("empty");
        let store = Files::in_profile(&dir);
        assert_eq!(store.read().expect("read"), Ledger::default());
        assert!(!dir.exists(), "constructing a store touches nothing");
    }

    #[test]
    fn the_file_is_readable_without_the_app() {
        let dir = scratch("readable");
        let store = Files::in_profile(&dir);
        let mut ledger = Ledger::default();
        ledger.set(
            "llama.cpp",
            crate::state::RowState::Managed {
                pin: "b10816".into(),
                archives: vec!["llama-b10816-bin-win-cuda-13.3-x64.zip".into()],
                on_disk_mib: 182.6,
            },
        );
        store.write(&ledger).expect("wrote");

        let text = std::fs::read_to_string(store.path()).expect("read");
        assert!(text.contains("\"pin\": \"b10816\""), "{text}");
        assert!(text.contains('\n'), "pretty printed, for a person: {text}");
    }

    #[test]
    fn a_write_leaves_no_staged_file_beside_the_ledger() {
        let dir = scratch("staged");
        let store = Files::in_profile(&dir);
        store.write(&Ledger::default()).expect("wrote");

        let left: Vec<String> = std::fs::read_dir(&dir)
            .expect("read the directory")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        assert_eq!(left, vec![FILE_NAME.to_owned()]);
    }

    #[test]
    fn an_unreadable_file_is_kept_rather_than_discarded() {
        let dir = scratch("unreadable");
        let store = Files::in_profile(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");
        std::fs::write(store.path(), "not a ledger").expect("wrote");

        let error = store.read().expect_err("reported");
        assert!(error.to_string().contains(KEPT_ASIDE));
        assert!(dir.join(KEPT_ASIDE).exists());
    }

    #[test]
    fn the_runtimes_directory_sits_beside_the_ledger() {
        let dir = scratch("layout");
        let store = Files::in_profile(&dir);
        assert_eq!(store.runtimes_dir(), dir.join("runtimes"));
    }
}
