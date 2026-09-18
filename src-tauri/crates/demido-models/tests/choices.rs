//! What a person can choose in a repository: its files, read into choices.
//!
//! Against the payloads `tests/index.rs` commits, parsed by the same parser
//! the window's request goes through, so what is asserted here is what the
//! browser would show for that repository as it was published on 2026-09-18.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_models::choices::{choices, Choice};
use demido_models::index;

fn repository(name: &str) -> Vec<Choice> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/index")
        .join(name);
    let body = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    choices(index::parse_files(&body).expect("a real file list parses"))
}

fn labels(choices: &[Choice]) -> Vec<&str> {
    choices
        .iter()
        .map(|choice| {
            choice
                .quant
                .as_ref()
                .map_or("-", |quant| quant.label.as_str())
        })
        .collect()
}

fn paths(files: &[index::File]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

/// Twenty-six near-identical names become an ordered list: every published
/// label from the most faithful down, then every label nobody published a
/// number for, in the order their names read.
#[test]
fn a_repository_of_quantisations_is_an_ordered_list() {
    let choices = repository("tree-unsloth--Qwen3-0.6B-GGUF.json");
    assert_eq!(
        labels(&choices),
        [
            "BF16",
            "Q8_0",
            "Q6_K",
            "Q5_K_M",
            "Q5_K_S",
            "Q4_1",
            "Q4_K_M",
            "IQ4_NL",
            "Q4_K_S",
            "Q4_0",
            "IQ4_XS",
            "Q3_K_M",
            "Q3_K_S",
            "Q2_K",
            // Unpublished, last. Every `IQ1` and `IQ2` here is Unsloth's own
            // recipe, so none of them borrows upstream's number.
            "Q2_K_L",
            "UD-IQ1_M",
            "UD-IQ1_S",
            "UD-IQ2_M",
            "UD-IQ2_XXS",
            "UD-IQ3_XXS",
            "UD-Q2_K_XL",
            "UD-Q3_K_XL",
            "UD-Q4_K_XL",
            "UD-Q5_K_XL",
            "UD-Q6_K_XL",
            "UD-Q8_K_XL",
        ]
    );
    let unpublished = choices
        .iter()
        .skip_while(|choice| {
            choice
                .quant
                .as_ref()
                .is_some_and(|quant| quant.bits.is_some())
        })
        .collect::<Vec<_>>();
    assert!(unpublished.iter().all(|choice| choice
        .quant
        .as_ref()
        .is_some_and(|quant| quant.bits.is_none())));
}

/// The number on the button is the number the server sent. `Q2_K` and
/// `Q2_K_L` are the same 296,238,784 bytes in this repository, which no
/// estimate from their labels would have said.
#[test]
fn the_size_is_the_number_the_server_sends() {
    let choices = repository("tree-unsloth--Qwen3-0.6B-GGUF.json");
    let bytes = |label: &str| {
        choices
            .iter()
            .find(|choice| {
                choice
                    .quant
                    .as_ref()
                    .is_some_and(|quant| quant.label == label)
            })
            .unwrap_or_else(|| panic!("{label} is offered"))
            .bytes
    };
    assert_eq!(bytes("Q2_K"), 296_238_784);
    assert_eq!(bytes("Q2_K_L"), 296_238_784);
    assert_eq!(bytes("UD-Q4_K_XL"), 405_372_608);
    assert_eq!(bytes("Q8_0"), 639_447_744);
}

/// A model published in two pieces is one choice whose pieces are its shards,
/// in order, and whose size is both of them.
#[test]
fn a_split_model_is_one_choice_whose_pieces_are_its_shards() {
    let choices = repository("tree-unsloth--gpt-oss-120b-GGUF.json");
    assert_eq!(choices.len(), 16, "fifteen split quantisations and one F16");

    let q4 = choices
        .iter()
        .find(|choice| {
            choice
                .quant
                .as_ref()
                .is_some_and(|quant| quant.label == "Q4_K_M")
        })
        .expect("offered");
    assert_eq!(q4.name, "gpt-oss-120b-Q4_K_M");
    assert_eq!(
        paths(&q4.pieces),
        [
            "Q4_K_M/gpt-oss-120b-Q4_K_M-00001-of-00002.gguf",
            "Q4_K_M/gpt-oss-120b-Q4_K_M-00002-of-00002.gguf",
        ]
    );
    assert_eq!(q4.bytes, 49_630_904_192 + 13_137_819_360);

    let whole = choices
        .iter()
        .find(|choice| {
            choice
                .quant
                .as_ref()
                .is_some_and(|quant| quant.label == "F16")
        })
        .expect("offered");
    assert_eq!(paths(&whole.pieces), ["gpt-oss-120b-F16.gguf"]);

    assert_eq!(
        labels(&choices)[13..],
        ["UD-Q4_K_XL", "UD-Q6_K_XL", "UD-Q8_K_XL"],
        "the publisher's own recipes, last"
    );
}

/// Pieces listed out of order still come back first to last, because the
/// first is the one a backend is handed.
#[test]
fn shards_are_in_order_whatever_order_they_were_listed_in() {
    let listed = br#"[
        {"type":"file","path":"m-Q4_K_M-00003-of-00003.gguf","size":3},
        {"type":"file","path":"m-Q4_K_M-00001-of-00003.gguf","size":1},
        {"type":"file","path":"m-Q4_K_M-00002-of-00003.gguf","size":2}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(choices.len(), 1);
    assert_eq!(
        paths(&choices[0].pieces),
        [
            "m-Q4_K_M-00001-of-00003.gguf",
            "m-Q4_K_M-00002-of-00003.gguf",
            "m-Q4_K_M-00003-of-00003.gguf",
        ]
    );
    assert_eq!(choices[0].bytes, 6);
}

/// A split with a piece the repository does not list cannot load, so it is
/// not something a person can choose.
#[test]
fn a_split_missing_a_piece_is_not_offered() {
    let listed = br#"[
        {"type":"file","path":"m-Q4_K_M-00001-of-00003.gguf","size":1},
        {"type":"file","path":"m-Q4_K_M-00003-of-00003.gguf","size":3},
        {"type":"file","path":"m-Q8_0.gguf","size":8}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(labels(&choices), ["Q8_0"]);
}

/// The projector is never a choice. It travels with every quantisation that
/// needs it, and its size is part of what the download costs.
#[test]
fn a_projector_is_fetched_with_the_weights_and_never_offered() {
    let choices = repository("tree-ggml-org--gemma-3-4b-it-GGUF.json");
    assert_eq!(labels(&choices), ["F16", "Q8_0", "Q4_K_M"]);
    for choice in &choices {
        assert!(!choice.name.contains("mmproj"), "{}", choice.name);
        let projector = choice.projector.as_ref().expect("a vision model needs one");
        assert_eq!(projector.path, "mmproj-model-f16.gguf");
        let weights: u64 = choice.pieces.iter().map(|piece| piece.bytes).sum();
        assert_eq!(choice.bytes, weights + 851_251_104);
    }

    let qat = repository("tree-google--gemma-3-4b-it-qat-q4_0-gguf.json");
    assert_eq!(labels(&qat), ["Q4_0"]);
    assert_eq!(
        qat[0].projector.as_ref().map(|file| file.path.as_str()),
        Some("mmproj-model-f16-4B.gguf")
    );
}

/// Three precisions of one projector are one projector fetched, not three.
/// F16 first, as llama.cpp's converter writes it.
#[test]
fn one_projector_is_fetched_when_several_precisions_are_published() {
    let listed = br#"[
        {"type":"file","path":"mmproj-F32.gguf","size":32},
        {"type":"file","path":"mmproj-BF16.gguf","size":16},
        {"type":"file","path":"mmproj-F16.gguf","size":17},
        {"type":"file","path":"Model-Q4_K_M.gguf","size":4}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(choices.len(), 1);
    assert_eq!(
        choices[0].projector.as_ref().map(|file| file.path.as_str()),
        Some("mmproj-F16.gguf")
    );
    assert_eq!(choices[0].bytes, 4 + 17);
}

/// A projector named for another model is not this one's.
#[test]
fn a_projector_named_for_another_model_is_not_fetched() {
    let listed = br#"[
        {"type":"file","path":"mmproj-Other-Model-f16.gguf","size":9},
        {"type":"file","path":"This-Model-Q4_K_M.gguf","size":4}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].projector, None);
    assert_eq!(choices[0].bytes, 4);
}

/// A text model has no projector, and nothing is invented for it.
#[test]
fn a_text_model_has_no_projector() {
    for choice in repository("tree-unsloth--Qwen3-0.6B-GGUF.json") {
        assert_eq!(choice.projector, None, "{}", choice.name);
    }
}

/// A draft model loads beside weights to make them faster and cannot answer
/// on its own, so it is classified and never offered.
#[test]
fn a_draft_model_is_never_offered() {
    let listed = br#"[
        {"type":"file","path":"mtp-gemma-4-E4B-it-Q8_0.gguf","size":1},
        {"type":"file","path":"gemma-4-E4B-it-Q8_0.gguf","size":8}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(labels(&choices), ["Q8_0"]);
    assert_eq!(paths(&choices[0].pieces), ["gemma-4-E4B-it-Q8_0.gguf"]);
    assert_eq!(choices[0].bytes, 8);
}

/// Weights whose name carries no label are still weights: offered under their
/// name, after everything that has one.
#[test]
fn weights_with_no_label_are_offered_last_under_their_name() {
    let listed = br#"[
        {"type":"file","path":"plain-model.gguf","size":5},
        {"type":"file","path":"plain-model-UD-Q4_K_XL.gguf","size":4},
        {"type":"file","path":"plain-model-Q4_K_M.gguf","size":3}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    assert_eq!(labels(&choices), ["Q4_K_M", "UD-Q4_K_XL", "-"]);
    assert_eq!(choices[2].name, "plain-model");
}

/// What crosses to the window, key for key.
#[test]
fn a_choice_crosses_to_the_window_as_these_keys() {
    let choices = repository("tree-ggml-org--gemma-3-4b-it-GGUF.json");
    let value = serde_json::to_value(&choices[2]).expect("serialises");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["bytes", "name", "pieces", "projector", "quant"]);
    assert_eq!(value["quant"]["label"], "Q4_K_M");
}

/// A projector in another quantisation's directory is that directory's; one
/// at the top of the repository is everybody's.
#[test]
fn a_projector_is_read_beside_the_weights_or_at_the_top() {
    let listed = br#"[
        {"type":"file","path":"mmproj-F16.gguf","size":16},
        {"type":"file","path":"Q8_0/mmproj-BF16.gguf","size":15},
        {"type":"file","path":"Q8_0/Model-Q8_0.gguf","size":8},
        {"type":"file","path":"Q4_K_M/Model-Q4_K_M.gguf","size":4}
    ]"#;
    let choices = choices(index::parse_files(listed).expect("parses"));
    let projector = |label: &str| {
        choices
            .iter()
            .find(|choice| {
                choice
                    .quant
                    .as_ref()
                    .is_some_and(|quant| quant.label == label)
            })
            .and_then(|choice| choice.projector.as_ref())
            .map(|file| file.path.as_str())
    };
    assert_eq!(projector("Q8_0"), Some("mmproj-F16.gguf"));
    assert_eq!(projector("Q4_K_M"), Some("mmproj-F16.gguf"));

    let nested = br#"[
        {"type":"file","path":"Q8_0/mmproj-F16.gguf","size":16},
        {"type":"file","path":"Q4_K_M/Model-Q4_K_M.gguf","size":4}
    ]"#;
    let choices = demido_models::choices(index::parse_files(nested).expect("parses"));
    assert_eq!(choices[0].projector, None, "not in its directory");
}

/// Hugging Face paths are case sensitive, so two spellings are two files and
/// neither is lost.
#[test]
fn two_files_differing_in_case_are_two_choices() {
    let listed = br#"[
        {"type":"file","path":"model-Q4_K_M.gguf","size":4},
        {"type":"file","path":"Model-Q4_K_M.gguf","size":5}
    ]"#;
    assert_eq!(
        choices(index::parse_files(listed).expect("parses")).len(),
        2
    );
}
