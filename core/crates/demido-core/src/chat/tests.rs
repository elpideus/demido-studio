//! A turn from end to end, against the stand-in runtime (`engine/stand_in.rs`).

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use super::*;
use crate::db::Db;
use crate::engine::stand_in::{Stand, StandLauncher};
use crate::runtime::{Runtimes, Source};
use crate::settings::Settings;

mod bots;
mod folders_and_permissions;
mod mail;
mod markets;
mod schedules;
mod the_internet;
mod tools_and_tasks;

/// It is always the same minute of the same Saturday, whatever the real clock says.
const CLOCK: tools::Clock = |_| tools::Moment { day: "Saturday, 21 November 2026".into(), time: "14:05".into(), date: (2026, 11, 21) };
/// What every model is told first, on that Saturday.
const TOLD: &str = "Today's date: Saturday, 21 November 2026.\n\nAnswer first. Then numbered steps, only if steps are needed. One idea per sentence. Exact values with units. No filler, no praise. Do not repeat the question.";

struct World {
    chat: Arc<Chat>,
    sessions: Arc<Sessions>,
    engine: Arc<Engine>,
    events: Arc<EventLog>,
    stand: Arc<Stand>,
    /// Removed with the world, whether the test passed or not.
    home: crate::testing::TempDir,
    settings: Arc<Settings>,
    /// The keys Demido keeps, in a file in `home`: none unless a test puts one there.
    vault: Arc<crate::vault::Vault>,
    /// The database everything here keeps its records in.
    db: Arc<Db>,
}

fn world(name: &str) -> World {
    world_where(name, None)
}

/// A data folder with an installed runtime and two models, and everything a turn needs on top of it.
/// `idle_after`: how soon an unused model counts as idle, for the test that needs it short.
fn world_where(name: &str, idle_after: Option<Duration>) -> World {
    let home = crate::testing::TempDir::new(&format!("demido-chat-{name}"));
    let models = home.join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("small.gguf"), vec![0u8; 100]).unwrap();
    std::fs::write(models.join("big.gguf"), vec![0u8; 3000]).unwrap();
    let folder = home.join("runtimes/llama.cpp/llama.cpp-test-cpu");
    std::fs::create_dir_all(&folder).unwrap();
    let note = json!({ "id": "llama.cpp-test-cpu", "runtime": "llama.cpp", "build": "test", "backend": "cpu", "name": "llama.cpp · CPU", "files": [], "checked": true,
        "server": "llama-server", "lib_dirs": [], "version": "version: test", "problem": null, "installed_at": 1, "size": 1 });
    std::fs::write(folder.join("demido-runtime.json"), note.to_string()).unwrap();
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db.clone(), events.clone()).unwrap());
    // These tests hold the list of tools to what they are about; artifacts have tests of their own.
    settings.set("tools.artifacts", serde_json::json!("never")).unwrap();
    let kept_settings = settings.clone();
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), Source::default()).unwrap());
    let stand = Stand::new(models);
    let engine = Engine::new(&home, runtimes, settings, events.clone(), Arc::new(StandLauncher(stand.clone())));
    let engine = Arc::new(match idle_after {
        Some(wait) => engine.idle_after(wait),
        None => engine,
    });
    let vault = Arc::new(crate::vault::Vault::in_file(&home));
    let providers = Arc::new(crate::providers::Providers::new(db.clone(), vault.clone(), kept_settings.clone(), events.clone()));
    let sessions = Arc::new(Sessions::new(db.clone()));
    // The tests in this file are about plain chat: one request to the model for each turn. Tools have their own.
    kept_settings.set("tools.use", json!(false)).unwrap();
    // Nor do they reach the internet: the tests that do have a stand-in site (`the_internet`).
    kept_settings.set("web.use", json!(false)).unwrap();
    let chat = Arc::new(
        Chat::new(sessions.clone(), engine.clone(), events.clone(), kept_settings.clone())
            .with_clock(CLOCK)
            .keeping_in(home.join("saved-folders"))
            .keeping_outputs_in(home.join("outputs"))
            .with_providers(providers),
    );
    World { chat, sessions, engine, events, stand, home, settings: kept_settings, vault, db }
}

impl World {
    /// Waits until the session is no longer being answered, and gives back its last message.
    async fn answered(&self, session_id: &str) -> Message {
        for _ in 0..1000 {
            if !self.chat.answering(session_id) {
                return self.sessions.messages(session_id).unwrap().pop().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the answer never ended");
    }

    async fn end(self) {
        // The world's folder goes with it, once the runtime is stopped.
        self.engine.shutdown().await;
    }
}

fn kinds(events: &EventLog) -> Vec<String> {
    events.after(0, 200).unwrap().into_iter().map(|e| e.kind).collect()
}

#[tokio::test]
async fn what_is_said_is_answered_streamed_and_stored() {
    let w = world("turn");
    let mut ear = w.events.listen();
    let session = w.chat.start(None, None).await.unwrap();
    assert_eq!(session.model, "big", "with nothing loaded, the first model on disk");

    let turn = w.chat.say(&session.id, "  Name three primary colours.  ").unwrap();
    assert_eq!((turn.said.content.as_str(), turn.said.seq, turn.answer.state.as_str(), turn.answer.seq), ("Name three primary colours.", 1, "answering", 2));
    let answer = w.answered(&session.id).await;
    assert_eq!((answer.state.as_str(), answer.content.as_str()), ("done", "Red, blue and yellow are the three primary colours of paint."));
    assert_eq!(answer.model.as_deref(), Some("big"));
    let timings = answer.timings.unwrap();
    assert_eq!((timings.written, timings.per_second), (11, 120.0));
    assert!(timings.reused + timings.read > 0);

    // The session took its title from the first thing said, and the model was loaded for it.
    assert_eq!(w.sessions.get(&session.id).unwrap().title, "Name three primary colours.");
    assert_eq!(w.engine.list().await.unwrap().models[0].state, "loaded");

    // Pieces were told as they came, not kept. The log has the turn's beginning and its end.
    let mut told = String::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.delta" {
            assert_eq!((e.seq, e.data["message"].as_str()), (0, Some(answer.id.as_str())));
            told.push_str(e.data["text"].as_str().unwrap());
        }
    }
    assert_eq!(told, answer.content, "what was told piece by piece is what was stored");
    let log = kinds(&w.events);
    assert!(log.contains(&"session.created".to_string()) && log.contains(&"message.added".to_string()) && log.contains(&"message.done".to_string()), "{log:?}");
    assert!(!log.contains(&"message.delta".to_string()));
    w.end().await;
}

#[tokio::test]
async fn the_beginning_of_the_prompt_never_changes() {
    let w = world("prefix");
    let session = w.chat.start(Some("Colours"), Some("small")).await.unwrap();
    w.chat.say(&session.id, "Name three primary colours.").unwrap();
    w.answered(&session.id).await;
    *w.stand.reply.lock().unwrap() = "Green, orange and purple.".into();
    w.chat.say(&session.id, "And three secondary ones?").unwrap();
    w.answered(&session.id).await;

    let asked = w.stand.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    let (first, second) = (asked[0]["messages"].as_array().unwrap(), asked[1]["messages"].as_array().unwrap());
    assert_eq!(first[0], json!({ "role": "system", "content": TOLD }), "how to answer, and what day it is");
    assert!(TOLD.ends_with(INSTRUCTIONS));
    assert_eq!(first.len(), 2);
    // The second prompt is the first one, unchanged, with the answer and the new question added at the end.
    assert_eq!(&second[..2], &first[..]);
    assert_eq!(second[2], json!({ "role": "assistant", "content": "Red, blue and yellow are the three primary colours of paint." }));
    assert_eq!(second[3], json!({ "role": "user", "content": "And three secondary ones?" }));
    assert_eq!(asked[1]["model"], "small");
    assert_eq!((&asked[1]["chat_template_kwargs"]["enable_thinking"], &asked[1]["reasoning_effort"]), (&json!(false), &json!("none")), "thinking is off unless the person turns it on");
    assert_eq!(w.sessions.get(&session.id).unwrap().title, "Colours", "a title that was given is kept");
    w.end().await;
}

#[tokio::test]
async fn an_answer_can_be_stopped_and_what_was_written_is_kept() {
    let w = world("stop");
    w.stand.word_every_ms.store(40, Ordering::Relaxed);
    let mut ear = w.events.listen();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let turn = w.chat.say(&session.id, "Name three primary colours.").unwrap();
    // One at a time: a second thing said while the first is being answered is refused.
    assert!(matches!(w.chat.say(&session.id, "And more?"), Err(Error::Busy(_))));
    // Wait for the first words, then stop.
    loop {
        let e = tokio::time::timeout(Duration::from_secs(10), ear.recv()).await.expect("words within ten seconds").unwrap();
        if e.kind == "message.delta" && !e.data["text"].as_str().unwrap_or("").is_empty() {
            break;
        }
    }
    assert!(w.chat.stop(&session.id));
    let answer = w.answered(&session.id).await;
    assert_eq!((answer.id.as_str(), answer.state.as_str()), (turn.answer.id.as_str(), "stopped"));
    assert!(!answer.content.is_empty() && answer.content.len() < 50 && "Red, blue and yellow are the three primary colours of paint.".starts_with(&answer.content), "kept: {:?}", answer.content);
    assert!(!w.chat.stop(&session.id), "nothing left to stop");
    assert!(kinds(&w.events).contains(&"message.stopped".to_string()));
    // The session goes on: the next thing said is answered, with the stopped answer in its history.
    w.stand.word_every_ms.store(1, Ordering::Relaxed);
    w.chat.say(&session.id, "Go on.").unwrap();
    assert_eq!(w.answered(&session.id).await.state, "done");
    assert_eq!(w.stand.asked.lock().unwrap()[1]["messages"][2]["content"], answer.content);
    w.end().await;
}

#[tokio::test]
async fn a_refusal_by_the_runtime_is_kept_in_words_and_the_session_goes_on() {
    let w = world("refused");
    let session = w.chat.start(None, Some("small")).await.unwrap();
    *w.stand.refuse_with.lock().unwrap() = Some(
        json!({ "error": { "code": 400, "message": "request (9009 tokens) exceeds the available context size (8192 tokens), try increasing it", "type": "exceed_context_size_error", "n_prompt_tokens": 9009, "n_ctx": 8192 } }),
    );
    w.chat.say(&session.id, "A very long question.").unwrap();
    let failed = w.answered(&session.id).await;
    assert_eq!((failed.state.as_str(), failed.content.as_str()), ("failed", ""));
    let why = failed.problem.unwrap();
    assert!(why.contains("9009 tokens long") && why.contains("room for 8192"), "{why}");
    assert!(kinds(&w.events).contains(&"message.failed".to_string()));

    w.chat.say(&session.id, "A short one.").unwrap();
    assert_eq!(w.answered(&session.id).await.state, "done");
    // The answer that failed said nothing, so it is not in the next prompt.
    let asked = w.stand.asked.lock().unwrap().clone();
    assert_eq!(asked[1]["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect::<Vec<_>>(), ["system", "user", "user"]);
    w.end().await;
}

#[tokio::test]
async fn a_session_needs_a_model_that_is_there_and_something_to_say() {
    let w = world("refusals");
    assert!(matches!(w.chat.start(None, Some("no-such-model")).await, Err(Error::NotFound(_))));
    let session = w.chat.start(None, Some("small")).await.unwrap();
    assert!(matches!(w.chat.say(&session.id, "   "), Err(Error::Invalid(_))));
    assert!(matches!(w.chat.say("no-such-session", "hello"), Err(Error::NotFound(_))));
    assert!(w.sessions.messages(&session.id).unwrap().is_empty(), "nothing was written down for a refusal");

    // Deleting a session stops its answer and takes everything with it.
    w.stand.word_every_ms.store(40, Ordering::Relaxed);
    w.chat.say(&session.id, "Name three primary colours.").unwrap();
    w.chat.delete(&session.id).unwrap();
    assert!(matches!(w.sessions.get(&session.id), Err(Error::NotFound(_))));
    for _ in 0..300 {
        if !w.chat.answering(&session.id) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!w.chat.answering(&session.id));

    // With no models at all, there is nothing to start a session with, and it says where to get one.
    std::fs::remove_file(w.home.join("models/small.gguf")).unwrap();
    std::fs::remove_file(w.home.join("models/big.gguf")).unwrap();
    let err = w.chat.start(None, None).await.unwrap_err().to_string();
    assert!(err.contains("put a GGUF file in") && err.contains("Providers screen"), "{err}");
    w.end().await;
}

#[tokio::test]
async fn a_model_that_is_writing_is_not_unloaded_for_being_idle_and_its_speed_is_learned() {
    // Idle after 100 ms, and an answer that takes five times that.
    let w = world_where("busy", Some(Duration::from_millis(100)));
    *w.stand.reply.lock().unwrap() = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen".into();
    w.stand.word_every_ms.store(30, Ordering::Relaxed);
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say(&session.id, "Count.").unwrap();
    // The answer takes about half a second. The caretaker looks several times meanwhile, with a model
    // that would count as idle after a tenth of that if writing did not count as use.
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(60)).await;
        w.engine.look_after().await;
    }
    let answer = w.answered(&session.id).await;
    assert_eq!((answer.state.as_str(), answer.timings.as_ref().map(|t| t.written)), ("done", Some(18)));
    let small = w.engine.list().await.unwrap().models.into_iter().find(|m| m.id == "small").unwrap();
    assert_eq!(small.tokens_per_second, Some(120.0), "a real answer says how fast the model is here");
    // Once it has stopped writing, being idle counts again.
    tokio::time::sleep(Duration::from_millis(250)).await;
    w.engine.look_after().await;
    assert_eq!(w.engine.list().await.unwrap().models.into_iter().find(|m| m.id == "small").unwrap().state, "unloaded");
    w.end().await;
}

#[tokio::test]
async fn a_session_can_think_first_and_the_thought_can_be_ended() {
    let w = world("thinks");
    *w.stand.thought.lock().unwrap() = "The user wants colours. Primary colours of paint are red blue yellow. I should answer briefly and not ramble on about it.".into();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    assert_eq!(session.thinking, "off");

    // Before the model was ever loaded, what it can do is not known: on is taken on trust, a level is not.
    assert!(matches!(w.chat.change(&session.id, None, None, Some("high")).await, Err(Error::Invalid(_))));
    assert_eq!(w.chat.change(&session.id, None, None, Some("on")).await.unwrap().thinking, "on");

    // Thinking on: the thought arrives apart from the answer and is kept apart from it.
    let mut ear = w.events.listen();
    w.chat.say(&session.id, "Name three primary colours.").unwrap();
    let answer = w.answered(&session.id).await;
    assert_eq!(answer.content, "Red, blue and yellow are the three primary colours of paint.");
    assert!(answer.thought.as_deref().is_some_and(|t| t.starts_with("The user wants colours.") && t.ends_with("about it.")), "{:?}", answer.thought);
    let mut told_thought = String::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "message.delta" {
            told_thought.push_str(e.data["thought"].as_str().unwrap_or(""));
        }
    }
    assert_eq!(Some(told_thought), answer.thought);
    let asked = w.stand.asked.lock().unwrap().last().cloned().unwrap();
    assert_eq!((&asked["chat_template_kwargs"]["enable_thinking"], &asked["reasoning_control"], asked.get("reasoning_effort")), (&json!(true), &json!(true), None));

    // Now that it was loaded, the runtime has said what it can do: think, on or off. A level it does not have is refused.
    assert_eq!(w.engine.list().await.unwrap().models.into_iter().find(|m| m.id == "small").unwrap().thinking, Some(vec!["on".to_string()]));
    let err = w.chat.change(&session.id, None, None, Some("high")).await.unwrap_err().to_string();
    assert!(err.contains("off, on"), "{err}");

    // A thought that runs long is ended from outside, and the model goes on to its answer.
    w.stand.word_every_ms.store(40, Ordering::Relaxed);
    let mut ear = w.events.listen();
    w.chat.say(&session.id, "Again.").unwrap();
    loop {
        let e = tokio::time::timeout(Duration::from_secs(10), ear.recv()).await.expect("a thought within ten seconds").unwrap();
        if e.kind == "message.delta" && !e.data["thought"].as_str().unwrap_or("").is_empty() {
            break;
        }
    }
    w.stand.word_every_ms.store(1, Ordering::Relaxed);
    assert!(w.chat.answer_now(&session.id).await.unwrap());
    let cut = w.answered(&session.id).await;
    assert_eq!((cut.state.as_str(), cut.content.as_str()), ("done", "Red, blue and yellow are the three primary colours of paint."));
    assert!(cut.thought.as_deref().is_some_and(|t| t.len() < 100), "the thought was cut short: {:?}", cut.thought);
    assert!(!w.chat.answer_now(&session.id).await.unwrap(), "nothing is being thought now");

    // Off again: no thought is asked for, and none is kept.
    w.chat.change(&session.id, None, None, Some("off")).await.unwrap();
    w.chat.say(&session.id, "Once more.").unwrap();
    assert_eq!(w.answered(&session.id).await.thought, None);
    w.end().await;
}

#[tokio::test]
async fn a_model_that_does_not_think_cannot_be_told_to() {
    let w = world("no-thought");
    let session = w.chat.start(None, Some("small")).await.unwrap();
    w.chat.say(&session.id, "Hello.").unwrap();
    w.answered(&session.id).await;
    assert_eq!(w.engine.list().await.unwrap().models.into_iter().find(|m| m.id == "small").unwrap().thinking, Some(vec![]));
    let err = w.chat.change(&session.id, None, None, Some("on")).await.unwrap_err().to_string();
    assert!(err.contains("does not think"), "{err}");
    assert_eq!(w.chat.change(&session.id, Some("Greeting"), None, Some("off")).await.unwrap().title, "Greeting");
    assert!(matches!(w.chat.change(&session.id, None, Some("no-such-model"), None).await, Err(Error::NotFound(_))));
    w.end().await;
}

/// Says something and waits for the answer.
async fn turn(w: &World, session_id: &str, text: &str) -> Message {
    w.chat.say(session_id, text).unwrap();
    w.answered(session_id).await
}

#[tokio::test]
async fn a_conversation_is_made_shorter_by_hand_and_the_model_gets_the_account() {
    let w = world("shorter");
    let session = w.chat.start(None, Some("small")).await.unwrap();
    // Too short to make shorter: there is nothing older than the last exchange.
    turn(&w, &session.id, "My name is Ada and I like green.").await;
    assert!(matches!(w.chat.compact(&session.id, "asked").await, Err(Error::Invalid(_))));
    turn(&w, &session.id, "I live in Turin.").await;
    turn(&w, &session.id, "What do I like?").await;

    *w.stand.reply.lock().unwrap() = "Ada likes green and lives in Turin.".into();
    let account = w.chat.compact(&session.id, "asked").await.unwrap();
    assert_eq!((account.upto_seq, account.content.as_str(), account.why.as_str()), (4, "Ada likes green and lives in Turin.", "asked"));
    // The model was asked for an account of the older four messages, and of nothing newer.
    let asked = w.stand.asked.lock().unwrap().last().cloned().unwrap();
    assert_eq!(asked["messages"][0]["content"], ACCOUNT_ROLE);
    let transcript = asked["messages"][1]["content"].as_str().unwrap();
    // The conversation first, fenced off as material. What to do with it comes after it: told first, small
    // models go on with the conversation in place of writing the account.
    assert!(transcript.starts_with("<conversation>\nPerson: My name is Ada and I like green.") && transcript.contains("Person: I live in Turin."), "{transcript}");
    assert!(transcript.ends_with(&format!("</conversation>\n\n{ACCOUNT_INSTRUCTIONS}")), "{transcript}");
    assert!(!transcript.contains("What do I like?"), "the last exchange stays word for word");
    assert_eq!(asked["chat_template_kwargs"]["enable_thinking"], false);
    assert!(kinds(&w.events).contains(&"session.compacted".to_string()));

    // From now on the model is given the account in place of those messages. The messages are all still there.
    *w.stand.reply.lock().unwrap() = "Green.".into();
    turn(&w, &session.id, "And where do I live?").await;
    let next = w.stand.asked.lock().unwrap().last().cloned().unwrap();
    let told = next["messages"][0]["content"].as_str().unwrap();
    assert!(told.starts_with(TOLD) && told.ends_with("Earlier in this conversation, in short:\nAda likes green and lives in Turin."), "{told}");
    assert_eq!(
        next["messages"].as_array().unwrap().iter().skip(1).map(|m| m["content"].as_str().unwrap()).collect::<Vec<_>>(),
        ["What do I like?", "Red, blue and yellow are the three primary colours of paint.", "And where do I live?"]
    );
    assert_eq!(w.sessions.messages(&session.id).unwrap().len(), 8);

    // Made shorter a second time: the new account is written from the old one and what came after it.
    *w.stand.reply.lock().unwrap() = "Ada, Turin, green. She asked what she likes.".into();
    let second = w.chat.compact(&session.id, "asked").await.unwrap();
    assert_eq!(second.upto_seq, 6);
    let again = w.stand.asked.lock().unwrap().last().cloned().unwrap();
    assert!(again["messages"][1]["content"].as_str().unwrap().starts_with("<conversation>\nNotes on what came before:\nAda likes green and lives in Turin."));
    assert_eq!(w.sessions.summaries(&session.id).unwrap().len(), 2);
    w.end().await;
}

#[tokio::test]
async fn a_conversation_that_grows_too_long_is_made_shorter_by_itself() {
    let w = world("auto");
    w.settings.set("context.compactAt", json!("1000")).unwrap();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let long = |about: &str| format!("{about} {}", "and so on ".repeat(100));
    turn(&w, &session.id, &long("First: the budget is 400 euros.")).await;
    let room = w.chat.context_use(&session.id).unwrap();
    assert_eq!((room.limit, room.compact_at, room.auto), (8192, 1000, true));
    assert!(room.used > 200 && room.used < 1000, "one long message is not yet too much: {}", room.used);
    assert!(w.sessions.summaries(&session.id).unwrap().is_empty());

    // More long messages, until one of them finds the conversation past the size. Before that message is
    // answered, an account of the older part is written; then the answer.
    *w.stand.reply.lock().unwrap() = "Budget 400 euros. More was said after.".into();
    let mut said = 1;
    let (before, text) = loop {
        said += 1;
        assert!(said <= 8, "the conversation was never made shorter");
        let before = w.stand.asked.lock().unwrap().len();
        let text = long(&format!("Message {said}: what was the budget?"));
        assert_eq!(turn(&w, &session.id, &text).await.state, "done");
        if !w.sessions.summaries(&session.id).unwrap().is_empty() {
            break (before, text);
        }
        assert_eq!(w.stand.asked.lock().unwrap().len(), before + 1, "under the size, the model is asked once: for the answer");
    };
    let asked = w.stand.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), before + 2, "one request for the account, one for the answer");
    assert_eq!(asked[before]["messages"][0]["content"], ACCOUNT_ROLE);
    assert!(asked[before]["messages"][1]["content"].as_str().unwrap().starts_with("<conversation>\nPerson: First: the budget is 400 euros."));
    let prompt = &asked[before + 1]["messages"];
    assert!(prompt[0]["content"].as_str().unwrap().ends_with("Earlier in this conversation, in short:\nBudget 400 euros. More was said after."));
    assert!(!prompt.to_string().contains("First: the budget"), "the oldest message is no longer given word for word");
    assert_eq!(prompt.as_array().unwrap().last().unwrap()["content"].as_str(), Some(text.trim()), "what was just said is");
    let accounts = w.sessions.summaries(&session.id).unwrap();
    assert_eq!((accounts.len(), accounts[0].why.as_str()), (1, "auto"));
    assert!(accounts[0].tokens_before > 1000, "it was past the size: {}", accounts[0].tokens_before);
    assert!(w.chat.context_use(&session.id).unwrap().used < accounts[0].tokens_before, "and is smaller now");
    let log = w.events.after(0, 200).unwrap();
    assert!(log.iter().any(|e| e.kind == "session.compacted" && e.data["why"] == "auto"));

    // Switched off, it is left alone however long it gets.
    w.settings.set("context.autoCompact", json!(false)).unwrap();
    for i in 0..4 {
        turn(&w, &session.id, &long(&format!("Later {i}."))).await;
    }
    assert_eq!(w.sessions.summaries(&session.id).unwrap().len(), 1);
    assert!(w.chat.context_use(&session.id).unwrap().used > 1000);
    w.end().await;
}

#[tokio::test]
async fn the_size_is_three_quarters_of_the_room_unless_a_smaller_one_is_chosen() {
    let w = world("sizes");
    let session = w.chat.start(None, Some("small")).await.unwrap();
    assert_eq!(w.chat.context_use(&session.id).unwrap().compact_at, 6144, "three quarters of 8192");
    w.settings.set("context.compactAt", json!("24000")).unwrap();
    assert_eq!(w.chat.context_use(&session.id).unwrap().compact_at, 6144, "a size above the room is brought down to it");
    w.settings.set("models.context", json!("32768")).unwrap();
    assert_eq!((w.chat.context_use(&session.id).unwrap().limit, w.chat.context_use(&session.id).unwrap().compact_at), (32768, 24000));
    // While an answer is being written, the session cannot be made shorter.
    w.stand.word_every_ms.store(40, Ordering::Relaxed);
    w.chat.say(&session.id, "Hello.").unwrap();
    assert!(matches!(w.chat.compact(&session.id, "asked").await, Err(Error::Busy(_))));
    w.chat.stop(&session.id);
    w.answered(&session.id).await;
    w.end().await;
}

#[tokio::test]
async fn an_answer_is_asked_with_the_models_own_settings_thinking_or_not() {
    use crate::engine::sampling::Sampling;
    let w = world("sampling");
    // The file says it is Qwen3 1.7B, as Qwen's own GGUF does: Qwen's card says how to ask it.
    let texts = [("general.architecture", "qwen3"), ("general.basename", "Qwen3"), ("general.size_label", "1.7B"), ("general.finetune", "Instruct")];
    crate::engine::gguf::write_for_test(&w.home.join("models/small.gguf"), &texts, &[]);
    // It can think: the stand-in thinks when it is given something to think.
    *w.stand.thought.lock().unwrap() = "A short thought.".into();
    let session = w.chat.start(None, Some("small")).await.unwrap();
    let ask = |text: &'static str| {
        let (w, id) = (&w, session.id.clone());
        async move {
            w.chat.say(&id, text).unwrap();
            w.answered(&id).await;
            w.stand.asked.lock().unwrap().last().cloned().unwrap()
        }
    };
    let plain = ask("Hello.").await;
    assert_eq!((plain["temperature"].as_f64(), plain["top_p"].as_f64(), plain["top_k"].as_u64(), plain["min_p"].as_f64()), (Some(0.7), Some(0.8), Some(20), Some(0.0)), "{plain}");
    w.chat.change(&session.id, None, None, Some("on")).await.unwrap();
    let thinking = ask("And now?").await;
    assert_eq!((thinking["temperature"].as_f64(), thinking["top_p"].as_f64()), (Some(0.6), Some(0.95)), "never greedy when it thinks");
    // The person's own setting goes over the maker's, and only that one.
    w.engine.set_model_settings("small", Sampling { temperature: Some(0.2), ..Sampling::default() }, Sampling::default()).unwrap();
    let mine = ask("Once more.").await;
    assert_eq!((mine["temperature"].as_f64(), mine["top_p"].as_f64()), (Some(0.2), Some(0.95)));
    let settings = w.engine.model_settings("small").unwrap();
    assert_eq!((settings.thinking.from.as_str(), settings.plain.from.as_str(), settings.maker.map(|m| m.who)), ("yours", "maker", Some("Qwen".to_string())));
    // A file that says nothing of itself is asked with nothing: the runtime's own settings apply.
    assert_eq!(w.engine.model_settings("big").unwrap().plain.from, "runtime");
    assert!(w.engine.sampling_for("big", false).is_empty());
    w.end().await;
}
