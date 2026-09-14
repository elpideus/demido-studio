//! What the answers, taken together, name as the thing to talk to.
//!
//! Both the wizard's last step and the composer ask that, and asking it in two
//! places is how they come to disagree. So it is asked here, once, out of the
//! runtimes ledger and the answers:
//!
//! - the **binary** is whatever the `llama.cpp` row is running, which is a
//!   directory Demido fetched or a path the person pointed at;
//! - the **model** is the GGUF the models step settled on.
//!
//! `None` is the ordinary first launch, and it is the state the composer
//! already knows how to draw.

use std::path::{Path, PathBuf};

use demido_runtimes::{directory_name, Ledger, RowState, LLAMA_CPP};

use crate::answers::Answers;

/// What `llama-server` is called inside a fetched build.
///
/// The same constant `demido_runtimes::verify` launches, taken from there
/// rather than typed again: a binary this crate names and that crate verifies
/// have to be one file.
pub use demido_runtimes::verify::LLAMA_SERVER;

/// The pair a turn needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `llama-server`, fetched from upstream and never bundled (hard rule 3).
    pub binary: PathBuf,
    /// The GGUF to load.
    pub model: PathBuf,
}

/// The target this profile has, or nothing.
///
/// `runtimes_dir` is the profile's runtimes folder, which is where a managed
/// row's directory lives.
///
/// Every part is checked against the disk rather than trusted from the ledger.
/// A row managed at a pin whose directory somebody deleted is a row that
/// cannot answer, and returning it would make the composer say a model is
/// loading and then fail several seconds later with a path in the message.
pub fn target(ledger: &Ledger, runtimes_dir: &Path, answers: &Answers) -> Option<Target> {
    let model = answers.model.clone().filter(|model| model.is_file())?;
    let binary = binary(ledger, runtimes_dir)?;
    Some(Target { binary, model })
}

/// Where `llama-server` is, according to the row that owns it.
pub fn binary(ledger: &Ledger, runtimes_dir: &Path) -> Option<PathBuf> {
    let path = match ledger.state(LLAMA_CPP)? {
        RowState::Managed { pin, .. } => runtimes_dir
            .join(directory_name(LLAMA_CPP, pin))
            .join(LLAMA_SERVER),
        RowState::Linked { path, .. } => path.clone(),
        RowState::Absent { .. } => return None,
    };
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-setup-target")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("made the directory");
        }
        std::fs::write(path, b"binary").expect("wrote");
    }

    #[test]
    fn a_managed_row_resolves_to_the_binary_inside_its_own_directory() {
        let dir = scratch("managed");
        let expected = dir
            .join(directory_name(LLAMA_CPP, "b10816"))
            .join(LLAMA_SERVER);
        touch(&expected);

        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Managed {
                pin: "b10816".into(),
                archives: vec![],
                on_disk_mib: 671.6,
            },
        );
        assert_eq!(binary(&ledger, &dir), Some(expected));
    }

    #[test]
    fn a_linked_row_resolves_to_the_path_the_person_pointed_at() {
        let dir = scratch("linked");
        let theirs = dir.join("tools").join(LLAMA_SERVER);
        touch(&theirs);

        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Linked {
                path: theirs.clone(),
                detected_version: None,
            },
        );
        assert_eq!(binary(&ledger, &dir), Some(theirs));
    }

    /// The ledger says managed and the bytes are gone. The composer is told
    /// there is nothing loaded, which is true, rather than being sent at a
    /// path that is not there.
    #[test]
    fn a_row_whose_bytes_have_gone_is_no_target() {
        let dir = scratch("deleted");
        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Managed {
                pin: "b10816".into(),
                archives: vec![],
                on_disk_mib: 671.6,
            },
        );
        assert_eq!(binary(&ledger, &dir), None);
    }

    #[test]
    fn a_model_that_is_not_there_is_no_target() {
        let dir = scratch("no-model");
        let installed = dir
            .join(directory_name(LLAMA_CPP, "b10816"))
            .join(LLAMA_SERVER);
        touch(&installed);
        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Managed {
                pin: "b10816".into(),
                archives: vec![],
                on_disk_mib: 671.6,
            },
        );

        let answers = Answers {
            model: Some(dir.join("gone.gguf")),
            ..Answers::default()
        };
        assert_eq!(target(&ledger, &dir, &answers), None);
    }

    #[test]
    fn a_finished_set_up_names_both_halves() {
        let dir = scratch("complete");
        let installed = dir
            .join(directory_name(LLAMA_CPP, "b10816"))
            .join(LLAMA_SERVER);
        touch(&installed);
        let model = dir.join("models").join("gemma-4-E4B-it-Q8_0.gguf");
        touch(&model);

        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Managed {
                pin: "b10816".into(),
                archives: vec![],
                on_disk_mib: 671.6,
            },
        );
        let answers = Answers {
            model: Some(model.clone()),
            ..Answers::default()
        };

        assert_eq!(
            target(&ledger, &dir, &answers),
            Some(Target {
                binary: installed,
                model
            })
        );
    }
}
