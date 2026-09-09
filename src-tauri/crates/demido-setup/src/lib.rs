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
//! [`discover`] is the fourth and it is not a half of anything: it reports
//! what is already on the machine so the model folder opens pre-filled, which
//! is section 7's escape and the brief's own mechanism.
//!
//! **Nothing here talks to a server, starts a process or touches the
//! network.** It is the wizard's reasoning, which is the part worth testing;
//! fetching is `demido-runtimes` and the window layer does the doing.

pub mod answers;
pub mod contract;
pub mod discover;
pub mod file;
pub mod plan;
pub mod store;
pub mod target;

pub use answers::{Answers, GENERATION};
pub use discover::Model;
pub use file::Files;
pub use plan::{Plan, Planned, Situation, Standing, Step};
pub use store::Store;
pub use target::{target, Target};
