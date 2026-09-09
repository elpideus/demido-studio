//! What the guided set-up was told, and how it survives a restart.
//!
//! **Answers only.** Nothing here says what is still outstanding, because that
//! is [`crate::plan`]'s and it is derived from disk on every read
//! (`docs/rules/setup.md` section 1: what is outstanding "cannot go stale
//! against what is actually there"). A file that remembered the answer to
//! "is llama.cpp installed" would be a second copy of the runtimes ledger,
//! and the two would eventually disagree.
//!
//! So each field below is a choice a person made, and there is exactly one
//! field that is not: [`Answers::closed`], which records the wizard having
//! been closed rather than anything about the machine. Without it, leaving the
//! wizard would reopen the wizard.

use std::path::PathBuf;

use demido_hardware::Ecosystem;
use serde::{Deserialize, Serialize};

/// The shape this build can read. An answers file from another generation is
/// kept aside rather than misread ([`crate::file::Files`]).
pub const GENERATION: u32 = 1;

/// Everything the wizard has been told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Answers {
    pub generation: u32,

    /// Which accelerator a fetched build is for.
    ///
    /// `None` is not "CPU": it is nobody having confirmed or overridden the
    /// pre-selection yet, and until they do the pre-selection is detection's
    /// (`docs/rules/setup.md` section 3: nothing detectable is ever asked as a
    /// question). Confirming writes what was detected, which is what makes the
    /// step settled and what the settings page later edits.
    pub ecosystem: Option<Ecosystem>,

    /// Where models are read from.
    ///
    /// Brief B55: "Multiple folders should be set-able for model detection"
    ///
    /// Pre-filled from a readable folder already on the machine and confirmed
    /// rather than typed, so a person with models from LM Studio moves no
    /// files and makes no symlinks. A folder that has since gone is left in
    /// the list: it is still what the person chose, and the models step says
    /// it read nothing from it.
    pub folders: Vec<PathBuf>,

    /// The GGUF that answers.
    ///
    /// The one answer the composer also needs, which is why
    /// [`crate::target`] exists: the wizard's last step and the composer
    /// asking two different questions is how they come to disagree about what
    /// is loaded.
    pub model: Option<PathBuf>,

    /// The manifest rows the person has left ticked.
    ///
    /// Ids rather than a struct per row, because the capability group is data
    /// (`docs/rules/setup.md` section 4) and a field per capability would be
    /// the second screen that rule exists to refuse. A row nobody has touched
    /// is not in here and is ticked by default, which is what makes clearing
    /// one a decision and leaving one alone free.
    pub unticked: Vec<String>,

    /// Whether the wizard has been closed, by leaving it or by finishing it.
    ///
    /// The only remembered thing here that is not an answer about the machine,
    /// and it is remembered because it is about a gesture rather than about
    /// disk: "leaving is never final" means the desk offers the rest, not that
    /// the wizard reopens over it (`docs/rules/setup.md` section 1). What is
    /// still outstanding is never read from here; that is the plan's, and the
    /// plan is derived.
    pub closed: bool,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            generation: GENERATION,
            ecosystem: None,
            folders: Vec::new(),
            model: None,
            unticked: Vec::new(),
            closed: false,
        }
    }
}

impl Answers {
    /// Whether a manifest row is one the person wants fetched.
    ///
    /// Ticked unless it was cleared, so a row added to the manifest after
    /// somebody set up arrives on rather than off, which is section 4's
    /// "all on by default" surviving the addition of a row.
    pub fn ticked(&self, id: &str) -> bool {
        !self.unticked.iter().any(|cleared| cleared == id)
    }

    /// Tick or clear a row.
    pub fn tick(&mut self, id: &str, on: bool) {
        self.unticked.retain(|cleared| cleared != id);
        if !on {
            self.unticked.push(id.to_owned());
        }
    }

    /// Add a folder models are read from, if it is not already one.
    pub fn add_folder(&mut self, folder: PathBuf) {
        if !self.folders.contains(&folder) {
            self.folders.push(folder);
        }
    }

    pub fn remove_folder(&mut self, folder: &PathBuf) {
        self.folders.retain(|kept| kept != folder);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn a_row_nobody_has_touched_is_ticked() {
        let answers = Answers::default();
        assert!(answers.ticked("llama.cpp"));
        assert!(
            answers.ticked("a capability added next year"),
            "a row added to the manifest arrives on, not off"
        );
    }

    #[test]
    fn clearing_a_row_survives_being_cleared_twice() {
        let mut answers = Answers::default();
        answers.tick("chrome", false);
        answers.tick("chrome", false);
        assert_eq!(answers.unticked, vec!["chrome".to_owned()]);
        answers.tick("chrome", true);
        assert!(answers.unticked.is_empty());
    }

    #[test]
    fn a_folder_is_added_once() {
        let mut answers = Answers::default();
        answers.add_folder(PathBuf::from("C:/models"));
        answers.add_folder(PathBuf::from("C:/models"));
        assert_eq!(answers.folders.len(), 1);
    }
}
