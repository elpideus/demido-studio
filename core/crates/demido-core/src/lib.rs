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
//! - [`auth`]      the key that every request needs, even from this computer
//! - [`api`]       the HTTP API, and the server that offers it
//! - [`ui`]        serving the web UI from the same address

pub mod api;
pub mod auth;
pub mod db;
pub mod desktop;
pub mod error;
pub mod events;
pub mod paths;
pub mod runtime;
pub mod settings;
pub mod system;
pub mod ui;

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
