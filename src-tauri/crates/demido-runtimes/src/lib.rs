//! Fetches what `demido-catalog` selects, verifies it, and owns it
//! afterwards.
//!
//! `docs/rules/runtimes.md`, scoped to the required group: `llama.cpp`, its
//! `cudart` companion, and one model (`docs/rules/setup.md` section 4). The
//! capability group (uv, Python, SearXNG, Node, `agent-browser`, Chrome) is
//! out of S1 and arrives as more rows, never a second screen and never a
//! second code path: [`state::RuntimeId`] is a string and
//! [`verify::REQUIRED`] is a table, so adding one is adding data.
//!
//! The parts, in the order a fetch moves through them:
//!
//! - [`fetch`] gets bytes onto disk, resuming a partial file and reporting
//!   progress. It has no seam for HTTP on purpose: a failure partway through
//!   a 373 MiB archive on a real disk is the only interesting behaviour it
//!   has, and a mock would test the mock.
//! - [`unpack`] expands an archive. A build and its `cudart` companion merge
//!   into one directory, because that is how `llama-server` resolves its DLL.
//! - [`verify`] runs the declared command that exercises the runtime. A
//!   version flag is refused as verification; this one starts the server,
//!   loads a model and generates.
//! - [`manage`]'s [`Runtimes`] is the state machine, and the only module that
//!   deletes anything. It decides what becomes managed, what is refused, when
//!   a predecessor goes, and what may never be touched.
//! - [`state`], [`store`], [`contract`] and [`file`] are the ledger: three
//!   states, a seam for where it lives, that seam's contract suite, and the
//!   per profile `runtimes.json` that keeps it.
//!
//! **Nothing here asks upstream what exists.** Every URL fetched comes from a
//! pin that shipped inside the build, or from a path the user pointed at. No
//! release feed is read on launch, on a schedule, or behind a button.

pub mod contract;
pub mod fetch;
pub mod file;
pub mod manage;
pub mod state;
pub mod store;
pub mod unpack;
pub mod verify;

pub use fetch::{Cancel, FetchError, Fetchable, Progress};
pub use file::Files;
pub use manage::{Error, Outcome, Runtimes, UnusedEntry, VerifyFn};
pub use state::{directory_name, Ledger, Row, RowState, RuntimeId};
pub use store::Store;
pub use verify::{Definition, Installed, Verification, VerifyError, LLAMA_CPP, REQUIRED};
