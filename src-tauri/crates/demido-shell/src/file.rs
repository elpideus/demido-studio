//! `shell.json`, one per profile.
//!
//! JSON in a file the user can open, delete or copy, for the same reason the
//! session log is JSON Lines: the thing a person reaches for when the app and
//! their memory disagree should be readable without the app. Deleting it is a
//! supported way to get the default desk back, which is why nothing here treats
//! a missing file as a fault.
//!
//! **Per profile is the operating system's doing, not Demido's.**
//! `docs/rules/profiles.md` rules that a Demido profile is a Windows profile,
//! so the directory handed to [`Files::in_profile`] is already inside
//! `%LOCALAPPDATA%` and already carries the ACL that keeps a second Windows
//! user out. This crate adds no boundary of its own, because a second boundary
//! is a second thing that can be subtly weaker than the first.
//!
//! The write is a temporary file and a rename, so a process killed mid write
//! leaves the previous layout rather than half of the new one. That matters
//! more here than it looks: the file is written after every gesture settles, so
//! the odds of being killed during one are not small, and a torn `shell.json`
//! is a desk that silently forgets.

use std::path::{Path, PathBuf};

use crate::layout::{Shell, Written, GENERATION};
use crate::store::{Error, Result, Store};

/// What the layout is called inside a profile.
pub const FILE_NAME: &str = "shell.json";

/// A layout on disk.
#[derive(Debug, Clone)]
pub struct Files {
    path: PathBuf,
}

impl Files {
    /// The layout of the profile whose data directory this is.
    pub fn in_profile(directory: impl AsRef<Path>) -> Self {
        Self::at(directory.as_ref().join(FILE_NAME))
    }

    /// A layout at an exact path.
    ///
    /// Nothing is created here. A store that made a directory when it was
    /// constructed would put a folder on disk for a profile that never
    /// arranged anything, and would make assembling the composition root a
    /// thing that touches the filesystem.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Where the layout is kept.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Store for Files {
    fn read(&self) -> Option<Shell> {
        let bytes = std::fs::read(&self.path).ok()?;

        // Everything below is a discard, and none of it is a report. A layout
        // file that will not load must never be a reason the desk does not
        // open, so there is exactly one thing this function can say about a bad
        // file, and it is `None`. The developer channel still gets a line,
        // because "my rail moved back to the left" is otherwise unexplainable.
        let written: Written = match serde_json::from_slice(&bytes) {
            Ok(written) => written,
            Err(error) => {
                tracing::debug!(
                    path = %self.path.display(),
                    %error,
                    "the remembered layout is not one; drawing the default desk"
                );
                return None;
            }
        };

        if written.generation != GENERATION {
            tracing::debug!(
                path = %self.path.display(),
                found = written.generation,
                expected = GENERATION,
                "the remembered layout is an older generation; discarded"
            );
            return None;
        }

        Some(written.shell)
    }

    fn write(&self, shell: &Shell) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| Error::io(format!("creating {}", parent.display()), error))?;
            }
        }

        let json = serde_json::to_vec_pretty(&Written::from(*shell))
            .map_err(|error| Error::io("writing the layout", std::io::Error::other(error)))?;

        // Beside the layout rather than in the system temp directory, because a
        // rename is only atomic within one volume and a profile can sit on a
        // different one.
        let staged = self.path.with_extension("json.writing");
        std::fs::write(&staged, &json)
            .map_err(|error| Error::io(format!("writing {}", staged.display()), error))?;
        std::fs::rename(&staged, &self.path).map_err(|error| {
            // The staged file is this store's litter, so it clears it rather
            // than leaving a `.json.writing` beside the layout forever.
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

    use super::*;
    use crate::layout::Side;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-shell-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_profile_directory_gets_one_shell_json() {
        let dir = scratch("named");
        let store = Files::in_profile(&dir);
        assert_eq!(store.path(), dir.join("shell.json"));
        assert!(
            !dir.exists(),
            "constructing a store touches nothing on disk"
        );
    }

    #[test]
    fn writing_creates_the_profile_directory_and_nothing_else() {
        let dir = scratch("created");
        let store = Files::in_profile(&dir);
        store.write(&Shell { rail: Side::Right }).expect("wrote");

        let left: Vec<String> = std::fs::read_dir(&dir)
            .expect("read the directory")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        assert_eq!(
            left,
            vec!["shell.json".to_owned()],
            "the staged file is renamed, never left beside the layout"
        );
    }

    #[test]
    fn a_layout_from_an_older_generation_is_discarded_without_a_word() {
        let dir = scratch("older");
        let store = Files::in_profile(&dir);
        store.write(&Shell { rail: Side::Right }).expect("wrote");
        std::fs::write(
            store.path(),
            format!(r#"{{"generation": {}, "rail": "right"}}"#, GENERATION - 1),
        )
        .expect("wrote an older file");

        assert_eq!(
            store.read(),
            None,
            "an older generation reads as no layout at all, so the desk draws the default"
        );
    }

    #[test]
    fn a_layout_that_is_not_a_layout_is_discarded_too() {
        let dir = scratch("garbage");
        let store = Files::in_profile(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");

        for spoiled in [
            "",
            "not json at all",
            r#"{"generation": 1}"#,
            r#"{"generation": 1, "rail": "underneath"}"#,
            r#"{"rail": "right"}"#,
        ] {
            std::fs::write(store.path(), spoiled).expect("wrote");
            assert_eq!(store.read(), None, "{spoiled:?} is not a layout");
        }
    }

    #[test]
    fn a_layout_the_user_deleted_is_the_default_desk_rather_than_a_fault() {
        let dir = scratch("deleted");
        let store = Files::in_profile(&dir);
        store.write(&Shell { rail: Side::Right }).expect("wrote");
        std::fs::remove_file(store.path()).expect("deleted");
        assert_eq!(store.read(), None);
    }

    #[test]
    fn the_file_is_readable_without_the_app() {
        let dir = scratch("readable");
        let store = Files::in_profile(&dir);
        store.write(&Shell { rail: Side::Right }).expect("wrote");

        let text = std::fs::read_to_string(store.path()).expect("read");
        assert!(text.contains("\"rail\": \"right\""), "{text}");
        assert!(text.contains('\n'), "pretty printed, for a person: {text}");
    }
}
