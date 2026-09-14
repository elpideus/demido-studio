//! The file store against the contract suite.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): "Each
//! implementation's test file calls that function with itself. An
//! implementation that does not call it is not an implementation."

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use demido_runtimes::{contract, Files};

/// A directory this run owns, emptied the **first** time a name is asked for
/// and never again: the suite asks for the same name twice to mean "opened
/// again after a restart", and a scratch that cleared itself every call would
/// answer that with an empty profile every time.
fn scratch(case: &str) -> PathBuf {
    static FRESH: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

    let dir = std::env::temp_dir()
        .join("demido-runtimes-contract")
        .join(format!("{case}-{}", std::process::id()));

    let mut fresh = FRESH.lock().expect("the map");
    let seen = fresh.get_or_insert_with(HashMap::new);
    if seen.insert(case.to_owned(), dir.clone()).is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    dir
}

#[test]
fn files_keeps_the_contract() {
    contract::assert_store(|name| Files::in_profile(scratch(name)));
}
