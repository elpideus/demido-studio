//! The settings ladder: what is in force, and which tier said so.
//!
//! Brief B09: "both at a model/global level as well as at a per-chat level"
//!
//! Four tiers, **global, model, character, chat**, resolved in that order with
//! the chat as the last word
//! ([`docs/decisions/0007-a-chat-outranks-its-character.md`](../../../../docs/decisions/0007-a-chat-outranks-its-character.md)).
//! Two of them are live in v0.1 and all four are stored, because the shape is
//! what a later tier costs: a ladder that grew one would be a migration of
//! every profile, and the character system is what that would foreclose
//! ([#32](https://github.com/elpideus/demido-studio/issues/32)).
//!
//! ## The system prompt is a ladder value
//!
//! Not a field on a chat, not a constant, not a file beside the session log.
//! That is the one thing S1 could have done that no later slice could undo: a
//! character is a persona, a persona is a system prompt plus a set of tools,
//! and a system prompt stored anywhere but here is a system prompt the
//! character tier cannot override.
//!
//! ## What is here
//!
//! | Module | What |
//! |---|---|
//! | [`schema`] | What a setting is. Three of them, declared once. |
//! | [`stack`] | The tiers, and the one function that collapses them. |
//! | [`document`] | The stored shape: four tiers, and who each one belongs to. |
//! | [`store`] | The seam: read the document, replace the document. |
//! | [`contract`] | What any store must do. |
//! | [`file`] | `settings.json`, one per profile. |
//! | [`memory`] | The same, without a disk. |
//! | [`settings`] | The ladder held over a store: resolve, view, set, clear. |
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod contract;
pub mod document;
pub mod file;
pub mod memory;
pub mod schema;
pub mod settings;
pub mod stack;
pub mod store;

pub use document::{Document, Ladder, Scope, GENERATION};
pub use file::Files;
pub use memory::Memory;
pub use schema::{id, setting, Invalid, Kind, Setting, SCHEMA};
pub use settings::{Error, Result, Settings};
pub use stack::{Origin, Resolved, Row, Stack, Tier, Values};
pub use store::Store;
