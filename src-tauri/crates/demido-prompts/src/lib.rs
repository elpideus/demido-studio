//! Every string Demido wrote that a model reads.
//!
//! The rule is one sentence, and it is hard rule 10 of `AGENTS.md`: if the
//! model reads it and Demido wrote it, it has an id, a default file on disk, a
//! hash and an [`Origin`]. Never a string literal, because a literal cannot be
//! edited, cannot be named in a log, and cannot be told apart from the wording
//! a measurement was taken against.
//!
//! `docs/rules/prompts.md` splits that text into two **registers**, by who asks
//! for it. This crate ships the first:
//!
//! | Register | Holds | Asked for by |
//! |---|---|---|
//! | Paragraphs | Text composed into a prompt | The composer, by id |
//! | Tools | One document per host tool | The registry, by tool name |
//!
//! [`catalog`] declares what paragraphs exist and what they say when nobody has
//! touched them; [`register`] decides what one says right now. The tool
//! register joins them in S2, when there is a tool call to put one in front of:
//! a prompt nothing sends is worse than no prompt, because the editor offers to
//! change something that cannot matter.
//!
//! Two registers rather than one list because a tool description is not a
//! paragraph the composer picks up, it is a field on a struct the registry
//! builds, and forcing them into one namespace makes `id` mean two things. One
//! crate rather than two because the versioning, the log record, the edit path
//! and the editor are the same in both cases.
//!
//! See `AGENTS.md` beside this file for the invariants and for what adding a
//! paragraph costs.

pub mod catalog;
pub mod register;

pub use catalog::{
    fill, id, paragraph, placeholders_in, Dependant, Dependency, Paragraph, CATALOG,
};
pub use register::{Error, Origin, Paragraphs, Prompt, Result};
