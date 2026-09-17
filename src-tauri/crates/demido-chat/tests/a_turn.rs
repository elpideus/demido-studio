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

use std::future::Ready;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_chat::{Asking, Chat, Decision, Delegations, Model, Pool, Presence, Toolbox, Update};
use demido_inference::scripted::{Script, Scripted};
use demido_inference::{Backend, FinishReason, Role, Supervisor};
use demido_tools::Registry;
use serde_json::json;

use demido_settings::{Ladder, Memory as SettingsMemory, Scope, Settings};
use demido_trace::{Body, Journal, Memory, Replay, Source};

/// The conversation these cases run in. Named because a per-chat setting is set
/// against it, and a ladder that names a different chat resolves to the global
/// value, which would pass for the wrong reason.
const SESSION: &str = "a-turn";

/// One mebibyte, so the cases below read like `docs/rules/done.md`'s own table.
const MIB: u64 = demido_vram::MIB;

/// A script that answers with these tokens, one chunk each.
///
/// The backend is `demido_inference::scripted`, which passes the contract suite
/// `llama.cpp` does: these cases are about the loop's ordering, and a fake with
/// promises of its own would be proving the loop against the wrong ones.
fn saying(tokens: &[&str]) -> Script {
    Script::serving("scripted").then_say(tokens)
}

/// The messages of a transcript, which is all these cases have in it: nothing
/// is offered here, so nothing is ever called. The call rows are
/// `tests/a_tool.rs`'s.
fn said<B: Backend, J: demido_trace::Journal>(chat: &Chat<B, J>) -> Vec<demido_chat::Said> {
    chat.transcript()
        .expect("a transcript")
        .into_iter()
        .filter_map(|moment| match moment {
            demido_chat::Moment::Said(said) => Some(said),
            demido_chat::Moment::Called(_) | demido_chat::Moment::Delegated(_) => None,
        })
        .collect()
}

/// Slow enough that a stop is a generation in flight rather than one that had
/// already finished.
fn slowly(script: Script) -> Script {
    script.pausing(Duration::from_millis(40))
}

/// Nothing offered: these cases are about a turn with nothing to call, which is
/// the same loop with nothing to do but answer.
///
/// The prompts directory is never created. Nothing is offered, so nothing is
/// read from it, and a refusal would read the built-in wording.
fn toolbox() -> Toolbox {
    Toolbox::open(
        Registry::default(),
        std::env::temp_dir()
            .join(format!("demido-a-turn-{}", std::process::id()))
            .join("prompts"),
    )
}

/// Nobody should be asked anything in a turn that offers nothing.
fn nobody(asking: Asking) -> Ready<Decision> {
    panic!(
        "nothing is offered, and yet {} was asked about",
        asking.tool
    )
}

/// A chat over a log in memory, talking to a script.
///
/// The journal is handed back by the opener as a clone, which is what "opened
/// again over the same storage" means for [`Memory`], so a second chat built
/// over the same handle is a restart.
fn chat(script: &Script, log: &Memory) -> Chat<Scripted, Memory> {
    over(
        script,
        log,
        &Arc::new(Settings::open(SettingsMemory::new())),
    )
    .0
}

/// The same chat, over a ladder the caller can set values on, and the
/// supervisor it loads through.
///
/// Handed back together because both are what a settings assertion reads: the
/// ladder is where a value is set, and the supervisor is what the backend it
/// produced can be asked about.
fn over(
    script: &Script,
    log: &Memory,
    settings: &Arc<Settings>,
) -> (Chat<Scripted, Memory>, Arc<Supervisor<Scripted>>) {
    let log = log.clone();
    let supervisor = Arc::new(Supervisor::new());
    let chat = Chat::new(
        SESSION,
        move || Ok(log.clone()),
        supervisor.clone(),
        Some(Model {
            config: script.clone(),
            id: "scripted".into(),
        }),
        settings.clone(),
        toolbox(),
        Delegations::none(),
    );
    (chat, supervisor)
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
    let script = saying(&["Par", "is", "."]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;

    let watched = Watched::default();
    let answer = chat
        .ask("What is the capital of France?", watched.sink(), nobody)
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

    let history = said(&chat);
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
    let script = saying(&["ok"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");

    let replay = Replay::of(&log).expect("read the log");
    let rebuilt = replay.assembly(1).expect("rebuilt");
    let sent = script.requests();
    assert_eq!(
        rebuilt, sent[0],
        "the log rebuilt a different request from the one the backend was given"
    );
}

/// A conversation rather than a series of first questions.
#[tokio::test]
async fn a_second_message_carries_the_first_exchange() {
    let script = saying(&["Paris"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;

    chat.ask("What is the capital of France?", |_| {}, nobody)
        .await
        .expect("an answer");
    chat.ask("And of Italy?", |_| {}, nobody)
        .await
        .expect("an answer");

    let sent = script.requests();
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
    let script = slowly(saying(&["one ", "two ", "three ", "four ", "five "]));
    let log = Memory::new();
    let chat = Arc::new(chat(&script, &log));
    chat.load(|_| {}).await;

    let stopping = chat.clone();
    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("count to five", |_| {}, nobody).await })
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
        chat.transcript().expect("a transcript").len(),
        2,
        "a stopped answer is still an answer in the transcript"
    );
    assert!(!chat.stop(), "nothing is generating once the turn is over");
}

/// A stop is not an unload. The distinction is the whole reason a person can
/// press stop and then ask something else.
#[tokio::test]
async fn the_message_after_a_stop_is_answered_normally() {
    let script = slowly(saying(&["a", "b", "c", "d"]));
    let log = Memory::new();
    let chat = Arc::new(chat(&script, &log));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("first", |_| {}, nobody).await })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    chat.stop();
    asking.await.expect("joined").expect("a stopped answer");

    assert!(chat.presence().is_ready(), "a stop is not an unload");
    let answer = chat.ask("second", |_| {}, nobody).await.expect("an answer");
    assert_eq!(answer.reason, FinishReason::Stop);
    assert_eq!(answer.turn, 2);
}

/// Closing the app and opening it again is the same read as opening it for the
/// first time, because there is nowhere else for a chat to have been.
#[tokio::test]
async fn a_chat_reopened_over_its_log_is_the_chat_that_was_there() {
    let script = saying(&["Paris"]);
    let log = Memory::new();

    {
        let chat = chat(&script, &log);
        chat.load(|_| {}).await;
        chat.ask("What is the capital of France?", |_| {}, nobody)
            .await
            .expect("an answer");
    }

    let reopened = chat(&script, &log);
    let history = said(&reopened);
    assert_eq!(history.len(), 2, "the chat is still there");
    assert_eq!(history[1].text, "Paris");

    reopened.load(|_| {}).await;
    let answer = reopened
        .ask("And of Italy?", |_| {}, nobody)
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
    let chat: Chat<Scripted, Memory> = Chat::new(
        "empty",
        move || Ok(log.clone()),
        Arc::new(Supervisor::new()),
        None,
        Arc::new(Settings::open(SettingsMemory::new())),
        toolbox(),
        Delegations::none(),
    );

    let (seen, report) = watch();
    assert_eq!(chat.load(report).await, Presence::Absent);
    assert_eq!(
        seen.lock().map(|seen| seen.len()).unwrap_or_default(),
        1,
        "a fresh install passes through no loading state"
    );

    let refused = chat.ask("hello", |_| {}, nobody).await;
    assert!(matches!(
        refused,
        Err(demido_chat::Error::NotReady(Presence::Absent))
    ));
    assert!(
        chat.transcript().expect("a transcript").is_empty(),
        "a refused turn writes no message"
    );
}

/// A subsystem that fails is reported and skipped: the desk stays usable, and
/// the sentence the backend wrote is the one the window gets.
#[tokio::test]
async fn a_model_that_will_not_start_is_reported_and_the_desk_stays_usable() {
    let log = Memory::new();
    let chat = chat(
        &Script::serving("scripted").refusing_to_start("it does not fit"),
        &log,
    );

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

    assert!(chat.transcript().is_ok(), "the desk is still usable");
    assert!(chat.ask("hello", |_| {}, nobody).await.is_err());
}

/// A process that exited leaves a handle that looks fine from the outside, so
/// the failure is found when a turn is attempted and reported rather than
/// thrown.
#[tokio::test]
async fn a_backend_that_crashed_is_reported_and_the_desk_stays_usable() {
    let script = saying(&["ok"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;
    assert!(chat.presence().is_ready());

    // It crashed. Nothing told anybody.
    script.crash();

    let failed = chat.ask("hello", |_| {}, nobody).await;
    assert!(
        matches!(failed, Err(demido_chat::Error::Gone)),
        "{failed:?}"
    );
    assert!(
        matches!(chat.presence(), Presence::Failed { .. }),
        "a dead process must not be reported as a model that is answering"
    );
    assert!(
        chat.transcript().expect("a transcript").is_empty(),
        "nothing was said, so nothing is on the log"
    );

    // And it can be started again, which is what makes this reported rather
    // than fatal.
    script.revive();
    chat.load(|_| {}).await;
    assert!(chat.ask("hello", |_| {}, nobody).await.is_ok());
}

/// A refused turn is an event on the same log, against the assembly it tried
/// to send. A session whose errors are only in a log file somewhere cannot
/// explain itself.
#[tokio::test]
async fn a_refused_turn_is_an_event_on_the_same_log() {
    let script = saying(&["ok"]);
    let log = Memory::new();
    // The backend serves `scripted` and this chat asks under another name,
    // which is the one refusal a supervised server can be made to produce on
    // demand.
    let chat: Chat<Scripted, Memory> = {
        let log = log.clone();
        Chat::new(
            "refused",
            move || Ok(log.clone()),
            Arc::new(Supervisor::new()),
            Some(Model {
                config: script.clone(),
                id: "a-model-this-backend-is-not-serving".into(),
            }),
            Arc::new(Settings::open(SettingsMemory::new())),
            toolbox(),
            Delegations::none(),
        )
    };
    chat.load(|_| {}).await;

    let watched = Watched::default();
    let refused = chat.ask("hello", watched.sink(), nobody).await;
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
            .requests()
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

    let chat: Chat<Scripted, Memory> = {
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
                config: saying(&["ok"]),
                id: "scripted".into(),
            }),
            Arc::new(Settings::open(SettingsMemory::new())),
            toolbox(),
            Delegations::none(),
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

    chat.ask("hello", |_| {}, nobody).await.expect("an answer");
    assert_eq!(opened.load(Ordering::SeqCst), 1);
    chat.ask("again", |_| {}, nobody).await.expect("an answer");
    assert_eq!(
        opened.load(Ordering::SeqCst),
        1,
        "the log is opened once and kept, not reopened per turn"
    );
    assert!(!log.events().expect("the log").is_empty());
}

// --- the settings ladder, as a turn reads it -------------------------------
// `demido-settings` proves the ladder resolves. These prove the turn loop asks
// it, which is the half that can be got wrong in this crate: a conversation
// that resolved once at construction, or sent `Options::default`, would pass
// every case above.

/// A settings page, as a test sets one value on it.
fn ladder() -> Arc<Settings> {
    Arc::new(Settings::open(SettingsMemory::new()))
}

/// The temperature that reaches the backend is the ladder's, and the chat is
/// the last word over the global tier.
#[tokio::test]
async fn the_temperature_sent_is_the_one_the_ladder_resolved() {
    let settings = ladder();
    settings
        .set(
            &Scope::Global,
            demido_settings::id::TEMPERATURE,
            &json!(0.2),
        )
        .expect("set globally");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, _) = over(&script, &log, &settings);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");

    assert_eq!(script.requests()[0].options.temperature, Some(0.2));

    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::TEMPERATURE,
            &json!(1.4),
        )
        .expect("set on this chat");
    chat.ask("again", |_| {}, nobody).await.expect("an answer");

    assert_eq!(
        script.requests()[1].options.temperature,
        Some(1.4),
        "a value changed while the window is open takes effect on the next turn"
    );
}

/// The system prompt is a ladder value, so it heads the assembly and a chat's
/// own overrides the global one.
#[tokio::test]
async fn the_system_prompt_heads_the_assembly_and_the_chat_outranks_the_global() {
    let settings = ladder();
    settings
        .set(
            &Scope::Global,
            demido_settings::id::SYSTEM_PROMPT,
            &json!("You are terse."),
        )
        .expect("set globally");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, _) = over(&script, &log, &settings);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");

    let first = &script.requests()[0].messages;
    assert_eq!(first[0].role, Role::System);
    assert_eq!(first[0].content, "You are terse.");
    assert_eq!(first[1].role, Role::User, "then what the person typed");

    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::SYSTEM_PROMPT,
            &json!("You are a pirate."),
        )
        .expect("set on this chat");
    chat.ask("again", |_| {}, nobody).await.expect("an answer");

    let second = &script.requests()[1].messages;
    assert_eq!(second[0].content, "You are a pirate.");
    assert_eq!(
        second[1].role,
        Role::User,
        "the system prompt is not carried twice: the earlier one is not history"
    );

    // On the log like anything else, so the assembly rebuilds with it. A prompt
    // that reached the model without reaching the log would be the one block of
    // a turn the session log could not account for.
    let replay = Replay::of(&log).expect("read the log");
    assert_eq!(
        replay.assembly(2).expect("rebuilt"),
        script.requests()[1],
        "the log rebuilt a different request from the one the backend was given"
    );

    // Attributed to whoever put it in the window. A prompt the monitor cannot
    // see is a prompt nobody can edit.
    let injected = replay
        .events()
        .iter()
        .filter(|event| event.source == Source::Inject)
        .count();
    assert_eq!(injected, 2, "one per turn, recorded before it was sent");
}

/// A prompt nobody wrote is not a blank message. An empty system prompt is left
/// out of the assembly entirely.
#[tokio::test]
async fn an_empty_system_prompt_is_absent_rather_than_blank() {
    let script = saying(&["ok"]);
    let log = Memory::new();
    let chat = chat(&script, &log);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");

    let messages = &script.requests()[0].messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, Role::User);
}

/// The context length is a flag on the process, so the ladder's number has to
/// reach the configuration the supervisor starts a backend from. A chat that
/// asked for a bigger window and got the default would be the defect
/// `docs/rules/done.md` records, one layer up from where it was measured.
#[tokio::test]
async fn the_context_length_the_chat_asked_for_is_what_the_backend_is_started_with() {
    let settings = ladder();
    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::CONTEXT_LENGTH,
            &json!(8192),
        )
        .expect("set on this chat");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, supervisor) = over(&script, &log, &settings);
    chat.load(|_| {}).await;

    let backend = supervisor.current().await.expect("a running backend");
    assert_eq!(
        backend.context_length().await.expect("the slot's context"),
        8192
    );

    // And changing it is a restart rather than a no-op, because the
    // configuration the supervisor compares is a different one.
    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::CONTEXT_LENGTH,
            &json!(16384),
        )
        .expect("set again");
    chat.load(|_| {}).await;

    let restarted = supervisor.current().await.expect("a running backend");
    assert_eq!(
        restarted
            .context_length()
            .await
            .expect("the slot's context"),
        16384
    );
}

/// The same chat, over a chosen card.
///
/// A conversation's slot count is a VRAM decision, and a suite that asked the
/// machine it happens to be running on would pass or fail by what else has a
/// browser open.
fn on_a_card(
    script: &Script,
    log: &Memory,
    settings: &Arc<Settings>,
    free: u64,
    per_slot: u64,
) -> (Chat<Scripted, Memory>, Arc<Supervisor<Scripted>>) {
    let (chat, supervisor) = over(script, log, settings);
    (
        chat.against(Pool::on_a_card_with(free, per_slot)),
        supervisor,
    )
}

/// The rig's own numbers, from `docs/rules/done.md`: the development model at
/// 32k leaves 5583 MiB on the 12 GB card and one more of its slots reserves
/// 563, so the card can hold what the setting asks for.
#[tokio::test]
async fn the_slots_the_card_can_hold_are_the_ones_the_backend_opens() {
    let settings = ladder();
    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::PARALLEL_AGENTS,
            &json!(4),
        )
        .expect("set on this chat");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, supervisor) = on_a_card(&script, &log, &settings, 5583 * MIB, 563 * MIB);
    chat.load(|_| {}).await;

    let backend = supervisor.current().await.expect("a running backend");
    assert_eq!(
        backend.slots().await.expect("the slots it opened"),
        4,
        "the setting counts slots, so four is three sub-agents beside the          conversation's own, and every one of them is paid for"
    );
    assert_eq!(
        chat.presence(),
        Presence::Ready {
            model: "scripted".into(),
            slots: 4,
        }
    );
}

/// The reference model at 32k leaves 423 MiB and a slot reserves 1129, so the
/// same setting on the same card cannot be honoured. It degrades to a queue:
/// the conversation keeps its own slot, nothing is evicted, and the number
/// shown is the number opened rather than the number asked for.
#[tokio::test]
async fn a_parallelism_the_card_cannot_honour_opens_one_slot_and_says_so() {
    let settings = ladder();
    settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::PARALLEL_AGENTS,
            &json!(4),
        )
        .expect("set on this chat");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, supervisor) = on_a_card(&script, &log, &settings, 423 * MIB, 1129 * MIB);
    chat.load(|_| {}).await;

    let backend = supervisor.current().await.expect("a running backend");
    assert_eq!(backend.slots().await.expect("the slots it opened"), 1);
    assert_eq!(
        chat.presence(),
        Presence::Ready {
            model: "scripted".into(),
            slots: 1,
        },
        "the window is told what opened, never what was preferred"
    );
}

/// The default, which is every conversation until somebody changes a setting:
/// one slot, on a card with nothing free at all. The conversation's own slot is
/// the model rather than a sub-agent, so no budget may refuse it.
#[tokio::test]
async fn the_default_loads_on_a_card_with_nothing_to_spare() {
    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, supervisor) = on_a_card(&script, &log, &ladder(), 0, 1129 * MIB);
    chat.load(|_| {}).await;

    assert!(chat.presence().is_ready());
    assert_eq!(
        supervisor
            .current()
            .await
            .expect("a running backend")
            .slots()
            .await
            .expect("the slots it opened"),
        1
    );
}

/// Another conversation is not this one. The ladder a chat resolves through
/// ends at its own id, which is what makes a per-chat override reach one chat
/// and no other.
#[tokio::test]
async fn an_override_made_in_one_chat_does_not_reach_another() {
    let settings = ladder();
    settings
        .set(
            &Scope::chat("somebody-elses-chat"),
            demido_settings::id::TEMPERATURE,
            &json!(1.4),
        )
        .expect("set");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, _) = over(&script, &log, &settings);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");

    assert_eq!(
        script.requests()[0].options.temperature,
        Some(0.7),
        "this chat resolves the schema's default, not the other chat's override"
    );
    assert_eq!(
        settings
            .resolve(&Ladder::for_chat("somebody-elses-chat"))
            .temperature(),
        Some(1.4),
        "and the other chat still has what was set on it"
    );
}

/// The mode is never prose. Nothing Demido assembles describes a mode to the
/// model, because a permission the model is told about is a permission that
/// depends on whether the model obeyed it.
///
/// Asserted at the seam, on every request the backend was handed, so it holds
/// for whatever the assembly grows to carry. When the mode arrives on the
/// ladder ([#56](https://github.com/elpideus/demido-studio/issues/56)) this
/// case sets it, and the assertion does not change.
#[tokio::test]
async fn nothing_sent_to_the_model_names_a_mode() {
    let settings = ladder();
    settings
        .set(
            &Scope::Global,
            demido_settings::id::SYSTEM_PROMPT,
            &json!("You are terse."),
        )
        .expect("set globally");

    let script = saying(&["ok"]);
    let log = Memory::new();
    let (chat, _) = over(&script, &log, &settings);
    chat.load(|_| {}).await;
    chat.ask("hello", |_| {}, nobody).await.expect("an answer");
    chat.ask("again", |_| {}, nobody).await.expect("an answer");

    let sent = script.requests();
    assert_eq!(sent.len(), 2);
    for request in sent {
        let payload = format!("{request:?}").to_lowercase();
        for name in demido_permission::Mode::names() {
            assert!(
                !payload.contains(name),
                "the {name} mode reached the payload"
            );
        }
    }
}
