//! The engine, against a stand-in for the runtime's router that runs inside the test (`stand_in.rs`).

use std::sync::atomic::Ordering;

use super::stand_in::{Stand, StandLauncher};
use super::*;
use crate::db::Db;
use crate::runtime::Source;

struct World {
    engine: Engine,
    stand: Arc<Stand>,
    events: Arc<EventLog>,
    settings: Arc<Settings>,
    home: PathBuf,
}

/// The same data folder, opened as a Demido that was started again would open it.
fn world_at(home: &Path) -> World {
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db, events.clone()).unwrap());
    let runtimes = Arc::new(Runtimes::new(home, events.clone(), Source::default()).unwrap());
    let stand = Stand::new(home.join("models"));
    let engine = Engine::new(home, runtimes, settings.clone(), events.clone(), Arc::new(StandLauncher(stand.clone()))).stop_empty_router_after(Duration::from_millis(40));
    World { engine, stand, events, settings, home: home.to_path_buf() }
}

/// A data folder with three models (one of them a folder of two files) and, unless `bare`, an installed runtime.
fn world(name: &str, bare: bool) -> World {
    let home = std::env::temp_dir().join(format!("demido-engine-{name}-{}", uuid::Uuid::new_v4()));
    let models = home.join("models");
    std::fs::create_dir_all(models.join("vision")).unwrap();
    std::fs::write(models.join("small.gguf"), vec![0u8; 100]).unwrap();
    std::fs::write(models.join("Big.gguf"), vec![0u8; 3000]).unwrap();
    std::fs::write(models.join("vision/vision.gguf"), vec![0u8; 500]).unwrap();
    std::fs::write(models.join("vision/mmproj-f16.gguf"), vec![0u8; 50]).unwrap();
    std::fs::write(models.join("notes.txt"), "not a model").unwrap();
    if !bare {
        let folder = home.join("runtimes/llama.cpp/llama.cpp-test-cpu");
        std::fs::create_dir_all(&folder).unwrap();
        let note = json!({ "id": "llama.cpp-test-cpu", "runtime": "llama.cpp", "build": "test", "backend": "cpu", "name": "llama.cpp · CPU", "files": [], "checked": true,
            "server": "llama-server", "lib_dirs": [], "version": "version: test", "problem": null, "installed_at": 1, "size": 1 });
        std::fs::write(folder.join("demido-runtime.json"), note.to_string()).unwrap();
    }
    let db = Arc::new(Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db.clone()));
    let settings = Arc::new(Settings::load(db, events.clone()).unwrap());
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), Source::default()).unwrap());
    let stand = Stand::new(models);
    let engine = Engine::new(&home, runtimes, settings.clone(), events.clone(), Arc::new(StandLauncher(stand.clone()))).stop_empty_router_after(Duration::from_millis(40));
    World { engine, stand, events, settings, home }
}

fn kinds(events: &EventLog) -> Vec<String> {
    events.after(0, 100).unwrap().into_iter().map(|e| e.kind).collect()
}

/// The router's news reaches the log a moment after the fact.
async fn until_logged(events: &EventLog, kind: &str) {
    // Generous: a shared build machine can be slow, and this only waits as long as it has to.
    for _ in 0..500 {
        if kinds(events).iter().any(|k| k == kind) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("\"{kind}\" never reached the log; it holds {:?}", kinds(events));
}

#[tokio::test]
async fn looking_at_the_models_starts_nothing() {
    let w = world("look", false);
    let list = w.engine.list().await.unwrap();
    assert_eq!(list.models.iter().map(|m| (m.id.as_str(), m.size, m.state.as_str())).collect::<Vec<_>>(), [("Big", 3000, "unloaded"), ("small", 100, "unloaded"), ("vision", 550, "unloaded")]);
    assert_eq!((list.engine.running, list.engine.runtime.as_deref()), (false, Some("llama.cpp · CPU (test)")));
    assert_eq!((list.engine.context, list.engine.margin_mb, list.engine.idle_unload_min), (8192, 922, Some(5)));
    assert_eq!(w.stand.launches.load(Ordering::Relaxed), 0);
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_model_is_deleted_from_the_folder_loaded_or_not_with_what_was_kept_about_it() {
    let w = world("delete", false);
    let models = w.home.join("models");
    std::fs::write(models.join(".sources.json"), r#"{"small": {"repo": "test/small-GGUF"}, "Big": {"repo": "test/big-GGUF"}}"#).unwrap();
    w.engine.set_model_settings("small", crate::engine::sampling::Sampling { temperature: Some(0.3), ..Default::default() }, Default::default()).unwrap();
    // Loaded: it is unloaded, then deleted.
    w.engine.load("small").await.unwrap();
    let gone = w.engine.delete("small").await.unwrap();
    assert_eq!((gone.id.as_str(), gone.size), ("small", 100));
    assert!(!models.join("small.gguf").exists());
    let ids: Vec<String> = w.engine.list().await.unwrap().models.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["Big", "vision"]);
    let sources: Value = serde_json::from_slice(&std::fs::read(models.join(".sources.json")).unwrap()).unwrap();
    assert!(sources.get("small").is_none() && sources.get("Big").is_some(), "{sources}");
    assert!(kinds(&w.events).contains(&"model.deleted".to_string()));
    // A model of several files goes as a folder; one that is not there is said to be not there.
    w.engine.delete("vision").await.unwrap();
    assert!(!models.join("vision").exists());
    assert!(matches!(w.engine.delete("nothing-here").await, Err(Error::NotFound(_))));
    // What was not a model stays.
    assert!(models.join("notes.txt").exists());
    w.engine.shutdown().await;
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_model_is_loaded_and_unloaded_and_the_runtime_is_started_once() {
    let w = world("load", false);
    let loaded = w.engine.load("small").await.unwrap();
    assert_eq!((loaded.state.as_str(), loaded.context_length), ("loaded", Some(40960)));
    assert!(loaded.unloads_in_s.is_some_and(|s| s > 290), "five minutes of idleness lie ahead: {:?}", loaded.unloads_in_s);

    // The runtime was started the way K22 says: nothing loads by itself, no second web UI, this computer only,
    // the context pinned and a margin set.
    let args = w.stand.args.lock().unwrap().join(" ");
    for wanted in ["--no-models-autoload", "--no-webui", "--host 127.0.0.1", "-c 8192", "--models-memory-margin 922", "--models-max 0", "--api-key-file"] {
        assert!(args.contains(wanted), "{wanted} is missing from: {args}");
    }

    w.engine.load("Big").await.unwrap();
    w.engine.load("small").await.unwrap();
    assert_eq!(w.stand.launches.load(Ordering::Relaxed), 1, "one runtime serves every model");
    assert_eq!(w.stand.loads_asked.load(Ordering::Relaxed), 2, "a model that is loaded is not asked for again");
    until_logged(&w.events, "model.loaded").await;

    w.engine.unload("small", "asked").await.unwrap();
    until_logged(&w.events, "model.unloaded").await;
    let list = w.engine.list().await.unwrap();
    assert_eq!(list.models.iter().map(|m| m.state.as_str()).collect::<Vec<_>>(), ["loaded", "unloaded", "unloaded"]);
    assert!(list.engine.running);
    assert!(matches!(w.engine.unload("small", "asked").await, Err(Error::Invalid(_))), "it is not loaded any more");
    assert!(kinds(&w.events).starts_with(&["engine.started".to_string()]));

    w.engine.shutdown().await;
    assert!(!w.stand.is_alive(), "stopping the server stops the runtime");
    let after = w.engine.list().await.unwrap();
    assert!(!after.engine.running);
    // What was learned while it ran is still shown, though nothing is running to ask.
    assert_eq!(after.models[1].context_length, Some(40960));
    assert!(after.models[1].loads_in_ms.is_some(), "how long the last load took is kept");
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_model_nobody_uses_is_unloaded_and_an_empty_runtime_is_stopped() {
    let mut w = world("idle", false);
    // The waits leave wide gaps on both sides, so a slow, busy build machine does not change the outcome.
    w.engine.idle_for_tests = Some(Duration::from_millis(600));
    w.engine.load("small").await.unwrap();
    w.engine.look_after().await;
    assert_eq!(w.engine.list().await.unwrap().models[1].state, "loaded", "it was used a moment ago");

    // Using it starts the wait again: 400 ms and then 100 ms is more than 600 in all, but neither is since the last use.
    tokio::time::sleep(Duration::from_millis(400)).await;
    w.engine.used("small").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    w.engine.look_after().await;
    assert_eq!(w.engine.list().await.unwrap().models[1].state, "loaded");

    tokio::time::sleep(Duration::from_millis(700)).await;
    w.engine.look_after().await;
    assert_eq!(w.engine.list().await.unwrap().models[1].state, "unloaded", "idle for longer than allowed");
    assert!(w.engine.list().await.unwrap().engine.running, "the runtime stays a while, in case another model is wanted");

    w.engine.look_after().await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    w.engine.look_after().await;
    assert!(!w.engine.list().await.unwrap().engine.running, "with nothing loaded for a while, the runtime is stopped");
    until_logged(&w.events, "engine.stopped").await;
    assert_eq!(w.events.after(0, 100).unwrap().iter().find(|e| e.kind == "engine.stopped").unwrap().data["unexpected"], false);
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn never_means_never() {
    let w = world("never", false);
    w.settings.set("models.idleUnload", json!("never")).unwrap();
    w.engine.load("small").await.unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    w.engine.look_after().await;
    let small = &w.engine.list().await.unwrap().models[1];
    assert_eq!((small.state.as_str(), small.unloads_in_s), ("loaded", None));
    w.engine.shutdown().await;
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_runtime_that_dies_is_noticed_and_started_again_when_needed() {
    let w = world("dies", false);
    w.engine.load("small").await.unwrap();
    w.stand.die();
    w.engine.look_after().await;
    assert!(!w.engine.list().await.unwrap().engine.running);
    assert_eq!(w.events.after(0, 100).unwrap().iter().find(|e| e.kind == "engine.stopped").unwrap().data["unexpected"], true);

    assert_eq!(w.engine.load("small").await.unwrap().state, "loaded");
    assert_eq!(w.stand.launches.load(Ordering::Relaxed), 2);
    w.engine.shutdown().await;
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_model_that_cannot_fit_is_refused_before_the_runtime_is_asked() {
    let w = world("fit", false);
    w.stand.needs_mb.lock().unwrap().insert("Big".into(), 11_500); // the card has 12,000 and 922 are kept free
    w.stand.needs_mb.lock().unwrap().insert("small".into(), 1_400);
    let err = w.engine.load("Big").await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    assert!(err.to_string().contains("needs 11500 MiB") && err.to_string().contains("cannot fit"), "{err}");
    assert_eq!(w.stand.loads_asked.load(Ordering::Relaxed), 0);
    // One that fits is loaded, and says what it needs.
    assert_eq!(w.engine.load("small").await.unwrap().needs_mb, Some(1_400));
    w.engine.shutdown().await;

    // What each model needs was written down: it is known without the runtime, and after a restart of Demido.
    let needs = |list: ModelList| list.models.iter().map(|m| (m.id.clone(), m.needs_mb)).collect::<Vec<_>>();
    assert_eq!(needs(w.engine.list().await.unwrap()), [("Big".to_string(), Some(11_500)), ("small".to_string(), Some(1_400)), ("vision".to_string(), None)]);
    let again = world_at(&w.home);
    assert_eq!(needs(again.engine.list().await.unwrap())[1], ("small".to_string(), Some(1_400)));
    assert_eq!(again.stand.launches.load(Ordering::Relaxed), 0, "nothing was started to know it");
    // It was measured at a context of 8192. At another context it is not known.
    again.settings.set("models.context", json!("4096")).unwrap();
    assert_eq!(needs(again.engine.list().await.unwrap())[1].1, None);
    again.settings.reset("models.context").unwrap();
    // Another file under the same name: what was known was about the old one.
    std::fs::write(w.home.join("models/small.gguf"), vec![0u8; 777]).unwrap();
    let replaced = again.engine.list().await.unwrap();
    assert_eq!((replaced.models[1].needs_mb, replaced.models[1].loads_in_ms, replaced.models[1].size), (None, None, 777));
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn without_a_runtime_or_without_the_model_it_says_what_is_missing() {
    let w = world("bare", true);
    let err = w.engine.load("small").await.unwrap_err();
    assert!(err.to_string().contains("No model runtime is installed"), "{err}");
    assert!(matches!(w.engine.load("no-such-model").await, Err(Error::NotFound(_))));
    assert!(matches!(w.engine.unload("small", "asked").await, Err(Error::Invalid(_))));
    assert_eq!(w.stand.launches.load(Ordering::Relaxed), 0);
    assert_eq!(w.engine.list().await.unwrap().engine.runtime, None);
    std::fs::remove_dir_all(&w.home).ok();
}

#[test]
fn the_routers_news_becomes_demidos() {
    let events = EventLog::new(Arc::new(Db::memory().unwrap()));
    let mut ear = events.listen();
    news_from(&events, &json!({ "model": "m", "event": "status_change", "data": { "status": "loading", "progress": { "current": "text_model", "value": 0.5 } } }));
    news_from(&events, &json!({ "model": "m", "event": "status_change", "data": { "status": "loaded", "info": {} } }));
    news_from(&events, &json!({ "model": "m", "event": "status_change", "data": { "status": "unloaded", "exit_code": 0 } }));
    news_from(&events, &json!({ "model": "m", "event": "status_change", "data": { "status": "unloaded", "exit_code": 137 } }));
    news_from(&events, &json!({ "event": "status_change" }));
    let told = ear.try_recv().unwrap();
    assert_eq!((told.kind.as_str(), told.seq, told.data["progress"].as_f64()), ("model.loading", 0, Some(0.5)), "progress is told, not kept");
    let kept = events.after(0, 10).unwrap();
    assert_eq!(kept.iter().map(|e| (e.kind.as_str(), e.data["crashed"].as_bool())).collect::<Vec<_>>(), [("model.loaded", None), ("model.unloaded", Some(false)), ("model.unloaded", Some(true))]);
}
