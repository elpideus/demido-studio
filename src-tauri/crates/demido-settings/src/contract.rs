//! What every [`Store`] must do, as one function any implementation calls.
//!
//! [`docs/rules/tiles.md`](../../../../../docs/rules/tiles.md): the contract
//! suite is the trait's second file, and it is written before the second
//! implementation exists. It is written here against the two that do exist and
//! against the third that will, which is a profile database rather than a file:
//! it has the same promises to keep, and a caller written against this cannot
//! tell which one it has.
//!
//! It takes a **factory** rather than a store, because the promise that matters
//! most cannot be tested on one handle: a value has to come back after the
//! process that set it is gone. For [`crate::Files`] that is opening the path
//! again, for [`crate::Memory`] it is cloning the handle, and the contract does
//! not care which.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// This module is a test suite that happens to ship in the library, so that any
// implementation anywhere can call it. A panic here is the report.

use serde_json::json;

use crate::document::{Document, Scope};
use crate::schema::id;
use crate::store::Store;

/// Exercise the whole trait. Call it from an implementation's test file.
pub fn assert_store<S: Store + 'static>(open: impl Fn(&str) -> S) {
    an_untouched_profile_reads_as_the_defaults(&open);
    what_was_written_comes_back(&open);
    a_reopened_store_still_has_it(&open);
    every_tier_survives_the_round_trip(&open);
    the_last_write_is_the_only_one_kept(&open);
}

fn an_untouched_profile_reads_as_the_defaults<S: Store>(open: &impl Fn(&str) -> S) {
    let store = open("fresh");
    assert_eq!(
        store
            .read()
            .expect("a profile that has never been set up is not a failure"),
        Document::default()
    );
}

fn what_was_written_comes_back<S: Store>(open: &impl Fn(&str) -> S) {
    let store = open("roundtrip");
    let mut document = Document::default();
    document
        .values_mut(&Scope::Global)
        .insert(id::TEMPERATURE.into(), json!(0.4));

    store.write(&document).expect("wrote");
    assert_eq!(
        store.read().expect("read"),
        document,
        "what went in is what comes back"
    );
}

fn a_reopened_store_still_has_it<S: Store>(open: &impl Fn(&str) -> S) {
    // The restart promise. This is the reason the trait exists, so it is here
    // rather than in one implementation's own tests.
    let mut document = Document::default();
    document
        .values_mut(&Scope::chat("session"))
        .insert(id::TEMPERATURE.into(), json!(1.2));

    open("restart").write(&document).expect("wrote");
    assert_eq!(
        open("restart").read().expect("read"),
        document,
        "a store opened again over the same storage reads back what the last one wrote"
    );
}

/// The shape carries four tiers whether or not this build sets them. A store
/// that dropped the two nothing uses would make the character system a
/// migration of every profile.
fn every_tier_survives_the_round_trip<S: Store>(open: &impl Fn(&str) -> S) {
    let store = open("four-tiers");
    let mut document = Document::default();
    for scope in [
        Scope::Global,
        Scope::Model("gemma".into()),
        Scope::Character("violet".into()),
        Scope::chat("session"),
    ] {
        document
            .values_mut(&scope)
            .insert(id::TEMPERATURE.into(), json!(0.5));
    }

    store.write(&document).expect("wrote");
    let read = store.read().expect("read");
    assert_eq!(read, document);
    assert_eq!(read.models.len(), 1, "the model tier survives storage");
    assert_eq!(
        read.characters.len(),
        1,
        "so does the character tier, which nothing in v0.1 writes"
    );
}

fn the_last_write_is_the_only_one_kept<S: Store>(open: &impl Fn(&str) -> S) {
    // There is one document per profile, so writing is replacing. A store that
    // merged could never clear a value, and revert would silently do nothing.
    let store = open("replace");
    let mut document = Document::default();
    document
        .values_mut(&Scope::Global)
        .insert(id::TEMPERATURE.into(), json!(0.4));
    store.write(&document).expect("wrote");

    document.values_mut(&Scope::Global).clear();
    store.write(&document).expect("wrote again");

    assert!(store.read().expect("read").global.is_empty());
    assert!(open("replace").read().expect("read").global.is_empty());
}
