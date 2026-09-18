//! Where the tools people already use keep GGUF files, and what to call each.
//!
//! Detection seeds the scan folders ([`crate::folders`]), and the same list is
//! how a borrowed model says which library it came from: a folder that is LM
//! Studio's is called LM Studio, and a folder somebody typed is called by its
//! path, because that is the best name it has.
//!
//! A fixed list rather than a search of the disk: scanning every drive for
//! several gigabyte files is minutes of I/O, and a wizard that does that has
//! spent the person's attention before asking them anything.
//!
//! **Ollama is deliberately absent.** It stores weights as content-addressed
//! blobs with no extension and a manifest beside them, so a `.gguf` scan finds
//! nothing there, and a row pre-filled with it would read as an empty folder
//! rather than as the unsupported layout it is.

use std::path::{Path, PathBuf};

use crate::folders::same;

/// Another tool's library, present or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub name: &'static str,
    pub root: PathBuf,
}

/// Every library this build knows where to look for, whether or not it is on
/// this machine.
pub fn known() -> Vec<Known> {
    known_under(
        home().as_deref(),
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .as_deref(),
    )
}

fn known_under(home: Option<&Path>, local: Option<&Path>) -> Vec<Known> {
    let mut known = Vec::new();
    if let Some(home) = home {
        // LM Studio, which is the tool the brief names, in both the places
        // its versions have kept models.
        known.push(Known {
            name: "LM Studio",
            root: home.join(".lmstudio").join("models"),
        });
        known.push(Known {
            name: "LM Studio",
            root: home.join(".cache").join("lm-studio").join("models"),
        });
        known.push(Known {
            name: "Hugging Face cache",
            root: home.join(".cache").join("huggingface").join("hub"),
        });
        known.push(Known {
            name: "Jan",
            root: home.join("jan").join("models"),
        });
    }
    if let Some(local) = local {
        known.push(Known {
            name: "GPT4All",
            root: local.join("nomic.ai").join("GPT4All"),
        });
    }
    known
}

/// The known libraries that are on this machine and can be listed.
pub fn detected() -> Vec<PathBuf> {
    known()
        .into_iter()
        .map(|known| known.root)
        .filter(|root| std::fs::read_dir(root).is_ok())
        .collect()
}

/// What to call the library at `folder`.
pub fn name_of(folder: &Path) -> String {
    name_among(folder, &known())
}

fn name_among(folder: &Path, known: &[Known]) -> String {
    known
        .iter()
        .find(|library| same(&library.root, folder))
        .map_or_else(
            || folder.display().to_string(),
            |library| library.name.to_owned(),
        )
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_tools_folder_is_called_by_the_tools_name() {
        let known = known_under(Some(Path::new("C:/Users/me")), None);
        assert_eq!(
            name_among(Path::new("c:\\users\\ME\\.lmstudio\\models\\"), &known),
            "LM Studio"
        );
    }

    #[test]
    fn a_folder_somebody_typed_is_called_by_where_it_is() {
        let known = known_under(Some(Path::new("C:/Users/me")), None);
        assert_eq!(name_among(Path::new("D:/weights"), &known), "D:/weights");
    }

    #[test]
    fn nothing_that_is_not_there_is_detected() {
        for root in detected() {
            assert!(root.is_dir(), "{} is not there", root.display());
        }
    }
}
