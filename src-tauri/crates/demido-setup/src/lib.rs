//! The path from a fresh profile to a model that has answered.
//!
//! Three halves that stay apart on purpose, and the line between them is the
//! line `docs/rules/setup.md` section 1 draws:
//!
//! - [`answers`] is what the person chose, and how it survives a restart.
//! - [`plan`] is what that leaves outstanding. **Derived on every read and
//!   stored nowhere**, so what the desk offers "cannot go stale against what
//!   is actually there".
//! - [`target`] is what the answers together name as the thing to talk to.
//!   Both the wizard's last step and the composer ask that, and asking it in
//!   two places is how they come to disagree.
//!
//! What is already on the machine, and which folders are read for models, is
//! `demido-models`' since #72: the folders are two settings on the ladder and
//! the library is read from them, so the wizard and the settings page draw one
//! answer rather than two.
//!
//! **Nothing here talks to a server, starts a process or touches the
//! network.** It is the wizard's reasoning, which is the part worth testing;
//! fetching is `demido-runtimes` and the window layer does the doing.

pub mod answers;
pub mod contract;
pub mod file;
pub mod plan;
pub mod store;
pub mod target;

pub use answers::{Answers, GENERATION};
pub use file::Files;
pub use plan::{Plan, Planned, Situation, Standing, Step};
pub use store::Store;
pub use target::{target, Target};
