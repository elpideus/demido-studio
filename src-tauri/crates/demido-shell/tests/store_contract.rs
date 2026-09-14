//! Both stores, against the same contract.
//!
//! `docs/rules/tiles.md`: "Each implementation's test file calls that function
//! with itself. An implementation that does not call it is not an
//! implementation." There are two here, and the point of running the suite over
//! both is that the difference between them is a disk and nothing else.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use demido_shell::{Files, Memory};

/// A directory of this test's own, emptied first so a previous run's layout is
/// never mistaken for this one's.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-shell-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_layout_on_disk_keeps_the_contract() {
    // Each name is its own profile directory, which is what makes "opened
    // again over the same storage" mean the thing a restart means.
    let dir = scratch("contract-files");
    demido_shell::contract::assert_store(|name| Files::in_profile(dir.join(name)));
}

#[test]
fn a_layout_in_memory_keeps_the_contract() {
    // Reopening, for this one, is handing back the same handle. The contract
    // does not care which it is, only that the arrangement is still there.
    let stores: Mutex<HashMap<String, Memory>> = Mutex::new(HashMap::new());
    demido_shell::contract::assert_store(|name| {
        stores
            .lock()
            .expect("the map")
            .entry(name.to_owned())
            .or_default()
            .clone()
    });
}
