//! Demido Studio's core.
//!
//! Everything that is not a window lives here: storage, settings, the log of what happened, reading the
//! computer's hardware, and the API that every client (the window, a browser, the command line) talks to.
//!
//! The parts, each in its own file:
//!
//! - [`paths`]     where Demido keeps its files
//! - [`db`]        the one SQLite file, and its migrations
//! - [`events`]    the append-only log of what happened, and its live stream
//! - [`settings`]  every setting there is: what it means, its default, its value
//! - [`system`]    what this computer has, and which model runtime suits it
//! - [`desktop`]   what the desktop looks like: its accent colour
//! - [`runtime`]   the program that runs models: finding it, downloading it, keeping it
//! - [`hub`]       finding models on Hugging Face, and downloading them
//! - [`engine`]    running models: starting the runtime, loading and unloading, unloading what is idle
//! - [`sessions`]  sessions and what was said in them
//! - [`chat`]      running a turn: asking the model, streaming its answer, storing it
//! - [`auth`]      the key that every request needs, even from this computer
//! - [`api`]       the HTTP API, and the server that offers it
//! - [`ui`]        serving the web UI from the same address
//! - [`web`]       reading from the internet: one page, or a search, and what may leave the computer
//! - [`kept`]      keeping an answer for a short while, so a burst of the same question costs one answer
//! - [`vault`]     where keys are kept: the system's keyring, or a file only the user can read
//! - [`providers`] models that run elsewhere (Anthropic, Gemini, OpenRouter), and what they cost
//! - [`programs`]  Claude Code and OpenCode: programs on this computer with loops of their own

pub mod api;
pub mod auth;
pub mod chat;
pub mod checkpoint;
pub mod db;
pub mod desktop;
pub mod engine;
pub mod error;
pub mod events;
pub mod hub;
pub mod kept;
pub mod paths;
pub mod permissions;
pub mod programs;
pub mod providers;
pub mod runtime;
pub mod sandbox;
pub mod sessions;
pub mod settings;
pub mod system;
pub mod tools;
pub mod ui;
pub mod vault;
pub mod web;

pub use error::{Error, Result};

/// The version of this build, as shown everywhere a version is shown.
///
/// A release is stamped with the version of its tag (the release workflow sets `DEMIDO_VERSION` while
/// compiling). Any other build says what `Cargo.toml` says.
pub const VERSION: &str = match option_env!("DEMIDO_VERSION") {
    Some(stamped) => stamped,
    None => env!("CARGO_PKG_VERSION"),
};

/// Milliseconds since 1970, the one clock the core writes down.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
