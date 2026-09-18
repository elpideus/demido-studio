//! A slot priced from a file on disk: the header's per-layer arrays read back
//! through the parser, and a split model's weights counted piece by piece
//! ([#105](https://github.com/elpideus/demido-studio/issues/105)).

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use demido_models::gguf::Header;
use demido_models::{price, Geometry};
use support::{write, Key, Spec};

const MIB: u64 = 1024 * 1024;

/// The development model's attention geometry, as its header states it: the
/// layout is in two per-layer arrays, which is why the parser keeps them.
fn development() -> Spec {
    let key = |name: &str, value| (format!("gemma4.{name}"), value);
    Spec {
        keys: vec![
            key("block_count", Key::Uint(42)),
            key("embedding_length", Key::Uint(2560)),
            key("attention.head_count", Key::Uint(8)),
            key("attention.head_count_kv", Key::Uint(2)),
            key("attention.key_length", Key::Uint(512)),
            key("attention.value_length", Key::Uint(512)),
            key("attention.key_length_swa", Key::Uint(256)),
            key("attention.value_length_swa", Key::Uint(256)),
            key("attention.sliding_window", Key::Uint(512)),
            key("attention.shared_kv_layers", Key::Uint(18)),
            key(
                "attention.sliding_window_pattern",
                Key::Flags((0..42).map(|layer| layer % 6 != 5).collect()),
            ),
            // One entry per token, as a real vocabulary is: long enough that
            // the parser steps over it rather than keeping it.
            (
                "tokenizer.ggml.token_type".to_owned(),
                Key::Uints(vec![1; 5000]),
            ),
        ],
        ..Spec::model("gemma4")
    }
}

#[test]
fn a_file_on_disk_is_priced_from_its_header() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = folder.path().join("gemma-4-E4B-it-Q8_0.gguf");
    let bytes = write(&path, &development());

    let header = Header::read(&path).expect("the header reads");
    assert_eq!(
        header
            .list("gemma4.attention.sliding_window_pattern")
            .map(<[_]>::len),
        Some(42)
    );
    assert_eq!(
        header.list("tokenizer.ggml.token_type"),
        None,
        "a vocabulary is skipped"
    );

    assert_eq!(
        Geometry::read(&path).map(|geometry| geometry.per_slot(32768)),
        Some(552 * MIB)
    );
    let priced = price(&path, 32768);
    assert_eq!(priced.per_slot, Some(552 * MIB));
    assert_eq!(priced.weights, bytes);
}

#[test]
fn a_model_this_reading_does_not_know_is_weighed_but_not_priced() {
    let folder = tempfile::tempdir().expect("a folder");
    let path = folder.path().join("Qwen3.5-9B-Q4_K_M.gguf");
    let bytes = write(&path, &Spec::model("qwen35"));

    let priced = price(&path, 32768);
    assert_eq!(priced.per_slot, None);
    assert_eq!(priced.weights, bytes);
}

#[test]
fn a_split_model_weighs_every_piece_and_nothing_beside_it() {
    let folder = tempfile::tempdir().expect("a folder");
    let piece = |index| folder.path().join(format!("big-0000{index}-of-00003.gguf"));
    let first = write(&piece(1), &development());
    let rest: u64 = [2, 3]
        .into_iter()
        .map(|index| {
            write(
                &piece(index),
                &Spec {
                    weights: 128,
                    ..Spec::default()
                },
            )
        })
        .sum();
    // Neither of these is a piece of it.
    write(
        &folder.path().join("big-00001-of-00002.gguf"),
        &Spec::model("gemma4"),
    );
    write(
        &folder.path().join("mmproj-BF16.gguf"),
        &Spec::model("clip"),
    );

    let priced = price(&piece(1), 32768);
    assert_eq!(priced.weights, first + rest);
    assert_eq!(priced.per_slot, Some(552 * MIB));
}

#[test]
fn a_file_that_is_not_there_is_not_priced() {
    let priced = price(std::path::Path::new("nowhere/at/all.gguf"), 32768);
    assert_eq!(priced.weights, 0);
    assert_eq!(priced.per_slot, None);
}
