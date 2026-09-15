//! What a model can do, and where it may do it.
//!
//! Two things live here and they are kept apart on purpose.
//!
//! **A tool** is a name, a parameter shape, a declaration of what one call is
//! about to do, and the doing of it. That is the whole surface
//! ([`tool::Tool`]). A tool decides nothing about permission and nothing about
//! rendering, so it is written correctly once rather than once per tool: the
//! capability matrix reads [`tool::Intent`] and decides, the transcript reads
//! the same declarations and draws, and neither of them asks the tool.
//!
//! **A workspace** is where a tool may act ([`workspace::Workspace`]). Every
//! path a model produces goes through [`workspace::Workspace::resolve`] before
//! any tool sees it, and what comes back is a [`workspace::Resolved`], which is
//! the only kind of path the tools here open. That is what makes the Read row
//! of the matrix honest: *outside the project* is not a state that can be
//! approved or refused, because it never reaches the point of asking.
//!
//! Three groups live here. Files is five tools that never leave the workspace;
//! Shell is [`command::RunCommand`], which starts in the workspace and whose
//! process tree dies with the call ([`tree`]); Delegation is
//! [`delegate::DelegateTask`], which declares the shell and hands its task to a
//! callback, because what a sub-agent is made of is a session, a backend and a
//! ladder, and a tool may know about none of them.
//!
//! The registry ([`registry::Registry`]) is what turns a call into an outcome,
//! and it stops one step short of running: [`registry::Registry::plan`] hands
//! back an understood call and its intent, which is where the matrix
//! ([#53](https://github.com/elpideus/demido-studio/issues/53)) and the
//! approval ([#55](https://github.com/elpideus/demido-studio/issues/55)) go.
//!
//! What is deliberately not here: any notion of a mode, any notion of a
//! session, and every tool's model-facing prose. The first two would make a
//! tool something written once per tool; the third is host prompt text and
//! belongs in the tool register
//! ([`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md)).
//!
//! See `AGENTS.md` beside this file, which also records what carried over from
//! v2's `demido-tools` and what was rewritten.

pub mod arguments;
pub mod command;
pub mod contract;
pub mod delegate;
pub mod files;
pub mod listing;
pub mod registry;
pub mod search;
pub mod tool;
pub mod tree;
pub mod workspace;

pub use command::RunCommand;
pub use delegate::{delegating, DelegateTask, Delegating};
pub use files::{DeleteFile, ReadFile, WriteFile};
pub use listing::ListDirectory;
pub use registry::{delegation, files, shell, Call, Planned, Registry, Spec};
pub use search::SearchFiles;
pub use tool::{Ability, Context, Failure, Intent, Outcome, Tool};
pub use workspace::{Resolved, Workspace};
