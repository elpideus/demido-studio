//! The live-model suite for the turn loop: a person types a message, and a real
//! model answers.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md) for
//! [#43](https://github.com/elpideus/demido-studio/issues/43). It runs from a
//! terminal, with no window and nobody at the keyboard, it holds one model
//! resident by the same process-wide permit the other live suites use, and it
//! is re-run every slice.
//!
//! **Two scenarios run on all three tiers**, because both are claims about what
//! a model *does* rather than about what Demido records. That a question gets
//! an answer is the product's own sentence. That the second question is
//! answered from the first exchange is the one the assembly has to earn: a
//! model that never sees the earlier turn answers "What is my name?" perfectly
//! confidently and wrongly, and nothing in an offline suite can tell the
//! difference between an assembly that carried history and one that looked as
//! though it did.
//!
//! The stop runs on all three as well, and it was written on one until a review
//! pointed out that cancellation reaching a `llama.cpp` slot mid-generation is
//! exactly the sort of thing that behaves differently on a model producing
//! twenty tokens a second and one producing eighty.
//!
//! The restart runs on the development tier alone, and that is not the same
//! allowance: it asserts nothing about a model. It closes a chat, opens it
//! again over the file, and compares the transcript against what was said. A
//! second tier would exercise the same lines of Demido with slower weights
//! behind them.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

// The rig is the inference crate's, included rather than copied. Two copies of
// where the models are is two places for a path to go stale.
#[path = "../../demido-inference/tests/rig.rs"]
mod rig;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_chat::{Chat, Model, Presence, Update};
use demido_inference::{FinishReason, LlamaCpp, Role, Supervisor};
use demido_trace::{Body, JsonLines, SessionId};

use rig::Tier;

/// A scratch directory for the log a run writes.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-chat-live")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made the directory");
    dir
}

/// A chat against a real model, logging to a real file.
fn chat(tier: Tier, dir: &std::path::Path) -> Chat<LlamaCpp, JsonLines> {
    let path = dir.join("session.jsonl");
    Chat::new(
        SessionId::new(format!("a-model-answers-{}", tier.label())),
        move || JsonLines::open(&path),
        Arc::new(Supervisor::new()),
        Some(Model {
            config: rig::require(tier),
            id: tier.label().to_owned(),
        }),
    )
}

/// Load, or say which tier failed and what the backend said about it.
async fn loaded(chat: &Chat<LlamaCpp, JsonLines>, tier: Tier) {
    match chat.load(|_| {}).await {
        Presence::Ready { .. } => {}
        other => panic!("the {} model did not load: {other:?}", tier.label()),
    }
}

/// Every update a turn produced, in order. The window is drawn from these, so
/// the suite watches what the window would have been shown.
#[derive(Default, Clone)]
struct Watched(Arc<Mutex<Vec<Update>>>);

impl Watched {
    fn sink(&self) -> impl FnMut(Update) + Send {
        let seen = self.0.clone();
        move |update| {
            if let Ok(mut seen) = seen.lock() {
                seen.push(update);
            }
        }
    }

    fn tokens(&self) -> usize {
        self.0
            .lock()
            .map(|seen| {
                seen.iter()
                    .filter(|update| matches!(update, Update::Text { .. }))
                    .count()
            })
            .unwrap_or_default()
    }
}

/// The thing the product is for, on every tier.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_message_gets_an_answer_that_arrives_as_it_is_generated() {
    for tier in Tier::ALL {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

        let dir = scratch(&format!("answers-{}", tier.label()));
        let chat = chat(tier, &dir);
        loaded(&chat, tier).await;

        let watched = Watched::default();
        let answer = chat
            .ask("What is the capital of France?", watched.sink())
            .await
            .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

        assert!(
            answer.text.to_lowercase().contains("paris"),
            "the {} model was asked the capital of France and said: {}",
            tier.label(),
            answer.text
        );
        assert_eq!(answer.reason, FinishReason::Stop);
        assert!(
            watched.tokens() > 1,
            "the {} model's answer arrived in one piece, so nothing streamed: a \
             bubble filling as it is generated is the difference between \
             progress and a spinner",
            tier.label()
        );

        // The transcript is the log read back, and it is the only record.
        let history = chat.history().expect("a transcript");
        assert_eq!(history.len(), 2, "a question and an answer");
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[1].text, answer.text);

        chat.shutdown().await;
    }
}

/// A conversation rather than a series of first questions, on every tier.
///
/// The name is chosen rather than the country's capital on purpose: a model
/// that never saw the first exchange can still answer "what is the capital of
/// France" from its weights, so that question would pass whether or not the
/// assembly carried anything.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_second_message_carries_the_first_exchange() {
    for tier in Tier::ALL {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

        let dir = scratch(&format!("carries-{}", tier.label()));
        let chat = chat(tier, &dir);
        loaded(&chat, tier).await;

        chat.ask(
            "My name is Ada Lovelace. Reply with just the word: noted.",
            |_| {},
        )
        .await
        .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

        let answer = chat
            .ask("What is my name? Answer with the name alone.", |_| {})
            .await
            .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

        assert!(
            answer.text.to_lowercase().contains("ada"),
            "the {} model was told a name and then asked for it, and said: {}",
            tier.label(),
            answer.text
        );

        // What was sent is what the log says was sent, for the turn that had
        // something to carry.
        let replay = chat.replay().expect("read the log");
        let assembly = replay.assembly(2).expect("rebuilt");
        assert_eq!(
            assembly.messages.len(),
            3,
            "the second turn carries the question, the answer, and the new \
             question, and the {} model saw {:?}",
            tier.label(),
            assembly.messages
        );

        chat.shutdown().await;
    }
}

/// A person presses stop. What was on screen is real output, and the log has to
/// say that is what happened.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_stop_records_the_partial_answer_and_the_stop() {
    for tier in Tier::ALL {
        stopping(tier).await;
    }
}

async fn stopping(tier: Tier) {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let dir = scratch(&format!("stopped-{}", tier.label()));
    let chat = Arc::new(chat(tier, &dir));
    loaded(&chat, tier).await;

    let watched = Watched::default();
    let asking = {
        let chat = chat.clone();
        let sink = watched.sink();
        tokio::spawn(async move {
            chat.ask("Count slowly from one to two hundred.", sink)
                .await
        })
    };

    // Stopped after the first real token rather than after a fixed delay, so
    // this is a generation in flight rather than a race with prompt
    // processing. A wall-clock wait measures how long the model took to start
    // talking, which is the one thing this scenario is not about, and on a cold
    // load it is the difference between a stop and a cancel that arrived first.
    let waiting = std::time::Instant::now();
    while watched.tokens() == 0 {
        assert!(
            waiting.elapsed() < Duration::from_secs(120),
            "the {} model generated nothing to stop",
            tier.label()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(chat.stop(), "there was a generation to stop");

    let answer = asking.await.expect("joined").expect("a stopped answer");
    assert_eq!(
        answer.reason,
        FinishReason::Cancelled,
        "the {} model's stop landed as a finished answer, which is a stop the          log cannot report",
        tier.label()
    );
    assert!(
        !answer.text.is_empty(),
        "the {} model's stop discarded what was generated before it, which is          real output and is kept",
        tier.label()
    );

    let replay = chat.replay().expect("read the log");
    let completion = replay
        .events()
        .iter()
        .find_map(|event| match &event.body {
            Body::Completion { text, reason, .. } => Some((text.clone(), *reason)),
            _ => None,
        })
        .expect("the stopped turn is on the log");
    assert_eq!(completion.1, FinishReason::Cancelled);
    assert_eq!(completion.0, answer.text);

    // A stop is not an unload, which is what makes it a stop button rather than
    // a restart button.
    assert!(chat.presence().is_ready());
    let next = chat
        .ask("What is the capital of France? Answer in one word.", |_| {})
        .await
        .expect("the message after a stop is answered normally");
    assert_eq!(next.reason, FinishReason::Stop);

    chat.shutdown().await;
}

/// Closing the app and opening it again is the same read as opening it for the
/// first time. Nothing is restored, because nothing was kept anywhere else.
///
/// It also writes the fixture `done.md` asks a closing comment for, so the
/// offline suites replay a real conversation rather than one this repo made up.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_chat_is_still_there_after_the_process_that_held_it_is_gone() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let dir = scratch("restarted");
    let path = dir.join("session.jsonl");

    let said = {
        let chat = chat(tier, &dir);
        loaded(&chat, tier).await;
        let answer = chat
            .ask("What is the capital of France? Answer in one word.", |_| {})
            .await
            .expect("an answer");
        chat.shutdown().await;
        answer.text
    };

    // Everything holding the log is dropped. What is left is the file, which is
    // the only thing a restart has.
    let reopened = chat(tier, &dir);
    let history = reopened.history().expect("a transcript");
    assert_eq!(history.len(), 2, "the chat is still there after a restart");
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[1].text, said);

    keep(&path);
}

/// Commit the trace beside the scenario that produced it.
///
/// `done.md`: "Evidence with a second job is evidence worth keeping".
fn keep(path: &PathBuf) {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixtures).expect("made the fixtures directory");
    let log = fixtures.join("a-message-and-an-answer.jsonl");
    std::fs::copy(path, &log).expect("kept the log");
    println!("trace fixture: {}", log.display());
}
