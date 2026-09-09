//! What is already on this machine, so the model step opens pre-filled.
//!
//! `docs/rules/setup.md` section 7: "The model folder is pre-filled from any
//! readable model folder already visible on the machine, for the user to
//! confirm", and the mechanism is the brief's own:
//!
//! Brief B55: "Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks."
//!
//! **Detection reports, it does not decide**, which is `demido-hardware`'s
//! rule and holds here for the same reason: a folder found is a row the person
//! confirms, never a folder Demido starts reading from on its own.
//!
//! Nothing here writes, creates or moves anything. A folder that cannot be
//! read is a folder that contributes no models, never an error: startup never
//! blocks (`AGENTS.md`), and neither does a wizard step.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// How deep a scan goes under a folder.
///
/// Four, because that is what the tools this exists to read from actually
/// nest: LM Studio keeps `publisher/repository/file.gguf`, and Hugging Face's
/// cache keeps `models--owner--repo/snapshots/<hash>/file.gguf`. Unbounded
/// recursion under a folder somebody pointed at a drive root is a wizard step
/// that hangs.
const DEPTH: usize = 4;

/// How many models one scan reports.
///
/// A library is a list a person picks from, and a person does not pick from
/// ten thousand. The number is stated on the step when it is reached rather
/// than the list being silently truncated.
pub const LIMIT: usize = 500;

/// What a model file is called. GGUF and nothing else: it is what
/// `llama.cpp` loads, and offering a person a file the backend will refuse is
/// a picker that lies.
const EXTENSION: &str = "gguf";

/// What a multimodal projector is called, and it is not a model.
///
/// An `mmproj` file is the vision half of a model, loaded beside one rather
/// than instead of one, and it is a GGUF sitting in the same folder under the
/// same extension. Offering it is a picker that lies twice: nobody can chat
/// with it, and it is usually the smallest file in the folder, which is
/// exactly what a runtime verification reaches for.
const COMPANION: &str = "mmproj";

/// One model file, as it was found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub path: PathBuf,
    /// The file's own name, which is what the composer and the log call it.
    pub name: String,
    pub size_mib: f64,
    /// The folder from [`Folders`] this one was read out of, so a list of
    /// models from three folders can say which is which.
    pub folder: PathBuf,
}

/// Every GGUF under `folders`, in the order the folders were given.
///
/// A folder that is missing or unreadable contributes nothing and is not an
/// error: it is a folder that has gone since it was confirmed, and the step
/// says it read nothing from it rather than refusing to draw.
pub fn models(folders: &[PathBuf]) -> Vec<Model> {
    let mut found = Vec::new();
    for folder in folders {
        collect(folder, folder, DEPTH, &mut found);
        if found.len() >= LIMIT {
            break;
        }
    }
    found.truncate(LIMIT);
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The smallest model on disk, which is what a runtime is verified against.
///
/// `docs/rules/runtimes.md` declares the required group's verification as
/// loading a model and generating one token, and the smallest one is the
/// cheapest honest way to do it: a verification that loaded a 30 GiB model
/// would take longer than the download it is checking.
pub fn smallest(folders: &[PathBuf]) -> Option<PathBuf> {
    models(folders)
        .into_iter()
        .min_by(|a, b| a.size_mib.total_cmp(&b.size_mib))
        .map(|model| model.path)
}

/// The model folders this machine already has, in the order they are offered.
///
/// Only folders that exist and can be listed, so a row here is a row the
/// person can confirm rather than a guess about where a tool might put things.
pub fn folders() -> Vec<PathBuf> {
    candidates()
        .into_iter()
        .filter(|folder| std::fs::read_dir(folder).is_ok())
        .collect()
}

/// The first folder that actually holds a model, or the first that exists.
///
/// What the model step opens on. A folder that exists and is empty is still
/// worth pre-filling with, because it is where that tool will put the next
/// download, but a folder with models in it is the better answer and comes
/// first.
pub fn preselected_folder() -> Option<PathBuf> {
    let folders = folders();
    folders
        .iter()
        .find(|folder| !models(std::slice::from_ref(folder)).is_empty())
        .or_else(|| folders.first())
        .cloned()
}

/// Where the tools people already use keep GGUF files on Windows.
///
/// A fixed list rather than a search of the disk: scanning every drive for
/// several gigabyte files is minutes of I/O on first launch, and a wizard that
/// does that has spent the person's attention before asking them anything.
///
/// Ollama is deliberately absent. It stores its weights as content-addressed
/// blobs with no extension and its own manifest beside them, so a `.gguf`
/// scan finds nothing there and a row that pre-filled with it would read as
/// an empty folder rather than as the unsupported layout it is.
fn candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = home() {
        // LM Studio, which is the tool the brief names.
        candidates.push(home.join(".lmstudio").join("models"));
        candidates.push(home.join(".cache").join("lm-studio").join("models"));
        candidates.push(home.join(".cache").join("huggingface").join("hub"));
        candidates.push(home.join("jan").join("models"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        candidates.push(local.join("nomic.ai").join("GPT4All"));
    }
    candidates
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Walk one folder, bounded, adding every GGUF to `found`.
fn collect(root: &Path, folder: &Path, depth: usize, found: &mut Vec<Model>) {
    if depth == 0 || found.len() >= LIMIT {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if found.len() >= LIMIT {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(root, &path, depth - 1, found);
            continue;
        }
        // A symlink to a model is still a model. It is read and launched and
        // never written to, which is the same bargain a linked runtime row
        // makes (`docs/rules/runtimes.md`).
        let is_model = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
        if !is_model {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        if name.to_ascii_lowercase().contains(COMPANION) {
            continue;
        }
        let size_mib = entry
            .metadata()
            .map_or(0.0, |metadata| metadata.len() as f64 / (1024.0 * 1024.0));
        found.push(Model {
            name,
            path,
            size_mib,
            folder: root.to_path_buf(),
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-setup-discover")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("made the directory");
        dir
    }

    fn model(at: &Path, name: &str, bytes: usize) {
        if let Some(parent) = at.join(name).parent() {
            std::fs::create_dir_all(parent).expect("made the directory");
        }
        std::fs::write(at.join(name), vec![0u8; bytes]).expect("wrote a model");
    }

    #[test]
    fn a_folder_nested_the_way_lm_studio_nests_one_is_read() {
        let dir = scratch("nested");
        model(&dir, "publisher/repository/gemma-4-E4B-it-Q8_0.gguf", 8);
        let found = models(std::slice::from_ref(&dir));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "gemma-4-E4B-it-Q8_0.gguf");
        assert_eq!(found[0].folder, dir, "a model says which folder it is from");
    }

    #[test]
    fn a_folder_that_is_not_there_contributes_nothing_rather_than_failing() {
        let dir = scratch("gone");
        assert!(models(&[dir.join("never-existed")]).is_empty());
    }

    #[test]
    fn nothing_but_a_gguf_is_offered() {
        let dir = scratch("mixed");
        model(&dir, "notes.txt", 1);
        model(&dir, "weights.safetensors", 1);
        model(&dir, "qwen3.5-9b.GGUF", 1);
        let found = models(&[dir]);
        assert_eq!(found.len(), 1, "the extension is matched case-blind");
    }

    /// The vision half of a model is a GGUF in the same folder, and it is
    /// usually the smallest file there, which is what makes it dangerous
    /// rather than merely noisy.
    #[test]
    fn a_projector_is_not_offered_as_a_model() {
        let dir = scratch("mmproj");
        model(&dir, "gemma-4-E4B-it-Q8_0.gguf", 64);
        model(&dir, "gemma-4-E4B-it.mmproj-f16.gguf", 8);
        let found = models(std::slice::from_ref(&dir));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "gemma-4-E4B-it-Q8_0.gguf");
        assert_eq!(
            smallest(std::slice::from_ref(&dir)),
            Some(dir.join("gemma-4-E4B-it-Q8_0.gguf")),
            "a verification that loaded a projector would refuse a runtime that works"
        );
    }

    #[test]
    fn the_smallest_is_what_a_runtime_is_verified_against() {
        let dir = scratch("smallest");
        model(&dir, "big.gguf", 4096);
        model(&dir, "small.gguf", 8);
        assert_eq!(
            smallest(std::slice::from_ref(&dir)),
            Some(dir.join("small.gguf"))
        );
    }

    #[test]
    fn a_scan_stops_before_it_walks_a_whole_drive() {
        let dir = scratch("deep");
        model(&dir, "a/b/c/d/e/too-deep.gguf", 1);
        assert!(
            models(&[dir]).is_empty(),
            "five levels down is past the layouts this reads"
        );
    }
}
