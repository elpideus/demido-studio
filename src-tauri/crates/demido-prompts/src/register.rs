//! The paragraphs in force, and the four things anyone does to them.
//!
//! This is the seam. Everything outside asks for one paragraph or the whole
//! list, edits one, or resets one; nothing outside builds a path, reads a
//! default file, or decides what a paragraph says when there is no edit.
//!
//! It holds no loaded state on purpose. Every call reads the directory again,
//! which is what makes hot reload a property of the design rather than a
//! feature somebody has to remember to wire up: a paragraph edited in one
//! window is what the next turn sends, with nothing to invalidate in between.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{self, Dependant, Dependency, Paragraph};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No paragraph by that id. Also the reason nothing outside the prompts
    /// directory is ever opened: an id with no declaration has no path.
    #[error("no prompt called {0}")]
    Unknown(String),

    /// The edit uses a name the declaration does not carry, so it could never
    /// expand. Refused rather than accepted, because the failure is invisible
    /// until a model is handed a literal pair of braces.
    ///
    /// This is the only thing `set` refuses, and it is not a judgement about
    /// the wording: `docs/rules/prompts.md` has no read-only entries.
    #[error("{id}: nothing will fill {{{{{name}}}}}")]
    Undeclared { id: String, name: String },

    #[error(transparent)]
    Write(#[from] demido_core::Error),
}

/// Where a prompt's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// The text this build ships. There is no file.
    BuiltIn,
    /// A file in the prompts directory, written by an edit.
    Edited,
}

/// One paragraph as it stands right now: what it says, where that came from,
/// the name the session log knows it by, and what the user gave up to get it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prompt {
    pub paragraph: &'static Paragraph,
    /// The text, with line endings normalised. Placeholders are still standing:
    /// filling them is the caller's business, because only the caller knows
    /// what it is asking for.
    pub text: String,
    pub origin: Origin,
    /// `sha256:` and the digest of `text`. This is what a turn records, and it
    /// is what makes "which version of this prompt produced that reply" a
    /// question with an answer.
    pub hash: String,
    /// For an edit, the hash of the built-in wording it was made from.
    ///
    /// Recorded because a later build improving a default is otherwise
    /// invisible to anyone who has edited that entry, and with the release gate
    /// above them that user is running a build whose measured claims do not
    /// describe their app. `None` on a built-in, and on an edit written by hand
    /// into the directory rather than through [`Paragraphs::set`].
    pub base: Option<String>,
    /// The measured claims this text no longer supports.
    ///
    /// Empty unless the paragraph has been edited. A user may degrade their own
    /// classifier: [`Origin::Edited`] records that they did, and nothing
    /// detects that it hurt. That cost is written down rather than glossed.
    pub suppressed: Vec<&'static Dependant>,
    /// Anything the user should be told about where this came from: an edited
    /// file that could not be read, or a built-in wording that has changed
    /// since the edit was made.
    ///
    /// It is a note. It never blocks a turn, and it never becomes a prompt to
    /// act.
    pub note: Option<String>,
}

impl Prompt {
    /// The text with its placeholders filled.
    ///
    /// The hash is not recomputed on purpose: it identifies the version of the
    /// wording, not the string one particular turn built out of it. Two turns
    /// that filled the same paragraph differently point at the same stored
    /// text, and the values they filled it with are recorded beside it.
    pub fn fill(&self, values: &[(&str, &str)]) -> String {
        catalog::fill(&self.text, values)
    }
}

/// The prompts directory, and the operations on it.
///
/// Cheap to build and cheap to throw away. See the module note: it deliberately
/// remembers nothing.
#[derive(Debug, Clone)]
pub struct Paragraphs {
    dir: PathBuf,
}

impl Paragraphs {
    /// Open the prompts directory, creating nothing.
    ///
    /// Nothing is created because the normal state of this directory is empty:
    /// a paragraph nobody has edited has no file.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// One paragraph, as it stands.
    pub fn get(&self, id: &str) -> Option<Prompt> {
        catalog::paragraph(id).map(|paragraph| self.read(paragraph))
    }

    /// Every paragraph, in register order. What the editor lists.
    pub fn all(&self) -> Vec<Prompt> {
        catalog::CATALOG
            .iter()
            .map(|paragraph| self.read(paragraph))
            .collect()
    }

    /// Replace one paragraph's text, and hand the prompt back.
    ///
    /// Text identical to the built-in default resets instead of writing a file,
    /// so a user who edits a paragraph back to what it was does not keep a copy
    /// that stops tracking future changes to the default.
    ///
    /// A placeholder the declaration carries may be dropped: making the reply
    /// and the reasoning read the same is a legitimate edit. One it does not
    /// carry may not, because nothing would ever fill it. Nothing else is
    /// refused, whatever the entry is load-bearing for.
    pub fn set(&self, id: &str, text: &str) -> Result<Prompt> {
        let paragraph = catalog::paragraph(id).ok_or_else(|| Error::Unknown(id.to_owned()))?;
        let text = normalise(text);

        for name in catalog::placeholders_in(&text) {
            if !paragraph.placeholders.contains(&name.as_str()) {
                return Err(Error::Undeclared {
                    id: id.to_owned(),
                    name,
                });
            }
        }

        if text == normalise(paragraph.default) {
            return self.reset(id);
        }

        write(&self.path(paragraph), &text)?;
        write(
            &self.base_path(paragraph),
            &digest(&normalise(paragraph.default)),
        )?;

        Ok(self.read(paragraph))
    }

    /// Forget the edit. The built-in text is what the next turn sends, and the
    /// claims it supports come back with it.
    pub fn reset(&self, id: &str) -> Result<Prompt> {
        let paragraph = catalog::paragraph(id).ok_or_else(|| Error::Unknown(id.to_owned()))?;
        remove(&self.path(paragraph))?;
        remove(&self.base_path(paragraph))?;

        Ok(self.read(paragraph))
    }

    /// Where an edit of this paragraph lives.
    ///
    /// Only ever called with a declaration from the register, which is what
    /// keeps an id supplied from the window layer from naming a file elsewhere.
    fn path(&self, paragraph: &Paragraph) -> PathBuf {
        self.dir.join(format!("{}.md", paragraph.id))
    }

    /// Where the base hash of that edit lives.
    ///
    /// Beside the text rather than inside it, so that `<id>.md` stays exactly
    /// the wording and nothing has to be stripped before it is hashed or sent.
    fn base_path(&self, paragraph: &Paragraph) -> PathBuf {
        self.dir.join(format!("{}.base", paragraph.id))
    }

    /// The text in force, and everything true about it.
    ///
    /// An unreadable edit falls back to the built-in text and says so, rather
    /// than failing the turn that asked. A prompt is not something a
    /// conversation is allowed to refuse to start over.
    fn read(&self, paragraph: &'static Paragraph) -> Prompt {
        let path = self.path(paragraph);
        let built_in = normalise(paragraph.default);

        let (text, origin, mut note) = match std::fs::read_to_string(&path) {
            Ok(text) => (normalise(&text), Origin::Edited, None),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                (built_in.clone(), Origin::BuiltIn, None)
            }
            Err(err) => {
                tracing::warn!(?path, %err, "an edited prompt could not be read");
                (
                    built_in.clone(),
                    Origin::BuiltIn,
                    Some(format!(
                        "{} could not be read ({err}), so the built-in text was used instead.",
                        path.display()
                    )),
                )
            }
        };

        let base = match origin {
            Origin::BuiltIn => None,
            Origin::Edited => std::fs::read_to_string(self.base_path(paragraph))
                .ok()
                .map(|base| base.trim().to_owned())
                .filter(|base| !base.is_empty()),
        };

        // The built-in wording has moved on since this edit was made. Said
        // once, as a note, with the editor's diff and reset behind it.
        // not-a-prompt: shown to the person who made the edit, never sent.
        if let Some(base) = &base {
            if base != &digest(&built_in) && note.is_none() {
                note = Some(
                    "This was edited from an earlier version of the built-in text, which has since changed. Reset to take the new wording, or keep this one."
                        .to_owned(),
                );
            }
        }

        let suppressed = match origin {
            Origin::BuiltIn => Vec::new(),
            Origin::Edited => paragraph
                .dependants
                .iter()
                .filter(|dependant| matches!(dependant.kind, Dependency::Measured { .. }))
                .collect(),
        };

        Prompt {
            paragraph,
            hash: digest(&text),
            text,
            origin,
            base,
            suppressed,
            note,
        }
    }
}

/// One line ending, whoever wrote the file.
///
/// Not cosmetic: the hash is the identity of a prompt version, and a hash that
/// changed because a file was opened in an editor that writes CRLF would report
/// an edit nobody made, on a machine that had merely checked the repository out
/// differently. `scripts/check-rules.mjs` normalises the same way.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The name a prompt version goes into the log under.
fn digest(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Write in one go, so a prompt file that exists is a prompt file that is
/// complete. A half written paragraph is a model reading half a rule.
fn write(path: &Path, text: &str) -> std::result::Result<(), demido_core::Error> {
    let context = || format!("writing {}", path.display());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| demido_core::Error::io(context(), err))?;
    }

    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, text).map_err(|err| demido_core::Error::io(context(), err))?;
    std::fs::rename(&temporary, path).map_err(|err| demido_core::Error::io(context(), err))
}

/// Delete, treating "it was not there" as success, which is what every caller
/// of this means by reset.
fn remove(path: &Path) -> std::result::Result<(), demido_core::Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(demido_core::Error::io(
            format!("removing {}", path.display()),
            err,
        )),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use crate::catalog::{id, TARGET};

    fn open() -> (tempfile::TempDir, Paragraphs) {
        let dir = tempfile::tempdir().unwrap();
        let paragraphs = Paragraphs::open(dir.path());
        (dir, paragraphs)
    }

    /// Whether an edit of this paragraph exists as a file, which is a different
    /// question from what it currently says.
    fn edited(dir: &Path, id: &str) -> bool {
        dir.join(format!("{id}.md")).exists()
    }

    #[test]
    fn an_unedited_paragraph_is_the_text_this_build_ships() {
        let (_dir, paragraphs) = open();
        let prompt = paragraphs.get(id::CAVEMAN_FULL).expect("a declared entry");

        assert_eq!(prompt.origin, Origin::BuiltIn);
        assert!(prompt.text.contains("caveman speech"));
        assert!(prompt.hash.starts_with("sha256:"), "{}", prompt.hash);
        assert!(prompt.base.is_none());
        assert!(prompt.note.is_none());
    }

    #[test]
    fn an_edit_is_what_the_next_read_returns_with_no_invalidation_in_between() {
        // The whole point of holding no state: two independent handles on the
        // same directory, and the second sees what the first wrote.
        let dir = tempfile::tempdir().unwrap();
        let written = Paragraphs::open(dir.path())
            .set(id::CAVEMAN_LITE, "be brief about {{target}}")
            .unwrap();

        let read = Paragraphs::open(dir.path()).get(id::CAVEMAN_LITE).unwrap();

        assert_eq!(read.origin, Origin::Edited);
        assert_eq!(read.text, "be brief about {{target}}");
        assert_eq!(read.hash, written.hash);
    }

    #[test]
    fn editing_changes_the_hash_and_resetting_puts_it_back() {
        let (_dir, paragraphs) = open();
        let before = paragraphs.get(id::CAVEMAN_ULTRA).unwrap();

        let edited = paragraphs
            .set(id::CAVEMAN_ULTRA, "one word about {{target}}")
            .unwrap();
        assert_ne!(edited.hash, before.hash);

        let after = paragraphs.reset(id::CAVEMAN_ULTRA).unwrap();
        assert_eq!(after.hash, before.hash);
        assert_eq!(after.origin, Origin::BuiltIn);
    }

    #[test]
    fn an_edit_back_to_the_default_leaves_no_file_behind() {
        // Otherwise the copy stops tracking the default, and a later build's
        // improved wording never reaches the person who typed it out by hand.
        let (dir, paragraphs) = open();
        let default = paragraphs.get(id::CAVEMAN_LITE).unwrap().text;

        paragraphs.set(id::CAVEMAN_LITE, "something else").unwrap();
        assert!(edited(dir.path(), id::CAVEMAN_LITE));

        let back = paragraphs.set(id::CAVEMAN_LITE, &default).unwrap();
        assert_eq!(back.origin, Origin::BuiltIn);
        assert!(!edited(dir.path(), id::CAVEMAN_LITE));
    }

    #[test]
    fn editing_a_measured_wording_suppresses_the_claim_rather_than_being_refused() {
        // The brief is unambiguous: "All prompts should be editable." What the
        // entry carries instead of a refusal is its dependants.
        let (_dir, paragraphs) = open();

        let shipped = paragraphs.get(id::LESSONS_CLASSIFY).unwrap();
        assert!(
            !shipped.paragraph.dependants.is_empty(),
            "the classifier is measured, and says so"
        );
        assert!(shipped.suppressed.is_empty());

        let edited = paragraphs
            .set(id::LESSONS_CLASSIFY, "guess the class")
            .expect("an edit is never refused for what depends on it");

        assert_eq!(edited.origin, Origin::Edited);
        assert_eq!(edited.suppressed.len(), 1);
        assert!(matches!(
            edited.suppressed[0].kind,
            Dependency::Measured { .. }
        ));

        // And resetting brings the claim back.
        let reset = paragraphs.reset(id::LESSONS_CLASSIFY).unwrap();
        assert!(reset.suppressed.is_empty());
    }

    #[test]
    fn an_edit_records_the_wording_it_was_made_from() {
        let (_dir, paragraphs) = open();
        let shipped = paragraphs.get(id::CAVEMAN_FULL).unwrap();

        let edited = paragraphs.set(id::CAVEMAN_FULL, "grunt").unwrap();

        assert_eq!(edited.base.as_deref(), Some(shipped.hash.as_str()));
        assert!(edited.note.is_none(), "the default has not moved");
    }

    #[test]
    fn an_edit_made_from_a_wording_this_build_no_longer_ships_says_so() {
        // The user is told, and nothing is blocked: the reply they get is still
        // the wording they chose.
        let (dir, paragraphs) = open();
        paragraphs.set(id::CAVEMAN_FULL, "grunt").unwrap();

        // Stand in for a later build whose default has changed.
        std::fs::write(
            dir.path().join(format!("{}.base", id::CAVEMAN_FULL)),
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();

        let prompt = paragraphs.get(id::CAVEMAN_FULL).unwrap();
        assert_eq!(prompt.text, "grunt");
        assert!(prompt.note.is_some(), "the user is told, not just the log");
    }

    #[test]
    fn an_edit_naming_a_placeholder_nothing_fills_is_refused() {
        let (dir, paragraphs) = open();
        let refused = paragraphs
            .set(id::CAVEMAN_FULL, "about {{whoever}}")
            .expect_err("nothing fills that");

        assert!(matches!(refused, Error::Undeclared { .. }), "{refused}");
        assert!(
            !edited(dir.path(), id::CAVEMAN_FULL),
            "a refusal must write nothing"
        );
    }

    #[test]
    fn an_edit_may_drop_a_placeholder_it_was_offered() {
        let (_dir, paragraphs) = open();
        let edited = paragraphs.set(id::CAVEMAN_FULL, "grunt only").unwrap();
        assert_eq!(edited.fill(&[(TARGET, "your reply")]), "grunt only");
    }

    #[test]
    fn an_unreadable_edit_still_answers_and_says_so() {
        let (dir, paragraphs) = open();
        // A directory where a file is expected: unreadable in a way that is the
        // same on every platform.
        std::fs::create_dir_all(dir.path().join(format!("{}.md", id::CAVEMAN_LITE))).unwrap();

        let prompt = paragraphs.get(id::CAVEMAN_LITE).unwrap();
        assert_eq!(prompt.origin, Origin::BuiltIn);
        assert!(prompt.note.is_some(), "the user is told, not just the log");
    }

    #[test]
    fn a_paragraph_nobody_declared_has_no_path_and_no_text() {
        let (_dir, paragraphs) = open();
        assert!(paragraphs.get("../../secrets").is_none());
        assert!(matches!(
            paragraphs.set("../../secrets", "hello"),
            Err(Error::Unknown(_))
        ));
    }

    #[test]
    fn line_endings_do_not_change_a_prompts_identity() {
        let (_dir, paragraphs) = open();
        let unix = paragraphs.set(id::CAVEMAN_LITE, "a\nb").unwrap();
        let windows = paragraphs.set(id::CAVEMAN_LITE, "a\r\nb").unwrap();
        assert_eq!(unix.hash, windows.hash);
    }

    #[test]
    fn every_shipped_paragraph_has_a_distinct_version() {
        let (_dir, paragraphs) = open();
        let mut seen = std::collections::BTreeSet::new();
        for prompt in paragraphs.all() {
            assert!(
                seen.insert(prompt.hash.clone()),
                "{} shares its text with another entry",
                prompt.paragraph.id
            );
        }
    }
}
