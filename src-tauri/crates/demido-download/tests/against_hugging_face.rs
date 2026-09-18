//! The queue against the real host, paired with the fake server in
//! `tests/over_http.rs` the way v2's CDN test was: that one misbehaves on
//! purpose, and this one is what the bytes actually come from.
//!
//! `#[ignore]`d because it needs the network and fetches a 214 MB model.
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml -p demido-download --test against_hugging_face -- --ignored --test-threads=1
//! ```

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::Duration;

use demido_download::{Failure, Files, Item, Queue, State, HOST};
use demido_models::index::{Answer, Index};
use demido_models::{choices, Folders, Library};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-download-against-hugging-face")
        .join(name);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

async fn chosen(
    repo: &str,
    library: &Library,
    pick: impl Fn(&[demido_models::Choice]) -> usize,
) -> Item {
    let Answer::Read { found } = Index::default().files(repo).await else {
        panic!("the tree was not readable");
    };
    let offered = choices(found);
    let choice = &offered[pick(&offered)];
    Item::chosen(HOST, repo, choice, library)
}

/// The smallest quantisation of a real model: fetched, paused part way,
/// resumed with a range the CDN honours, hashed against the digest the index
/// published, checked as a GGUF, and then offered by the library.
#[tokio::test]
#[ignore = "needs the network, and fetches 214 MB"]
async fn a_real_model_arrives_through_a_pause_verified_and_offered() {
    let dir = scratch("qwen");
    let folders = Folders {
        download: dir.join("models"),
        scan: vec![],
    };
    let library = Library::open(&folders);
    let item = chosen("unsloth/Qwen3-0.6B-GGUF", &library, |offered| {
        (0..offered.len())
            .min_by_key(|at| offered[*at].bytes)
            .expect("something to choose")
    })
    .await;
    assert!(
        item.files[0].sha256.is_some(),
        "the index published a digest"
    );
    let piece = item.files[0].clone();

    let queue = Queue::open(Files::in_profile(&dir));
    let id = queue.enqueue(item);
    tokio::time::timeout(Duration::from_secs(120), async {
        while queue
            .row(id)
            .is_none_or(|row| row.received <= 16 * 1024 * 1024)
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("sixteen megabytes arrived");
    queue.pause(id);
    assert_eq!(queue.settled(id).await, Some(State::Paused));
    let kept = std::fs::metadata(piece.partial()).expect("kept").len();
    assert!(kept > 0);

    queue.resume(id);
    let done = tokio::time::timeout(Duration::from_secs(900), queue.settled(id))
        .await
        .expect("finished in fifteen minutes");
    assert_eq!(done, Some(State::Done));

    let scan = Library::open(&folders).scan();
    assert_eq!(scan.models.len(), 1, "damaged: {:?}", scan.damaged);
    assert_eq!(scan.models[0].path, piece.destination);
    assert_eq!(scan.models[0].bytes, piece.bytes);
}

/// A gated repository answers a keyless fetch with a refusal, and the row
/// says the file needs an account rather than that the network failed.
#[tokio::test]
#[ignore = "needs the network"]
async fn a_gated_file_fails_as_gated() {
    let dir = scratch("gated");
    let library = Library::open(&Folders {
        download: dir.join("models"),
        scan: vec![],
    });
    let item = chosen("google/gemma-3-4b-it-qat-q4_0-gguf", &library, |_| 0).await;

    let queue = Queue::open(Files::in_profile(&dir));
    let id = queue.enqueue(item);
    let state = tokio::time::timeout(Duration::from_secs(60), queue.settled(id))
        .await
        .expect("answered");
    assert!(
        matches!(
            state,
            Some(State::Failed {
                failure: Failure::Gated { .. }
            })
        ),
        "got {state:?}"
    );
}
