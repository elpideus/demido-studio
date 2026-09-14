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

use demido_hardware::Ecosystem;

use crate::answers::Answers;
use crate::store::Store;

/// Exercise a store against every promise the trait makes.
///
/// `open` is handed a name and returns a store over storage of its own. Asked
/// twice for the same name it must return a store over the **same** storage,
/// which is how the suite asks for a profile to be opened again after a
/// restart.
pub fn assert_store<S: Store>(open: impl Fn(&str) -> S) {
    a_profile_that_has_never_set_up_reads_as_the_default_answers(&open);
    what_was_written_last_is_what_comes_back(&open);
    a_store_opened_again_reads_what_the_last_one_wrote(&open);
}

/// The ordinary first launch, and not a failure.
fn a_profile_that_has_never_set_up_reads_as_the_default_answers<S: Store>(
    open: impl Fn(&str) -> S,
) {
    let store = open("fresh");
    assert_eq!(
        store.read().expect("an unwritten store reads as default"),
        Answers::default()
    );
}

fn what_was_written_last_is_what_comes_back<S: Store>(open: impl Fn(&str) -> S) {
    let store = open("round-trip");
    let answers = answered();
    store.write(&answers).expect("wrote");
    assert_eq!(store.read().expect("read"), answers);
}

/// A restart. This is the whole reason the trait exists.
fn a_store_opened_again_reads_what_the_last_one_wrote<S: Store>(open: impl Fn(&str) -> S) {
    let answers = answered();
    open("restarted").write(&answers).expect("wrote");
    assert_eq!(open("restarted").read().expect("read"), answers);
}

/// A profile somebody has actually set up: every field carrying something, so
/// a store that drops one is caught rather than passing on the empty case.
fn answered() -> Answers {
    let mut answers = Answers {
        ecosystem: Some(Ecosystem::Cuda),
        model: Some(PathBuf::from("D:/models/gemma-4-E4B-it-Q8_0.gguf")),
        closed: true,
        ..Answers::default()
    };
    answers.add_folder(PathBuf::from("D:/models"));
    answers.tick("chrome", false);
    answers
}
