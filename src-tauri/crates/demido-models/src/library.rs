//! The models on this machine.
//!
//! **The directory is the registry.** Carried over from v2's decision 0007: a
//! file at its destination is the record that it was downloaded, so a second
//! record could only ever disagree with the disk, and the disk would be right.
//! There is no bookkeeping file. A model deleted by another tool is gone on the
//! next scan rather than lingering, and a file copied in by hand is there on
//! the next scan without being registered.
//!
//! **One folder is Demido's and the rest are borrowed.** Borrowed folders are
//! read, listed and offered, and never written to, downloaded into or deleted
//! from. That is not kept by being careful. It is kept by [`Library::remove`]
//! refusing every path outside Demido's own root, and by
//! [`Library::destination`] having nowhere to put a file but under it.
//!
//! **A model is verified before it is offered.** Every weights file's header is
//! read and the file is checked to be as long as the header says
//! ([`crate::gguf`]), so a download cut short is a damaged row here rather than
//! something a backend discovers for the user.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::folders::{key, same, Folders};
use crate::gguf::{self, Damage, Header};
use crate::parts::{belongs_to, is_gguf, part_of, shard_of, stem_of, Companion, Part};
use crate::sources;
use crate::{Error, Result};

/// How deep a scan goes under a folder.
///
/// Four levels, because that is what the tools this reads from actually nest:
/// LM Studio keeps `publisher/repository/file.gguf`, and Hugging Face's cache
/// keeps `models--owner--repo/snapshots/<hash>/file.gguf`. Unbounded recursion
/// under a folder somebody pointed at a drive root is a scan that hangs, and a
/// symlink loop inside a models folder should not be able to hang anything.
const DEPTH: usize = 4;

/// How many models one scan reports. A library is a list a person picks from,
/// and a person does not pick from ten thousand.
pub const LIMIT: usize = 500;

/// What Demido's own library is called on a row.
const OWN: &str = "Demido";

/// Whether a model is capable of something, as far as its files say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fact {
    Yes,
    No,
    /// Nothing in the files says either way. Drawn differently from `No`,
    /// because a tag that claimed an absence nobody measured would be a guess
    /// dressed as a fact.
    Unknown,
}

/// The four capabilities `design/shell.md` draws as tags.
///
/// **For a model on disk these are read from the file**, per #36: the GGUF's
/// own metadata, and the presence of a projector for vision. Never from the
/// name, which is the publisher's marketing rather than the file's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    /// A projector is beside the weights, and it does not say it has no vision
    /// encoder.
    pub vision: Fact,
    /// The chat template renders a list of tools.
    pub tools: Fact,
    /// The chat template has a thinking block.
    pub reasoning: Fact,
    /// The projector says it has an audio encoder.
    pub audio: Fact,
}

/// A model on disk that loaded its header, is as long as it says, and can be
/// offered.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Local {
    /// The file a backend is pointed at. For a split model, the first piece:
    /// `llama.cpp` is handed that one and finds the rest.
    pub path: PathBuf,
    /// The filename without its extension or its split suffix, which is what
    /// the person who downloaded it recognises.
    pub label: String,
    /// Every piece of the weights. Companions are counted in
    /// [`Companion::bytes`], not here.
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shards: Option<u32>,
    /// `publisher/model`, read from the folders it sits in. Absent for a file
    /// dropped in by hand, which is allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Which library it came from: `Demido`, a tool's name, or the folder.
    pub library: String,
    /// The folder of the two settings it was read out of, so a list of models
    /// from three folders can say which is which.
    pub folder: PathBuf,
    /// Read, listed and offered, never written to or deleted from.
    pub borrowed: bool,
    /// The projector and draft files that travel with it and are never offered
    /// on their own.
    pub companions: Vec<Companion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    /// The context the model was trained with, as the file states it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    pub capabilities: Capabilities,
}

/// A weights file that is on disk and is not offered, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Damaged {
    pub path: PathBuf,
    pub library: String,
    pub folder: PathBuf,
    pub borrowed: bool,
    pub damage: Damage,
}

/// One read of every folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub models: Vec<Local>,
    pub damaged: Vec<Damaged>,
    /// Every byte under Demido's own folder: the disk Demido spent.
    ///
    /// **Borrowed bytes are never counted.** `docs/rules/runtimes.md`'s rule
    /// for a linked runtime, applied to weights: what another tool downloaded
    /// is not something Demido spent, and a total that included it would make
    /// the set-up wizard's disk figure a number about somebody else's choices.
    pub spent: u64,
}

impl Scan {
    /// The smallest model on disk, which is what a runtime is verified against
    /// before anybody has chosen one.
    ///
    /// `docs/rules/runtimes.md` declares the required group's verification as
    /// loading a model and generating one token, and the smallest is the
    /// cheapest honest way to do it. Only models are candidates: a projector
    /// or a draft head is usually the smallest file in its folder and cannot
    /// be loaded on its own, and #79 found exactly that throwing away a
    /// runtime that worked.
    pub fn smallest(&self) -> Option<&Local> {
        self.models.iter().min_by_key(|model| model.bytes)
    }
}

/// One folder a scan reads, and what its models are called.
#[derive(Debug, Clone)]
struct Root {
    path: PathBuf,
    name: String,
    borrowed: bool,
}

/// Demido's own folder and the borrowed ones.
#[derive(Debug, Clone)]
pub struct Library {
    own: Root,
    borrowed: Vec<Root>,
}

impl Library {
    /// The library these folders make. Reads nothing yet.
    pub fn open(folders: &Folders) -> Library {
        let mut borrowed: Vec<Root> = Vec::new();
        for folder in &folders.scan {
            if same(folder, &folders.download)
                || borrowed.iter().any(|root| same(&root.path, folder))
            {
                continue;
            }
            borrowed.push(Root {
                name: sources::name_of(folder),
                path: folder.clone(),
                borrowed: true,
            });
        }
        Library {
            own: Root {
                path: folders.download.clone(),
                name: OWN.to_owned(),
                borrowed: false,
            },
            borrowed,
        }
    }

    /// Demido's own folder.
    pub fn root(&self) -> &Path {
        &self.own.path
    }

    /// Read every folder.
    ///
    /// Never fails: a folder that is missing or unreadable contributes nothing,
    /// because startup never blocks (`AGENTS.md`) and a drive that is not
    /// plugged in is not a broken library.
    pub fn scan(&self) -> Scan {
        let roots: Vec<&Root> = std::iter::once(&self.own).chain(&self.borrowed).collect();
        let others = |root: &Root| -> Vec<PathBuf> {
            roots
                .iter()
                .filter(|other| !same(&other.path, &root.path))
                .map(|other| other.path.clone())
                .collect()
        };

        let mut found = Found::default();
        for root in &roots {
            walk(root, &root.path, DEPTH, &others(root), &mut found);
        }

        let mut spent = 0;
        tally(&self.own.path, DEPTH, &others(&self.own), &mut spent);

        let mut models = found.models;
        models.sort_by(|a, b| {
            a.label
                .to_lowercase()
                .cmp(&b.label.to_lowercase())
                .then_with(|| a.path.cmp(&b.path))
        });
        models.truncate(LIMIT);
        Scan {
            models,
            damaged: found.damaged,
            spent,
        }
    }

    /// Where a file from `repo` goes: `publisher/model/file.gguf` under
    /// Demido's own folder, which is the layout LM Studio and the rest of the
    /// ecosystem already use, so a folder can be moved between two tools'
    /// libraries by dragging it.
    ///
    /// **There is no argument that puts it anywhere else.** Every segment is
    /// taken down to a plain name: no drive, no root, no `..`, nothing a
    /// filesystem refuses. A repository called `../../etc` is a folder called
    /// `etc` under the download folder.
    pub fn destination(&self, repo: &str, filename: &str) -> PathBuf {
        let mut path = self.own.path.clone();
        for segment in repo.split(['/', '\\']).filter_map(plain) {
            path.push(segment);
        }
        let name = filename
            .rsplit(['/', '\\'])
            .find_map(plain)
            .unwrap_or_else(|| "model.gguf".to_owned());
        path.push(name);
        path
    }

    /// Delete a model Demido downloaded, and every piece of it.
    ///
    /// **Refuses every path outside Demido's own root**, which is what makes
    /// borrowing safe rather than merely intended. Compared on the resolved
    /// path, so `models\..\elsewhere` is where it goes rather than where it is
    /// spelled, and a junction inside the root that points out of it is out.
    /// A borrowed folder that sits inside the root is refused too: the closer
    /// folder decides whose a file is.
    pub fn remove(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let outside = || Error::Outside {
            path: path.to_path_buf(),
        };
        let resolved = std::fs::canonicalize(path).map_err(|error| Error::io(path, error))?;
        let own = std::fs::canonicalize(&self.own.path).map_err(|_| outside())?;
        if !resolved.starts_with(&own) || resolved == own {
            return Err(outside());
        }
        for root in &self.borrowed {
            if let Ok(borrowed) = std::fs::canonicalize(&root.path) {
                if resolved.starts_with(&borrowed) {
                    return Err(outside());
                }
            }
        }
        if !resolved.is_file() || !is_gguf(&resolved) {
            return Err(Error::NotAModel {
                path: path.to_path_buf(),
            });
        }

        let pieces = pieces_on_disk(&resolved);
        for piece in &pieces {
            std::fs::remove_file(piece).map_err(|error| Error::io(piece, error))?;
        }
        Ok(pieces)
    }
}

/// A segment reduced to a plain name, or nothing if it has none.
fn plain(segment: &str) -> Option<String> {
    let cleaned: String = segment
        .chars()
        .map(|character| match character {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim_matches([' ', '.']);
    if trimmed.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// The pieces of one model on disk, each with its index from one. A model that
/// is not split is one piece at index one.
type Pieces = Vec<(u32, PathBuf)>;

/// What a walk has turned up so far.
#[derive(Default)]
struct Found {
    models: Vec<Local>,
    damaged: Vec<Damaged>,
    /// Every weights file already read, by the key Windows compares, so a
    /// folder named twice or nested inside another is read once.
    seen: Vec<Vec<String>>,
}

/// Read one folder and the folders under it, down to `depth`, stopping at any
/// folder that is a root of its own.
fn walk(root: &Root, folder: &Path, depth: usize, others: &[PathBuf], found: &mut Found) {
    if depth == 0 || found.models.len() >= LIMIT {
        return;
    }
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(folder = %folder.display(), %error, "a models folder could not be read");
            }
            return;
        }
    };

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !others.iter().any(|other| same(other, &path)) {
                walk(root, &path, depth - 1, others, found);
            }
        } else if is_gguf(&path) {
            files.push(path);
        }
    }
    read_folder(root, &files, found);
}

/// Turn one folder's GGUF files into models, companions and damage.
fn read_folder(root: &Root, files: &[PathBuf], found: &mut Found) {
    let companions: Vec<Companion> = files
        .iter()
        .filter(|path| !part_of(name(path)).is_choosable())
        .map(|path| Companion {
            kind: part_of(name(path)),
            bytes: length(path),
            path: path.clone(),
        })
        .collect();

    // Weights grouped into models: one file, or every piece of a split one.
    let mut models: BTreeMap<String, (Option<u32>, Pieces)> = BTreeMap::new();
    for path in files
        .iter()
        .filter(|path| part_of(name(path)).is_choosable())
    {
        let (label, shard) = shard_of(stem_of(path));
        let group = format!(
            "{}|{:?}",
            label.to_lowercase(),
            shard.map(|shard| shard.total)
        );
        let entry = models
            .entry(group)
            .or_insert_with(|| (shard.map(|shard| shard.total), Vec::new()));
        entry
            .1
            .push((shard.map_or(1, |shard| shard.index), path.clone()));
    }

    for (total, mut pieces) in models.into_values() {
        pieces.sort();
        let first = pieces[0].1.clone();
        let identity = key(&first);
        if found.seen.contains(&identity) {
            continue;
        }
        found.seen.push(identity);

        match model(root, total, &pieces, &companions) {
            Ok(local) => found.models.push(local),
            Err(damage) => found.damaged.push(Damaged {
                path: first,
                library: root.name.clone(),
                folder: root.path.clone(),
                borrowed: root.borrowed,
                damage,
            }),
        }
    }
}

/// One model, verified, or the reason it is not offered.
fn model(
    root: &Root,
    total: Option<u32>,
    pieces: &[(u32, PathBuf)],
    companions: &[Companion],
) -> std::result::Result<Local, Damage> {
    if let Some(total) = total {
        if let Some(index) = (1..=total).find(|index| !pieces.iter().any(|(at, _)| at == index)) {
            return Err(Damage::MissingPiece { index, total });
        }
    }

    let mut header = None;
    let mut bytes = 0u64;
    for (_, piece) in pieces {
        let read = gguf::verify(piece)?;
        bytes = bytes.saturating_add(length(piece));
        header.get_or_insert(read);
    }
    let first = &pieces[0].1;
    let header = header.ok_or(Damage::NotGguf)?;

    let (label, _) = shard_of(stem_of(first));
    let stem = label.to_lowercase();
    let companions: Vec<Companion> = companions
        .iter()
        .filter(|companion| belongs_to(name(&companion.path), &stem))
        .cloned()
        .collect();

    let architecture = header.text("general.architecture").map(str::to_owned);
    let context = architecture
        .as_deref()
        .and_then(|architecture| header.uint(&format!("{architecture}.context_length")));

    Ok(Local {
        label: label.to_owned(),
        bytes,
        shards: total,
        repo: repo_of(first, &root.path),
        library: root.name.clone(),
        folder: root.path.clone(),
        borrowed: root.borrowed,
        capabilities: capabilities(&header, &companions),
        companions,
        architecture,
        context,
        path: first.clone(),
    })
}

/// What the files say this model can do.
fn capabilities(weights: &Header, companions: &[Companion]) -> Capabilities {
    let templates: Vec<String> = weights
        .metadata
        .keys()
        .filter(|key| key.starts_with("tokenizer.chat_template"))
        .filter_map(|key| weights.text(key))
        .map(str::to_lowercase)
        .collect();
    let template_says = |markers: &[&str]| {
        if templates.is_empty() {
            Fact::Unknown
        } else if templates
            .iter()
            .any(|template| markers.iter().any(|marker| template.contains(marker)))
        {
            Fact::Yes
        } else {
            Fact::No
        }
    };

    let projector = companions
        .iter()
        .find(|companion| companion.kind == Part::Projector);
    let (vision, audio) = match projector.map(|projector| Header::read(&projector.path)) {
        None => (Fact::No, Fact::No),
        Some(Err(_)) => (Fact::Unknown, Fact::Unknown),
        Some(Ok(header)) => (
            match header.flag("clip.has_vision_encoder") {
                Some(false) => Fact::No,
                _ => Fact::Yes,
            },
            match header.flag("clip.has_audio_encoder") {
                Some(true) => Fact::Yes,
                _ => Fact::No,
            },
        ),
    };

    Capabilities {
        vision,
        // A template that renders tools is a model trained to be handed them.
        tools: template_says(&["tools"]),
        // Every thinking template in the wild opens a block by one of these.
        reasoning: template_says(&[
            "<think>",
            "enable_thinking",
            "reasoning_content",
            "thinking",
        ]),
        audio,
    }
}

/// Which repository a file's folders name, given the root it was found under.
///
/// `publisher/model/file.gguf` is that repository, and so is Hugging Face's
/// cache shape, `models--publisher--model/snapshots/<hash>/file.gguf`. A file
/// directly under a root, or one folder down, names none.
fn repo_of(path: &Path, root: &Path) -> Option<String> {
    let under = path.strip_prefix(root).ok()?.parent()?;
    let folders: Vec<String> = under
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let first = folders.first()?;
    if let Some(cached) = first.strip_prefix("models--") {
        return cached
            .split_once("--")
            .map(|(publisher, model)| format!("{publisher}/{model}"));
    }
    match folders.as_slice() {
        [publisher, model, ..] => Some(format!("{publisher}/{model}")),
        _ => None,
    }
}

/// Every piece of the model at `path`, found on disk. The file alone unless it
/// is a piece of a split model.
fn pieces_on_disk(path: &Path) -> Vec<PathBuf> {
    let (label, Some(shard)) = shard_of(stem_of(path)) else {
        return vec![path.to_path_buf()];
    };
    let Some(folder) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let mut pieces: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| {
            is_gguf(candidate) && {
                let (other, piece) = shard_of(stem_of(candidate));
                other.eq_ignore_ascii_case(label)
                    && piece.is_some_and(|piece| piece.total == shard.total)
            }
        })
        .collect();
    pieces.sort();
    pieces
}

/// Add up every file under `folder`, the way [`walk`] reads it.
fn tally(folder: &Path, depth: usize, others: &[PathBuf], spent: &mut u64) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !others.iter().any(|other| same(other, &path)) {
                tally(&path, depth - 1, others, spent);
            }
        } else if kind.is_file() {
            *spent = spent.saturating_add(length(&path));
        }
    }
}

fn name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

fn length(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |meta| meta.len())
}
