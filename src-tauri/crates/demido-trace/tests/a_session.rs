//! A session recorded, then rebuilt out of its own log.
//!
//! The offline half of this ticket's claim. What it cannot do is prove the
//! rebuilt assembly is what a backend received, because there is no backend
//! here; that is `a_real_model.rs`, and the log that suite leaves behind is
//! committed under `fixtures/` and replayed at the bottom of this file.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;

use demido_inference::{FinishReason, Options, Request, Role, Usage};
use demido_prompts::{Paragraphs, Prompt};
use demido_trace::{Basis, Body, Journal, JsonLines, Memory, Replay, Session, Source};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-trace-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made the directory");
    dir
}

fn options() -> Options {
    Options {
        temperature: Some(0.0),
        max_tokens: Some(768),
        seed: Some(1),
    }
}

/// A paragraph as it stands, from the register the app itself reads.
///
/// A hand written `Prompt` would let this suite pass over a rebuild that cannot
/// fill a real one.
fn paragraph(dir: &PathBuf, id: &str) -> Prompt {
    Paragraphs::open(dir).get(id).expect("a paragraph")
}

/// One turn, recorded exactly the way a composer will record it.
fn ask(session: &Session<impl Journal>, prompt: &Prompt, question: &str) -> demido_trace::Sent {
    let mut turn = session.begin();
    turn.fragment(
        Source::System,
        Role::System,
        prompt,
        &[("root", "S:/work"), ("tree", "src/\n  main.rs")],
    )
    .expect("recorded");
    turn.user(question).expect("recorded");
    turn.parameters("development", options()).expect("recorded");
    turn.send().expect("sent")
}

#[test]
fn the_log_rebuilds_the_request_that_was_sent() {
    let prompts = scratch("rebuild-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("rebuild", Memory::new());
    let sent = ask(&session, &prompt, "What is in this project?");

    let rebuilt = Replay::of(session.journal())
        .expect("read")
        .assembly(sent.turn)
        .expect("rebuilt");

    assert_eq!(
        rebuilt, sent.request,
        "the log rebuilt something other than what was sent"
    );
    assert!(
        rebuilt.messages[0].content.contains("S:/work"),
        "the paragraph was refilled from the wording the log stored, not copied: {}",
        rebuilt.messages[0].content
    );
}

#[test]
fn a_fragment_is_stored_as_a_hash_and_its_values_and_never_as_its_output() {
    // The difference between rebuilding an assembly and describing one. A log
    // that kept the filled text would pass the rebuild above by copying.
    let prompts = scratch("no-copy-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("no-copy", Memory::new());
    let sent = ask(&session, &prompt, "anything");
    let filled = &sent.request.messages[0].content;

    let events = session.journal().events().expect("read");
    let fragment = events
        .iter()
        .find(|event| matches!(event.body, Body::Fragment { .. }))
        .expect("a fragment");

    let line = serde_json::to_string(fragment).expect("a line");
    let wording: String = prompt.text.chars().take(40).collect();
    assert!(
        !line.contains(&wording),
        "the fragment carries the wording it was made from: {line}"
    );
    assert!(
        !line.contains(filled.as_str()),
        "the fragment carries the text it produced: {line}"
    );
    // What it does carry is the hash and the values, which is everything a
    // rebuild needs and nothing a rebuild could cheat with.
    assert!(line.contains(&prompt.hash), "the wording is named by hash");
    assert!(
        line.contains("S:/work"),
        "the values that filled it are kept"
    );
    assert!(
        filled.contains("S:/work"),
        "the assembly that was sent was filled all the same"
    );
}

#[test]
fn history_is_the_log_and_not_a_second_copy_of_it() {
    let prompts = scratch("history-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("history", Memory::new());
    let first = ask(&session, &prompt, "What is the capital of France?");
    session
        .completed(
            &first,
            "Paris.",
            "the user asked for a capital",
            FinishReason::Stop,
            Usage {
                prompt_tokens: 30,
                completion_tokens: 3,
            },
        )
        .expect("recorded");

    let history = Replay::of(session.journal()).expect("read").history();
    assert_eq!(history.len(), 2, "one question and one answer");
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[0].text, "What is the capital of France?");
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].text, "Paris.");
    assert_eq!(
        history[1].source,
        Source::Reasoning,
        "the answer is the model's, and the transcript says so"
    );

    // The system paragraph is in the log and in the assembly, and it is not
    // part of the conversation. One store, two projections.
    assert!(
        history.iter().all(|said| said.role != Role::System),
        "a system paragraph reached the transcript"
    );

    let events = session.journal().events().expect("read");
    let answers = events
        .iter()
        .filter(|event| match &event.body {
            Body::Message { text, .. } => text == "Paris.",
            Body::Completion { text, .. } => text == "Paris.",
            _ => false,
        })
        .count();
    assert_eq!(
        answers, 1,
        "the answer is in the log once; a second event saying it is the parallel table"
    );
}

#[test]
fn the_answer_goes_back_in_as_a_reference_rather_than_a_copy() {
    // The second turn carries the first turn's blocks. Nothing is written
    // twice, and the rebuild of the second turn still produces the whole
    // conversation.
    let prompts = scratch("carry-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("carry", Memory::new());
    let first = ask(&session, &prompt, "What is the capital of France?");
    session
        .completed(
            &first,
            "Paris.",
            "",
            FinishReason::Stop,
            Usage {
                prompt_tokens: 30,
                completion_tokens: 3,
            },
        )
        .expect("recorded");

    let history = Replay::of(session.journal()).expect("read").history();
    let mut second = session.begin();
    second
        .fragment(
            Source::System,
            Role::System,
            &prompt,
            &[("root", "S:/work"), ("tree", "src/\n  main.rs")],
        )
        .expect("recorded");
    for said in &history {
        second.carry(said.seq);
    }
    second.user("And of Italy?").expect("recorded");
    second
        .parameters("development", options())
        .expect("recorded");
    let sent = second.send().expect("sent");

    let replay = Replay::of(session.journal()).expect("read");
    assert_eq!(
        replay.assembly(sent.turn).expect("rebuilt"),
        sent.request,
        "the second turn rebuilds, references and all"
    );
    assert_eq!(
        sent.request.messages.len(),
        4,
        "the paragraph, the first question, the answer, the second question"
    );
    assert_eq!(sent.request.messages[2].role, Role::Assistant);
    assert_eq!(sent.request.messages[2].content, "Paris.");

    // The version of the paragraph is written once for the session, not once
    // per turn, which is what `docs/rules/prompts.md` asks for.
    let versions = session
        .journal()
        .events()
        .expect("read")
        .iter()
        .filter(|event| matches!(event.body, Body::Version { .. }))
        .count();
    assert_eq!(versions, 1, "one copy of a paragraph per session per hash");
}

#[test]
fn a_reply_from_before_an_edit_still_rebuilds_with_the_wording_that_produced_it() {
    // The reason a fragment names a hash rather than an id. Editing a paragraph
    // mid session must not rewrite what an earlier turn was sent.
    let prompts = scratch("edit-prompts");
    let register = Paragraphs::open(&prompts);
    let before = register
        .get(demido_prompts::id::CONTEXT_TREE)
        .expect("a paragraph");

    let session = Session::new("edit", Memory::new());
    let first = ask(&session, &before, "What is in this project?");

    let after = register
        .set(
            demido_prompts::id::CONTEXT_TREE,
            "The workspace is {{root}} and holds:\n{{tree}}\n",
        )
        .expect("edited");
    assert_ne!(after.hash, before.hash, "an edit is a new wording");

    let second = ask(&session, &after, "And now?");

    let replay = Replay::of(session.journal()).expect("read");
    let old = replay.assembly(first.turn).expect("rebuilt");
    let new = replay.assembly(second.turn).expect("rebuilt");

    assert_eq!(
        old, first.request,
        "the earlier turn rebuilds as it was sent"
    );
    assert_eq!(new, second.request);
    assert!(
        new.messages[0]
            .content
            .starts_with("The workspace is S:/work"),
        "the later turn used the edited wording: {}",
        new.messages[0].content
    );
    assert_ne!(
        old.messages[0].content, new.messages[0].content,
        "an edit rewrote what an earlier turn had already been sent"
    );
}

#[test]
fn a_turn_sent_with_no_parameter_set_is_refused_at_the_point_of_sending() {
    let session = Session::new("unparameterised", Memory::new());
    let mut turn = session.begin();
    turn.user("hello").expect("recorded");
    let error = turn.send().expect_err("refused");
    assert!(
        matches!(error, demido_trace::Error::Unparameterised { turn: 1 }),
        "the report names the turn: {error}"
    );
}

#[test]
fn every_event_carries_a_source_and_a_weight() {
    let prompts = scratch("weight-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("weight", Memory::new());
    let sent = ask(&session, &prompt, "What is the capital of France?");
    session
        .completed(
            &sent,
            "Paris.",
            "",
            FinishReason::Stop,
            Usage {
                prompt_tokens: 30,
                completion_tokens: 3,
            },
        )
        .expect("recorded");
    session
        .failed(sent.turn, "unavailable", "the backend went away")
        .expect("recorded");

    let replay = Replay::of(session.journal()).expect("read");

    let occupancy = replay.occupancy(sent.turn).expect("weighed");
    assert!(
        occupancy.tokens > 0,
        "a turn that sent a paragraph and a question occupied nothing"
    );
    assert_eq!(
        occupancy.basis,
        Basis::Estimated,
        "an unweighed session says its numbers are estimates"
    );

    let ledger = replay.ledger();
    assert_eq!(
        ledger[&Source::Reasoning].tokens,
        3,
        "the answer is weighed by the backend's own count"
    );
    assert_eq!(
        ledger[&Source::Reasoning].basis,
        Some(Basis::Counted),
        "a count is not reported as an estimate"
    );
    assert_eq!(ledger[&Source::User].events, 1);
    assert_eq!(ledger[&Source::Error].events, 1);
    assert!(
        !ledger.contains_key(&Source::Tool),
        "a source with nothing in it is absent rather than zero"
    );
}

#[test]
fn a_counted_session_says_so_on_every_event() {
    // The wiring line for the cost axis: one weigher, and nothing else changes.
    let prompts = scratch("counted-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::weighed_by(
        "counted",
        Memory::new(),
        demido_trace::Counting(|text: &str| text.split_whitespace().count() as u32),
    );
    let sent = ask(&session, &prompt, "What is the capital of France?");

    let occupancy = Replay::of(session.journal())
        .expect("read")
        .occupancy(sent.turn)
        .expect("weighed");
    assert_eq!(occupancy.basis, Basis::Counted);
}

#[test]
fn the_log_survives_a_restart_and_rebuilds_the_same_assembly() {
    let dir = scratch("restart");
    let prompts = dir.join("prompts");
    std::fs::create_dir_all(&prompts).expect("made the directory");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);
    let path = dir.join("session.jsonl");

    let sent = {
        let session = Session::new("restart", JsonLines::open(&path).expect("opened"));
        let sent = ask(&session, &prompt, "What is the capital of France?");
        session
            .completed(
                &sent,
                "Paris.",
                "",
                FinishReason::Stop,
                Usage {
                    prompt_tokens: 30,
                    completion_tokens: 3,
                },
            )
            .expect("recorded");
        sent
    };

    // Everything that held the log is gone. What is left is the file.
    let reopened = JsonLines::open(&path).expect("opened again");
    let replay = Replay::of(&reopened).expect("read");
    assert_eq!(
        replay.assembly(sent.turn).expect("rebuilt"),
        sent.request,
        "the assembly did not survive the log being closed and opened again"
    );
    assert_eq!(replay.history().len(), 2);

    // And it carries on where it left off rather than starting again.
    let session = Session::new("restart", reopened);
    session.resume().expect("resumed");
    assert_eq!(
        session.begin().number(),
        sent.turn + 1,
        "a resumed session numbered its next turn over the top of an old one"
    );
}

#[test]
fn a_resumed_session_does_not_write_a_paragraph_out_a_second_time() {
    // `docs/rules/prompts.md` asks for the full wording once per session per
    // hash, and a session that came back from disk is the same session. A
    // resume that restored only the turn counter would put a second copy of
    // every paragraph in the log on the first turn after every restart.
    let dir = scratch("resume");
    let prompts = dir.join("prompts");
    std::fs::create_dir_all(&prompts).expect("made the directory");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);
    let path = dir.join("session.jsonl");

    {
        let session = Session::new("resume", JsonLines::open(&path).expect("opened"));
        ask(&session, &prompt, "What is in this project?");
    }

    let session = Session::new("resume", JsonLines::open(&path).expect("opened again"));
    session.resume().expect("resumed");
    let sent = ask(&session, &prompt, "And now?");

    let replay = Replay::of(session.journal()).expect("read");
    let versions = replay
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Version { .. }))
        .count();
    assert_eq!(
        versions, 1,
        "the wording was written out again after the restart"
    );
    assert_eq!(
        replay.assembly(sent.turn).expect("rebuilt"),
        sent.request,
        "the turn after the restart still rebuilds from the version written before it"
    );
}

#[test]
fn a_carried_block_is_the_log_s_text_and_not_the_caller_s() {
    // The one way left to send an assembly the session did not record would be
    // a `carry` that took the text as well: it would look like a correct call
    // and put words in the log's mouth. It takes a position, and the position
    // is resolved from the log.
    let prompts = scratch("carry-text-prompts");
    let prompt = paragraph(&prompts, demido_prompts::id::CONTEXT_TREE);

    let session = Session::new("carry-text", Memory::new());
    let first = ask(&session, &prompt, "What is the capital of France?");
    let question = Replay::of(session.journal()).expect("read").history()[0].seq;

    let mut second = session.begin();
    second.carry(question);
    second
        .parameters("development", options())
        .expect("recorded");
    let sent = second.send().expect("sent");

    assert_eq!(
        sent.request.messages[0].content, "What is the capital of France?",
        "the carried block came out of the log"
    );
    assert_eq!(first.turn + 1, sent.turn);

    // And a position nothing wrote is refused rather than silently dropped.
    let mut third = session.begin();
    third.carry(9_999);
    third
        .parameters("development", options())
        .expect("recorded");
    let error = third.send().expect_err("refused");
    assert!(
        matches!(error, demido_trace::Error::Dangling { seq: 9_999 }),
        "the report names the block: {error}"
    );
}

/// The log a live run left behind, replayed by a different process on a
/// different day.
///
/// `docs/rules/done.md` asks a closing comment for a trace fixture and says
/// what it is for: "a pruned, deterministic trace is exactly a replayable
/// fixture. It is committed beside the scenario that produced it and becomes
/// that scenario's input." This is the offline suite reading a real session
/// off disk, which is the restart promise across a boundary no test harness
/// can fake.
#[test]
fn the_committed_trace_of_a_live_run_still_rebuilds_what_was_sent() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/a-model-answers-in-a-terminal.jsonl");
    // Read rather than opened. `JsonLines::open` creates the file and cuts a
    // torn last line off it, which is right for a log about to be written to
    // and wrong for one that is committed evidence.
    let replay = Replay::over(JsonLines::read(&path).expect("read the fixture"));

    assert!(
        !replay.is_empty(),
        "the fixture at {} is empty; the live suite writes it",
        path.display()
    );

    let sent: Request = serde_json::from_str(
        &std::fs::read_to_string(path.with_extension("sent.json")).expect("the request beside it"),
    )
    .expect("a request");

    let turn = *replay.turns().first().expect("a turn was sent");
    assert_eq!(
        replay.assembly(turn).expect("rebuilt"),
        sent,
        "the committed log no longer rebuilds the request the live run sent with it"
    );

    let history = replay.history();
    assert_eq!(history.len(), 2, "a question and an answer");
    assert!(
        history[1].text.to_lowercase().contains("paris"),
        "the answer in the fixture: {}",
        history[1].text
    );
    assert_eq!(
        replay.occupancy(turn).expect("weighed").basis,
        Basis::Counted,
        "the live run weighed its events with the model's own tokeniser"
    );
}
