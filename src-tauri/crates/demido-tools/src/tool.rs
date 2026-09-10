//! What a tool is.
//!
//! Deliberately small, and the smallness is the point. A tool says what it is
//! called, what arguments it takes, what a call is about to do, and how to do
//! it. It decides **nothing about permission and nothing about rendering**: not
//! whether it is allowed to run, not whether the person should be asked, not
//! how its result is drawn in the transcript, not what mode anything is in.
//!
//! That is what makes a tool something written correctly once rather than once
//! per tool. The matrix ([#53](https://github.com/elpideus/demido-studio/issues/53))
//! reads [`Intent`] and decides; the transcript
//! ([#55](https://github.com/elpideus/demido-studio/issues/55)) reads the same
//! declarations and draws; neither of them asks the tool, and the tool has no
//! way to ask either of them. This crate depends on no mode type and no
//! frontend type, which is the enforceable half of that sentence.
//!
//! The other half is that a tool cannot be reached with a path nobody checked.
//! A tool is handed arguments and a [`Context`], and the only route from a
//! string in the arguments to something on disk is [`Context::resolve`], which
//! answers with a [`Resolved`] or refuses.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workspace::{Resolved, Workspace};

/// Everything a tool is given about the world it runs in.
///
/// A struct rather than a bare `&Workspace` because this is where the next
/// things a tool needs will arrive (the call it is running for, the approval it
/// was granted), and adding one is a change to one file rather than to every
/// tool. It also keeps the workspace itself out of a tool's hands: what a tool
/// can do with this is resolve a path, confine one it found, and write one back
/// for the model to read.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    workspace: &'a Workspace,
}

impl<'a> Context<'a> {
    /// A world with a workspace in it.
    ///
    /// There is no world without one. Every tool in this slice acts on the
    /// project, so a registry with no workspace offers nothing at all rather
    /// than offering tools that cannot succeed however they are called. The day
    /// a tool arrives that needs no workspace (a web search, say) is the day
    /// that distinction should be built, with the tool that justifies it in
    /// front of whoever builds it.
    pub fn over(workspace: &'a Workspace) -> Self {
        Self { workspace }
    }

    /// Turn a path the model wrote into one this tool may act on.
    ///
    /// The refusal is a [`Failure`] rather than a workspace error because it is
    /// going into the conversation as this call's result. It is retryable: a
    /// model that climbed out of the project by getting a relative path wrong
    /// fixes that by writing a different path, which is exactly the case a
    /// retryable failure is for.
    pub fn resolve(&self, given: &str) -> std::result::Result<Resolved, Failure> {
        self.workspace
            .resolve(given)
            .map_err(|err| Failure::retryable(err.to_string()))
    }

    /// The same, for a path that has to be a directory.
    ///
    /// The two tools that walk rather than open both wanted the same three
    /// steps and the same two sentences: resolve it, refuse a file by naming
    /// the tool that reads one, and answer a name that is not there with what
    /// the parent does hold.
    pub fn resolve_dir(&self, given: &str) -> std::result::Result<Resolved, Failure> {
        let at = self.resolve(given)?;
        if at.path().is_dir() {
            return Ok(at);
        }
        Err(Failure::retryable(match at.path().exists() {
            true => {
                format!("{given} is a file, not a directory. Use read_file to see what is in it.")
            }
            false => self.absent(given),
        }))
    }

    /// Confine a path Demido itself produced, such as one a walk just found.
    ///
    /// See [`Workspace::confine`]. A walk that started inside the project stays
    /// inside it right up until it meets a symlinked directory.
    pub fn confine(&self, path: &std::path::Path) -> std::result::Result<Resolved, Failure> {
        self.workspace
            .confine(path)
            .map_err(|err| Failure::retryable(err.to_string()))
    }

    /// How a path is written back to the model: relative to the workspace,
    /// forward slashes, and never the user's account name.
    pub fn relative(&self, at: &Resolved) -> String {
        self.workspace.relative(at)
    }

    /// What to say about a path in the workspace that is not there.
    ///
    /// It names what the parent directory actually holds, and it lives here
    /// rather than with any one tool because all four that take a path say it,
    /// and because what it reads is the workspace, which is this type's to
    /// know and not theirs.
    ///
    /// v2 learned the wording the hard way: the first version said only that
    /// the file did not exist, and a live run watched a model read that as
    /// settled and stop looking, where the same model recovers from this one by
    /// reading the right file. A sentence that ends an avenue and a sentence
    /// that opens one are different sentences, and there should only be one of
    /// them in this crate.
    pub fn absent(&self, given: &str) -> String {
        let parent = std::path::Path::new(given)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let named = match parent.as_os_str().is_empty() {
            true => ".".to_owned(),
            false => parent.to_string_lossy().into_owned(),
        };

        let siblings = self
            .resolve(&named)
            .ok()
            .and_then(|at| std::fs::read_dir(at.path()).ok())
            .map(|entries| {
                let mut names: Vec<String> = entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .take(40)
                    .collect();
                names.sort();
                names
            })
            .filter(|names| !names.is_empty());

        let where_it_looked = match named.as_str() {
            "." => "the workspace root".to_owned(),
            named => named.to_owned(),
        };

        match siblings {
            Some(names) => format!(
                "{given} does not exist. {where_it_looked} contains: {}.",
                names.join(", ")
            ),
            None => format!("{given} does not exist."),
        }
    }
}

/// What kind of thing a call is. The axis the capability matrix decides on.
///
/// Four, and no more without a very good reason: every one of these is a
/// question a person has to be able to answer about a prompt they are seeing
/// for the first time, at speed, probably annoyed. The definitions are
/// [`docs/rules/tools.md`](../../../../docs/rules/tools.md)'s, and the two that
/// are easy to get wrong are written out there: reading an account's data is
/// `Network` and never `Read`, and a browser is `Network` when it fetches and
/// `Shell` when the model drives a click.
///
/// Two of the four have no tool in this slice. They ship because the matrix is
/// a table over the whole axis and a row nobody can express is a row nobody can
/// test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ability {
    /// Looks at something in the workspace and changes nothing.
    Read,
    /// Changes a file in the workspace.
    Write,
    /// Runs a program somebody else wrote.
    Shell,
    /// Talks to something that is not on this machine, including anything
    /// reaching an account.
    Network,
}

/// What one call is about to do, decided from its arguments before it runs.
///
/// This is how the capability matrix decides without knowing what any tool is.
/// A tool that answered it dishonestly would defeat the whole mechanism, which
/// is why it is a declaration rather than something inferred afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub ability: Ability,
    /// One line, for the person deciding whether to allow it. Written for
    /// somebody who has been asked this eleven times today: name the thing,
    /// not the tool.
    pub summary: String,
    /// True when Demido cannot put it back afterwards. **Always asked about, in
    /// every mode**, including the one that approves everything else, and not
    /// waivable by *always for this tool*.
    ///
    /// This declaration is the only place that fact lives. Nothing keeps a list
    /// of destructive tool names, and nothing infers it from an ability: a tool
    /// that is unsure should say true, because erring towards asking costs a
    /// click and erring the other way costs the thing.
    pub destructive: bool,
    /// Files this call will change, already confined to the workspace.
    ///
    /// Declared so that whatever wants to take a copy before a call can do it
    /// once, for every tool there will ever be, rather than each tool thinking
    /// about undo. A tool that writes a file it did not list here is a file
    /// that cannot be put back.
    pub touches: Vec<Resolved>,
}

impl Intent {
    /// A call that changes nothing. The overwhelmingly common case.
    pub fn reading(summary: impl Into<String>) -> Self {
        Self {
            ability: Ability::Read,
            summary: summary.into(),
            destructive: false,
            touches: Vec::new(),
        }
    }
}

/// What a tool produced, or why it could not.
pub type Outcome = std::result::Result<String, Failure>;

/// A failure written for a model to act on.
///
/// Not a log line and not an error type: the string here goes into the
/// conversation as the tool's result, and it is the model's only chance to
/// understand what to do differently. "No such file or directory (os error 2)"
/// is a fact; "src/mian.rs does not exist. src contains main.rs" is something a
/// model can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// What the model is told.
    pub message: String,
    /// Whether calling again with different arguments could work.
    ///
    /// False means the model should stop trying this tool and say something to
    /// the user instead. A loop that retries an unretryable failure is the
    /// commonest way a small model burns a context window.
    pub retryable: bool,
}

impl Failure {
    /// Something the model could get right on another attempt.
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    /// Something no wording of the arguments will fix.
    pub fn final_(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.message)
    }
}

/// One thing the model can do.
#[async_trait]
pub trait Tool: Send + Sync {
    /// What the model calls it, and what the log records. Stable: renaming one
    /// makes every recorded call in every old session name a tool that is gone.
    ///
    /// Borrowed rather than `&'static str` because the one kind of tool that is
    /// not known at compile time is coming: an MCP tool is named by a server
    /// Demido did not write, and its qualified name is a `String` a struct is
    /// holding.
    fn name(&self) -> &str;

    /// JSON Schema for the arguments, and the shape only.
    ///
    /// The prose inside a schema (`description` on a property, and the tool's
    /// own description) is host-authored prompt text and belongs in the tool
    /// register, per hard rule 10 and
    /// [`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md).
    /// It arrives with that register on
    /// [#52](https://github.com/elpideus/demido-studio/issues/52) and is merged
    /// onto this shape when a payload is assembled. What is here is the half
    /// that is a contract with the parser rather than a prompt: the property
    /// names, their types, which are required, and that the schema closes
    /// itself.
    ///
    /// Every schema here closes itself (`additionalProperties: false`), which
    /// is what lets [`crate::arguments::faults`] refuse a property the tool
    /// never declared instead of passing it on.
    fn parameters(&self) -> Value;

    /// What this call will do, before it does it.
    ///
    /// No default on purpose. A tool that forgot to declare itself would be
    /// approved as a read, and the first tool to get that wrong is the one that
    /// deletes something. [`Intent::reading`] is one line for a tool that only
    /// looks.
    ///
    /// Called with arguments that have already been parsed and checked against
    /// [`Tool::parameters`], so it is reading them rather than defending
    /// against them.
    fn intent(&self, arguments: &Value, context: &Context<'_>) -> Intent;

    /// Do it.
    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome;
}
