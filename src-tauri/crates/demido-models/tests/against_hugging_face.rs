//! The index against the real host.
//!
//! `#[ignore]`d because it needs a network and Hugging Face's goodwill. The
//! fixtures in `tests/index.rs` are what CI trusts; this is how a new fixture
//! is found to be needed, which is the day these fail and those pass.
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml -p demido-models --test against_hugging_face -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_models::index::{Answer, Index, LISTING};

#[tokio::test]
#[ignore = "needs the network"]
async fn a_search_by_author_and_name_reads_a_capped_ordered_listing() {
    let Answer::Read { found } = Index::default().search("ggml-org/gemma-3").await else {
        panic!("the index was not readable");
    };
    assert!(!found.is_empty() && found.len() <= LISTING);
    assert!(found.iter().all(|repo| repo.id.starts_with("ggml-org/")));
    assert!(found
        .windows(2)
        .all(|pair| pair[0].downloads >= pair[1].downloads));
}

#[tokio::test]
#[ignore = "needs the network"]
async fn a_gated_repository_is_gated_and_still_lists_its_files() {
    let Answer::Read { found } = Index::default()
        .search("google/gemma-3-4b-it-qat-q4_0-gguf")
        .await
    else {
        panic!("the index was not readable");
    };
    let repo = found
        .iter()
        .find(|repo| repo.id == "google/gemma-3-4b-it-qat-q4_0-gguf")
        .expect("listed");
    assert!(repo.gated);

    let Answer::Read { found } = Index::default().files(&repo.id).await else {
        panic!("the tree was not readable");
    };
    assert!(found.iter().any(|file| file.path.ends_with(".gguf")));
}

#[tokio::test]
#[ignore = "needs the network"]
async fn a_repository_that_is_not_there_is_missing() {
    let answer = Index::default()
        .files("demido-studio/no-such-repository-anywhere")
        .await;
    assert!(
        matches!(
            answer,
            Answer::Unreadable {
                cause: demido_models::index::Cause::Missing,
                ..
            }
        ),
        "{answer:?}"
    );
}
