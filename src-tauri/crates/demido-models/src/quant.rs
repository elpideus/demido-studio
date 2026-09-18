//! A quantisation label, read off a GGUF filename into a value with an order.
//!
//! Brief B22: "Models Browser & Downloader"
//!
//! A repository offers the same weights fifteen times over and the only thing
//! separating them is a suffix: `Q4_K_M`, `IQ3_XXS`, `BF16`. That suffix is the
//! one decision a person actually makes in a models browser, so it is parsed
//! into a value that orders by fidelity rather than left as a string that sorts
//! alphabetically and wrong. *Smaller* and *better* are the same axis pointing
//! in opposite directions, and bits per weight is that axis.
//!
//! **Nothing guesses.** A label llama.cpp publishes no number for keeps its
//! name exactly as written, carries no number, and sorts after every label
//! that has one. The `UD-Q4_K_XL` family is the common case: a publisher's own
//! recipe, and inventing a size for it is the thing this module exists to
//! refuse. v2's `quant.rs` placed such labels beside their family; that was a
//! guess about the order, and #71 takes it out.

use std::cmp::Ordering;

use serde::Serialize;

use crate::parts::shard_of;

/// A quantisation, as a filename names it.
#[derive(Debug, Clone, Serialize)]
pub struct Quant {
    /// Upstream's spelling for a label llama.cpp publishes (`Q4_K_M`, whatever
    /// case the file used), and the file's own spelling, exactly, for one it
    /// does not (`UD-Q4_K_XL`).
    pub label: String,
    /// Bits per weight, as llama.cpp publishes it. `None` for a label nobody
    /// has published a number for, which the window shows with no number
    /// rather than an approximate one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<f32>,
}

/// Every label with a number, and the number.
///
/// Where llama.cpp's `tools/quantize/README.md` has a `bits/weight` row (all
/// measured on one model, Llama-3.1-8B, so they compare with each other) the
/// number is that row. The rest are the figure `tools/quantize/quantize.cpp`
/// states for the type, or the one ggml's block layout fixes (`Q4_0` is 18
/// bytes per 32 weights). Both files at llama.cpp `972d2313`. A table rather
/// than the digit in the name, because `Q4_K_M` and `Q4_0` are both "four bit"
/// and are not the same size.
const PUBLISHED: &[(&str, f32)] = &[
    // The README's measured row.
    ("F16", 16.0005),
    ("Q8_0", 8.5008),
    ("Q6_K", 6.5633),
    ("Q5_K_M", 5.7036),
    ("Q5_K_S", 5.5704),
    ("Q4_K_M", 4.8944),
    ("IQ4_NL", 4.6818),
    ("Q4_K_S", 4.6672),
    ("IQ4_XS", 4.4597),
    ("Q3_K_L", 4.2979),
    ("Q3_K_M", 3.9960),
    ("IQ3_M", 3.7628),
    ("IQ3_S", 3.6606),
    ("Q3_K_S", 3.6429),
    ("IQ3_XS", 3.4977),
    ("IQ3_XXS", 3.2548),
    ("Q2_K", 3.1593),
    ("Q2_K_S", 2.9697),
    ("IQ2_M", 2.9294),
    ("IQ2_S", 2.7403),
    ("IQ2_XS", 2.5882),
    ("IQ2_XXS", 2.3824),
    ("IQ1_M", 2.1460),
    ("IQ1_S", 2.0042),
    // `quantize.cpp`'s aliases: `Q4_K` is `Q4_K_M`, and so on.
    ("Q5_K", 5.7036),
    ("Q4_K", 4.8944),
    ("Q3_K", 3.9960),
    // The type's own width: ggml's block layout, or `quantize.cpp`'s stated
    // bpw where the README has no row.
    ("F32", 32.0),
    ("BF16", 16.0),
    ("Q5_1", 6.0),
    ("Q5_0", 5.5),
    ("Q4_1", 5.0),
    ("Q4_0", 4.5),
    ("Q2_0", 2.25),
    ("TQ2_0", 2.06),
    ("TQ1_0", 1.69),
    ("Q1_0", 1.125),
];

/// Words a publisher puts in front of a label to say the recipe is their own.
/// `UD` is Unsloth Dynamic, which requantises layers the label does not
/// describe, so `UD-IQ1_M` is not an `IQ1_M` and does not get its number.
const RECIPES: &[&str] = &["UD"];

/// Every label llama.cpp publishes a number for, and the number.
pub fn published() -> impl Iterator<Item = (&'static str, f32)> {
    PUBLISHED.iter().copied()
}

impl Quant {
    /// The quantisation a GGUF filename carries, if it carries one.
    ///
    /// Read off the end, after the split suffix, because a model's own name
    /// can hold what looks like a label (`Q8-Coder-7B-Q4_K_M`) and only the
    /// last one is the answer. A directory in front is ignored.
    pub fn parse(filename: &str) -> Option<Quant> {
        let name = filename.rsplit('/').next().unwrap_or(filename);
        let stem = match name.len().checked_sub(".gguf".len()) {
            Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".gguf") => {
                &name[..at]
            }
            _ => name,
        };
        let (stem, _) = shard_of(stem);

        let (head, tail) = match stem.rfind(['-', '.']) {
            Some(at) => (&stem[..at], &stem[at + 1..]),
            None => ("", stem),
        };
        let known = known(tail);
        if known.is_none() && !shaped(tail) {
            return None;
        }

        // A recipe word in front makes the whole thing the publisher's label.
        let recipe = head.rsplit('-').next().unwrap_or_default();
        if RECIPES.iter().any(|word| recipe.eq_ignore_ascii_case(word)) {
            let at = head.len() - recipe.len();
            return Some(Quant {
                label: stem[at..].to_owned(),
                bits: None,
            });
        }

        Some(match known {
            Some((label, bits)) => Quant {
                label: label.to_owned(),
                bits: Some(bits),
            },
            None => Quant {
                label: tail.to_owned(),
                bits: None,
            },
        })
    }
}

/// The published label `token` ends in, as a whole word: the whole token, or
/// a tail after an underscore (`model_q5_k_s`). The longest one, since `Q4_K`
/// is a suffix of nothing but `Q4_K_M` is not a suffix of `Q4_K`.
fn known(token: &str) -> Option<(&'static str, f32)> {
    let upper = token.to_ascii_uppercase();
    PUBLISHED
        .iter()
        .filter(|(label, _)| {
            upper
                .strip_suffix(label)
                .is_some_and(|before| before.is_empty() || before.ends_with('_'))
        })
        .max_by_key(|(label, _)| label.len())
        .copied()
}

/// Whether a token is shaped like a quantisation: a type prefix, a digit, and
/// nothing but letters, digits and underscores. `Q9_K_ULTRA` and `MXFP4` are;
/// `v2`, `8B` and `Qwen3` are not.
fn shaped(token: &str) -> bool {
    let upper = token.to_ascii_uppercase();
    ["MXFP", "IQ", "TQ", "BF", "FP", "Q", "F"]
        .iter()
        .find_map(|prefix| upper.strip_prefix(prefix))
        .is_some_and(|rest| {
            rest.starts_with(|c: char| c.is_ascii_digit())
                && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Fidelity: more bits per weight is greater, and a label with no number is
/// less than every label with one, so a list sorted greatest first puts the
/// unpublished ones last. Between two of the same fidelity (or two with no
/// number) the one whose name reads first is greater, so greatest first reads
/// alphabetically there.
impl Ord for Quant {
    fn cmp(&self, other: &Self) -> Ordering {
        let fidelity = match (self.bits, other.bits) {
            (Some(mine), Some(theirs)) => mine.total_cmp(&theirs),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        };
        fidelity.then_with(|| other.label.cmp(&self.label))
    }
}

impl PartialOrd for Quant {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Quant {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Quant {}
