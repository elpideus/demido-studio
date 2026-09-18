//! A quantisation label, read off a filename into a value with an order.
//!
//! The table below is the test: every label llama.cpp publishes a number for,
//! and that number, copied from the source rather than from `quant.rs`. A
//! number changed in one place and not the other fails here.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_models::quant::Quant;

/// Every published label and its bits per weight.
///
/// The `bits/weight` row of llama.cpp's `tools/quantize/README.md` where it
/// has one (measured on Llama-3.1-8B), and otherwise the figure
/// `tools/quantize/quantize.cpp` states or ggml's block layout fixes, both at
/// llama.cpp `972d2313`.
const PUBLISHED: &[(&str, f32)] = &[
    ("F32", 32.0),
    ("F16", 16.0005),
    ("BF16", 16.0),
    ("Q8_0", 8.5008),
    ("Q6_K", 6.5633),
    ("Q5_1", 6.0),
    ("Q5_K_M", 5.7036),
    ("Q5_K", 5.7036),
    ("Q5_K_S", 5.5704),
    ("Q5_0", 5.5),
    ("Q4_1", 5.0),
    ("Q4_K_M", 4.8944),
    ("Q4_K", 4.8944),
    ("IQ4_NL", 4.6818),
    ("Q4_K_S", 4.6672),
    ("Q4_0", 4.5),
    ("IQ4_XS", 4.4597),
    ("Q3_K_L", 4.2979),
    ("Q3_K_M", 3.9960),
    ("Q3_K", 3.9960),
    ("IQ3_M", 3.7628),
    ("Q3_K_S", 3.6429),
    ("IQ3_S", 3.6606),
    ("IQ3_XS", 3.4977),
    ("IQ3_XXS", 3.2548),
    ("Q2_K", 3.1593),
    ("Q2_K_S", 2.9697),
    ("IQ2_M", 2.9294),
    ("IQ2_S", 2.7403),
    ("IQ2_XS", 2.5882),
    ("Q2_0", 2.25),
    ("IQ2_XXS", 2.3824),
    ("TQ2_0", 2.06),
    ("IQ1_M", 2.1460),
    ("IQ1_S", 2.0042),
    ("TQ1_0", 1.69),
    ("Q1_0", 1.125),
];

fn parse(filename: &str) -> Quant {
    Quant::parse(filename).unwrap_or_else(|| panic!("{filename} carries a quantisation"))
}

#[test]
fn every_published_label_maps_to_its_bits() {
    for (label, bits) in PUBLISHED {
        for filename in [
            format!("Some-Model-7B-{label}.gguf"),
            format!("some-model-7b.{}.gguf", label.to_ascii_lowercase()),
            format!("Some-Model-7B-{label}-00002-of-00003.gguf"),
        ] {
            let quant = parse(&filename);
            assert_eq!(quant.label, *label, "{filename}: upstream's spelling");
            assert_eq!(quant.bits, Some(*bits), "{filename}");
        }
    }
}

/// The table in `quant.rs` holds nothing this test does not, so a label added
/// there without a source is a failure here.
#[test]
fn the_table_publishes_nothing_the_test_does_not_know() {
    for (label, bits) in demido_models::quant::published() {
        assert!(
            PUBLISHED.contains(&(label, bits)),
            "{label} = {bits} is in quant.rs and not in this test"
        );
    }
    assert_eq!(demido_models::quant::published().count(), PUBLISHED.len());
}

/// The end of the name is the answer: a model called `Q8-Coder` is still a
/// `Q4_K_M` file.
#[test]
fn the_label_is_read_off_the_end_of_the_name() {
    assert_eq!(parse("Q8-Coder-7B-Q4_K_M.gguf").label, "Q4_K_M");
    assert_eq!(parse("model_q5_k_s.gguf").label, "Q5_K_S");
    assert_eq!(parse("gemma-3-4b-it-q4_0.gguf").label, "Q4_0");
    assert_eq!(
        parse("Meta-Llama-3-8B-Instruct.IQ3_XXS.gguf").label,
        "IQ3_XXS"
    );
}

/// The `UD-Q4_K_XL` family is the common case: a publisher's own recipe,
/// which keeps its name exactly as written and gets no number.
#[test]
fn an_unpublished_label_keeps_its_spelling_and_has_no_number() {
    for (filename, label) in [
        ("Qwen3-0.6B-UD-Q4_K_XL.gguf", "UD-Q4_K_XL"),
        ("Qwen3-0.6B-UD-Q8_K_XL.gguf", "UD-Q8_K_XL"),
        ("gpt-oss-120b-UD-Q4_K_XL-00001-of-00002.gguf", "UD-Q4_K_XL"),
        ("Qwen3-0.6B-Q2_K_L.gguf", "Q2_K_L"),
        ("gpt-oss-20b-MXFP4.gguf", "MXFP4"),
        ("model-q9_k_ultra.gguf", "q9_k_ultra"),
    ] {
        let quant = parse(filename);
        assert_eq!(quant.label, label, "{filename}");
        assert_eq!(quant.bits, None, "{filename}: nobody published one");
    }
}

/// `UD-IQ1_M` ends in a published label, and is still not it: the qualifier
/// says the recipe is the publisher's, and borrowing `IQ1_M`'s number for it
/// would be inventing one.
#[test]
fn a_publishers_recipe_does_not_borrow_the_number_of_the_label_it_ends_in() {
    let quant = parse("Qwen3-0.6B-UD-IQ1_M.gguf");
    assert_eq!(quant.label, "UD-IQ1_M");
    assert_eq!(quant.bits, None);
}

#[test]
fn a_name_with_no_quantisation_has_none() {
    for filename in [
        "some-finetune-v2.gguf",
        "mixtral-8x7b-instruct.gguf",
        "Qwen3-8B.gguf",
        "phi-4.gguf",
    ] {
        assert_eq!(Quant::parse(filename), None, "{filename}");
    }
}

/// More faithful is greater, so a list sorted greatest first runs from the
/// biggest file to the smallest, and every unpublished label comes after
/// every published one, however large it looks.
#[test]
fn the_order_is_fidelity_and_an_unpublished_label_sorts_last() {
    let mut quants: Vec<Quant> = [
        "m-UD-Q8_K_XL.gguf",
        "m-Q4_0.gguf",
        "m-IQ1_S.gguf",
        "m-Q4_K_M.gguf",
        "m-Q2_K_L.gguf",
        "m-BF16.gguf",
        "m-UD-IQ1_M.gguf",
        "m-Q8_0.gguf",
    ]
    .iter()
    .map(|filename| parse(filename))
    .collect();
    quants.sort_by(|a, b| b.cmp(a));
    let labels: Vec<&str> = quants.iter().map(|quant| quant.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "BF16",
            "Q8_0",
            "Q4_K_M",
            "Q4_0",
            "IQ1_S",
            // Unpublished, in the order their names read.
            "Q2_K_L",
            "UD-IQ1_M",
            "UD-Q8_K_XL",
        ]
    );
}

/// The whole reason the number is a table rather than the digit in the name.
#[test]
fn two_four_bit_labels_are_not_the_same_size() {
    assert!(parse("m-Q4_K_M.gguf") > parse("m-Q4_0.gguf"));
    assert!(parse("m-Q4_1.gguf") > parse("m-Q4_K_M.gguf"));
}

#[test]
fn a_quantisation_crosses_to_the_window_with_its_number_or_without_one() {
    assert_eq!(
        serde_json::to_value(parse("m-Q4_K_M.gguf")).expect("serialises"),
        serde_json::json!({ "label": "Q4_K_M", "bits": 4.8944_f32 })
    );
    assert_eq!(
        serde_json::to_value(parse("m-UD-Q4_K_XL.gguf")).expect("serialises"),
        serde_json::json!({ "label": "UD-Q4_K_XL" })
    );
}
