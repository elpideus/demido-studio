//! A turn from end to end, against the stand-in runtime (`engine/stand_in.rs`).

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use super::*;
use crate::db::Db;
use crate::engine::stand_in::{Stand, StandLauncher};
use crate::runtime::{Runtimes, Source};
use crate::settings::Settings;

struct World {
    chat: Arc<Chat>,
    sessions: Arc<Sessions>,
    engine: Arc<Engine>,
    events: Arc<EventLog>,
    stand: Arc<Stand>,
    home: PathBuf,
}

fn world(name: &str) -> World {
    world_where(name, None)
}

/// A data folder with an installed runtime and two models, and everything a turn needs on top of it.
/// `idle_after`: how soon an unused model counts as idle, for the test that needs it short.
fn world_where(name: &str, idle_after: Option<Duration>) -> World {
    let home = std::env::temp_dir().join(format!("demido-chat-{name}-{}", uuid::Uuid::new_v4()));
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
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), Source::default()).unwrap());
    let stand = Stand::new(models);
    let engine = Engine::new(&home, runtimes, settings, events.clone(), Arc::new(StandLauncher(stand.clone())));
    let engine = Arc::new(match idle_after {
        Some(wait) => engine.idle_after(wait),
        None => engine,
    });
    let sessions = Arc::new(Sessions::new(db));
    let chat = Arc::new(Chat::new(sessions.clone(), engine.clone(), events.clone()));
    World { chat, sessions, engine, events, stand, home }
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
        self.engine.shutdown().await;
        std::fs::remove_dir_all(&self.home).ok();
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
    assert_eq!(first[0], json!({ "role": "system", "content": INSTRUCTIONS }));
    assert_eq!(first.len(), 2);
    // The second prompt is the first one, unchanged, with the answer and the new question added at the end.
    assert_eq!(&second[..2], &first[..]);
    assert_eq!(second[2], json!({ "role": "assistant", "content": "Red, blue and yellow are the three primary colours of paint." }));
    assert_eq!(second[3], json!({ "role": "user", "content": "And three secondary ones?" }));
    assert_eq!(asked[1]["model"], "small");
    assert_eq!(asked[1]["chat_template_kwargs"]["enable_thinking"], false, "thinking is off until there is a switch for it");
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

    // With no models at all, there is nothing to start a session with, and it says where to put one.
    std::fs::remove_file(w.home.join("models/small.gguf")).unwrap();
    std::fs::remove_file(w.home.join("models/big.gguf")).unwrap();
    let err = w.chat.start(None, None).await.unwrap_err().to_string();
    assert!(err.contains("Put a GGUF file in"), "{err}");
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
