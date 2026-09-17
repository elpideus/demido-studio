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
use crate::model::{Chunk, FinishReason, Message, Options, Request, ToolCall, ToolSpec};

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
        tools: Vec::new(),
        options: Options {
            temperature: Some(0.0),
            max_tokens: Some(32),
            seed: Some(1),
        },
    }
}

/// A request that offers a tool and carries a call already made and answered.
///
/// What a turn loop sends on its second step, and so the shape a backend has to
/// accept: a tool on offer, an assistant message carrying a call, and a tool
/// message answering it by id. Whether the model then calls again or answers is
/// its business, and the case below asserts on neither.
pub fn calling_request(model: &str) -> Request {
    // not-a-prompt: the suite's own probe, sent by a test and never by a turn.
    let question = "What does notes.txt say?";
    Request {
        model: model.to_owned(),
        messages: vec![
            Message::system("You are terse."),
            Message::user(question),
            Message::calling(
                "",
                vec![ToolCall {
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path": "notes.txt"}"#.into(),
                }],
            ),
            Message::result("call-1", "1: The meeting moved to Thursday."),
        ],
        tools: vec![ToolSpec {
            name: "read_file".into(),
            description: "Read a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
                "additionalProperties": false,
            }),
        }],
        options: Options {
            temperature: Some(0.0),
            max_tokens: Some(256),
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
    // And through the slot writer, for the same reason: the number the VRAM
    // budget admitted is what reaches a process, and the case below reads back
    // what the server opened rather than what it was handed.
    let config = B::with_slots(config, SLOTS);

    it_names_itself::<B>();
    starting_gives_a_backend_that_is_ready::<B>(config.clone()).await;
    it_serves_the_model_it_was_started_with::<B>(config.clone(), model).await;
    the_context_length_asked_for_is_the_one_the_slot_gets::<B>(config.clone()).await;
    the_slots_asked_for_are_the_ones_that_open::<B>(config.clone()).await;
    a_stream_ends_with_exactly_one_done::<B>(config.clone(), model).await;
    nothing_follows_done::<B>(config.clone(), model).await;
    a_call_arrives_whole_and_before_done::<B>(config.clone(), model).await;
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

/// The slot count every case asks for.
///
/// Two rather than one, because one is the number a backend that ignored
/// [`Backend::with_slots`](crate::Backend::with_slots) would report anyway, and
/// two is also what makes the context case above mean something: `--ctx-size`
/// is per slot, so a backend that divided its pool instead of multiplying it
/// hands back half the window at this number and all of it at one.
pub const SLOTS: u32 = 2;

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

/// The slot half of the same idea, and the reason both slot methods are on the
/// trait. A sub-agent runs on a second slot of the conversation's own weights,
/// so the slot count is a VRAM decision (`demido_vram::admit`) and this is
/// where it is checked against what the server says it opened. **The number of
/// slots shown to the user is the number actually opened**, which is only true
/// if somebody asks.
async fn the_slots_asked_for_are_the_ones_that_open<B: Backend>(config: B::Config) {
    let backend = start::<B>(config).await;
    let got = backend.slots().await.expect("the slots it opened");
    backend.stop().await;

    assert_eq!(
        got, SLOTS,
        "a backend that opens a different number of slots than it was told to          has made the slot strip a drawing of a preference, and one of those          slots is an agent that is never going to generate"
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

/// A request carrying tools and an answered call is accepted, and whatever calls
/// come back come back whole.
///
/// Three things a turn loop depends on and cannot check for itself: a call has
/// an id to answer it by and a name to dispatch it by, every call is handed
/// over before `Done`, and `Done` says `ToolCalls` exactly when there were
/// calls, so a loop learns whether the turn is over from one chunk.
async fn a_call_arrives_whole_and_before_done<B: Backend>(config: B::Config, model: &str) {
    let backend = start::<B>(config).await;
    let chunks = collect::<B>(&backend, calling_request(model), Cancel::new()).await;
    backend.stop().await;

    assert_well_formed(&chunks);
    let calls: Vec<&ToolCall> = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            Ok(Chunk::Call { call }) => Some(call),
            _ => None,
        })
        .collect();
    for call in &calls {
        assert!(
            !call.id.is_empty() && !call.name.is_empty(),
            "a call with no id cannot be answered and one with no name cannot be run: {call:?}"
        );
    }
    match chunks.last() {
        Some(Ok(Chunk::Done { reason, .. })) => assert_eq!(
            *reason == FinishReason::ToolCalls,
            !calls.is_empty(),
            "Done says ToolCalls exactly when calls came back, and {} did",
            calls.len()
        ),
        other => panic!("a request carrying a call was not answered: {other:?}"),
    }
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
