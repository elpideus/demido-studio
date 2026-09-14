//! The [`Store`] trait's second file.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): "The contract
//! test suite is the trait's second file, and it is written before the second
//! implementation exists." One implementation ships today
//! ([`crate::file::Files`]); this is what a second one would have to keep.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// The suite asserts by panicking. The workspace denies these in application
// code, where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;

use crate::state::{Ledger, RowState};
use crate::store::Store;

/// Exercise a store against every promise the trait makes.
///
/// `open` is handed a name and returns a store over storage of its own. Asked
/// twice for the same name it must return a store over the **same** storage,
/// which is how the suite asks for a profile to be opened again after a
/// restart.
pub fn assert_store<S: Store>(open: impl Fn(&str) -> S) {
    a_store_nobody_has_written_to_is_empty(&open);
    what_was_written_last_is_what_comes_back(&open);
    a_store_opened_again_reads_what_the_last_one_wrote(&open);
}

/// A profile that has fetched nothing is the ordinary case, not a failure.
fn a_store_nobody_has_written_to_is_empty<S: Store>(open: impl Fn(&str) -> S) {
    let store = open("empty");
    assert_eq!(
        store
            .read()
            .expect("an unwritten store reads as an empty ledger"),
        Ledger::default()
    );
}

fn what_was_written_last_is_what_comes_back<S: Store>(open: impl Fn(&str) -> S) {
    let store = open("round-trip");
    let mut ledger = Ledger::default();
    ledger.set(
        "llama.cpp",
        RowState::Managed {
            pin: "b10816".into(),
            archives: vec!["llama-b10816-bin-win-cuda-13.3-x64.zip".into()],
            on_disk_mib: 671.6,
        },
    );
    ledger.set(
        "model",
        RowState::Linked {
            path: PathBuf::from("D:/models/gemma-4-E4B-it-Q8_0.gguf"),
            detected_version: None,
        },
    );
    store.write(&ledger).expect("wrote");
    assert_eq!(store.read().expect("read"), ledger);

    // Every state survives, including a refusal's reason: a row that came back
    // as a bare absence would lose why it is absent, which is the one thing
    // section 2 asks a refused row to carry.
    // not-a-prompt: a refusal's reason, as a store has to be able to carry it.
    let mut refused = Ledger::default();
    refused.set(
        "llama.cpp",
        RowState::Absent {
            reason: Some("the server started and generated nothing".into()),
        },
    );
    store.write(&refused).expect("wrote");
    assert_eq!(store.read().expect("read"), refused);
}

fn a_store_opened_again_reads_what_the_last_one_wrote<S: Store>(open: impl Fn(&str) -> S) {
    let mut ledger = Ledger::default();
    ledger.set(
        "llama.cpp",
        RowState::Managed {
            pin: "b10816".into(),
            archives: vec![],
            on_disk_mib: 182.6,
        },
    );
    open("reopened").write(&ledger).expect("wrote");
    assert_eq!(
        open("reopened").read().expect("read"),
        ledger,
        "this is the whole reason the trait exists"
    );
}
