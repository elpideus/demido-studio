//! What every [`Store`] must do, as one function any implementation calls.
//!
//! `docs/rules/tiles.md`: the contract suite is the trait's second file, and it
//! is written before the second implementation exists. It is written here
//! against the two that do exist and against the third that will, because a
//! layout kept in a profile database rather than a file has the same four
//! promises to keep and a caller written against this cannot tell which one it
//! has.
//!
//! It takes a **factory** rather than a store, because the promise that matters
//! most cannot be tested on one handle: an arrangement has to come back after
//! the process that arranged it is gone. For [`crate::Files`] that is opening
//! the path again, for [`crate::Memory`] it is cloning the handle, and the
//! contract does not care which.
//!
//! The factory is given a name. The same name is the same storage, which is how
//! reopening is asked for; a different name is a fresh store, which is how one
//! case is kept from reading another's layout.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// This module is a test suite that happens to ship in the library, so that any
// implementation anywhere can call it. A panic here is the report.

use crate::layout::{Shell, Side};
use crate::store::Store;

/// Exercise the whole trait. Call it from an implementation's test file.
pub fn assert_store<S: Store + 'static>(open: impl Fn(&str) -> S) {
    an_unarranged_desk_reads_as_nothing(&open);
    what_was_written_comes_back(&open);
    a_reopened_store_still_has_it(&open);
    the_last_write_is_the_only_one_kept(&open);
}

fn an_unarranged_desk_reads_as_nothing<S: Store>(open: &impl Fn(&str) -> S) {
    let store = open("fresh");
    assert!(
        store.read().is_none(),
        "a profile that has never arranged the desk has no layout, which is not a failure"
    );
}

fn what_was_written_comes_back<S: Store>(open: &impl Fn(&str) -> S) {
    let store = open("roundtrip");
    let arranged = Shell { rail: Side::Right };
    store.write(&arranged).expect("wrote");
    assert_eq!(
        store.read(),
        Some(arranged),
        "the arrangement that went in is the arrangement that comes back"
    );
}

fn a_reopened_store_still_has_it<S: Store>(open: &impl Fn(&str) -> S) {
    // The restart promise. This is the reason the trait exists, so it is here
    // rather than in one implementation's own tests.
    let arranged = Shell { rail: Side::Right };
    open("restart").write(&arranged).expect("wrote");
    assert_eq!(
        open("restart").read(),
        Some(arranged),
        "a store opened again over the same storage reads back what the last one wrote"
    );
}

fn the_last_write_is_the_only_one_kept<S: Store>(open: &impl Fn(&str) -> S) {
    // There is one layout per profile, so writing is replacing. A store that
    // accumulated would read back the first arrangement forever, which is the
    // failure that looks like "the desk does not remember" and is really "the
    // desk remembers too much".
    let store = open("replace");
    store.write(&Shell { rail: Side::Right }).expect("wrote");
    store
        .write(&Shell { rail: Side::Left })
        .expect("wrote again");
    assert_eq!(store.read(), Some(Shell { rail: Side::Left }));
    assert_eq!(open("replace").read(), Some(Shell { rail: Side::Left }));
}
