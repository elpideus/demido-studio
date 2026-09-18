//! The tool register: one document per host tool.
//!
//! `docs/rules/prompts.md` and
//! [`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md):
//! a tool's description and the prose inside its parameter schema are host
//! prompt text, sent on every turn, and v2's own trait doc called a
//! description *"the single highest-leverage string in the crate"*. Here they
//! get what every paragraph already had: an id, a default file, a hash and an
//! [`Origin`].
//!
//! **A tool is one document.** Its description and its parameter prose live in
//! one file, are edited as one text and are hashed together, so one hash covers
//! everything about that tool the model reads. The file is the description,
//! then one `## <parameter>` section per property:
//!
//! ```text
//! Read a text file from the workspace. ...
//!
//! ## path
//!
//! Path relative to the workspace root, such as src/main.rs
//! ```
//!
//! **The schema's shape is not here and is not editable.** Property names,
//! types, which are required: those are a contract with the parser and live
//! with the tool in `demido-tools`, which merges this prose onto them when it
//! offers a tool. What an entry declares is the list of parameter names it
//! gives prose to, and `demido-tools`' `tests/documents.rs` binds that list to
//! the real schema in both directions.
//!
//! Keyed by tool name rather than by paragraph id, and stored under `tools/` in
//! the prompts directory, because a tool name and a paragraph id are two
//! namespaces and forcing them into one makes `id` mean two things.

use std::path::PathBuf;

use serde::Serialize;

use crate::catalog::{placeholders_in, Dependant, Dependency};
use crate::register::{self, Error, Origin, Result, Stored};

/// One host tool's document: what it is called, what it gives prose to, what
/// it costs to change, and what it says when nobody has edited it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ToolEntry {
    /// The tool's name, exactly as `Tool::name` returns it. The file name on
    /// disk and the name `tools/offered` records, so renaming one is a
    /// migration.
    pub name: &'static str,
    /// The editor's heading for this document. A label, never payload.
    pub title: &'static str,
    /// One sentence about what the tool does for the person, shown above the
    /// field. A label, never payload: what the model reads is `default`.
    pub summary: &'static str,
    /// Every parameter the tool's schema declares, in schema order.
    ///
    /// Declared rather than discovered so that an edit giving prose to a
    /// property that does not exist is refused, and so that `demido-tools` can
    /// hold this list to the schema it actually parses against.
    pub parameters: &'static [&'static str],
    /// What this wording is load-bearing for, rendered above the field. Never
    /// empty: every document is at least what a model picks its tool on.
    pub dependants: &'static [Dependant],
    /// The document this build ships. Never mutated; an edit is a file on disk.
    pub default: &'static str,
}

/// What every tool document is load-bearing for, whoever the tool is.
///
/// A tool's document is not composed into anything a person reads first: it
/// is the words a model picks the tool on, sent beside every other tool's on
/// every turn the tool is switched on, and to every sub-agent it is passed
/// down to. So one edit changes what every one of those contexts is told, and
/// the person making it should hear that before they make it.
// not-a-prompt: the sentence the editor renders above the field, never sent.
const OFFERED: Dependant = Dependant {
    note: "Every conversation and every sub-agent this tool is switched on for is sent this wording, and a small model picks a tool almost entirely on it.",
    kind: Dependency::Shared,
};

/// The planted file in S2's live suite names a file and never a tool, and the
/// model picks this one out of six on its words
/// ([#59](https://github.com/elpideus/demido-studio/issues/59)).
// not-a-prompt: the sentence the editor renders above the field, never sent.
const CHOSEN_LIVE: Dependant = Dependant {
    note: "The live tools suite watched a model choose this tool out of six, with nothing naming it, against this wording.",
    kind: Dependency::Driven {
        suite: "src-tauri/crates/demido-chat/tests/a_real_model_with_tools.rs",
    },
};

/// S4's election: whether a model delegates with nothing naming delegation,
/// measured on all three tiers against this wording
/// ([#69](https://github.com/elpideus/demido-studio/issues/69)). No tier did,
/// and a rewording is exactly what that result is waiting on, which is why the
/// person rewriting it should know the number was taken against this one.
// not-a-prompt: the sentence the editor renders above the field, never sent.
const ELECTED_LIVE: Dependant = Dependant {
    note: "The live delegation suite measured whether a model delegates unprompted against this wording.",
    kind: Dependency::Driven {
        suite: "src-tauri/crates/demido-chat/tests/a_real_model_delegating.rs",
    },
};

const OFFERED_ONLY: &[Dependant] = &[OFFERED];

/// Every host tool there is a document for.
// not-a-prompt: what a model reads is the `include_str!` default beside each
// entry. The titles and summaries here are the editor's labels, and
// `check-rules.mjs` refuses a `default` that is anything but a file.
pub static TOOLS: &[ToolEntry] = &[
    ToolEntry {
        name: "read_file",
        title: "Read a file",
        summary: "How the model is told it can open a file in the workspace, and a range of its lines.",
        parameters: &["path", "from_line", "lines"],
        dependants: &[OFFERED, CHOSEN_LIVE],
        default: include_str!("../defaults/tools/read_file.md"),
    },
    ToolEntry {
        name: "list_directory",
        title: "List a directory",
        summary: "How the model is told it can see what a folder holds, a part at a time.",
        parameters: &["path", "from"],
        dependants: OFFERED_ONLY,
        default: include_str!("../defaults/tools/list_directory.md"),
    },
    ToolEntry {
        name: "search_files",
        title: "Search files",
        summary: "How the model is told it can find a phrase across the workspace before reading anything.",
        parameters: &["text", "path", "from"],
        dependants: OFFERED_ONLY,
        default: include_str!("../defaults/tools/search_files.md"),
    },
    ToolEntry {
        name: "write_file",
        title: "Write a file",
        summary: "How the model is told it can replace a file's contents, and that it replaces all of them.",
        parameters: &["path", "content"],
        dependants: OFFERED_ONLY,
        default: include_str!("../defaults/tools/write_file.md"),
    },
    ToolEntry {
        name: "delete_file",
        title: "Delete a file",
        summary: "How the model is told it can remove one file, and that nothing brings it back.",
        parameters: &["path"],
        dependants: OFFERED_ONLY,
        default: include_str!("../defaults/tools/delete_file.md"),
    },
    ToolEntry {
        name: "run_command",
        title: "Run a command",
        summary: "How the model is told it can run a shell command, where it starts and when it gives up.",
        parameters: &["command", "cwd", "timeout_seconds"],
        dependants: OFFERED_ONLY,
        default: include_str!("../defaults/tools/run_command.md"),
    },
    // An entry from the first commit that ships the tool
    // ([#61](https://github.com/elpideus/demido-studio/issues/61)), and
    // deliberately so: of everything Demido says to a model this is the wording
    // most likely to be rewritten, because whether a small model elects to
    // delegate at all is the open question of S4. The one that will need
    // rewording is never the one to retrofit.
    ToolEntry {
        name: "delegate_task",
        title: "Delegate a task",
        summary: "How the model is told it can hand work to a sub-agent with a clean context, and when that is worth it.",
        parameters: &["task"],
        dependants: &[OFFERED, ELECTED_LIVE],
        default: include_str!("../defaults/tools/delegate_task.md"),
    },
];

/// The declaration for one tool, if there is one.
///
/// Also what keeps a name arriving from the window layer from naming a file
/// elsewhere: a name with no declaration has no path.
pub fn tool(name: &str) -> Option<&'static ToolEntry> {
    TOOLS.iter().find(|entry| entry.name == name)
}

/// A document read as its parts: the description, and the prose each parameter
/// is given, in the order the document gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sections<'a> {
    pub description: &'a str,
    pub parameters: Vec<(&'a str, &'a str)>,
}

/// Split a document at its `## <parameter>` headings.
///
/// A heading is a line that is `## ` and one word of letters, digits and
/// underscores, which is every property name a schema here declares. Anything
/// else is prose, including a markdown heading with a space in it. Each part
/// is trimmed, so the blank lines that make the file readable reach nobody.
pub fn sections(text: &str) -> Sections<'_> {
    let mut headings: Vec<(usize, usize, &str)> = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if let Some(name) = heading(line) {
            headings.push((at, at + line.len(), name));
        }
        at += line.len();
    }

    let description = match headings.first() {
        Some((start, _, _)) => &text[..*start],
        None => text,
    };

    let parameters = headings
        .iter()
        .enumerate()
        .map(|(index, (_, body, name))| {
            let end = headings
                .get(index + 1)
                .map_or(text.len(), |(next, _, _)| *next);
            (*name, text[*body..end].trim())
        })
        .collect();

    Sections {
        description: description.trim(),
        parameters,
    }
}

/// A schema's shape with a document's parameter prose on it.
///
/// Prose is added to a property the shape already declares and to nothing
/// else. A document that gives prose to a property the tool does not take adds
/// no property, whoever wrote the file, because the shape is a contract with
/// the parser and the document is not a party to it.
///
/// Here rather than beside the tools because two crates have to merge it the
/// same way: the registry when it offers a tool, and the session log when it
/// rebuilds what was offered out of the shape and the wording it recorded. Two
/// copies of this function would be two answers to what a model was shown.
pub fn describe(mut shape: serde_json::Value, text: &str) -> serde_json::Value {
    let prose = sections(text).parameters;
    let Some(properties) = shape
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return shape;
    };
    for (name, declared) in properties.iter_mut() {
        let (Some((_, prose)), Some(declared)) = (
            prose.iter().find(|(given, _)| given == name),
            declared.as_object_mut(),
        ) else {
            continue;
        };
        declared.insert(
            "description".to_owned(),
            serde_json::Value::String((*prose).to_owned()),
        );
    }
    shape
}

/// The parameter a line opens a section for, if it is a heading.
fn heading(line: &str) -> Option<&str> {
    let name = line.trim_end().strip_prefix("## ")?;
    let word = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    word.then_some(name)
}

/// One tool's document as it stands right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Document {
    pub tool: &'static ToolEntry,
    /// The whole document, line endings normalised.
    pub text: String,
    pub origin: Origin,
    /// `sha256:` and the digest of the whole document, description and
    /// parameter prose together. What `tools/offered` records beside the name.
    pub hash: String,
    /// The same, for the document this build ships. See [`crate::Prompt`]: it
    /// is half of the comparison `note` is written from, and it is on both
    /// registers because the two never disagree about what an entry carries.
    pub shipped: String,
    /// For an edit, the hash of the built-in document it was made from.
    pub base: Option<String>,
    /// The measured claims this text no longer supports.
    pub suppressed: Vec<&'static Dependant>,
    /// Anything the user should be told about where this came from. A note,
    /// never a refusal.
    pub note: Option<String>,
}

impl Document {
    /// What the model is told the tool is for.
    pub fn description(&self) -> &str {
        sections(&self.text).description
    }

    /// The prose one parameter is given, if the document gives it any.
    ///
    /// `None` for a section an edit dropped: a property with no prose is still
    /// a property, and the shape it belongs to is not this document's.
    pub fn parameter(&self, name: &str) -> Option<&str> {
        sections(&self.text)
            .parameters
            .into_iter()
            .find(|(given, _)| *given == name)
            .map(|(_, prose)| prose)
    }

    /// `shape` with this document's parameter prose on it. See [`describe`].
    pub fn describe(&self, shape: serde_json::Value) -> serde_json::Value {
        describe(shape, &self.text)
    }
}

/// The tool documents in force, and the four things anyone does to them.
///
/// Opened over the same prompts directory as [`crate::Paragraphs`], and like it
/// remembers nothing: every call reads the directory again.
#[derive(Debug, Clone)]
pub struct Tools {
    dir: PathBuf,
}

impl Tools {
    /// Open the prompts directory. Edits live under `tools/` inside it.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into().join("tools"),
        }
    }

    /// One tool's document, as it stands.
    pub fn get(&self, name: &str) -> Option<Document> {
        tool(name).map(|entry| self.read(entry))
    }

    /// Every document, in register order. What the editor lists.
    pub fn all(&self) -> Vec<Document> {
        TOOLS.iter().map(|entry| self.read(entry)).collect()
    }

    /// Replace one tool's document, and hand it back.
    ///
    /// Two things are refused and neither is a judgement about the wording: a
    /// section for a parameter the tool does not take, and a placeholder, which
    /// no tool document declares and nothing would fill. A section may be
    /// dropped, and a measured wording may be edited: the claim is suppressed.
    pub fn set(&self, name: &str, text: &str) -> Result<Document> {
        let entry = tool(name).ok_or_else(|| Error::Unknown(name.to_owned()))?;
        let text = register::normalise(text);

        if let Some(placeholder) = placeholders_in(&text).into_iter().next() {
            return Err(Error::Undeclared {
                id: name.to_owned(),
                name: placeholder,
            });
        }
        for (given, _) in sections(&text).parameters {
            if !entry.parameters.contains(&given) {
                return Err(Error::UnknownParameter {
                    tool: name.to_owned(),
                    name: given.to_owned(),
                });
            }
        }

        register::edit(
            &self.path(entry),
            &self.base_path(entry),
            entry.default,
            &text,
        )?;
        Ok(self.read(entry))
    }

    /// Forget the edit. The built-in document is what the next turn offers.
    pub fn reset(&self, name: &str) -> Result<Document> {
        let entry = tool(name).ok_or_else(|| Error::Unknown(name.to_owned()))?;
        register::forget(&self.path(entry), &self.base_path(entry))?;
        Ok(self.read(entry))
    }

    fn path(&self, entry: &ToolEntry) -> PathBuf {
        self.dir.join(format!("{}.md", entry.name))
    }

    fn base_path(&self, entry: &ToolEntry) -> PathBuf {
        self.dir.join(format!("{}.base", entry.name))
    }

    fn read(&self, entry: &'static ToolEntry) -> Document {
        let Stored {
            text,
            origin,
            hash,
            shipped,
            base,
            note,
        } = register::stored(&self.path(entry), &self.base_path(entry), entry.default);

        Document {
            tool: entry,
            suppressed: register::suppressed(origin, entry.dependants),
            text,
            origin,
            hash,
            shipped,
            base,
            note,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::catalog::Dependency;
    use crate::Paragraphs;

    fn open() -> (tempfile::TempDir, Tools) {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::open(dir.path());
        (dir, tools)
    }

    #[test]
    fn every_name_is_unique_and_carries_its_own_prose() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in TOOLS {
            assert!(seen.insert(entry.name), "duplicate tool {}", entry.name);
            assert!(
                !sections(entry.default).description.is_empty(),
                "{} ships no description",
                entry.name
            );
        }
    }

    #[test]
    fn a_default_gives_prose_to_exactly_the_parameters_it_declares() {
        // Both directions, the way a paragraph's placeholders are checked. A
        // section for an undeclared name describes nothing; a declared name
        // with no section ships a property to a 4B model with no words on it.
        for entry in TOOLS {
            let given: Vec<&str> = sections(entry.default)
                .parameters
                .iter()
                .map(|(name, prose)| {
                    assert!(
                        !prose.is_empty(),
                        "{}.{name} has an empty section",
                        entry.name
                    );
                    *name
                })
                .collect();
            assert_eq!(
                given, entry.parameters,
                "{}'s default and its declaration disagree about its parameters",
                entry.name
            );
        }
    }

    #[test]
    fn a_default_takes_no_placeholder() {
        // Nothing here fills one. `read_skill`'s `{{skills}}` is the first
        // document that will, and it arrives with a declaration for it.
        for entry in TOOLS {
            assert!(
                placeholders_in(entry.default).is_empty(),
                "{} ships a placeholder nothing fills",
                entry.name
            );
        }
    }

    #[test]
    fn a_measured_claim_names_a_file_that_holds_the_pin() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for entry in TOOLS {
            for dependant in entry.dependants {
                let Dependency::Measured { pinned_in } = dependant.kind else {
                    continue;
                };
                let pin = std::fs::read_to_string(root.join(pinned_in)).unwrap_or_default();
                assert!(
                    pin.contains(entry.name),
                    "{pinned_in} does not pin {}",
                    entry.name
                );
            }
        }
    }

    #[test]
    fn every_entry_is_labelled_and_says_what_it_is_load_bearing_for() {
        // The editor renders the dependants above the field, so an entry
        // declaring none would open onto a field with nothing said about what
        // an edit costs. Every tool document costs something: a small model
        // picks a tool on these words.
        for entry in TOOLS {
            assert!(!entry.title.is_empty(), "{} has no title", entry.name);
            assert!(!entry.summary.is_empty(), "{} has no summary", entry.name);
            assert!(
                !entry.dependants.is_empty(),
                "{} declares nothing it is load-bearing for",
                entry.name
            );
        }
    }

    #[test]
    fn a_driven_claim_names_a_live_suite_that_offers_the_tool() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        for entry in TOOLS {
            for dependant in entry.dependants {
                let Dependency::Driven { suite } = dependant.kind else {
                    continue;
                };
                let source = std::fs::read_to_string(root.join(suite))
                    .unwrap_or_else(|_| panic!("{} names {suite}, which is not there", entry.name));
                assert!(
                    source.contains(entry.name),
                    "{suite} never names {}",
                    entry.name
                );
            }
        }
    }

    #[test]
    fn an_edit_suppresses_the_live_claim_and_keeps_the_shared_one() {
        // The live suites were driven against the shipped wording. On a
        // machine where somebody rewrote it, that is no longer a claim about
        // their app; that the document is sent wherever the tool is on still
        // is.
        let (_dir, tools) = open();
        let shipped = tools.get("read_file").unwrap();
        assert!(shipped.suppressed.is_empty());

        let edited = tools
            .set("read_file", "Read a file.\n\n## path\n\nWhere.")
            .unwrap();
        assert!(!edited.suppressed.is_empty());
        assert!(edited
            .suppressed
            .iter()
            .all(|dependant| matches!(dependant.kind, Dependency::Driven { .. })));
    }

    #[test]
    fn a_document_splits_into_a_description_and_its_parameters() {
        let split = sections("Read a file.\n\n## path\n\nWhere it is.\n\n## lines\nHow many.\n");
        assert_eq!(split.description, "Read a file.");
        assert_eq!(
            split.parameters,
            vec![("path", "Where it is."), ("lines", "How many.")]
        );
    }

    #[test]
    fn a_heading_with_a_space_in_it_is_prose() {
        let split = sections("Read a file.\n## Two words\nstill prose\n## path\nhere\n");
        assert_eq!(split.description, "Read a file.\n## Two words\nstill prose");
        assert_eq!(split.parameters, vec![("path", "here")]);
    }

    #[test]
    fn an_unedited_document_is_the_text_this_build_ships() {
        let (_dir, tools) = open();
        let document = tools.get("read_file").expect("a declared tool");

        assert_eq!(document.origin, Origin::BuiltIn);
        assert!(document.hash.starts_with("sha256:"), "{}", document.hash);
        assert!(document.description().contains("line numbers"));
        assert!(document
            .parameter("path")
            .unwrap()
            .contains("workspace root"));
        assert!(document.note.is_none());
    }

    #[test]
    fn the_description_and_the_parameter_prose_hash_together() {
        // One hash covers everything about the tool the model reads, so a
        // change to one parameter's prose is a new version of the whole tool.
        let (_dir, tools) = open();
        let shipped = tools.get("read_file").unwrap();

        let reworded = shipped.text.replace(
            "First line to return, counting from 1.",
            "The line to start at.",
        );
        let edited = tools.set("read_file", &reworded).unwrap();

        assert_ne!(edited.hash, shipped.hash);
        assert_eq!(edited.description(), shipped.description());
        assert_eq!(
            edited.parameter("from_line").unwrap(),
            "The line to start at. Omit to start at the top."
        );
    }

    #[test]
    fn an_edit_is_what_the_next_read_returns_and_reset_puts_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let before = Tools::open(dir.path()).get("delete_file").unwrap();

        Tools::open(dir.path())
            .set("delete_file", "Remove a file.\n\n## path\n\nWhich one.")
            .unwrap();
        let read = Tools::open(dir.path()).get("delete_file").unwrap();
        assert_eq!(read.origin, Origin::Edited);
        assert_eq!(read.description(), "Remove a file.");
        assert_eq!(read.base.as_deref(), Some(before.hash.as_str()));

        let reset = Tools::open(dir.path()).reset("delete_file").unwrap();
        assert_eq!(reset.hash, before.hash);
        assert_eq!(reset.origin, Origin::BuiltIn);
    }

    #[test]
    fn prose_for_a_parameter_the_tool_does_not_take_is_refused_and_writes_nothing() {
        // The shape is a contract with the parser. A section cannot add a
        // property, so one naming a property that does not exist describes
        // nothing, and the person who wrote it should hear so.
        let (dir, tools) = open();
        let refused = tools
            .set("read_file", "Read.\n\n## colour\n\nWhich colour.")
            .expect_err("read_file takes no colour");

        assert!(
            matches!(refused, Error::UnknownParameter { .. }),
            "{refused}"
        );
        assert!(!dir.path().join("tools").join("read_file.md").exists());
    }

    #[test]
    fn an_edit_may_drop_a_parameters_prose() {
        let (_dir, tools) = open();
        let edited = tools
            .set("read_file", "Read a file.\n\n## path\n\nWhere.")
            .unwrap();
        assert_eq!(edited.parameter("lines"), None);
    }

    #[test]
    fn a_placeholder_in_a_tool_document_is_refused() {
        let (_dir, tools) = open();
        assert!(matches!(
            tools.set("read_file", "Read {{something}}."),
            Err(Error::Undeclared { .. })
        ));
    }

    #[test]
    fn an_edit_back_to_the_default_leaves_no_file_behind() {
        let (dir, tools) = open();
        let default = tools.get("write_file").unwrap().text;

        tools.set("write_file", "Write.").unwrap();
        let back = tools
            .set("write_file", &default.replace('\n', "\r\n"))
            .unwrap();

        assert_eq!(back.origin, Origin::BuiltIn);
        assert!(!dir.path().join("tools").join("write_file.md").exists());
    }

    #[test]
    fn a_tool_name_and_a_paragraph_id_are_two_namespaces() {
        let (dir, tools) = open();
        assert!(tools.get(crate::id::CAVEMAN_FULL).is_none());
        assert!(matches!(
            Paragraphs::open(dir.path()).set("read_file", "hello"),
            Err(Error::Unknown(_))
        ));
        assert!(tools.get("../../secrets").is_none());
    }

    #[test]
    fn an_unreadable_edit_still_answers_and_says_so() {
        let (dir, tools) = open();
        std::fs::create_dir_all(dir.path().join("tools").join("list_directory.md")).unwrap();

        let document = tools.get("list_directory").unwrap();
        assert_eq!(document.origin, Origin::BuiltIn);
        assert!(
            document.note.is_some(),
            "the user is told, not just the log"
        );
    }

    #[test]
    fn every_shipped_document_has_a_distinct_version() {
        let (_dir, tools) = open();
        let mut seen = std::collections::BTreeSet::new();
        for document in tools.all() {
            assert!(
                seen.insert(document.hash.clone()),
                "{} shares its text",
                document.tool.name
            );
        }
    }
}
