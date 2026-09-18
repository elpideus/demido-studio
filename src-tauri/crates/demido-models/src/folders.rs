//! Which folder is Demido's, and which folders are read.
//!
//! Brief B55: "Also a download folder should be set-able by the user. Multiple folders should be set-able for model detection, so that Demido Studio can use models downloaded by other tools (like LM Studio) without the need to move them or create symlinks."
//!
//! Two settings on the ladder, both per profile (`docs/rules/profiles.md`):
//!
//! - **The download folder** is Demido's own. Downloads land there, and it is
//!   the only folder anything is ever deleted from. Unset, it is a folder
//!   inside the profile, because a default that writes outside the profile is
//!   a default that needs permissions Demido should not ask for.
//! - **The scan folders** are borrowed. Read, listed and offered, never written
//!   to. Unset, they are what detection found, which is what "seeded by
//!   detection" means: the person's first answer is the machine's, and the
//!   first edit makes it theirs.
//!
//! Everything here is a pure function over paths. Nothing is read from disk
//! except by the detection closure the caller hands in, and nothing is written.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// The folders a library is read from, resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folders {
    /// Demido's own. Never also a scan folder.
    pub download: PathBuf,
    /// Borrowed, in the order the person gave them, each once.
    pub scan: Vec<PathBuf>,
}

impl Folders {
    /// What the two settings resolve to.
    ///
    /// `download` and `scan` are the ladder's answers, `None` where nobody has
    /// one. Detection is a closure so it is only run when the scan folders are
    /// unset: a person who has chosen their folders does not pay for a probe
    /// of the disk on every read.
    pub fn resolve(
        download: Option<PathBuf>,
        scan: Option<Vec<PathBuf>>,
        default_download: &Path,
        detect: impl FnOnce() -> Vec<PathBuf>,
    ) -> Folders {
        let download = download.unwrap_or_else(|| default_download.to_path_buf());
        let scan = scan.unwrap_or_else(detect);
        Folders {
            scan: distinct(scan, &download),
            download,
        }
    }

    /// The scan folder `to` is, or sits inside, if any.
    ///
    /// A download folder there would be Demido writing into a borrowed
    /// folder, which is the one thing a borrowed folder is promised never
    /// happens (#72), so the host refuses the move rather than quietly
    /// making another tool's library Demido's.
    pub fn borrowed_at(&self, to: &Path) -> Option<&Path> {
        let at = key(to);
        self.scan
            .iter()
            .find(|folder| at.starts_with(&key(folder)))
            .map(PathBuf::as_path)
    }

    /// The folders after the download folder moves to `to`.
    ///
    /// **Nothing moves.** The models already downloaded stay where they are,
    /// and the folder they are in stays a folder that is read: it becomes a
    /// scan folder, so every one of them is still offered. It is borrowed from
    /// then on, which means Demido will not delete from it, and that is the
    /// honest reading of a setting change that did not take the files with it.
    pub fn with_download(&self, to: PathBuf) -> Folders {
        let mut scan = self.scan.clone();
        if !same(&self.download, &to) {
            scan.push(self.download.clone());
        }
        Folders {
            scan: distinct(scan, &to),
            download: to,
        }
    }
}

/// Each folder once, in order, and never the download folder.
fn distinct(folders: Vec<PathBuf>, download: &Path) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if same(&folder, download) || kept.iter().any(|seen| same(seen, &folder)) {
            continue;
        }
        kept.push(folder);
    }
    kept
}

/// Whether two paths name the same folder, as Windows reads a path.
///
/// Case-blind, separator-blind and blind to a trailing separator: `C:\Models`
/// and `c:/models/` are one folder, and a list holding both would list every
/// model in it twice.
pub fn same(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// A path reduced to what Windows compares.
pub(crate) fn key(path: &Path) -> Vec<String> {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .replace('/', "\\")
                .to_lowercase()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn unset_is_the_profiles_folder_and_what_detection_found() {
        let folders = Folders::resolve(None, None, Path::new("P:/profile/models"), || {
            paths(&["C:/Users/me/.lmstudio/models"])
        });
        assert_eq!(folders.download, PathBuf::from("P:/profile/models"));
        assert_eq!(folders.scan, paths(&["C:/Users/me/.lmstudio/models"]));
    }

    #[test]
    fn a_chosen_list_is_the_persons_and_detection_is_not_asked() {
        let folders = Folders::resolve(
            None,
            Some(paths(&["D:/weights"])),
            Path::new("P:/profile/models"),
            || panic!("detection ran over a list somebody chose"),
        );
        assert_eq!(folders.scan, paths(&["D:/weights"]));
    }

    #[test]
    fn an_empty_list_is_an_answer_rather_than_a_request_for_detection() {
        let folders = Folders::resolve(None, Some(Vec::new()), Path::new("P:/m"), || {
            panic!("an empty list is somebody having removed them all")
        });
        assert!(folders.scan.is_empty());
    }

    #[test]
    fn the_download_folder_is_never_also_borrowed() {
        let folders = Folders::resolve(
            Some(PathBuf::from("D:/Weights")),
            Some(paths(&["d:\\weights\\", "E:/more", "e:/MORE"])),
            Path::new("P:/m"),
            Vec::new,
        );
        assert_eq!(folders.download, PathBuf::from("D:/Weights"));
        assert_eq!(folders.scan, paths(&["E:/more"]), "each folder once");
    }

    #[test]
    fn moving_the_download_folder_keeps_the_old_one_as_a_scan_folder() {
        let before = Folders {
            download: PathBuf::from("P:/profile/models"),
            scan: paths(&["C:/lmstudio"]),
        };
        let after = before.with_download(PathBuf::from("D:/big-drive"));
        assert_eq!(after.download, PathBuf::from("D:/big-drive"));
        assert_eq!(after.scan, paths(&["C:/lmstudio", "P:/profile/models"]));
    }

    #[test]
    fn a_download_folder_on_or_inside_a_borrowed_folder_is_named_as_one() {
        let folders = Folders {
            download: PathBuf::from("P:/m"),
            scan: paths(&["D:/weights"]),
        };
        for to in ["D:/Weights/", "d:\\weights\\demido"] {
            assert_eq!(
                folders.borrowed_at(Path::new(to)),
                Some(Path::new("D:/weights")),
                "{to}"
            );
        }
        assert_eq!(folders.borrowed_at(Path::new("D:/weights-2")), None);
        assert_eq!(folders.borrowed_at(Path::new("E:/new")), None);
    }

    #[test]
    fn choosing_the_same_folder_again_changes_nothing() {
        let before = Folders {
            download: PathBuf::from("P:/m"),
            scan: paths(&["D:/weights"]),
        };
        assert_eq!(
            before.with_download(PathBuf::from("p:\\M")).scan,
            before.scan
        );
    }
}
