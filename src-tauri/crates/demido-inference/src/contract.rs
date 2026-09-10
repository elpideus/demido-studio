//! The suite every [`Backend`] must pass.
//!
//! This is the definition of the seam. Everything above it is written against
//! these guarantees, so a backend that breaks one breaks its caller in a way
//! that is very hard to diagnose from the symptom: a turn that never ends, a
//! stop button that leaves a model in VRAM, a context window a quarter the size
//! the user asked for.
//!
//! It exists before the second implementation does, which is
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md)'s rule and the only
//! thing that makes "swappable" true rather than aspirational.
//!
//! Add a case when you find something a caller depends on. Never add one that
//! describes how a particular backend happens to behave.

// This module ships in the library rather than under `cfg(test)`, because every
// implementation's own test file calls it. It is still a test: it asserts by
// panicking, and the workspace's denial of that is about application code,
// where a panic is a window that vanishes.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::time::{Duration, Instant};

use futures_util::StreamExt;

use crate::backend::{Backend, Cancel, Result};
use crate::model::{Chunk, FinishReason, Message, Options, Request};

/// How long a cancelled stream may take to end.
///
/// Generous, because it covers a network round trip and a server finishing the
/// token it was already producing. Anything beyond this is not slowness, it is
/// a cancel that did not reach the generation.
const CANCEL_DEADLINE: Duration = Duration::from_secs(10);

/// A request any backend should be able to answer.
///
/// Seeded and cold, so that asking twice asks the same question. The suite
/// never asserts on what a model says, only on the shape of the stream around
/// it: a contract that depended on the words would be a contract about one
/// model.
pub fn simple_request(model: &str) -> Request {
    Request {
        model: model.to_owned(),
        messages: vec![
            Message::system("You are terse."),
            Message::user("Say hello."),
        ],
        options: Options {
            temperature: Some(0.0),
            max_tokens: Some(32),
            seed: Some(1),
        },
    }
}

/// Run every case against a backend started from `config`.
///
/// Takes the configuration rather than a started backend, because starting and
/// stopping are half of what this trait promises and a suite handed a live one
/// could not test either.
pub async fn run<B: Backend>(config: B::Config, model: &str) {
    // Through the trait's own writer rather than by editing a field, so the
    // case below measures the path the settings ladder actually takes: a number
    // resolved from the ladder, handed to `with_context_length`, and read back
    // off the running server.
    let config = B::with_context_length(config, CONTEXT);

    it_names_itself::<B>();
    starting_gives_a_backend_that_is_ready::<B>(config.clone()).await;
    it_serves_the_model_it_was_started_with::<B>(config.clone(), model).await;
    the_context_length_asked_for_is_the_one_the_slot_gets::<B>(config.clone()).await;
    a_stream_ends_with_exactly_one_done::<B>(config.clone(), model).await;
    nothing_follows_done::<B>(config.clone(), model).await;
    a_cancel_ends_the_stream_and_keeps_what_was_generated::<B>(config.clone(), model).await;
    a_cancelled_generation_does_not_end_the_backend::<B>(config.clone(), model).await;
    an_unknown_model_is_refused::<B>(config.clone()).await;
    stopping_makes_it_not_ready::<B>(config).await;
}

/// The context length every case that is not about context asks for.
///
/// Not a round 4096: a backend that quietly substitutes its own default would
/// pass against a number that happens to be the default.
pub const CONTEXT: u32 = 3072;

fn it_names_itself<B: Backend>() {
    assert!(
        !B::name().is_empty(),
        "the name appears in errors the user reads, so it cannot be blank"
    );
}

async fn starting_gives_a_backend_that_is_ready<B: Backend>(config: B::Config) {
    let backend = start::<B>(config).await;
    assert!(
        backend.ready().await,
        "start returns only once the backend is answering, so a caller holding \
         one has a backend that works"
    );
    backend.stop().await;
}

async fn it_serves_the_model_it_was_started_with<B: Backend>(config: B::Config, model: &str) {
    let backend = start::<B>(config).await;
    let loaded = backend.loaded().await.expect("what is loaded");
    assert_eq!(
        loaded.id, model,
        "a caller puts this id in Request::model, so a backend that reports \
         something else has made every request it can answer unbuildable"
    );
    backend.stop().await;
}

/// The one case that is not about stream shape, and the reason both context
/// methods are on the trait at all. The number a user sets in the settings
/// ladder reaches a process through
/// [`Backend::with_context_length`](crate::Backend::with_context_length), and
/// this is where it is checked against what the server says it got. See
/// [`Backend::context_length`].
async fn the_context_length_asked_for_is_the_one_the_slot_gets<B: Backend>(config: B::Config) {
    let backend = start::<B>(config).await;
    let got = backend.context_length().await.expect("the slot's context");
    backend.stop().await;

    assert_eq!(
        got, CONTEXT,
        "the number the user was shown has to be the number they get. \
         llama.cpp's --ctx-size is per slot, so a backend that divides it by \
         its slot count silently hands back a quarter of the window"
    );
}

async fn a_stream_ends_with_exactly_one_done<B: Backend>(config: B::Config, model: &str) {
    let backend = start::<B>(config).await;
    let chunks = collect::<B>(&backend, simple_request(model), Cancel::new()).await;
    backend.stop().await;

    let done = chunks
        .iter()
        .filter(|chunk| matches!(chunk, Ok(Chunk::Done { .. })))
        .count();
    assert_eq!(
        done, 1,
        "a turn is over when Done arrives, so callers must not have to count \
         anything else"
    );
}

async fn nothing_follows_done<B: Backend>(config: B::Config, model: &str) {
    let backend = start::<B>(config).await;
    let chunks = collect::<B>(&backend, simple_request(model), Cancel::new()).await;
    backend.stop().await;

    let at = chunks
        .iter()
        .position(|chunk| matches!(chunk, Ok(Chunk::Done { .. })))
        .expect("a Done chunk");
    assert_eq!(
        at,
        chunks.len() - 1,
        "Done is the last chunk, or a caller that stops reading at it loses output"
    );
}

/// Cancelling is a person pressing stop, so two things are asserted: the stream
/// ends soon, and what was already on screen is still in the stream.
async fn a_cancel_ends_the_stream_and_keeps_what_was_generated<B: Backend>(
    config: B::Config,
    model: &str,
) {
    let backend = start::<B>(config).await;

    let mut request = simple_request(model);
    // not-a-prompt: the suite's own probe, sent by a test and never by a turn.
    request.messages = vec![Message::user("Count slowly from one to two hundred.")];
    request.options.max_tokens = Some(2048);

    let cancel = Cancel::new();
    let mut stream = backend
        .generate(request, cancel.clone())
        .await
        .expect("generate");

    // Cancelled after the first real output, so this is a generation in flight
    // rather than one that had not started.
    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        let text = matches!(chunk, Ok(Chunk::Text { .. }) | Ok(Chunk::Thinking { .. }));
        chunks.push(chunk);
        if text {
            break;
        }
    }
    assert!(
        !chunks.is_empty(),
        "nothing was generated, so this case tested nothing"
    );

    let at = Instant::now();
    cancel.cancel();
    while let Some(chunk) = stream.next().await {
        chunks.push(chunk);
        assert!(
            at.elapsed() < CANCEL_DEADLINE,
            "a cancelled stream that keeps producing is a stop button that does nothing"
        );
    }
    backend.stop().await;

    assert!(
        chunks
            .iter()
            .any(|chunk| matches!(chunk, Ok(Chunk::Text { .. }) | Ok(Chunk::Thinking { .. }))),
        "what was generated before the cancel is real output and is kept: a \
         transcript that discards it does not match what happened"
    );
    match chunks.last() {
        Some(Ok(Chunk::Done {
            reason: FinishReason::Cancelled,
            ..
        })) => {}
        other => panic!(
            "a cancelled stream still ends with one Done, saying it was cancelled, \
             so the log records a stop rather than a completed answer. Got {other:?}"
        ),
    }
}

/// Cancelling one generation is not stopping the backend. The distinction is
/// the whole reason a person can press stop and then ask something else.
async fn a_cancelled_generation_does_not_end_the_backend<B: Backend>(
    config: B::Config,
    model: &str,
) {
    let backend = start::<B>(config).await;

    let cancel = Cancel::new();
    let mut request = simple_request(model);
    request.options.max_tokens = Some(2048);
    // not-a-prompt: the suite's own probe, sent by a test and never by a turn.
    request.messages = vec![Message::user("Count slowly from one to two hundred.")];

    let mut stream = backend
        .generate(request, cancel.clone())
        .await
        .expect("generate");
    stream.next().await;
    cancel.cancel();
    while stream.next().await.is_some() {}
    drop(stream);

    assert!(
        backend.ready().await,
        "cancelling a generation must leave the backend answering, or stop \
         becomes a button that unloads the model"
    );

    let chunks = collect::<B>(&backend, simple_request(model), Cancel::new()).await;
    backend.stop().await;
    assert!(
        chunks
            .iter()
            .any(|chunk| matches!(chunk, Ok(Chunk::Done { .. }))),
        "the next question after a cancel is answered normally"
    );
}

async fn an_unknown_model_is_refused<B: Backend>(config: B::Config) {
    let backend = start::<B>(config).await;
    let request = simple_request("definitely-not-a-real-model-9d3f");

    // Either the call fails or the stream does. Both are honest; quietly
    // answering with some other model is not, because the log would then record
    // an answer against a model that never produced it.
    let refused = match backend.generate(request, Cancel::new()).await {
        Err(_) => true,
        Ok(stream) => stream.collect::<Vec<_>>().await.iter().any(Result::is_err),
    };
    backend.stop().await;

    assert!(
        refused,
        "an unknown model must be reported, never quietly substituted"
    );
}

async fn stopping_makes_it_not_ready<B: Backend>(config: B::Config) {
    let backend = start::<B>(config).await;
    backend.stop().await;
    assert!(
        !backend.ready().await,
        "a stopped backend that still reports ready is one the supervisor will \
         hand out to the next caller"
    );
    // Twice, because whoever decides a backend should stop is rarely its last
    // holder and both of them will call this.
    backend.stop().await;
}

async fn start<B: Backend>(config: B::Config) -> B {
    match B::start(config).await {
        Ok(backend) => backend,
        Err(error) => panic!("{} did not start: {error}", B::name()),
    }
}

async fn collect<B: Backend>(backend: &B, request: Request, cancel: Cancel) -> Vec<Result<Chunk>> {
    backend
        .generate(request, cancel)
        .await
        .expect("generate")
        .collect()
        .await
}

/// Assert that a collected stream is a well-formed turn.
///
/// The shape half of [`run`], for a backend's own tests where the full suite is
/// too much but the ordering still matters.
pub fn assert_well_formed(chunks: &[Result<Chunk>]) {
    let mut seen_done = false;
    for chunk in chunks {
        assert!(!seen_done, "a chunk arrived after Done");
        if let Ok(Chunk::Done { .. }) = chunk {
            seen_done = true;
        }
    }
    assert!(seen_done, "the stream ended without Done");
}
