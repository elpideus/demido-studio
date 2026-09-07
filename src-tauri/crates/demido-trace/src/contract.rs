//! What every [`Journal`] must do, as one function any implementation calls.
//!
//! `docs/rules/tiles.md`: the contract suite is the trait's second file, and it
//! is written before the second implementation exists. It is written here
//! against the two that do exist, and against the third that will: a journal
//! backed by something other than a file has the same four promises to keep,
//! and a caller written against this cannot tell which one it has.
//!
//! It takes a **factory** rather than a journal, because the promise that
//! matters most cannot be tested on one handle: a log has to come back after
//! the process that wrote it is gone. For [`crate::jsonl::JsonLines`] that is
//! reopening the file, for [`crate::memory::Memory`] it is cloning the handle,
//! and the contract does not care which, only that the events are still there
//! and the numbering continues.
//!
//! The factory is given a name. The same name is the same storage, which is how
//! reopening is asked for; a different name is a fresh log, which is how one
//! case is kept from reading another's events.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// This module is a test suite that happens to ship in the library, so that any
// implementation anywhere can call it. A panic here is the report.

use demido_inference::Role;

use crate::event::{Body, Entry, SessionId, Source, Weight};
use crate::journal::Journal;

/// Exercise the whole trait. Call it from an implementation's test file.
///
/// `open` must hand back a journal over the storage that name refers to,
/// creating it if it is new and reopening it if it is not, the way a path
/// does.
pub fn assert_journal<J: Journal + 'static>(open: impl Fn(&str) -> J) {
    a_log_starts_empty(&open);
    numbering_starts_at_one_and_has_no_gaps(&open);
    what_went_in_comes_back_unchanged(&open);
    a_reopened_log_replays_and_carries_on(&open);
    time_never_goes_backwards(&open);
    concurrent_appends_get_distinct_positions(&open);
}

fn said(turn: u32, what: &str) -> Entry {
    Entry::new(
        SessionId::new("contract"),
        turn,
        Source::User,
        Weight::estimated(1),
        Body::Message {
            role: Role::User,
            text: what.to_owned(),
        },
    )
}

fn a_log_starts_empty<J: Journal>(open: &impl Fn(&str) -> J) {
    let journal = open("empty");
    assert!(
        journal.events().expect("a fresh log reads").is_empty(),
        "a log nobody has written to is empty rather than an error"
    );
}

fn numbering_starts_at_one_and_has_no_gaps<J: Journal>(open: &impl Fn(&str) -> J) {
    let journal = open("numbering");
    for expected in 1..=5u64 {
        let event = journal.append(said(1, "x")).expect("appended");
        assert_eq!(
            event.seq, expected,
            "sequence numbers start at one and go up by one"
        );
    }

    let events = journal.events().expect("read");
    let numbers: Vec<u64> = events.iter().map(|event| event.seq).collect();
    assert_eq!(
        numbers,
        vec![1, 2, 3, 4, 5],
        "no gaps, no repeats, in order"
    );
}

fn what_went_in_comes_back_unchanged<J: Journal>(open: &impl Fn(&str) -> J) {
    let journal = open("unchanged");

    // A body of every shape, because the log is only useful if it is exact:
    // a rebuild is made of what comes back out of here.
    // not-a-prompt: every string below is a value this suite writes into a log
    // and reads back out of it, chosen for the characters that survive a round
    // trip badly. Nothing sends any of it to a model.
    let bodies = [
        Body::Version {
            id: "caveman.ultra".into(),
            hash: "sha256:0".into(),
            text: "one fact a line.\n\ttabbed, \"quoted\", 漢字\n".into(),
        },
        Body::Fragment {
            role: Role::System,
            hash: "sha256:0".into(),
            values: vec![crate::event::Filling::new("target", "your reply")],
        },
        Body::Message {
            role: Role::User,
            text: "  leading and trailing spaces  ".into(),
        },
        Body::Parameters {
            model: "development".into(),
            options: demido_inference::Options {
                temperature: Some(0.0),
                max_tokens: Some(768),
                seed: Some(1),
            },
        },
        Body::Assembly {
            parameters: 4,
            blocks: vec![2, 3],
        },
        Body::Completion {
            text: "Paris.".into(),
            thinking: "the user asked for a capital".into(),
            reason: demido_inference::FinishReason::Stop,
            usage: demido_inference::Usage {
                prompt_tokens: 12,
                completion_tokens: 3,
            },
        },
        Body::Failure {
            kind: "unavailable".into(),
            detail: "the backend is not reachable".into(),
        },
    ];

    let written: Vec<_> = bodies
        .iter()
        .map(|body| {
            journal
                .append(Entry::new(
                    SessionId::new("contract"),
                    2,
                    Source::System,
                    Weight::counted(7),
                    body.clone(),
                ))
                .expect("appended")
        })
        .collect();

    let read = journal.events().expect("read");
    assert_eq!(read, written, "an event reads back exactly as it went in");
}

fn a_reopened_log_replays_and_carries_on<J: Journal>(open: &impl Fn(&str) -> J) {
    // The restart promise, and the reason this trait exists rather than a
    // vector somebody remembers to save.
    let before = {
        let journal = open("reopened");
        journal.append(said(1, "first")).expect("appended");
        journal.append(said(1, "second")).expect("appended");
        journal.events().expect("read")
    };

    let journal = open("reopened");
    assert_eq!(
        journal.events().expect("read"),
        before,
        "a log reopened over the same storage replays everything it held"
    );

    let next = journal.append(said(2, "third")).expect("appended");
    assert_eq!(
        next.seq, 3,
        "numbering carries on where the last handle stopped rather than starting again"
    );
}

fn time_never_goes_backwards<J: Journal>(open: &impl Fn(&str) -> J) {
    let journal = open("clock");
    let first = journal.append(said(1, "first")).expect("appended");
    let second = journal.append(said(1, "second")).expect("appended");
    assert!(
        second.at >= first.at,
        "a later event is not stamped earlier: {} then {}",
        first.at,
        second.at
    );
}

fn concurrent_appends_get_distinct_positions<J: Journal + 'static>(open: &impl Fn(&str) -> J) {
    // Two turns can be recording at once as soon as there are sub-agents, and
    // a log with two lines claiming position 9 cannot be replayed at all.
    let journal = std::sync::Arc::new(open("threads"));
    let writers: Vec<_> = (0..4)
        .map(|writer| {
            let journal = journal.clone();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    journal.append(said(writer, "x")).expect("appended");
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().expect("a writer finished");
    }

    let events = journal.events().expect("read");
    assert_eq!(events.len(), 100, "every append is in the log");
    let numbers: Vec<u64> = events.iter().map(|event| event.seq).collect();
    assert_eq!(
        numbers,
        (1..=100).collect::<Vec<u64>>(),
        "one hundred appends from four threads is one hundred distinct positions, in order"
    );
}
