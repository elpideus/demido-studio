//! The pinned manifest, and which archive a machine's accelerator asks for.
//!
//! Demido bundles no inference backend (`AGENTS.md` rule 3), so this is the
//! crate that says what has to arrive, what it costs, and which of the pins a
//! given machine should be given. It fetches nothing and unpacks nothing:
//! getting the bytes onto the disk and owning them afterwards is
//! `demido-runtimes` ([#46](https://github.com/elpideus/demido-studio/issues/46))
//! and `docs/rules/runtimes.md`.
//!
//! Three parts, in the order a person meets them:
//!
//! - [`MANIFEST`] is the pins and their measured sizes. Data, checked against
//!   the measurements in `docs/rules/setup.md` section 4 by a table-driven
//!   test.
//! - [`Selector`] is the accelerator row the set-up wizard draws: pre-selected
//!   from detection with the reason attached, still overridable, and honest
//!   about the accelerators no build is fetched for.
//! - [`select`] is the pure choice underneath it, which is where
//!   [#19](https://github.com/elpideus/demido-studio/issues/19)'s defect is
//!   fixed rather than inherited.
//!
//! **This crate reads the machine and never the network.** Pins ship inside the
//! build, so nothing here asks upstream what exists.

mod manifest;
mod select;
mod selector;

pub use manifest::{Arch, Archive, Kind, License, Os, MANIFEST, RELEASE};
pub use select::{select, NoBuild, Selection, Target};
pub use selector::{Availability, Row, Selector, ACCELERATORS};
