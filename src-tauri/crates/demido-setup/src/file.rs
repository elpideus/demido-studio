//! `setup.json`, one per profile, beside `settings.json` and `runtimes.json`.
//!
//! Same shape as `demido_runtimes::file::Files`: a temporary file and a
//! rename, so a process killed mid write leaves the previous answers rather
//! than half of the new ones, and a file this build cannot read is moved aside
//! rather than overwritten on the next write.
//!
//! Per profile, because a profile is a Windows user
//! (`docs/rules/profiles.md`) and `docs/rules/setup.md` section 7 scopes
//! set-up to one: a second Windows user gets their own set-up and their own
//! runtimes.

use std::path::{Path, PathBuf};

use crate::answers::{Answers, GENERATION};
use crate::store::{Error, Result, Store};

pub const FILE_NAME: &str = "setup.json";
pub const KEPT_ASIDE: &str = "setup.json.unreadable";

#[derive(Debug, Clone)]
pub struct Files {
    path: PathBuf,
}

impl Files {
    /// The answers of the profile whose data directory this is.
    pub fn in_profile(directory: impl AsRef<Path>) -> Self {
        Self::at(directory.as_ref().join(FILE_NAME))
    }

    /// An answers file at an exact path. Nothing is created here: a store that
    /// made a directory when constructed would put a folder on disk for a
    /// profile that has never opened the wizard.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
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
    fn read(&self) -> Result<Answers> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Answers::default())
            }
            Err(error) => return Err(Error::io(format!("reading {}", self.path.display()), error)),
        };

        let answers: Answers = match serde_json::from_slice(&bytes) {
            Ok(answers) => answers,
            Err(error) => return Err(self.keep_aside(error)),
        };

        if answers.generation != GENERATION {
            return Err(self.keep_aside(format!(
                "generation {} is not {GENERATION}",
                answers.generation
            )));
        }

        Ok(answers)
    }

    fn write(&self, answers: &Answers) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
            }
        }

        let json = serde_json::to_vec_pretty(answers)
            .map_err(|error| Error::io("writing the answers", std::io::Error::other(error)))?;

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
            .join("demido-setup-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_file_keeps_every_promise_the_trait_makes() {
        let root = scratch("contract");
        crate::contract::assert_store(|name| Files::in_profile(root.join(name)));
    }

    #[test]
    fn a_profile_that_has_never_set_up_touches_nothing() {
        let dir = scratch("fresh");
        let store = Files::in_profile(&dir);
        assert_eq!(store.read().expect("read"), Answers::default());
        assert!(!dir.exists(), "constructing a store touches nothing");
    }

    #[test]
    fn the_file_is_readable_without_the_app() {
        let dir = scratch("readable");
        let store = Files::in_profile(&dir);
        let answers = Answers {
            model: Some(PathBuf::from("D:/models/gemma-4-E4B-it-Q8_0.gguf")),
            ..Answers::default()
        };
        store.write(&answers).expect("wrote");

        let text = std::fs::read_to_string(store.path()).expect("read");
        assert!(text.contains("gemma-4-E4B-it-Q8_0.gguf"), "{text}");
        assert!(text.contains('\n'), "pretty printed, for a person: {text}");
    }

    #[test]
    fn a_write_leaves_no_staged_file_beside_the_answers() {
        let dir = scratch("staged");
        let store = Files::in_profile(&dir);
        store.write(&Answers::default()).expect("wrote");

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
        std::fs::write(store.path(), "not an answers file").expect("wrote");

        let error = store.read().expect_err("reported");
        assert!(error.to_string().contains(KEPT_ASIDE));
        assert!(dir.join(KEPT_ASIDE).exists());
    }
}
