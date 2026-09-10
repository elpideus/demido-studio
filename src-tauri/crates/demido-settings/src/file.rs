//! `settings.json`, one per profile.
//!
//! JSON in a file the user can open, edit or copy, for the same reason the
//! session log is JSON Lines: the thing a person reaches for when the app and
//! their memory disagree should be readable without the app. Deleting it is a
//! supported way to get every default back.
//!
//! **Per profile is the operating system's doing, not Demido's.**
//! [`docs/rules/profiles.md`](../../../../../docs/rules/profiles.md) rules that
//! a Demido profile is a Windows profile, so the directory handed to
//! [`Files::in_profile`] is already inside `%LOCALAPPDATA%` and already carries
//! the ACL that keeps a second Windows user out.
//!
//! The write is a temporary file and a rename, so a process killed mid write
//! leaves the previous settings rather than half of the new ones.

use std::path::{Path, PathBuf};

use crate::document::{Document, GENERATION};
use crate::store::{Error, Result, Store};

/// What the document is called inside a profile.
pub const FILE_NAME: &str = "settings.json";

/// Where an unreadable file is moved, so that reading it wrongly never costs
/// somebody the values they typed.
pub const KEPT_ASIDE: &str = "settings.json.unreadable";

/// Settings on disk.
#[derive(Debug, Clone)]
pub struct Files {
    path: PathBuf,
}

impl Files {
    /// The settings of the profile whose data directory this is.
    pub fn in_profile(directory: impl AsRef<Path>) -> Self {
        Self::at(directory.as_ref().join(FILE_NAME))
    }

    /// Settings at an exact path.
    ///
    /// Nothing is created here. A store that made a directory when it was
    /// constructed would put a folder on disk for a profile that never changed
    /// a setting, and would make assembling the composition root a thing that
    /// touches the filesystem.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where the settings are kept.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Move a file this build cannot read out of the way, and say where it
    /// went.
    ///
    /// Never a delete. The next write would otherwise overwrite settings
    /// somebody typed with a document assembled from defaults, which is the one
    /// failure mode of a store that reports rather than discards.
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
    fn read(&self) -> Result<Document> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            // A profile that has never changed a setting, which is every
            // profile until somebody does. Not a failure, and not a file.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document::default())
            }
            Err(error) => return Err(Error::io(format!("reading {}", self.path.display()), error)),
        };

        let document: Document = match serde_json::from_slice(&bytes) {
            Ok(document) => document,
            Err(error) => return Err(self.keep_aside(error)),
        };

        if document.generation != GENERATION {
            return Err(self.keep_aside(format!(
                "generation {} is not {GENERATION}",
                document.generation
            )));
        }

        Ok(document)
    }

    fn write(&self, document: &Document) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
            }
        }

        let json = serde_json::to_vec_pretty(document)
            .map_err(|error| Error::io("writing the settings", std::io::Error::other(error)))?;

        // Beside the settings rather than in the system temp directory, because
        // a rename is only atomic within one volume and a profile can sit on a
        // different one.
        let staged = self.path.with_extension("json.writing");
        std::fs::write(&staged, &json)
            .map_err(|error| Error::io(format!("writing {}", staged.display()), error))?;
        std::fs::rename(&staged, &self.path).map_err(|error| {
            // The staged file is this store's litter, so it clears it rather
            // than leaving a `.json.writing` beside the settings forever.
            let _ = std::fs::remove_file(&staged);
            Error::io(format!("replacing {}", self.path.display()), error)
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use serde_json::json;

    use super::*;
    use crate::document::Scope;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-settings-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_profile_directory_gets_one_settings_json() {
        let dir = scratch("named");
        let store = Files::in_profile(&dir);
        assert_eq!(store.path(), dir.join("settings.json"));
        assert!(
            !dir.exists(),
            "constructing a store touches nothing on disk"
        );
    }

    #[test]
    fn writing_creates_the_profile_directory_and_nothing_else() {
        let dir = scratch("created");
        let store = Files::in_profile(&dir);
        store.write(&Document::default()).expect("wrote");

        let left: Vec<String> = std::fs::read_dir(&dir)
            .expect("read the directory")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        assert_eq!(
            left,
            vec!["settings.json".to_owned()],
            "the staged file is renamed, never left beside the settings"
        );
    }

    /// The difference between this store and the desk's: work somebody typed is
    /// never thrown away, even when this build cannot read it.
    #[test]
    fn a_file_this_build_cannot_read_is_kept_rather_than_discarded() {
        let dir = scratch("unreadable");
        let store = Files::in_profile(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");
        std::fs::write(store.path(), "not settings at all").expect("wrote");

        let error = store.read().expect_err("an unreadable file is reported");
        assert!(
            error.to_string().contains(KEPT_ASIDE),
            "the report says where the file went: {error}"
        );
        assert!(dir.join(KEPT_ASIDE).exists());
        assert!(!store.path().exists(), "the next write starts clean");
    }

    #[test]
    fn a_document_from_another_generation_is_kept_too() {
        let dir = scratch("generation");
        let store = Files::in_profile(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");
        std::fs::write(store.path(), r#"{"generation": 99, "global": {}}"#).expect("wrote");

        assert!(store.read().is_err());
        assert!(dir.join(KEPT_ASIDE).exists());
    }

    #[test]
    fn settings_a_user_deleted_are_the_defaults_rather_than_a_fault() {
        let dir = scratch("deleted");
        let store = Files::in_profile(&dir);
        store.write(&Document::default()).expect("wrote");
        std::fs::remove_file(store.path()).expect("deleted");
        assert_eq!(store.read().expect("read"), Document::default());
    }

    #[test]
    fn the_file_is_readable_without_the_app() {
        let dir = scratch("readable");
        let store = Files::in_profile(&dir);
        let mut document = Document::default();
        document
            .values_mut(&Scope::Global)
            .insert("conversation.temperature".into(), json!(0.4));
        store.write(&document).expect("wrote");

        let text = std::fs::read_to_string(store.path()).expect("read");
        assert!(text.contains("\"conversation.temperature\": 0.4"), "{text}");
        assert!(text.contains('\n'), "pretty printed, for a person: {text}");
    }
}
