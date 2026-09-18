//! Which files make up one model.
//!
//! A folder frequently holds more than the weights. A vision model ships a
//! projector beside them (`mmproj-*.gguf`); some ship a small
//! multi-token-prediction or draft model (`mtp-*.gguf`); a large one is split
//! into `-00001-of-00004` pieces. All of them are GGUF, all of them sit in the
//! same folder, and only one of them is a thing somebody can choose to talk
//! to: loading a projector on its own gets a refusal, and a picker that offered
//! one would be offering something that cannot answer.
//!
//! So they are classified rather than listed. The directory is still the
//! registry ([`crate::library`]), and reading it correctly means knowing which
//! entries are choices and which are parts.
//!
//! Carried from v2's `demido-models::parts`, which found every one of these
//! layouts in a real library on a real machine. The same rules read a
//! repository before anything is fetched ([`crate::choices`], #71).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// What one `.gguf` in a model folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// The weights. The file a backend is pointed at, and the only kind that
    /// appears in a model picker.
    Weights,
    /// A vision projector, loaded alongside the weights to give a model eyes.
    Projector,
    /// A draft or multi-token-prediction model, loaded alongside the weights to
    /// make them faster.
    Draft,
}

impl Part {
    /// Whether this is a thing somebody can choose to talk to.
    pub fn is_choosable(self) -> bool {
        matches!(self, Part::Weights)
    }
}

/// Classify one filename.
///
/// By name, because that is what publishers actually use and it is the only
/// signal that does not need the file opened.
pub fn part_of(filename: &str) -> Part {
    let lower = filename.to_ascii_lowercase();

    // `mmproj-BF16.gguf`, and also `Model-Name.mmproj-f16.gguf`, which is the
    // other layout in the wild.
    if lower.starts_with("mmproj") || lower.contains(".mmproj") || lower.contains("-mmproj") {
        return Part::Projector;
    }
    // A prefix rather than a substring, because a model whose name merely
    // contains the three letters is still a model.
    if lower.starts_with("mtp-") || lower.contains(".mtp-") {
        return Part::Draft;
    }
    Part::Weights
}

/// A file that belongs to a model and never loads instead of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Companion {
    pub path: PathBuf,
    pub kind: Part,
    pub bytes: u64,
}

/// Whether `companion` belongs to the weights file whose stem is
/// `weights_stem`.
///
/// The two kinds pair differently, and both rules come from what publishers
/// actually ship:
///
/// - **A projector belongs to the whole folder.** One is published per model,
///   at its own precision, for use with any quantisation of it: `mmproj-BF16`
///   beside a Q8 and a Q4. So the comparison is on the model name with the
///   precision taken off, and a projector that names no model is the folder's.
///   So is one named `model`: `mmproj-model-f16` is the name llama.cpp's
///   converter writes when nobody gives it one, and `ggml-org` and `google`
///   both publish it that way (`tests/fixtures/index/`).
/// - **A draft belongs to one file.** `mtp-Model-Q8_0` is built against the Q8
///   weights, and offering it beside the Q4 is a pairing `llama.cpp` refuses
///   for a reason the user cannot see. So that one matches exactly.
pub fn belongs_to(companion: &str, weights_stem: &str) -> bool {
    let named = trimmed(&companion.to_ascii_lowercase());
    let weights = weights_stem.to_ascii_lowercase();

    match part_of(companion) {
        Part::Weights | Part::Draft => named == weights,
        Part::Projector => {
            let family = without_precision(&named);
            family.is_empty()
                || family == "model"
                || family.starts_with("model-")
                || weights.starts_with(family)
        }
    }
}

/// The companion's name with its marker taken off. `mmproj-BF16` becomes
/// `bf16`, and `Model-Name.mmproj-f16` becomes `model-name`.
fn trimmed(lower: &str) -> String {
    let stem = lower.rsplit_once('.').map_or(lower, |(stem, _)| stem);

    for marker in [".mmproj", "-mmproj", ".mtp-"] {
        if let Some((head, _)) = stem.split_once(marker) {
            return head.to_owned();
        }
    }
    for marker in ["mmproj-", "mmproj", "mtp-"] {
        if let Some(rest) = stem.strip_prefix(marker) {
            return rest.trim_start_matches(['-', '.']).to_owned();
        }
    }
    stem.to_owned()
}

/// The name with a trailing precision token removed. `bf16` alone is a
/// precision and nothing else, which is how a folder-wide projector is named.
fn without_precision(named: &str) -> &str {
    const PRECISIONS: [&str; 10] = [
        "f16", "bf16", "f32", "fp16", "fp32", "q8_0", "q4_k_m", "q4_k_s", "16", "32",
    ];
    match named.rsplit_once('-') {
        Some((head, last)) if PRECISIONS.contains(&last) => head,
        _ if PRECISIONS.contains(&named) => "",
        _ => named,
    }
}

/// Which piece of a split model a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Shard {
    /// Counted from one, as the filename counts.
    pub index: u32,
    pub total: u32,
}

/// A filename stem without its `-00001-of-00003` suffix, and the piece it
/// named if it had one.
pub fn shard_of(stem: &str) -> (&str, Option<Shard>) {
    let Some((head, total)) = stem.rsplit_once("-of-") else {
        return (stem, None);
    };
    let Some((name, index)) = head.rsplit_once('-') else {
        return (stem, None);
    };
    let digits = |text: &str| text.len() == 5 && text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(index) || !digits(total) {
        return (stem, None);
    }
    match (index.parse(), total.parse()) {
        (Ok(index), Ok(total)) if index >= 1 && index <= total => {
            (name, Some(Shard { index, total }))
        }
        _ => (stem, None),
    }
}

/// The last segment of a repository path: `Q4_K_M/model.gguf` is
/// `model.gguf`.
pub fn filename_in(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A repository path's filename without its extension.
pub fn stem_in(path: &str) -> &str {
    let name = filename_in(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// A path's filename without its extension.
pub fn stem_of(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
}

/// Whether a filename is one this library reads: `.gguf`, any case.
pub fn is_gguf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_publishers_layouts_are_all_recognised() {
        for name in [
            "mmproj-BF16.gguf",
            "mmproj-F32.gguf",
            "mmproj-Qwythos-9B-Claude-Mythos-5-1M-f16.gguf",
            "Huihui-Qwen3.5-9B-abliterated.mmproj-f16.gguf",
        ] {
            assert_eq!(part_of(name), Part::Projector, "{name}");
        }
        assert_eq!(part_of("mtp-gemma-4-E4B-it-Q8_0.gguf"), Part::Draft);
        for name in [
            "gemma-4-E4B-it-Q8_0.gguf",
            "Huihui-Qwen3.5-9B-abliterated.Q4_K_S.gguf",
            "smtp-helper-Q8_0.gguf",
        ] {
            assert_eq!(part_of(name), Part::Weights, "{name}");
        }
    }

    #[test]
    fn a_projector_named_for_nothing_belongs_to_every_quant_in_the_folder() {
        assert!(belongs_to("mmproj-BF16.gguf", "gemma-4-e4b-it-q8_0"));
        assert!(belongs_to("mmproj-F32.gguf", "qwen3.5-9b-q4_k_m"));
        assert!(belongs_to(
            "mmproj-Qwythos-9B-Claude-Mythos-5-1M-f16.gguf",
            "qwythos-9b-claude-mythos-5-1m-q4_k_m"
        ));
        assert!(!belongs_to(
            "mmproj-Other-Model-f16.gguf",
            "qwythos-9b-claude-mythos-5-1m-q4_k_m"
        ));
    }

    /// The converter's default name, as two publishers ship it.
    #[test]
    fn a_projector_named_model_belongs_to_the_folder() {
        assert!(belongs_to("mmproj-model-f16.gguf", "gemma-3-4b-it-q4_k_m"));
        assert!(belongs_to("mmproj-model-f16-4B.gguf", "gemma-3-4b-it-q4_0"));
        assert!(!belongs_to("mmproj-modelo-f16.gguf", "gemma-3-4b-it-q4_0"));
    }

    #[test]
    fn a_draft_named_for_one_quant_belongs_only_to_that_one() {
        assert!(belongs_to(
            "mtp-gemma-4-E4B-it-Q8_0.gguf",
            "gemma-4-e4b-it-q8_0"
        ));
        assert!(!belongs_to(
            "mtp-gemma-4-E4B-it-Q8_0.gguf",
            "gemma-4-e4b-it-q4_k_m"
        ));
    }

    #[test]
    fn a_split_suffix_is_read_and_anything_like_one_is_not() {
        assert_eq!(
            shard_of("DeepSeek-Q4_K_M-00002-of-00003"),
            ("DeepSeek-Q4_K_M", Some(Shard { index: 2, total: 3 }))
        );
        assert_eq!(shard_of("model-of-the-year"), ("model-of-the-year", None));
        assert_eq!(shard_of("m-00004-of-00003"), ("m-00004-of-00003", None));
        assert_eq!(shard_of("m-1-of-3"), ("m-1-of-3", None));
    }
}
