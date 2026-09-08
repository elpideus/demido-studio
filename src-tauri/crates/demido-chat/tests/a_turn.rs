//! The turn loop, against a backend that is entirely bookkeeping.
//!
//! Every rule this crate has is about *ordering*: what is recorded before what
//! is sent, what a stop leaves behind, what a second message carries. None of
//! them can be observed through a real `llama.cpp` without a card and several
//! gigabytes, and a suite that needed one would run once a day instead of on
//! every save. So the seam is [`demido_inference::Backend`], and what is on the
//! other side of it here is a script.
//!
//! The claims that need a real model are in `tests/a_real_model.rs`, which is
//! the model gate of [`docs/rules/done.md`](../../../../docs/rules/done.md).
//! These are the ones a fake can prove, and they are the ones a fake proves
//! better.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_chat::{Chat, Model, Presence, Update};
use demido_inference::{
    Backend, Cancel, ChunkStream, Error as BackendError, FinishReason, Loaded, Request, Result,
    Role, Supervisor, Usage,
};
use demido_trace::{Body, Journal, Memory, Replay, Source};

/// What the fake says and how it behaves, shared by the config, the backend it
/// starts and the test that is watching.
#[derive(Clone)]
struct Script {
    /// The answer, one chunk per token, so a stop can land in the middle of it.
    tokens: Vec<String>,
    /// How long a token takes. Long enough that a stop is a generation in
    /// flight rather than one that had already finished.
    pause: Duration,
    /// It refuses to start, the way a model too large for the card does.
    broken: bool,
    /// Every request the backend was handed. This is how "the second message
    /// carries the first exchange" is asserted: at the seam, on what was
    /// actually sent.
    seen: Arc<Mutex<Vec<Request>>>,
    /// The process is still there. A test flips it to stage a crash.
    alive: Arc<AtomicBool>,
}

impl Script {
    fn saying(tokens: &[&str]) -> Self {
        Self {
            tokens: tokens.iter().map(|token| (*token).to_owned()).collect(),
            pause: Duration::from_millis(1),
            broken: false,
            seen: Arc::new(Mutex::new(Vec::new())),
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    fn slowly(mut self) -> Self {
        self.pause = Duration::from_millis(40);
        self
    }

    fn broken() -> Self {
        let mut script = Self::saying(&[]);
        script.broken = true;
        script
    }

    fn sent(&self) -> Vec<Request> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

/// Two configurations are the same backend when they load the same thing. The
/// recorder is test scaffolding and has no say in it.
impl PartialEq for Script {
    fn eq(&self, other: &Self) -> bool {
        self.tokens == other.tokens && self.broken == other.broken
    }
}

impl Eq for Script {}

impl std::fmt::Debug for Script {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Script")
            .field("broken", &self.broken)
            .finish()
    }
}

struct Fake {
    script: Script,
}

#[async_trait::async_trait]
impl Backend for Fake {
    type Config = Script;

    fn name() -> &'static str {
        "fake"
    }

    async fn start(config: Script) -> Result<Self> {
        if config.broken {
            return Err(BackendError::DidNotStart {
                backend: "fake".into(),
                detail: "it does not fit".into(),
            });
        }
        Ok(Fake { script: config })
    }

    async fn ready(&self) -> bool {
        self.script.alive.load(Ordering::SeqCst)
    }

    async fn loaded(&self) -> Result<Loaded> {
        Ok(Loaded {
            id: "scripted".into(),
            size: None,
        })
    }

    async fn context_length(&self) -> Result<u32> {
        Ok(4096)
    }

    async fn generate(&self, request: Request, cancel: Cancel) -> Result<ChunkStream> {
        if let Ok(mut seen) = self.script.seen.lock() {
            seen.push(request.clone());
        }
        // A backend serves one model and refuses any other name rather than
        // answering with what it has, which is what the contract suite holds
        // every implementation to.
        if request.model != "scripted" {
            return Err(BackendError::Refused {
                backend: "fake".into(),
                detail: format!("this server is serving scripted, not {}", request.model),
            });
        }
        let script = self.script.clone();

        Ok(Box::pin(async_stream::stream! {
            let mut said = 0u32;
            for token in &script.tokens {
                tokio::select! {
                    // Cancelling ends the stream promptly, and what was already
                    // generated is kept: the contract's own words.
                    () = cancel.cancelled() => {
                        yield Ok(demido_inference::Chunk::Done {
                            reason: FinishReason::Cancelled,
                            usage: Usage { prompt_tokens: 7, completion_tokens: said },
                        });
                        return;
                    }
                    () = tokio::time::sleep(script.pause) => {}
                }
                said += 1;
                yield Ok(demido_inference::Chunk::Text { text: token.clone() });
            }
            yield Ok(demido_inference::Chunk::Done {
                reason: FinishReason::Stop,
                usage: Usage { prompt_tokens: 7, completion_tokens: said },
            });
        }))
    }

    async fn stop(&self) {
        self.script.alive.store(false, Ordering::SeqCst);
    }
}

/// A chat over a log in memory, talking to a script.
///
/// The journal is handed back by the opener as a clone, which is what "opened
/// again over the same storage" means for [`Memory`], so a second chat built
/// over the same handle is a restart.
fn chat(script: &Script, log: &Memory) -> Chat<Fake, Memory> {
    let log = log.clone();
    Chat::new(
        "a-turn",
        move || Ok(log.clone()),
        Arc::new(Supervisor::new()),
        Some(Model {
            config: script.clone(),
            id: "scripted".into(),
        }),
    )
}

/// Every update a turn produced, in order.
#[derive(Default)]
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

    fn updates(&self) -> Vec<Update> {
        self.0.lock().map(|seen| seen.clone()).unwrap_or_default()
    }
}

/// The states a load passed through, which is what the composer is drawn from.
fn watch() -> (Arc<Mutex<Vec<Presence>>>, impl FnMut(&Presence)) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    (seen, move |presence: &Presence| {
        if let Ok(mut seen) = recorder.lock() {
            seen.push(presence.clone());
        }
    })
}

/// The product's own sentence: a person types a message and an answer arrives.
///
/// Asserted three ways, because the three are what the rest of the app reads:
/// the tokens the window was handed, the answer the caller got back, and the
/// transcript, which is a projection of the log and not a copy of either.
#[tokio::test]
async fn a_message_gets_an_answer_and_the_transcript_comes_out_of_the_log() {
    let script = Script::saying(&["Par", "is", "."]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;

    let watched = Watched::default();
    let answer = chat
        .ask("What is the capital of France?", watched.sink())
        .await
        .expect("an answer");

    assert_eq!(answer.text, "Paris.");
    assert_eq!(answer.reason, FinishReason::Stop);

    let updates = watched.updates();
    assert_eq!(
        updates
            .iter()
            .filter(|update| matches!(update, Update::Text { .. }))
            .count(),
        3,
        "a token is delivered as it arrives, not the answer so far"
    );
    assert!(
        matches!(updates.last(), Some(Update::Done { .. })),
        "the turn ends with exactly one done, so the window stops without counting"
    );

    let history = chat.history().expect("a transcript");
    assert_eq!(history.len(), 2, "a question and an answer");
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[0].text, "What is the capital of France?");
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].text, "Paris.");
    assert_eq!(
        history[1].seq, answer.seq,
        "the transcript keys the answer where the log put it"
    );
}

/// The whole assembly is recorded before anything is sent, so the log says what
/// was about to happen even if the process dies mid-request.
#[tokio::test]
async fn what_was_sent_is_what_the_log_says_was_sent() {
    let script = Script::saying(&["ok"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}).await.expect("an answer");

    let replay = Replay::of(&log).expect("read the log");
    let rebuilt = replay.assembly(1).expect("rebuilt");
    let sent = script.sent();
    assert_eq!(
        rebuilt, sent[0],
        "the log rebuilt a different request from the one the backend was given"
    );
}

/// A conversation rather than a series of first questions.
#[tokio::test]
async fn a_second_message_carries_the_first_exchange() {
    let script = Script::saying(&["Paris"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;

    chat.ask("What is the capital of France?", |_| {})
        .await
        .expect("an answer");
    chat.ask("And of Italy?", |_| {}).await.expect("an answer");

    let sent = script.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        sent[0].messages.len(),
        1,
        "the first message has nothing to carry"
    );

    let second = &sent[1].messages;
    assert_eq!(
        second.len(),
        3,
        "the second turn carries the question, the answer, and the new question"
    );
    assert_eq!(second[0].role, Role::User);
    assert_eq!(second[0].content, "What is the capital of France?");
    assert_eq!(second[1].role, Role::Assistant);
    assert_eq!(second[1].content, "Paris");
    assert_eq!(second[2].content, "And of Italy?");

    // Carried by position, never by copy: a turn that copied what came before
    // would grow the log as the square of the session.
    let carried = Replay::of(&log)
        .expect("read the log")
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Message { .. }))
        .count();
    assert_eq!(carried, 2, "two questions, and no second copy of the first");
}

/// A stop is a person pressing stop, and the log has to match what happened
/// rather than what was intended.
#[tokio::test]
async fn a_stop_records_the_partial_answer_and_the_stop() {
    let script = Script::saying(&["one ", "two ", "three ", "four ", "five "]).slowly();
    let log = Memory::new();
    let chat = Arc::new(chat(&script, &log));
    chat.load(|_| {}).await;

    let stopping = chat.clone();
    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("count to five", |_| {}).await })
    };

    // After the generation is really under way, so this is a stop rather than a
    // race with the first token.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(stopping.stop(), "there was a generation to stop");

    let answer = asking.await.expect("joined").expect("a stopped answer");
    assert_eq!(answer.reason, FinishReason::Cancelled);
    assert!(
        !answer.text.is_empty() && answer.text.len() < "one two three four five ".len(),
        "a stop keeps what was generated and does not wait for the rest: {:?}",
        answer.text
    );

    let replay = Replay::of(&log).expect("read the log");
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

    assert_eq!(
        chat.history().expect("a transcript").len(),
        2,
        "a stopped answer is still an answer in the transcript"
    );
    assert!(!chat.stop(), "nothing is generating once the turn is over");
}

/// A stop is not an unload. The distinction is the whole reason a person can
/// press stop and then ask something else.
#[tokio::test]
async fn the_message_after_a_stop_is_answered_normally() {
    let script = Script::saying(&["a", "b", "c", "d"]).slowly();
    let log = Memory::new();
    let chat = Arc::new(chat(&script, &log));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("first", |_| {}).await })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    chat.stop();
    asking.await.expect("joined").expect("a stopped answer");

    assert!(chat.presence().is_ready(), "a stop is not an unload");
    let answer = chat.ask("second", |_| {}).await.expect("an answer");
    assert_eq!(answer.reason, FinishReason::Stop);
    assert_eq!(answer.turn, 2);
}

/// Closing the app and opening it again is the same read as opening it for the
/// first time, because there is nowhere else for a chat to have been.
#[tokio::test]
async fn a_chat_reopened_over_its_log_is_the_chat_that_was_there() {
    let script = Script::saying(&["Paris"]);
    let log = Memory::new();

    {
        let chat = chat(&script, &log);
        chat.load(|_| {}).await;
        chat.ask("What is the capital of France?", |_| {})
            .await
            .expect("an answer");
    }

    let reopened = chat(&script, &log);
    let history = reopened.history().expect("a transcript");
    assert_eq!(history.len(), 2, "the chat is still there");
    assert_eq!(history[1].text, "Paris");

    reopened.load(|_| {}).await;
    let answer = reopened
        .ask("And of Italy?", |_| {})
        .await
        .expect("an answer");
    assert_eq!(
        answer.turn, 2,
        "a resumed session numbers the next turn after the ones on the log \
         rather than writing turn one over an old one"
    );
}

/// The composer is disabled with nothing loaded, and the loop refuses for the
/// same reason rather than sending a request nothing can answer.
#[tokio::test]
async fn a_chat_with_nothing_configured_says_so_and_refuses() {
    let log = Memory::new();
    let chat: Chat<Fake, Memory> = Chat::new(
        "empty",
        move || Ok(log.clone()),
        Arc::new(Supervisor::new()),
        None,
    );

    let (seen, report) = watch();
    assert_eq!(chat.load(report).await, Presence::Absent);
    assert_eq!(
        seen.lock().map(|seen| seen.len()).unwrap_or_default(),
        1,
        "a fresh install passes through no loading state"
    );

    let refused = chat.ask("hello", |_| {}).await;
    assert!(matches!(
        refused,
        Err(demido_chat::Error::NotReady(Presence::Absent))
    ));
    assert!(
        chat.history().expect("a transcript").is_empty(),
        "a refused turn writes no message"
    );
}

/// A subsystem that fails is reported and skipped: the desk stays usable, and
/// the sentence the backend wrote is the one the window gets.
#[tokio::test]
async fn a_model_that_will_not_start_is_reported_and_the_desk_stays_usable() {
    let log = Memory::new();
    let chat = chat(&Script::broken(), &log);

    let (seen, report) = watch();
    let presence = chat.load(report).await;

    match &presence {
        Presence::Failed { detail } => assert!(
            detail.contains("it does not fit"),
            "the backend's own sentence is what a person can act on: {detail}"
        ),
        other => panic!("a model that does not fit reported {other:?}"),
    }
    let states = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(
        matches!(states.first(), Some(Presence::Loading { .. })),
        "loading is reported while it lasts, or a slow load reads as a broken app"
    );

    assert!(chat.history().is_ok(), "the desk is still usable");
    assert!(chat.ask("hello", |_| {}).await.is_err());
}

/// A process that exited leaves a handle that looks fine from the outside, so
/// the failure is found when a turn is attempted and reported rather than
/// thrown.
#[tokio::test]
async fn a_backend_that_crashed_is_reported_and_the_desk_stays_usable() {
    let script = Script::saying(&["ok"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;
    assert!(chat.presence().is_ready());

    // It crashed. Nothing told anybody.
    script.alive.store(false, Ordering::SeqCst);

    let failed = chat.ask("hello", |_| {}).await;
    assert!(
        matches!(failed, Err(demido_chat::Error::Gone)),
        "{failed:?}"
    );
    assert!(
        matches!(chat.presence(), Presence::Failed { .. }),
        "a dead process must not be reported as a model that is answering"
    );
    assert!(
        chat.history().expect("a transcript").is_empty(),
        "nothing was said, so nothing is on the log"
    );

    // And it can be started again, which is what makes this reported rather
    // than fatal.
    script.alive.store(true, Ordering::SeqCst);
    chat.load(|_| {}).await;
    assert!(chat.ask("hello", |_| {}).await.is_ok());
}

/// A refused turn is an event on the same log, against the assembly it tried
/// to send. A session whose errors are only in a log file somewhere cannot
/// explain itself.
#[tokio::test]
async fn a_refused_turn_is_an_event_on_the_same_log() {
    let script = Script::saying(&["ok"]);
    let log = Memory::new();
    // The backend serves `scripted` and this chat asks under another name,
    // which is the one refusal a supervised server can be made to produce on
    // demand.
    let chat: Chat<Fake, Memory> = {
        let log = log.clone();
        Chat::new(
            "refused",
            move || Ok(log.clone()),
            Arc::new(Supervisor::new()),
            Some(Model {
                config: script.clone(),
                id: "a-model-this-backend-is-not-serving".into(),
            }),
        )
    };
    chat.load(|_| {}).await;

    let watched = Watched::default();
    let refused = chat.ask("hello", watched.sink()).await;
    assert!(
        matches!(refused, Err(demido_chat::Error::Backend(_))),
        "{refused:?}"
    );
    assert!(
        matches!(watched.updates().last(), Some(Update::Failed { .. })),
        "the window is told the turn ended, however it ended"
    );

    let replay = Replay::of(&log).expect("read the log");
    let failure = replay
        .events()
        .iter()
        .find(|event| matches!(event.body, Body::Failure { .. }))
        .expect("a failure is on the log");
    assert_eq!(failure.source, Source::Error);
    assert_eq!(failure.turn, 1);
    assert_eq!(
        replay.assembly(1).expect("rebuilt"),
        script
            .sent()
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("nothing reached the backend")),
        "a turn that failed still rebuilds what it tried to send"
    );
    assert!(
        chat.presence().is_ready(),
        "a refusal is the request being wrong, not the backend being gone"
    );
}

/// The composition root runs before the window exists, and a root that opened a
/// session log would put a file in the profile of somebody who never said
/// anything.
#[tokio::test]
async fn the_log_is_not_opened_until_there_is_something_to_put_in_it() {
    let opened = Arc::new(AtomicUsize::new(0));
    let log = Memory::new();

    let chat: Chat<Fake, Memory> = {
        let opened = opened.clone();
        let log = log.clone();
        Chat::new(
            "lazy",
            move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Ok(log.clone())
            },
            Arc::new(Supervisor::new()),
            Some(Model {
                config: Script::saying(&["ok"]),
                id: "scripted".into(),
            }),
        )
    };

    assert_eq!(
        opened.load(Ordering::SeqCst),
        0,
        "constructing opens nothing"
    );
    chat.load(|_| {}).await;
    assert_eq!(
        opened.load(Ordering::SeqCst),
        0,
        "loading a model opens nothing"
    );

    chat.ask("hello", |_| {}).await.expect("an answer");
    assert_eq!(opened.load(Ordering::SeqCst), 1);
    chat.ask("again", |_| {}).await.expect("an answer");
    assert_eq!(
        opened.load(Ordering::SeqCst),
        1,
        "the log is opened once and kept, not reopened per turn"
    );
    assert!(!log.events().expect("the log").is_empty());
}
