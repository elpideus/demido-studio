//! Both journals, against the same contract.
//!
//! `docs/rules/tiles.md`: "Each implementation's test file calls that function
//! with itself. An implementation that does not call it is not an
//! implementation." There are two here, and the point of running the suite over
//! both is that the difference between them is a disk and nothing else.
//!
//! The restart promise is in the contract, where reopening is a fresh handle
//! over the same storage. That it holds across a real process boundary is
//! proved in `a_session.rs`, against the log a live run left behind.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use demido_trace::{JsonLines, Memory};

/// A directory of this test's own, emptied first so a previous run's log is
/// never mistaken for this one's.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-trace-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_log_on_disk_keeps_the_contract() {
    let dir = scratch("contract-jsonl");
    demido_trace::contract::assert_journal(|name| {
        JsonLines::open(dir.join(format!("{name}.jsonl"))).expect("opened")
    });
}

#[test]
fn a_log_in_memory_keeps_the_contract() {
    // Reopening, for this one, is handing back the same handle. The contract
    // does not care which it is, only that the events are still there and the
    // numbering continues.
    let logs: Mutex<HashMap<String, Memory>> = Mutex::new(HashMap::new());
    demido_trace::contract::assert_journal(|name| {
        logs.lock()
            .expect("the map")
            .entry(name.to_owned())
            .or_default()
            .clone()
    });
}
