//! The remembered shell layout: what the desk looked like last time.
//!
//! The desk is chat as the surface the application is, with an icons-only rail
//! down one edge (`design/shell.md`). How that desk is arranged is a fact about
//! a person on a machine rather than about a preference, so it is kept by Rust,
//! in a file of its own, and it is deliberately **not** a setting.
//!
//! Brief B42: "a VSCode-like Icons-only sidebar"
//!
//! ## Three decisions, and the reason each is here rather than in the window
//!
//! **The layout is Rust's, not the webview's.** v2 kept it in `localStorage`,
//! which multi-account makes wrong: `docs/rules/profiles.md` rules that a
//! Demido profile is a Windows profile, and the webview's storage is scoped to
//! an origin rather than to a person. One profile, one `shell.json`, is the
//! same boundary the vault and the chats already sit inside, drawn by the
//! operating system rather than by a browser.
//!
//! **The layout is not a setting.** Settings are a ladder the user reads and
//! edits (`#8`, `#32`); a layout is the residue of gestures nobody typed. Put
//! it in the settings schema and every drag of a seam becomes a settings
//! change, with a settings history, a settings default and a settings page row
//! for something that has a window to be seen in. See
//! `docs/decisions/0010-the-desk-remembers-itself.md`.
//!
//! **A layout that will not load is discarded, silently.** It is the one piece
//! of state whose failure has no correct report: nobody can act on "your window
//! arrangement was corrupt", the arrangement costs one gesture to remake, and
//! startup never blocks (`AGENTS.md`). [`Store::read`] returns an `Option`
//! rather than a `Result` so that no caller can be written that does otherwise.
//!
//! ## What is here
//!
//! | Module | What |
//! |---|---|
//! | [`layout`] | [`Shell`], and the generation number a file is stamped with. |
//! | [`store`] | The seam: read the arrangement, replace the arrangement. |
//! | [`contract`] | What any store must do. |
//! | [`file`] | `shell.json`, one per profile. |
//! | [`memory`] | The same, without a disk. |
//! | [`debounce`] | Written once the gesture settles, and on the way out. |
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod contract;
pub mod debounce;
pub mod file;
pub mod layout;
pub mod memory;
pub mod store;

pub use debounce::{Debounced, SETTLES_AFTER};
pub use file::Files;
pub use layout::{Shell, Side, Written, GENERATION};
pub use memory::Memory;
pub use store::{Error, Result, Store};
