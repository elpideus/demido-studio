//! The update signature check lives in `demido_core::signature`, shared with setup, which checks
//! the uninstaller it copies when it runs elevated.

pub use demido_core::signature::{PUBLIC_KEY, verify_file};
