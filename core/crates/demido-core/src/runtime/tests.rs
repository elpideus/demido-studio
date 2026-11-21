//! The whole installation, against a stand-in publisher on this computer.
//!
//! The stand-in answers like GitHub's release list and serves the files, and can be told to misbehave:
//! break a connection half-way, serve slowly, or hand out a file that is not the one it listed.
//! The "runtime" it publishes is a three-line script that says a version, so the last step (does it start?)
//! is real too. No test touches the network.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};

use super::unpack::tests::{tar_with, temp};
use super::*;
use crate::db::Db;
use crate::system::Build;

const MAIN: &str = "llama-b777-bin-ubuntu-cuda-13.5-x64.tar.gz";
const EXTRA: &str = "cudart-llama-b777-bin-ubuntu-cuda-13.5-x64.tar.gz";

#[derive(Default)]
struct Stand {
    files: Mutex<Vec<(String, Vec<u8>)>>,
    /// What the release list claims each file's SHA-256 is, when it should differ from the truth.
    claimed: Mutex<Option<String>>,
    /// Where the release list says the files are, when not here.
    elsewhere: Mutex<Option<String>>,
    break_once: AtomicBool,
    slow: AtomicBool,
    ranges_seen: Mutex<Vec<String>>,
    /// Questions about the stable release that got a full answer, and those answered "not changed".
    stable_asked: AtomicUsize,
    stable_unchanged: AtomicUsize,
    /// Questions about a build's list of files.
    build_asked: AtomicUsize,
    /// The stable release's mark. Changing it is how the stand-in "publishes something new".
    mark: Mutex<String>,
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

async fn latest(State((stand, base)): State<(Arc<Stand>, String)>, headers: HeaderMap) -> Response {
    let mark = stand.mark.lock().unwrap().clone();
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(mark.as_str()) {
        stand.stable_unchanged.fetch_add(1, Ordering::Relaxed);
        return StatusCode::NOT_MODIFIED.into_response();
    }
    stand.stable_asked.fetch_add(1, Ordering::Relaxed);
    let body = json!({ "tag_name": "v9.9.9", "assets": [{ "name": "nightly-tag.txt", "size": 5, "digest": null, "browser_download_url": format!("{base}/dl/v9.9.9/nightly-tag.txt") }] });
    ([(header::ETAG, mark)], axum::Json(body)).into_response()
}

async fn build(State((stand, base)): State<(Arc<Stand>, String)>) -> axum::Json<Value> {
    stand.build_asked.fetch_add(1, Ordering::Relaxed);
    let from = stand.elsewhere.lock().unwrap().clone().unwrap_or(format!("{base}/dl/b777"));
    let claimed = stand.claimed.lock().unwrap().clone();
    let assets: Vec<Value> = stand
        .files
        .lock()
        .unwrap()
        .iter()
        .map(|(name, bytes)| json!({ "name": name, "size": bytes.len(), "digest": format!("sha256:{}", claimed.clone().unwrap_or_else(|| sha(bytes))), "browser_download_url": format!("{from}/{name}") }))
        .collect();
    axum::Json(json!({ "tag_name": "b777", "assets": assets }))
}

async fn file(State((stand, _)): State<(Arc<Stand>, String)>, UrlPath((_tag, name)): UrlPath<(String, String)>, headers: HeaderMap) -> Response {
    if name == "nightly-tag.txt" {
        return "b777\n".into_response();
    }
    let Some(bytes) = stand.files.lock().unwrap().iter().find(|(n, _)| *n == name).map(|(_, b)| b.clone()) else { return StatusCode::NOT_FOUND.into_response() };
    let from = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(range) => {
            stand.ranges_seen.lock().unwrap().push(range.to_string());
            range.trim_start_matches("bytes=").trim_end_matches('-').parse::<usize>().unwrap_or(0)
        }
        None => 0,
    };
    if from > bytes.len() {
        return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
    }
    let status = if from > 0 { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK };
    let rest = bytes[from..].to_vec();
    if stand.break_once.swap(false, Ordering::Relaxed) {
        // Half of it, then the line goes dead.
        let half = rest[..rest.len() / 2].to_vec();
        let pieces = tokio_stream::StreamExt::then(tokio_stream::iter(vec![Ok::<_, std::io::Error>(half), Err(std::io::Error::other("the line went dead"))]), |piece| async move {
            // Let the first half reach the other side before the line dies.
            if piece.is_err() {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            piece
        });
        return (status, Body::from_stream(pieces)).into_response();
    }
    if stand.slow.load(Ordering::Relaxed) {
        let pieces: Vec<Vec<u8>> = rest.chunks(rest.len().div_ceil(40).max(1)).map(<[u8]>::to_vec).collect();
        let slowly = tokio_stream::StreamExt::then(tokio_stream::iter(pieces), |piece| async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok::<_, std::io::Error>(piece)
        });
        return (status, Body::from_stream(slowly)).into_response();
    }
    (status, rest).into_response()
}

struct World {
    stand: Arc<Stand>,
    runtimes: Arc<Runtimes>,
    events: Arc<EventLog>,
    home: PathBuf,
    advice: Advice,
    source: Source,
}

/// A stand-in publisher with one build: the program, and a library beside it as the "extra" file.
async fn world(name: &str) -> World {
    let home = temp(name);
    let scratch = home.join("making");
    std::fs::create_dir_all(&scratch).unwrap();
    use tar::EntryType as T;
    // Padded so that a download has a middle to break in.
    let padding = vec![b'#'; 200_000];
    let script = [b"#!/bin/sh\nif [ \"$1\" = --list-devices ]; then echo 'Available devices:'; echo '  CUDA0: Stand-in card (1024 MiB, 1000 MiB free)'; exit 0; fi\necho 'version: 777 (stand-in)'\necho 'built for a test'\n# ".as_slice(), &padding, b"\n"].concat();
    tar_with(&scratch.join(MAIN), &[("build/bin/", T::Directory, b"", None), ("build/bin/llama-server", T::Regular, &script, None), ("build/bin/libllama.so", T::Regular, b"lib", None)]);
    tar_with(&scratch.join(EXTRA), &[("libcudart.so.13", T::Regular, b"cuda", None)]);
    let stand = Arc::new(Stand::default());
    *stand.mark.lock().unwrap() = "W/\"first\"".into();
    *stand.files.lock().unwrap() = vec![(MAIN.into(), std::fs::read(scratch.join(MAIN)).unwrap()), (EXTRA.into(), std::fs::read(scratch.join(EXTRA)).unwrap())];

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/repos/o/r/releases/latest", get(latest))
        .route("/repos/o/r/releases/tags/b777", get(build))
        .route("/dl/{tag}/{name}", get(file))
        .with_state((stand.clone(), base.clone()));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let events = Arc::new(EventLog::new(Arc::new(Db::memory().unwrap())));
    let source = Source { api: base.clone(), repo: "o/r".into(), downloads: format!("{base}/dl/") };
    let runtimes = Arc::new(Runtimes::new(&home, events.clone(), source.clone()).unwrap());
    let advice = Advice {
        runtime: RUNTIME.into(),
        source: String::new(),
        recommended: Build { id: "cuda-13.5".into(), name: "llama.cpp · CUDA 13.5".into(), file: "llama-{build}-bin-ubuntu-cuda-13.5-x64.tar.gz".into(), why: "for the test".into() },
        others: vec![],
    };
    World { stand, runtimes, events, home, advice, source }
}

fn kinds(events: &EventLog) -> Vec<String> {
    events.after(0, 100).unwrap().into_iter().map(|e| e.kind).collect()
}

/// How often the stand-in was asked: (the stable release in full, "not changed", a build's files).
fn asked(stand: &Stand) -> (usize, usize, usize) {
    (stand.stable_asked.load(Ordering::Relaxed), stand.stable_unchanged.load(Ordering::Relaxed), stand.build_asked.load(Ordering::Relaxed))
}

#[tokio::test]
async fn the_publisher_is_asked_as_little_as_possible() {
    let w = world("list").await;
    let a = w.runtimes.available(&w.advice, false).await.unwrap();
    assert_eq!((a.release.as_str(), a.build.as_str(), a.stale.as_deref()), ("v9.9.9", "b777", None));
    assert_eq!(a.offers.len(), 1);
    assert!(a.offers[0].recommended && a.offers[0].file.sha256.is_some());
    assert_eq!(a.offers[0].extra[0].name, EXTRA);
    assert_eq!(asked(&w.stand), (1, 0, 1), "the first look asks for the stable release and its build");

    // Within the hour: nothing is asked.
    w.runtimes.available(&w.advice, false).await.unwrap();
    assert_eq!(asked(&w.stand), (1, 0, 1));

    // Asked to look again now: one question, "only if it changed". It did not.
    let again = w.runtimes.available(&w.advice, true).await.unwrap();
    assert_eq!(asked(&w.stand), (1, 1, 1));
    assert!(again.asked_at >= a.asked_at && again.offers.len() == 1);

    // After a restart the answer is still there, read from disk: nothing is asked.
    let restarted = Runtimes::new(&w.home, w.events.clone(), w.source.clone()).unwrap();
    let kept = restarted.available(&w.advice, false).await.unwrap();
    assert_eq!((kept.build.as_str(), asked(&w.stand)), ("b777", (1, 1, 1)));

    // The stable release changes but still points at the same build: its files are not asked for again.
    *w.stand.mark.lock().unwrap() = "W/\"second\"".into();
    restarted.available(&w.advice, true).await.unwrap();
    assert_eq!(asked(&w.stand), (2, 1, 1));
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn without_the_publisher_the_last_answer_is_given_and_says_so() {
    let w = world("offline").await;
    w.runtimes.available(&w.advice, false).await.unwrap();
    // The same folder, but the publisher is now somewhere that does not answer.
    let nowhere = Source { api: "http://127.0.0.1:9".into(), ..w.source.clone() };
    let cut_off = Runtimes::new(&w.home, w.events.clone(), nowhere.clone()).unwrap();
    let last = cut_off.available(&w.advice, true).await.unwrap();
    assert_eq!(last.build, "b777");
    assert!(last.stale.as_deref().is_some_and(|why| why.contains("Could not reach")), "{:?}", last.stale);
    // With nothing kept, there is nothing to give: the refusal is passed on.
    let empty = temp("offline-empty");
    let nothing = Runtimes::new(&empty, w.events.clone(), nowhere).unwrap();
    assert!(matches!(nothing.available(&w.advice, false).await, Err(Error::Net(_))));
    std::fs::remove_dir_all(&w.home).ok();
    std::fs::remove_dir_all(&empty).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn a_runtime_is_downloaded_checked_unpacked_and_tried() {
    let w = world("install").await;
    let mut ear = w.events.listen();
    let done = w.runtimes.install(&w.advice, "cuda-13.5", true).await.unwrap();

    assert_eq!(done.id, "llama.cpp-b777-cuda-13.5");
    assert!(done.checked, "both files matched their published checksums");
    assert_eq!(done.server.as_deref(), Some("build/bin/llama-server"));
    assert_eq!(done.version.as_deref(), Some("version: 777 (stand-in)"));
    assert_eq!(done.problem, None);
    assert_eq!(done.devices, ["CUDA0: Stand-in card (1024 MiB)"]);
    assert_eq!(done.caution, None);
    assert_eq!(done.lib_dirs, ["build/bin"]);
    // Trying it again later finds the same and writes nothing new.
    assert_eq!(w.runtimes.check(&done.id).unwrap().devices, done.devices);
    assert!(matches!(w.runtimes.check("no-such"), Err(Error::NotFound(_))));
    let root = w.home.join("runtimes/llama.cpp/llama.cpp-b777-cuda-13.5");
    assert_eq!(std::fs::read(root.join("build/bin/libcudart.so.13")).unwrap(), b"cuda", "the extra library sits beside the program");
    assert!(!w.home.join("runtimes/llama.cpp/llama.cpp-b777-cuda-13.5.partial").exists());
    assert_eq!(std::fs::read_dir(w.home.join("runtimes/llama.cpp/downloads")).unwrap().count(), 0, "the archives are dropped once unpacked");

    // It is listed from its folder, and the log says what happened. Progress was told, not kept.
    let listed = w.runtimes.status().installed;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, done.id);
    assert_eq!(kinds(&w.events), ["runtime.install.started", "runtime.installed"]);
    let mut told = Vec::new();
    while let Ok(e) = ear.try_recv() {
        if e.kind == "runtime.install.progress" {
            assert_eq!(e.seq, 0);
            told.push(e.data["step"].as_str().unwrap().to_string());
        }
    }
    assert!(told.contains(&"downloading".to_string()) && told.contains(&"unpacking".to_string()) && told.contains(&"trying".to_string()), "{told:?}");

    // Removing it removes the folder, and only something listed can be removed.
    assert!(matches!(w.runtimes.remove("../../etc"), Err(Error::NotFound(_))));
    w.runtimes.remove(&done.id).unwrap();
    assert!(!root.exists() && w.runtimes.installed().is_empty());
    std::fs::remove_dir_all(&w.home).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn a_download_that_breaks_goes_on_from_where_it_stopped() {
    let w = world("resume").await;
    w.stand.break_once.store(true, Ordering::Relaxed);
    let err = w.runtimes.install(&w.advice, "cuda-13.5", false).await.unwrap_err();
    assert!(matches!(err, Error::Net(_)), "{err}");
    assert!(err.to_string().contains("goes on from there"), "{err}");
    let part = w.home.join("runtimes/llama.cpp/downloads").join(format!("{MAIN}.part"));
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(kept > 0, "what arrived before the break is kept");
    assert!(w.runtimes.installed().is_empty());

    let done = w.runtimes.install(&w.advice, "cuda-13.5", false).await.unwrap();
    assert_eq!(done.version.as_deref(), Some("version: 777 (stand-in)"));
    assert!(done.checked);
    assert_eq!(*w.stand.ranges_seen.lock().unwrap(), [format!("bytes={kept}-")], "the second try asked only for the rest");
    assert_eq!(kinds(&w.events), ["runtime.install.started", "runtime.install.failed", "runtime.install.started", "runtime.installed"]);
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn a_file_that_is_not_the_one_published_is_refused() {
    let w = world("tampered").await;
    *w.stand.claimed.lock().unwrap() = Some("0".repeat(64));
    let err = w.runtimes.install(&w.advice, "cuda-13.5", false).await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");
    assert!(err.to_string().contains("checksum does not match"), "{err}");
    assert!(w.runtimes.installed().is_empty());
    assert_eq!(std::fs::read_dir(w.home.join("runtimes/llama.cpp/downloads")).unwrap().count(), 0, "nothing of it is kept");
    assert!(!w.home.join("runtimes/llama.cpp/llama.cpp-b777-cuda-13.5").exists());
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn files_are_only_taken_from_the_publishers_own_address() {
    let w = world("elsewhere").await;
    *w.stand.elsewhere.lock().unwrap() = Some("http://example.invalid/files".into());
    let err = w.runtimes.install(&w.advice, "cuda-13.5", false).await.unwrap_err();
    assert!(err.to_string().contains("not where this runtime's files live"), "{err}");
    assert!(!w.home.join("runtimes/llama.cpp/downloads").exists(), "nothing was even started");
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn it_can_be_stopped_and_only_one_runs_at_a_time() {
    let w = world("stop").await;
    w.stand.slow.store(true, Ordering::Relaxed);
    let mut ear = w.events.listen();
    let (runtimes, advice) = (w.runtimes.clone(), w.advice.clone());
    let running = tokio::spawn(async move { runtimes.install(&advice, "cuda-13.5", false).await });
    // Wait until bytes are arriving.
    loop {
        let e = tokio::time::timeout(Duration::from_secs(10), ear.recv()).await.expect("progress within ten seconds").unwrap();
        if e.kind == "runtime.install.progress" && e.data["done"].as_u64().unwrap_or(0) > 0 {
            break;
        }
    }
    assert!(w.runtimes.status().installing.is_some());
    let second = w.runtimes.install(&w.advice, "cuda-13.5", false).await.unwrap_err();
    assert!(matches!(second, Error::Busy(_)), "{second}");
    assert!(w.runtimes.stop());
    let err = running.await.unwrap().unwrap_err();
    assert!(matches!(err, Error::Stopped(_)), "{err}");
    assert!(w.runtimes.status().installing.is_none());
    assert!(!w.runtimes.stop(), "nothing left to stop");
    assert!(std::fs::metadata(w.home.join("runtimes/llama.cpp/downloads").join(format!("{MAIN}.part"))).unwrap().len() > 0, "what was downloaded is kept");
    assert_eq!(kinds(&w.events), ["runtime.install.started", "runtime.install.stopped"]);
    std::fs::remove_dir_all(&w.home).ok();
}

#[tokio::test]
async fn asking_for_a_build_that_is_not_offered_says_what_is() {
    let w = world("unknown").await;
    let err = w.runtimes.install(&w.advice, "rocm-10.0", false).await.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));
    assert!(err.to_string().contains("Offered: cuda-13.5"), "{err}");
    std::fs::remove_dir_all(&w.home).ok();
}

#[test]
fn devices_are_read_as_the_program_lists_them() {
    // What the real llama-server (build b12359, CUDA 13.5) printed on the computer this was written on.
    let said = "\nAvailable devices:\n  CUDA0: NVIDIA GeForce RTX 3060 (11908 MiB, 10990 MiB free)\n\n";
    assert_eq!(devices_in(said), ["CUDA0: NVIDIA GeForce RTX 3060 (11908 MiB)"], "the memory free at that moment is not written down");
    assert_eq!(devices_in("Available devices:\n  Vulkan0: Some card\n  Metal: Apple M1 (shared, with a note)\n"), ["Vulkan0: Some card", "Metal: Apple M1 (shared, with a note)"]);
    assert!(devices_in("Available devices:\n").is_empty());
    assert!(devices_in("version: 1.0.0\nbuilt with gcc\n").is_empty());
}
