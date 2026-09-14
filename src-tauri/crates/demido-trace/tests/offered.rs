//! The offered set, recorded as names and hashes and rebuilt as wording.
//!
//! `docs/rules/prompts.md` and `docs/rules/tools.md`: `tools/offered` carries
//! `(name, hash)` per tool, never names alone, and each tool's document goes
//! into the log once per session per hash, the rule `prompt/version` already
//! keeps. With names alone the monitor would render today's wording against a
//! reply produced by yesterday's, and nothing would say so.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_prompts::{Document, Tools};
use demido_trace::{Body, Journal, JsonLines, Layer, Memory, Replay, Session};

/// Each tool as a turn offers it: its document, and a shape for the prose.
fn documents(tools: &Tools, names: &[&str]) -> Vec<(Document, serde_json::Value)> {
    names
        .iter()
        .map(|name| {
            (
                tools.get(name).expect("a host tool"),
                serde_json::json!({ "type": "object" }),
            )
        })
        .collect()
}

fn count(session: &Session<impl Journal>, kind: fn(&Body) -> bool) -> usize {
    session
        .journal()
        .events()
        .unwrap()
        .iter()
        .filter(|event| kind(&event.body))
        .count()
}

fn versions(body: &Body) -> bool {
    matches!(body, Body::ToolVersion { .. })
}

fn offers(body: &Body) -> bool {
    matches!(body, Body::Offered { .. })
}

#[test]
fn the_offered_set_is_a_name_and_a_hash_per_tool_and_never_the_text() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let offered = documents(&tools, &["read_file", "list_directory"]);

    let session = Session::new("names-and-hashes", Memory::new());
    let mut turn = session.begin();
    let seq = turn
        .offer(Layer::Registry, &offered)
        .unwrap()
        .expect("a first set is a change");

    let event = session
        .journal()
        .events()
        .unwrap()
        .into_iter()
        .find(|event| event.seq == seq)
        .unwrap();
    let line = serde_json::to_value(&event).unwrap();

    assert_eq!(line["event"], "tools/offered");
    assert_eq!(line["layer"], "registry");
    assert_eq!(line["tools"][0]["name"], "read_file");
    assert_eq!(line["tools"][0]["hash"], offered[0].0.hash.as_str());
    assert_eq!(line["tools"][1]["name"], "list_directory");
    assert!(
        line["tools"][0].get("text").is_none(),
        "the set names wording, it does not carry a second copy of it"
    );
}

#[test]
fn a_document_is_stored_once_per_session_and_an_unchanged_set_writes_nothing() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let offered = documents(&tools, &["read_file", "write_file", "run_command"]);

    let session = Session::new("once", Memory::new());
    for _ in 0..3 {
        let mut turn = session.begin();
        turn.offer(Layer::Registry, &offered).unwrap();
    }

    assert_eq!(
        count(&session, versions),
        3,
        "one version per tool, not per turn"
    );
    assert_eq!(
        count(&session, offers),
        1,
        "the set did not change after the first turn"
    );
}

#[test]
fn a_change_to_the_set_is_an_event_carrying_the_layer_that_decided_it() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());

    let session = Session::new("narrowed", Memory::new());
    session
        .begin()
        .offer(
            Layer::Registry,
            &documents(&tools, &["read_file", "run_command"]),
        )
        .unwrap();
    let narrowed = session
        .begin()
        .offer(Layer::Registry, &documents(&tools, &["read_file"]))
        .unwrap();

    assert!(narrowed.is_some(), "a narrower set is a change");
    assert_eq!(count(&session, offers), 2);
    assert_eq!(count(&session, versions), 2, "nothing new was worded");

    let replay = Replay::of(session.journal()).unwrap();
    let now = replay.offered(u64::MAX).unwrap().expect("a set");
    assert_eq!(now.layer, Layer::Registry);
    assert_eq!(
        now.tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["read_file"]
    );
}

#[test]
fn a_reply_from_before_an_edit_is_rebuilt_with_the_wording_it_was_offered() {
    // The whole reason the set is hashed. An edit made after the fact must not
    // rewrite the record of what an earlier turn was shown.
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());

    let session = Session::new("edited", Memory::new());
    let before = session
        .begin()
        .offer(Layer::Registry, &documents(&tools, &["read_file"]))
        .unwrap()
        .unwrap();
    let old = tools.get("read_file").unwrap();

    tools
        .set("read_file", "Open one file.\n\n## path\n\nWhich file.")
        .unwrap();
    let after = session
        .begin()
        .offer(Layer::Registry, &documents(&tools, &["read_file"]))
        .unwrap()
        .expect("same names, new wording, is a different set");

    assert_eq!(count(&session, versions), 2, "the edit is a second version");

    let replay = Replay::of(session.journal()).unwrap();
    let then = replay.offered(before).unwrap().unwrap();
    let now = replay.offered(after).unwrap().unwrap();

    assert_eq!(
        then.tools[0].text, old.text,
        "yesterday's reply keeps yesterday's wording"
    );
    assert_eq!(
        now.tools[0].text,
        "Open one file.\n\n## path\n\nWhich file."
    );
    assert_ne!(then.tools[0].hash, now.tools[0].hash);
}

#[test]
fn before_anything_was_offered_there_is_no_set() {
    let session = Session::new("empty", Memory::new());
    session.begin().user("hello").unwrap();

    let replay = Replay::of(session.journal()).unwrap();
    assert!(replay.offered(u64::MAX).unwrap().is_none());
}

#[test]
fn a_resumed_session_neither_restates_a_document_nor_a_set_the_log_holds() {
    let scratch = tempfile::tempdir().unwrap();
    let log = scratch.path().join("session.jsonl");
    let tools = Tools::open(scratch.path().join("prompts"));
    let offered = documents(&tools, &["read_file", "delete_file"]);

    {
        let session = Session::new("resumed", JsonLines::open(&log).unwrap());
        session.begin().offer(Layer::Registry, &offered).unwrap();
    }

    let session = Session::new("resumed", JsonLines::open(&log).unwrap());
    session.resume().unwrap();
    let again = session.begin().offer(Layer::Registry, &offered).unwrap();

    assert!(again.is_none(), "the log already holds this set");
    assert_eq!(count(&session, versions), 2);
    assert_eq!(count(&session, offers), 1);
}
