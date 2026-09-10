//! Both stores against the contract suite.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): "Each
//! implementation's test file calls that function with itself. An
//! implementation that does not call it is not an implementation." This is that
//! file.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use demido_settings::{contract, Files, Memory};

/// A directory this run owns, so two cases cannot read each other's file.
///
/// Emptied the **first** time a name is asked for and never again: the suite
/// asks for the same name twice to mean "opened again after a restart", and a
/// scratch that cleared itself on every call would answer that with an empty
/// profile every time.
fn scratch(case: &str) -> PathBuf {
    static FRESH: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

    let dir = std::env::temp_dir()
        .join("demido-settings-contract")
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
    // A fresh directory per name, and the same directory for the same name,
    // which is how the suite asks for a store to be opened again.
    contract::assert_store(|name| Files::in_profile(scratch(name)));
}

#[test]
fn memory_keeps_the_contract() {
    // "Opened again" is a clone for this one, so the same name has to hand back
    // a handle over the same document.
    let opened: Mutex<HashMap<String, Memory>> = Mutex::new(HashMap::new());
    contract::assert_store(|name| {
        opened
            .lock()
            .expect("the map")
            .entry(name.to_owned())
            .or_default()
            .clone()
    });
}
