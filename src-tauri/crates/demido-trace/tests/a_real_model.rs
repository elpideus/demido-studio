//! The live-model suite for the session log: a real model answers, and the log
//! it left behind rebuilds what the backend was given.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md) for
//! [#41](https://github.com/elpideus/demido-studio/issues/41). It runs with no
//! window and nobody at the keyboard, it holds one model resident by the same
//! process-wide permit the inference suite uses, and it is re-run every slice.
//!
//! The assertion that matters is not that two Rust values are equal. It is that
//! the text `llama.cpp` builds out of the rebuilt assembly is character for
//! character the text it built out of the one that was sent: the log is fed
//! back through the backend's own chat template, which is the last thing that
//! touches a prompt before the model sees it. A rebuild that dropped a
//! paragraph, reordered two blocks or filled a placeholder differently comes
//! back as a different string from the server, not as a passing test.
//!
//! **The string it is compared against is taken before the turn runs**, from
//! the value that is then handed to `Backend::generate`. Rendering both sides
//! at the end, after asserting the two requests equal, would be a comparison
//! that cannot fail on its own, and it would prove the endpoint is a function
//! rather than proving anything about the log.
//!
//! It also writes the fixture `done.md` asks a closing comment for, so the
//! offline suite replays a real session rather than one this repo made up.
//!
//! **One tier, on purpose.** The three tier ladder exists for claims about what
//! a model *does*, and every one of those lives in `demido-inference`. What is
//! asserted here is that the log rebuilds what was sent, which is a claim about
//! Demido: a reference or breadth run would exercise the same code with a
//! slower model and prove nothing extra. The one thing the model is asked for
//! is an answer, and that it can answer is already the inference suite's
//! scenario on all three.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

// The rig is the inference crate's, included rather than copied. Two copies of
// where the models are is two places for a path to go stale, and the rig is
// exactly the sort of thing that is edited once and forgotten in the other
// file.
#[path = "../../demido-inference/tests/rig.rs"]
mod rig;

use std::path::PathBuf;

use futures_util::StreamExt;

use demido_inference::{
    Backend, Cancel, Chunk, FinishReason, LlamaCpp, Options, Request, Role, Supervisor, Usage,
};
use demido_prompts::Paragraphs;
use demido_trace::{Basis, Body, Counting, JsonLines, Replay, Session, Source};

use rig::Tier;

/// The tree the fixture is written into, so a live run leaves the offline suite
/// its input.
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A scratch directory for the log this run writes.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-trace-live")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made the directory");
    dir
}

/// Ask the running server to count tokens.
///
/// This is the counted half of the cost axis, and the reason
/// [`demido_trace::Weigher`] is a seam rather than a function: the tokeniser
/// that matters is the one inside the model that is loaded, and it is one HTTP
/// call away for as long as that model is loaded.
async fn count(port: u16, text: &str) -> u32 {
    #[derive(serde::Deserialize)]
    struct Tokens {
        tokens: Vec<i64>,
    }

    let counted: Tokens = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/tokenize"))
        .json(&serde_json::json!({ "content": text }))
        .send()
        .await
        .expect("the tokeniser answered")
        .json()
        .await
        .expect("a token list");
    counted.tokens.len() as u32
}

/// What the model will actually be shown, built by the backend's own chat
/// template.
///
/// `llama.cpp` exposes this precisely so a caller can see the string it would
/// generate from. It is the closest thing there is to "what the backend
/// received", short of reading the process's memory.
async fn as_the_model_sees_it(port: u16, request: &Request) -> String {
    #[derive(serde::Deserialize)]
    struct Applied {
        prompt: String,
    }

    let messages: Vec<serde_json::Value> = request
        .messages
        .iter()
        .map(|message| {
            serde_json::json!({
                "role": match message.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                "content": message.content,
            })
        })
        .collect();

    let applied: Applied = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/apply-template"))
        .json(&serde_json::json!({ "messages": messages }))
        .send()
        .await
        .expect("the template endpoint answered")
        .json()
        .await
        .expect("a formatted prompt");
    applied.prompt
}

/// The whole claim of this ticket, against a model that is really answering.
///
/// One turn is composed and recorded, sent, and answered. The log is then
/// closed, opened again from the file, and asked to produce the request: what
/// comes back has to be the request that went out, and the string the server
/// builds from it has to be the string the server built from the original.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_log_of_a_live_turn_rebuilds_the_assembly_that_was_sent() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let supervisor = Supervisor::<LlamaCpp>::new();
    let backend = match supervisor.ensure(rig::require(tier)).await {
        Ok(backend) => backend,
        Err(error) => panic!("the {} model did not start: {error}", tier.label()),
    };
    let port = backend.port();

    let dir = scratch("rebuild");
    let path = dir.join("session.jsonl");
    let register = Paragraphs::open(dir.join("prompts"));
    let reply = register
        .get(demido_prompts::id::CAVEMAN_LITE)
        .expect("a paragraph");
    let reasoning = register
        .get(demido_prompts::id::CAVEMAN_ULTRA)
        .expect("a paragraph");

    // Every event weighed by the model's own tokeniser. `block_in_place` is
    // what lets a synchronous weigher make an asynchronous call on a
    // multi-threaded runtime, and the weigher is synchronous because recording
    // must never be a reason for a function to change colour
    // (`demido_trace::journal`).
    let handle = tokio::runtime::Handle::current();
    let session = Session::weighed_by(
        "a-model-answers-in-a-terminal",
        JsonLines::open(&path).expect("opened the log"),
        Counting(move |text: &str| {
            let text = text.to_owned();
            let handle = handle.clone();
            tokio::task::block_in_place(move || handle.block_on(count(port, &text)))
        }),
    );

    let mut turn = session.begin();
    turn.fragment(
        Source::System,
        Role::System,
        &reply,
        &[(demido_prompts::catalog::TARGET, "your reply")],
    )
    .expect("recorded");
    turn.fragment(
        Source::System,
        Role::System,
        &reasoning,
        &[(demido_prompts::catalog::TARGET, "your reasoning")],
    )
    .expect("recorded");
    turn.user("What is the capital of France?")
        .expect("recorded");
    turn.parameters(
        tier.label(),
        Options {
            temperature: Some(0.0),
            // Enough for a model that thinks first, as the inference suite
            // found: a smaller budget produces a well-formed stream with no
            // answer in it, which measures the budget rather than the model.
            max_tokens: Some(768),
            seed: Some(1),
        },
    )
    .expect("recorded");
    let sent = turn.send().expect("sent");

    // **Taken before anything is generated, and before the log is read back.**
    // This is the server's own rendering of the exact value that is about to be
    // handed to `Backend::generate`, and it is what the rebuild is compared
    // against at the end. Taking it afterwards, beside the rebuilt one, would
    // be two renderings of two values already asserted equal: a comparison that
    // cannot fail on its own.
    let as_sent = as_the_model_sees_it(port, &sent.request).await;

    let chunks: Vec<_> = backend
        .generate(sent.request.clone(), Cancel::new())
        .await
        .expect("generate")
        .collect()
        .await;
    demido_inference::contract::assert_well_formed(&chunks);

    let (text, thinking, reason, usage) = finished(&chunks);
    session
        .completed(&sent, &text, &thinking, reason, usage)
        .expect("recorded");

    assert!(
        text.to_lowercase().contains("paris"),
        "the {} model was asked the capital of France and said: {text}",
        tier.label()
    );

    // Everything holding the log is dropped. What is left is the file, which is
    // the only thing a restart has.
    drop(session);
    let reopened = JsonLines::open(&path).expect("opened the log again");
    let replay = Replay::of(&reopened).expect("read the log");
    let rebuilt = replay.assembly(sent.turn).expect("rebuilt");

    // The assertion this ticket is about, and it is deliberately made against
    // the string taken before the turn ran: what the backend builds out of the
    // log has to be, character for character, what it built out of what it was
    // given.
    assert_eq!(
        as_the_model_sees_it(port, &rebuilt).await,
        as_sent,
        "the backend builds a different prompt out of the rebuilt assembly than \
         out of the one it was sent"
    );
    assert_eq!(
        rebuilt, sent.request,
        "the log rebuilt a different request from the one the backend was given"
    );

    // The transcript, out of the same events and no others.
    let history = replay.history();
    assert_eq!(history.len(), 2, "a question and an answer");
    assert_eq!(history[0].text, "What is the capital of France?");
    assert_eq!(history[1].text, text);

    // The cost axis, counted rather than estimated, and the counts that back it
    // are the model's own.
    let occupancy = replay.occupancy(sent.turn).expect("weighed");
    assert_eq!(
        occupancy.basis,
        Basis::Counted,
        "a session weighed by the model's tokeniser reported estimates"
    );
    assert!(
        occupancy.tokens > 0 && occupancy.tokens <= usage.prompt_tokens,
        "the blocks weigh {} against a prompt the backend counted at {}; the \
         difference is the chat template's own framing and can never be \
         negative",
        occupancy.tokens,
        usage.prompt_tokens
    );
    let ledger = replay.ledger();
    assert_eq!(ledger[&Source::Reasoning].tokens, usage.completion_tokens);
    assert!(
        ledger[&Source::User].tokens > 0,
        "the question weighed nothing"
    );
    assert!(
        ledger[&Source::System].tokens > 0,
        "the paragraphs weighed nothing"
    );

    keep(&path, &sent.request);

    supervisor.shutdown().await;
}

/// A failure is an event like anything else.
///
/// The backend is asked for a model it is not serving, which is the one refusal
/// a supervised server can be made to produce on demand. What the log has to
/// show afterwards is the turn, the assembly that was attempted, and the
/// failure against it: a session whose errors are only in a log file somewhere
/// cannot explain itself.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_refusal_is_an_event_on_the_same_log() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let supervisor = Supervisor::<LlamaCpp>::new();
    let backend = supervisor
        .ensure(rig::require(tier))
        .await
        .expect("the model started");

    let dir = scratch("refusal");
    let session = Session::new(
        "a-refusal-is-an-event",
        JsonLines::open(dir.join("session.jsonl")).expect("opened the log"),
    );

    let mut turn = session.begin();
    turn.user("What is the capital of France?")
        .expect("recorded");
    turn.parameters("a-model-this-server-is-not-serving", Options::default())
        .expect("recorded");
    let sent = turn.send().expect("sent");

    let error = match backend.generate(sent.request.clone(), Cancel::new()).await {
        Err(error) => error,
        Ok(_) => panic!("a server serving one model answered to another name"),
    };
    session
        .failed(sent.turn, "refused", &error.to_string())
        .expect("recorded");

    let replay = Replay::of(session.journal()).expect("read the log");
    let failure = replay
        .events()
        .iter()
        .find(|event| matches!(event.body, Body::Failure { .. }))
        .expect("a failure is on the log");
    assert_eq!(failure.source, Source::Error);
    assert_eq!(failure.turn, sent.turn);
    assert_eq!(
        replay.assembly(sent.turn).expect("rebuilt"),
        sent.request,
        "a turn that failed still rebuilds what it tried to send"
    );

    supervisor.shutdown().await;
}

/// Everything a finished stream carries, in the shape the log wants it.
fn finished(chunks: &[demido_inference::Result<Chunk>]) -> (String, String, FinishReason, Usage) {
    let mut text = String::new();
    let mut thinking = String::new();

    for chunk in chunks.iter().flatten() {
        match chunk {
            Chunk::Text { text: said } => text.push_str(said),
            Chunk::Thinking { text: thought } => thinking.push_str(thought),
            Chunk::Done { .. } => {}
        }
    }

    let Some(Ok(Chunk::Done { reason, usage })) = chunks.last() else {
        panic!("the stream did not end with a done chunk")
    };
    (text, thinking, *reason, *usage)
}

/// Commit the trace beside the scenario that produced it.
///
/// `done.md`: "Evidence with a second job is evidence worth keeping". The log
/// and the request it produced go into `tests/fixtures/`, where the offline
/// suite reads them back in a later process on a later day.
fn keep(path: &PathBuf, request: &Request) {
    let fixtures = fixtures();
    std::fs::create_dir_all(&fixtures).expect("made the fixtures directory");

    let log = fixtures.join("a-model-answers-in-a-terminal.jsonl");
    std::fs::copy(path, &log).expect("kept the log");
    std::fs::write(
        fixtures.join("a-model-answers-in-a-terminal.sent.json"),
        serde_json::to_string_pretty(request).expect("a request"),
    )
    .expect("kept the request");

    println!("trace fixture: {}", log.display());
}
