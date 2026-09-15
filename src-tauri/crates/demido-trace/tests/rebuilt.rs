//! The assembly as it stood at one event, block by block, against the one
//! before it.
//!
//! `design/windows.md`: "Selecting an event rebuilds the prompt **as it stood at
//! that moment**, block by block, **diffed against the previous assembly**: an
//! injection appears as an inserted block you can read, an evicted one as a
//! struck-out block with its cost."
//!
//! [#57](https://github.com/elpideus/demido-studio/issues/57) gives the rebuild
//! no seam of its own, because it is a projection of the log: the assertion
//! belongs here, and drawing it is the window gate's.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;

use demido_inference::{FinishReason, Options, Role, Usage};
use demido_prompts::{Document, Paragraphs, Prompt, Tools};
use demido_trace::{Change, Journal, Layer, Memory, Replay, Sent, Session, Source};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-trace-rebuilt")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made the directory");
    dir
}

fn options() -> Options {
    Options {
        temperature: Some(0.0),
        max_tokens: None,
        seed: None,
    }
}

fn paragraph(dir: &PathBuf, id: &str) -> Prompt {
    Paragraphs::open(dir).get(id).expect("a paragraph")
}

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

/// One turn, recorded the way the composer records it: who the model is, then
/// whatever came before, then the question.
fn ask(session: &Session<impl Journal>, prompt: &Prompt, carried: &[u64], question: &str) -> Sent {
    let mut turn = session.begin();
    turn.fragment(
        Source::Inject,
        Role::System,
        prompt,
        &[("root", "S:/work"), ("tree", "src/\n  main.rs")],
    )
    .expect("recorded");
    for seq in carried {
        turn.carry(*seq);
    }
    turn.user(question).expect("recorded");
    turn.parameters("development", options()).expect("recorded");
    turn.send().expect("sent")
}

fn answered(session: &Session<impl Journal>, sent: &Sent, text: &str) -> u64 {
    session
        .completed(
            sent,
            text,
            "",
            FinishReason::Stop,
            Usage {
                prompt_tokens: 30,
                completion_tokens: 3,
            },
        )
        .expect("recorded")
}

/// The blocks of a rebuild, as (text, change) pairs, which is what a reader of
/// a failed assertion wants to see.
fn blocks(rebuild: &demido_trace::Rebuild) -> Vec<(String, Change)> {
    rebuild
        .blocks
        .iter()
        .map(|block| (block.text.clone(), block.change))
        .collect()
}

#[test]
fn the_assembly_in_force_at_an_event_is_rebuilt_block_by_block() {
    let prompts = scratch("in-force");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("in-force", Memory::new());
    let sent = ask(&session, &prompt, &[], "What is the capital of France?");
    let answer = answered(&session, &sent, "Paris.");

    let replay = Replay::of(session.journal()).expect("read");
    // Selected on the answer, which is not an assembly: what the rebuild shows
    // is the assembly that was in force when that answer was produced.
    let rebuild = replay
        .rebuild(answer)
        .expect("rebuilt")
        .expect("an assembly");

    assert_eq!(rebuild.at, answer);
    assert_eq!(rebuild.seq, sent.seq);
    assert_eq!(rebuild.turn, sent.turn);
    assert_eq!(rebuild.model, "development");
    assert_eq!(rebuild.options, options());
    assert_eq!(
        rebuild.blocks.len(),
        sent.request.messages.len(),
        "a block per message that was sent"
    );

    let paragraph = &rebuild.blocks[0];
    assert_eq!(paragraph.role, Role::System);
    assert_eq!(
        paragraph.source,
        Source::Inject,
        "a block carries the source the monitor colours it by"
    );
    assert!(
        paragraph.text.contains("S:/work"),
        "the paragraph was refilled from the wording the log stored: {}",
        paragraph.text
    );
    assert!(paragraph.weight.tokens > 0, "a block carries its own cost");

    assert_eq!(rebuild.blocks[1].text, "What is the capital of France?");
    assert_eq!(rebuild.blocks[1].source, Source::User);
}

#[test]
fn the_first_assembly_has_nothing_to_be_diffed_against() {
    // Nothing was evicted and nothing was injected: this is what the model was
    // shown to begin with. Drawing every block as inserted would be reporting a
    // change against an assembly that does not exist.
    let prompts = scratch("first");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("first", Memory::new());
    let sent = ask(&session, &prompt, &[], "hello");

    let rebuild = Replay::of(session.journal())
        .expect("read")
        .rebuild(sent.seq)
        .expect("rebuilt")
        .expect("an assembly");

    assert_eq!(rebuild.previous, None);
    assert!(
        rebuild
            .blocks
            .iter()
            .all(|block| block.change == Change::Unchanged),
        "{:?}",
        blocks(&rebuild)
    );
}

#[test]
fn a_block_the_second_assembly_added_is_drawn_as_inserted() {
    let prompts = scratch("inserted");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("inserted", Memory::new());
    let first = ask(&session, &prompt, &[], "What is the capital of France?");
    answered(&session, &first, "Paris.");

    let carried = Replay::of(session.journal()).expect("read").conversation();
    let second = ask(&session, &prompt, &carried, "And of Italy?");

    let rebuild = Replay::of(session.journal())
        .expect("read")
        .rebuild(second.seq)
        .expect("rebuilt")
        .expect("an assembly");

    assert_eq!(rebuild.previous, Some(first.seq));
    let changes = blocks(&rebuild);
    assert_eq!(
        changes
            .iter()
            .filter(|(_, change)| *change == Change::Inserted)
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>(),
        ["Paris.", "And of Italy?"],
        "what turn two put in front of the model that turn one did not: {changes:?}"
    );
    assert_eq!(
        changes
            .iter()
            .filter(|(text, change)| *change == Change::Unchanged
                && text == "What is the capital of France?")
            .count(),
        1,
        "the question that was carried is the same block, not a second one"
    );
    assert!(
        !changes.iter().any(|(_, change)| *change == Change::Evicted),
        "nothing was dropped: {changes:?}"
    );
}

#[test]
fn a_block_the_next_assembly_does_not_name_is_evicted_and_keeps_its_cost() {
    // The eviction case, which is the whole reason an assembly refers to its
    // blocks rather than copying them. The block is still on the log, and what
    // changed is that the next assembly stopped naming it.
    let prompts = scratch("evicted");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("evicted", Memory::new());
    let first = ask(&session, &prompt, &[], "What is the capital of France?");
    answered(&session, &first, "Paris.");

    // The window is full, so the oldest question is dropped rather than
    // carried: the answer goes on alone.
    let conversation = Replay::of(session.journal()).expect("read").conversation();
    let kept = &conversation[1..];
    let second = ask(&session, &prompt, kept, "And of Italy?");

    let rebuild = Replay::of(session.journal())
        .expect("read")
        .rebuild(second.seq)
        .expect("rebuilt")
        .expect("an assembly");

    let evicted: Vec<&demido_trace::Placed> = rebuild
        .blocks
        .iter()
        .filter(|block| block.change == Change::Evicted)
        .collect();
    assert_eq!(evicted.len(), 1, "{:?}", blocks(&rebuild));
    assert_eq!(evicted[0].text, "What is the capital of France?");
    assert!(
        evicted[0].weight.tokens > 0,
        "an evicted block is struck out with its cost, so it needs one"
    );
    assert_eq!(
        evicted[0].seq, conversation[0],
        "an evicted block is the block that was dropped, read off the log"
    );
}

#[test]
fn a_rebuild_carries_the_tool_wording_that_was_actually_sent() {
    // The per-tool hash earning its keep. An edit made today must not rewrite
    // the record of a reply produced yesterday, and the way that failure would
    // look on screen is a monitor rendering today's wording against an old
    // answer, silently.
    let dir = scratch("wording");
    let prompt = paragraph(&dir.join("paragraphs"), demido_prompts::id::CONTEXT_TREE);
    let tools = Tools::open(dir.join("tools"));

    let session = Session::new("wording", Memory::new());

    let mut first = session.begin();
    first
        .fragment(
            Source::Inject,
            Role::System,
            &prompt,
            &[("root", "S:/work"), ("tree", "src/")],
        )
        .expect("recorded");
    first.user("What is in README.md?").expect("recorded");
    first
        .offer(Layer::Registry, &documents(&tools, &["read_file"]))
        .expect("recorded");
    first
        .parameters("development", options())
        .expect("recorded");
    let first = first.send().expect("sent");
    let before = tools.get("read_file").expect("a host tool");

    // Somebody edits the tool register after that reply was produced.
    tools
        .set("read_file", "Open one file.\n\n## path\n\nWhich file.")
        .expect("edited");

    let mut second = session.begin();
    second.user("And now?").expect("recorded");
    second
        .offer(Layer::Chat, &documents(&tools, &["read_file"]))
        .expect("recorded");
    second
        .parameters("development", options())
        .expect("recorded");
    let second = second.send().expect("sent");

    let replay = Replay::of(session.journal()).expect("read");
    let then = replay
        .rebuild(first.seq)
        .expect("rebuilt")
        .expect("an assembly");
    let now = replay
        .rebuild(second.seq)
        .expect("rebuilt")
        .expect("an assembly");

    let then = then.tools.expect("a set was offered");
    let now = now.tools.expect("a set was offered");

    assert_eq!(then.layer, Layer::Registry);
    assert_eq!(
        then.tools[0].text, before.text,
        "yesterday's assembly is rebuilt in yesterday's wording"
    );
    assert_eq!(now.layer, Layer::Chat, "and it says who decided the set");
    assert_eq!(
        now.tools[0].text,
        "Open one file.\n\n## path\n\nWhich file."
    );
    assert_ne!(then.tools[0].hash, now.tools[0].hash);
}

#[test]
fn an_event_from_before_anything_was_sent_rebuilds_nothing() {
    let session = Session::new("nothing", Memory::new());
    let said = session.begin().user("hello").expect("recorded");

    let replay = Replay::of(session.journal()).expect("read");
    assert!(
        replay.rebuild(said).expect("read").is_none(),
        "there is no assembly to show at a moment before one was composed"
    );
}
