//! Shared types and the error shape.
//!
//! Nothing in here knows about Tauri, about the frontend, or about any other
//! crate in the workspace. It is the bottom of the dependency graph on purpose:
//! every other crate may depend on it, and it depends on none of them.
//!
//! The composition root is **not** here, and this crate's own rule is why: it
//! names one implementation per trait, so it depends on every crate that has
//! one, which is upwards. It lives in the application package, which is the
//! ceiling the tiles sit in.
//!
//! See `AGENTS.md` beside this file for what belongs here and what does not.

pub mod error;

pub use error::{Error, Result};

/// The version this build carries, read from the crate manifest.
///
/// `docs/rules/versioning.md` requires the tag, `tauri.conf.json` and the Cargo
/// manifests to agree, and `scripts/check-release.mjs` checks it at the tag.
/// Reading it here rather than writing it out again is what keeps a fourth copy
/// from existing.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
