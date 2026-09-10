//! The one block a hash alone would not catch.
//!
//! `docs/rules/prompts.md` draws the line between prose and a value with one
//! question: could a reviewer diff this block in a pull request? The
//! classifier's class table could, so it is prose in the default file, typed
//! out in full, rather than a `{{classes}}` hole the composer fills from the
//! vocabulary `docs/rules/lessons.md` owns.
//!
//! That choice has a cost, and this file is the payment. A hash over a frame
//! with a hole in it does not change when a class is added, which is precisely
//! the change most likely to move the agreement rates, so the release gate in
//! `scripts/check-rules.mjs` would sleep through it. A hash over the table
//! typed out in full does change, and the gate fires; what the gate cannot see
//! is a class added to `lessons.md` and never carried into the prompt. This
//! test is the other half, and it binds the two in both directions.
//!
//! It reaches out of the crate for the rule file on purpose. The vocabulary has
//! one owner, and a copy of it in here to test against would be the third place
//! the thirteen classes are written down.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;

use demido_prompts::{id, paragraph};

/// The class table in `docs/rules/lessons.md`, which owns the vocabulary.
const LESSONS: &str = include_str!("../../../../docs/rules/lessons.md");

/// The class names in the rule file's table, which are the rows that open with
/// a backticked name under the `## The classes` heading.
fn classes_the_rules_own() -> BTreeSet<String> {
    LESSONS
        .split("## The classes")
        .nth(1)
        .expect("lessons.md declares the classes under that heading")
        .lines()
        .take_while(|line| !line.starts_with("Adding a class"))
        .filter_map(|line| line.trim().strip_prefix("| `"))
        .filter_map(|rest| rest.split_once('`'))
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// The class names in the shipped classifier prompt, which are the first token
/// of each line in the block the frame introduces.
fn classes_the_prompt_offers(text: &str) -> BTreeSet<String> {
    text.split("The classes, and the mistake each one names:")
        .nth(1)
        .expect("the classifier introduces its table with that line")
        .lines()
        .map(str::trim_end)
        .take_while(|line| !line.starts_with("Choose "))
        .filter_map(|line| line.split_once("  "))
        .map(|(name, _)| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn the_shipped_classifier_offers_exactly_the_classes_the_rules_own() {
    let classifier = paragraph(id::LESSONS_CLASSIFY).expect("the classifier is in the register");

    let owned = classes_the_rules_own();
    let offered = classes_the_prompt_offers(classifier.default);

    assert_eq!(
        owned.len(),
        13,
        "the vocabulary is closed at thirteen; {owned:?} was read out of lessons.md"
    );

    // Both directions, and they fail differently. A class in the rules and not
    // in the prompt is a class no failure can ever be given. A class in the
    // prompt and not in the rules is a label the lesson store cannot hold.
    let missing: Vec<_> = owned.difference(&offered).collect();
    assert!(
        missing.is_empty(),
        "lessons.md holds {missing:?}, and the classifier never offers them"
    );

    let invented: Vec<_> = offered.difference(&owned).collect();
    assert!(
        invented.is_empty(),
        "the classifier offers {invented:?}, which lessons.md does not own"
    );
}

#[test]
fn the_class_table_is_typed_out_rather_than_filled_in() {
    // The whole argument in one assertion: if this ever becomes a placeholder,
    // the digest stops covering the vocabulary and the release gate goes quiet.
    let classifier = paragraph(id::LESSONS_CLASSIFY).expect("the classifier is in the register");

    assert!(
        classifier.placeholders.is_empty(),
        "the classifier takes no placeholder; its table is prose"
    );
    assert!(
        !classifier.default.contains("{{"),
        "a hash over a frame with a hole in it does not change when a class is added"
    );
}
