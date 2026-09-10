//! The live-model suite: a real small model, answering, from a terminal.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md). It runs with no
//! window and nobody at the keyboard, it holds one model resident by a
//! process-wide permit, and it is **re-run every slice** thereafter.
//!
//! The three tiers are not interchangeable and this file treats them by role:
//! development on every iteration, reference before a slice closes, breadth
//! once per slice. A red on breadth alone is a note rather than a defect, so it
//! is a separate case that names itself as such rather than a parameter of the
//! others.
//!
//! Nothing here asserts on what a model *says* beyond the least a working turn
//! implies. An assertion about wording is an assertion about one model, and the
//! point of three tiers is that there are three.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

mod rig;

use futures_util::StreamExt;

use demido_inference::{Backend, Cancel, Chunk, LlamaCpp, Message, Options, Request, Supervisor};

use rig::Tier;

/// S1's whole claim, one tier at a time: a message goes in, an answer comes
/// back, and the stream says it is over exactly once.
async fn it_answers(tier: Tier) {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let config = rig::require(tier);
    let supervisor = Supervisor::<LlamaCpp>::new();
    let backend = match supervisor.ensure(config).await {
        Ok(backend) => backend,
        Err(error) => panic!("the {} model did not start: {error}", tier.label()),
    };

    let request = Request {
        model: tier.label().to_owned(),
        messages: vec![
            Message::system("You are terse. Answer in one short sentence."),
            Message::user("What is the capital of France?"),
        ],
        options: Options {
            temperature: Some(0.0),
            // Enough for a model that thinks first. The reference model spends
            // its first hundreds of tokens in `reasoning_content`, so a budget
            // of 64 produced a well-formed stream with no answer in it: a test
            // that measured the budget rather than the model.
            max_tokens: Some(768),
            seed: Some(1),
        },
    };

    let chunks: Vec<_> = backend
        .generate(request, Cancel::new())
        .await
        .expect("generate")
        .collect()
        .await;

    contract_shape(&chunks, tier);

    let answer = text(&chunks);
    assert!(
        !answer.trim().is_empty(),
        "the {} model produced a well-formed stream with nothing in it",
        tier.label()
    );
    assert!(
        answer.to_lowercase().contains("paris"),
        "the {} model was asked the capital of France and said: {answer}",
        tier.label()
    );

    // The counts the cost axis is built on. They arrive only because the wire
    // asks for them, so a backend that stopped asking would show up here rather
    // than as an empty column in a window nobody has built yet.
    let Some(Ok(Chunk::Done { usage, .. })) = chunks.last() else {
        unreachable!("the shape was just checked")
    };
    assert!(
        usage.prompt_tokens > 0 && usage.completion_tokens > 0,
        "a turn with no token counts leaves the session log unable to weigh an \
         event: {usage:?}"
    );

    supervisor.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_development_model_answers() {
    it_answers(Tier::Development).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_reference_model_answers() {
    it_answers(Tier::Reference).await;
}

/// A red here alone is a **note, not a defect** (`done.md`). IQ2_M on a dense
/// 27B is roughly 2.9 bits, and the first thing that degrades there is the
/// thing being measured. It is still surprising enough to look at twice: on #19
/// all four models on this rig passed every probe.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_breadth_model_answers() {
    it_answers(Tier::Breadth).await;
}

/// A person pressed stop. The partial answer is kept, the stream ends, and the
/// model is still there to answer the next thing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_stopped_generation_keeps_what_was_said_and_the_model_stays_loaded() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let supervisor = Supervisor::<LlamaCpp>::new();
    let backend = supervisor
        .ensure(rig::require(Tier::Development))
        .await
        .expect("started");

    let long = Request {
        model: Tier::Development.label().to_owned(),
        messages: vec![Message::user("Count slowly from one to two hundred.")],
        options: Options {
            temperature: Some(0.0),
            max_tokens: Some(2048),
            seed: Some(1),
        },
    };

    let cancel = Cancel::new();
    let mut stream = backend
        .generate(long, cancel.clone())
        .await
        .expect("generate");

    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        let generated = matches!(chunk, Ok(Chunk::Text { .. }) | Ok(Chunk::Thinking { .. }));
        chunks.push(chunk);
        if generated {
            break;
        }
    }
    cancel.cancel();
    while let Some(chunk) = stream.next().await {
        chunks.push(chunk);
    }
    drop(stream);

    assert!(
        !generated(&chunks).trim().is_empty(),
        "what was on screen when the user pressed stop is part of what happened"
    );
    assert!(
        matches!(
            chunks.last(),
            Some(Ok(Chunk::Done {
                reason: demido_inference::FinishReason::Cancelled,
                ..
            }))
        ),
        "the log records a stop rather than a completed answer: {:?}",
        chunks.last()
    );

    // Still the same server, still loaded: the second ask must not reload the
    // weights, which is the supervisor's whole reason for existing.
    let again = supervisor
        .ensure(rig::require(Tier::Development))
        .await
        .expect("still running");
    assert!(
        std::sync::Arc::ptr_eq(&backend, &again),
        "pressing stop unloaded the model"
    );

    supervisor.shutdown().await;
}

/// The number the user is shown is the number the slot gets.
///
/// `--ctx-size` is per slot on the pinned build, so this is the live half of
/// the contract case: four slots asking for 4096 each is 16k of KV, and a
/// window a quarter the size claimed is the same defect seen from the user's
/// end.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_context_the_user_asked_for_is_the_context_the_slot_has() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let mut config = rig::require(Tier::Development);
    config.context_length = 3072;
    // More than one slot, because with one slot every arithmetic agrees and the
    // case proves nothing. This is the case that caught `--ctx-size` being the
    // whole pool rather than one slot's share.
    config.parallel = 2;

    let backend = LlamaCpp::start(config).await.expect("started");
    let got = backend.context_length().await.expect("the slot's context");
    backend.stop().await;

    assert_eq!(
        got, 3072,
        "asked for 3072 across 2 slots and the slot reports {got}"
    );
}

fn contract_shape(chunks: &[demido_inference::Result<Chunk>], tier: Tier) {
    for chunk in chunks {
        if let Err(error) = chunk {
            panic!(
                "the {} model's stream carried an error: {error}",
                tier.label()
            );
        }
    }
    demido_inference::contract::assert_well_formed(chunks);
}

/// The visible answer.
fn text(chunks: &[demido_inference::Result<Chunk>]) -> String {
    chunks
        .iter()
        .flatten()
        .filter_map(|chunk| match chunk {
            Chunk::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Everything the model produced, reasoning included.
///
/// What a cancel is asked about, rather than [`text`]: a model that thinks
/// first is still generating, the transcript shows it, and a stop pressed
/// during the thinking stage has just as much to keep.
fn generated(chunks: &[demido_inference::Result<Chunk>]) -> String {
    chunks
        .iter()
        .flatten()
        .filter_map(|chunk| match chunk {
            Chunk::Text { text } | Chunk::Thinking { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
