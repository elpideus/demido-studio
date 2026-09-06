//! Shared types, the error shape, and the composition root.
//!
//! Nothing in here knows about Tauri, about the frontend, or about any other
//! crate in the workspace. It is the bottom of the dependency graph on purpose:
//! every other crate may depend on it, and it depends on none of them.
//!
//! See `AGENTS.md` beside this file for what belongs here and what does not.

pub mod error;
pub mod wiring;

pub use error::{Error, Result};
pub use wiring::Wiring;

/// The version this build carries, read from the crate manifest.
///
/// `docs/rules/versioning.md` requires the tag, `tauri.conf.json` and the Cargo
/// manifests to agree, and `scripts/check-release.mjs` checks it at the tag.
/// Reading it here rather than writing it out again is what keeps a fourth copy
/// from existing.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
