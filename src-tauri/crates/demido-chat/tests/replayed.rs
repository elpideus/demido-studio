//! The log a live run with tools in it left behind, replayed by a different
//! process on a different day.
//!
//! `docs/rules/done.md` asks a closing comment for a trace fixture and says what
//! it is for: "a pruned, deterministic trace is exactly a replayable fixture. It
//! is committed beside the scenario that produced it and becomes that scenario's
//! input."
//!
//! The scenario is `a_real_model_with_tools::the_log_of_a_turn_with_tools_rebuilds_every_assembly_that_was_sent`,
//! which writes both halves: the log, and the requests `llama.cpp` was actually
//! handed. This reads them back with no card, no server and no model, and
//! asserts the one thing the pair exists to assert.
//!
//! `demido-trace/tests/a_session.rs` does the same for S1's fixture, over a turn
//! that offered nothing. What is new here is the tools: a rebuild that dropped a
//! tool document, or filled a placeholder differently, would come back as a
//! request the model never saw.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;

use demido_inference::{Request, Role};
use demido_trace::{Body, JsonLines, Replay};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn the_committed_trace_of_a_planted_file_still_rebuilds_every_request_that_was_sent() {
    // Read rather than opened. `JsonLines::open` creates the file and cuts a
    // torn last line off it, which is right for a log about to be written to
    // and wrong for one that is committed evidence.
    let path = fixture("a-planted-file.jsonl");
    let replay = Replay::over(JsonLines::read(&path).expect("read the fixture"));
    assert!(
        !replay.is_empty(),
        "the fixture at {} is empty; the live suite writes it",
        path.display()
    );

    let sent: Vec<Request> = serde_json::from_str(
        &std::fs::read_to_string(fixture("a-planted-file.sent.json"))
            .expect("the requests beside it"),
    )
    .expect("the requests");

    let assemblies: Vec<u64> = replay
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Assembly { .. }))
        .map(|event| event.seq)
        .collect();
    assert_eq!(
        assemblies.len(),
        sent.len(),
        "the committed log records a different number of steps from the run \
         that wrote it"
    );
    assert!(
        sent.len() > 1,
        "the committed turn called nothing, so it says nothing about tools"
    );

    for (seq, request) in assemblies.iter().zip(&sent) {
        let rebuilt = replay.request(*seq).expect("rebuilt");
        assert_eq!(
            serde_json::to_string(&rebuilt).expect("a request"),
            serde_json::to_string(request).expect("a request"),
            "the committed log no longer rebuilds the request the live run sent \
             at step {seq}"
        );
    }

    // The tool documents are in it, which is what makes this fixture different
    // from S1's: an assembly without them is one the model never saw.
    let offered: Vec<&str> = sent[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert!(
        offered.contains(&"read_file"),
        "the committed run offered {offered:?}"
    );
    assert!(
        sent[0]
            .tools
            .iter()
            .all(|tool| !tool.description.trim().is_empty()),
        "a tool was offered with no description, so the words the model read \
         are not in the log"
    );

    // And the file the model read came back to it, in a message answering the
    // call it made.
    let answered = sent
        .iter()
        .flat_map(|request| &request.messages)
        .any(|message| {
            message.role == Role::Tool && message.content.contains("7731-MARGATE-OXIDE")
        });
    assert!(
        answered,
        "the planted code never reached the model as a tool result, so the run \
         that wrote this fixture did not prove what it claims"
    );
}

/// The prune is part of the fixture's contract, not a tidying step: a fixture
/// carrying a scratch path or a wall-clock reading is one that cannot be diffed
/// against the next run of the same scenario.
#[test]
fn the_committed_trace_carries_nothing_from_the_machine_that_wrote_it() {
    let raw = std::fs::read_to_string(fixture("a-planted-file.jsonl")).expect("read the fixture");
    for (line, text) in raw.lines().enumerate() {
        let event: serde_json::Value = serde_json::from_str(text).expect("an event");
        assert_eq!(
            event["at"],
            0,
            "line {} carries a wall-clock reading",
            line + 1
        );
    }
    assert!(
        drive_letter(&raw).is_none(),
        "the fixture carries an absolute path from the machine that wrote it, \
         around {:?}",
        drive_letter(&raw)
    );
}

/// Where a Windows absolute path starts in `text`, if one does.
///
/// Narrower than a colon and a slash twice over, because the fixture is allowed
/// to carry the words the model read and those contain both.
///
/// A drive letter and a colon, then either two backslashes, which is how one
/// separator reaches a JSON string, or one forward slash that is not the second
/// of two, which would be a URL. A single backslash is not enough: the model
/// numbering its steps writes `Then:\n1.`, and the three characters before that
/// `n` are a letter, a colon and a backslash.
fn drive_letter(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(4)
        .position(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == ':'
                && match window[2] {
                    '\\' => window[3] == '\\',
                    '/' => window[3] != '/',
                    _ => false,
                }
        })
        .map(|at| chars[at..(at + 40).min(chars.len())].iter().collect())
}
