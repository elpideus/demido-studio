//! What a person can choose in a repository.
//!
//! Brief B22: "Models Browser & Downloader"
//!
//! The index lists files; a person chooses a model. Between the two sit the
//! rules [`crate::parts`] already keeps for a folder on disk, applied to a
//! repository before anything is fetched:
//!
//! - **Only weights are a choice.** A projector is fetched as a companion of
//!   the weights that need it and never offered on its own, and a draft model
//!   is never offered at all: offering either in a picker is offering
//!   something that cannot answer.
//! - **A split model is one choice**, and its shards are its pieces, in order.
//!   A model published in four pieces is one download.
//! - **The size is the number the server sends**, every piece and the
//!   projector added up, never bits per weight times a parameter count.
//! - **Ordered by fidelity**, most faithful first, with every label nobody
//!   published a number for after every label somebody did ([`Quant`]).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::index::File;
use crate::parts::{belongs_to, part_of, shard_of, Part};
use crate::quant::Quant;

/// One thing a person can choose to download from a repository.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    /// The filename without its directory, extension or split suffix: what
    /// the file is called, and what the window shows when there is no label.
    pub name: String,
    /// The quantisation the name carries. `None` for weights whose name
    /// carries none, which are offered after everything that does.
    pub quant: Option<Quant>,
    /// The weights: one file, or every shard of a split model, first to last.
    /// The first is the one a backend is handed.
    pub pieces: Vec<File>,
    /// The projector these weights need to see, fetched with them.
    pub projector: Option<File>,
    /// What the download costs: every piece and the projector, each as the
    /// server states it, added up.
    pub bytes: u64,
}

/// One model's key: its directory and name, lowercased, and how many pieces
/// it says it has.
type Model = (String, Option<u32>);

/// A repository's files, read into what a person can choose, most faithful
/// first.
///
/// A split model the listing does not have every piece of is left out: it
/// cannot load, so it is not something anybody can choose.
pub fn choices(files: Vec<File>) -> Vec<Choice> {
    let mut projectors = Vec::new();
    let mut models: BTreeMap<Model, Vec<(u32, File)>> = BTreeMap::new();

    for file in files {
        match part_of(filename(&file.path)) {
            Part::Projector => projectors.push(file),
            Part::Draft => {}
            Part::Weights => {
                let (name, shard) = shard_of(stem(&file.path));
                // The directory is part of the key: `Q4_K_M/model` and
                // `Q8_0/model` are different models whatever they are named.
                let directory = file.path.rsplit_once('/').map_or("", |(dir, _)| dir);
                let key = (
                    format!("{directory}/{name}").to_lowercase(),
                    shard.map(|shard| shard.total),
                );
                let index = shard.map_or(1, |shard| shard.index);
                models.entry(key).or_default().push((index, file));
            }
        }
    }

    let mut choices: Vec<Choice> = models
        .into_iter()
        .filter_map(|((_, total), mut pieces)| {
            pieces.sort_by_key(|(index, _)| *index);
            let whole = pieces
                .iter()
                .map(|(index, _)| *index)
                .eq(1..=total.unwrap_or(1));
            if !whole {
                tracing::warn!(
                    first = pieces.first().map(|(_, file)| file.path.as_str()),
                    "a split model is listed without every piece, and is not offered"
                );
                return None;
            }
            let pieces: Vec<File> = pieces.into_iter().map(|(_, file)| file).collect();
            Some(choice(pieces, &projectors))
        })
        .collect();

    choices.sort_by(|a, b| b.quant.cmp(&a.quant).then_with(|| a.name.cmp(&b.name)));
    choices
}

fn choice(pieces: Vec<File>, projectors: &[File]) -> Choice {
    let first = pieces.first().map_or("", |file| file.path.as_str());
    let name = shard_of(stem(first)).0.to_owned();
    let quant = Quant::parse(filename(first));

    let weights = name.to_lowercase();
    let projector = projectors
        .iter()
        .filter(|projector| belongs_to(filename(&projector.path), &weights))
        .min_by_key(|projector| (preference(projector), projector.path.as_str()))
        .cloned();

    let bytes = pieces
        .iter()
        .chain(projector.as_ref())
        .fold(0u64, |total, file| total.saturating_add(file.bytes));

    Choice {
        name,
        quant,
        pieces,
        projector,
        bytes,
    }
}

/// Which of several precisions of one projector is fetched. `F16` first, as
/// llama.cpp's converter writes a projector by default; then `BF16`, then
/// `F32`, then anything else. Three precisions of one projector are one
/// projector a person needs, not three downloads.
fn preference(projector: &File) -> usize {
    let label = Quant::parse(filename(&projector.path)).map(|quant| quant.label);
    ["F16", "BF16", "F32"]
        .iter()
        .position(|precision| label.as_deref() == Some(*precision))
        .unwrap_or(3)
}

/// The last segment of a repository path.
fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The filename without its extension.
fn stem(path: &str) -> &str {
    let name = filename(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}
