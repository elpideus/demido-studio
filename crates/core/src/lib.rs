//! Types shared by the Demido Studio installer and the app.
//!
//! The installer decides *where* things live and *what* was fetched; the app only reads that
//! decision back. This crate is the contract between the two: [`manifest::InstallManifest`] is
//! written once at install time as `install.json` next to the app binary, and [`paths`] knows the
//! well-known folders both sides agree on.

pub mod backend;
pub mod brand;
pub mod fsx;
pub mod manifest;
pub mod paths;
pub mod platform;

pub use backend::Backend;
pub use manifest::{InstallManifest, InstallScope};
pub use platform::{Arch, Os};
