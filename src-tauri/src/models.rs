//! The library, as a profile sees it: its two folders read off the ladder, and
//! the models read out of them.
//!
//! `demido-models` does the reading and `demido-settings` holds the folders.
//! What is here is the one place the two meet, so the wizard's models step,
//! the settings page and the runtime verification all read the same folders
//! the same way ([#72](https://github.com/elpideus/demido-studio/issues/72)).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use demido_models::{sources, Folders, Library, Scan};
use demido_settings::{id, Ladder, Scope, Settings};
use demido_setup::Store as _;

use crate::wiring::AnswersStore;

pub struct Models {
    settings: Arc<Settings>,
    /// Read for the folders an earlier build of the wizard confirmed, and for
    /// nothing else.
    answers: Arc<AnswersStore>,
    /// Where downloads land when nobody has said: inside the profile, because
    /// a default that writes outside it needs permissions Demido should not
    /// ask for (`docs/rules/profiles.md`).
    default_download: PathBuf,
}

impl Models {
    pub fn new(
        settings: Arc<Settings>,
        answers: Arc<AnswersStore>,
        profile: &std::path::Path,
    ) -> Self {
        Self {
            settings,
            answers,
            default_download: profile.join("models"),
        }
    }

    /// The download folder and the scan folders, resolved.
    ///
    /// The scan folders are the setting, or the folders an earlier build of
    /// the wizard confirmed, or what detection finds, in that order. The
    /// middle one is written onto the ladder the first time it is read, so a
    /// profile set up before #72 keeps what its person confirmed rather than
    /// having it replaced by a fresh probe.
    pub fn folders(&self) -> Folders {
        let resolved = self.settings.resolve(&Ladder::global());
        let mut scan = resolved.scan_folders();
        if scan.is_none() {
            let earlier = self
                .answers
                .read()
                .map(|answers| answers.folders)
                .unwrap_or_default();
            if !earlier.is_empty() {
                if let Err(error) = self.write_scan(&earlier) {
                    tracing::warn!(%error, "the confirmed model folders could not be moved onto the ladder");
                }
                scan = Some(earlier);
            }
        }
        Folders::resolve(
            resolved.download_folder(),
            scan,
            &self.default_download,
            sources::detected,
        )
    }

    /// Every model on disk, verified.
    pub fn scan(&self) -> Scan {
        Library::open(&self.folders()).scan()
    }

    /// Read models from one more folder.
    pub fn add_scan(&self, folder: PathBuf) -> demido_core::Result<()> {
        let mut scan = self.folders().scan;
        scan.push(folder);
        self.write_scan(&scan)
    }

    /// Stop reading models from a folder. The folder is untouched.
    pub fn remove_scan(&self, folder: &std::path::Path) -> demido_core::Result<()> {
        let mut scan = self.folders().scan;
        scan.retain(|kept| !demido_models::folders::same(kept, folder));
        self.write_scan(&scan)
    }

    /// Move the download folder, or put it back inside the profile with
    /// `None`.
    ///
    /// **Nothing moves.** The folder downloads used to land in becomes a scan
    /// folder, so every model in it is still offered, and it is borrowed from
    /// then on. Both settings are written here, together, because a download
    /// folder changed without the scan folders is the one move that would make
    /// models disappear from the library.
    ///
    /// A folder that is, or is inside, a scan folder is refused: downloading
    /// there would be writing into a borrowed folder.
    pub fn set_download(&self, to: Option<PathBuf>) -> demido_core::Result<()> {
        let now = self.folders();
        if let Some(borrowed) = to.as_deref().and_then(|to| now.borrowed_at(to)) {
            // not-a-prompt: a refusal the window shows the person who moved it.
            return Err(demido_core::Error::invalid(
                "a download folder",
                format!("{} is read from and never written to", borrowed.display()),
            ));
        }
        let after = now.with_download(to.clone().unwrap_or_else(|| self.default_download.clone()));
        self.write_scan(&after.scan)?;
        match to {
            Some(folder) => self.settings.set(
                &Scope::Global,
                id::DOWNLOAD_FOLDER,
                &json!(folder.display().to_string()),
            )?,
            None => self.settings.clear(&Scope::Global, id::DOWNLOAD_FOLDER)?,
        }
        Ok(())
    }

    fn write_scan(&self, folders: &[PathBuf]) -> demido_core::Result<()> {
        let value = Value::from(
            folders
                .iter()
                .map(|folder| folder.display().to_string())
                .collect::<Vec<_>>(),
        );
        self.settings
            .set(&Scope::Global, id::SCAN_FOLDERS, &value)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use demido_settings::Memory;

    /// A profile of this test's own, which nothing else touches.
    fn profile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-models-host")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("made the profile");
        dir
    }

    fn models(profile: &std::path::Path) -> (Models, Arc<AnswersStore>) {
        let answers = Arc::new(AnswersStore::in_profile(profile));
        let settings = Arc::new(Settings::open(Memory::new()));
        (Models::new(settings, answers.clone(), profile), answers)
    }

    #[test]
    fn a_profile_downloads_inside_itself_until_somebody_says_otherwise() {
        let profile = profile("inside");
        let (models, _) = models(&profile);
        assert_eq!(models.folders().download, profile.join("models"));
    }

    /// The acceptance criterion, through the settings rather than around them.
    #[test]
    fn changing_the_download_folder_keeps_the_old_one_as_a_scan_folder() {
        let profile = profile("moved");
        let (models, _) = models(&profile);
        // Whatever this machine detects is not what the test is about.
        models.write_scan(&[]).expect("cleared");
        models
            .add_scan(PathBuf::from("D:/lmstudio"))
            .expect("added");

        models
            .set_download(Some(PathBuf::from("E:/weights")))
            .expect("moved");

        let folders = models.folders();
        assert_eq!(folders.download, PathBuf::from("E:/weights"));
        assert_eq!(
            folders.scan,
            vec![PathBuf::from("D:/lmstudio"), profile.join("models")]
        );
    }

    #[test]
    fn a_download_folder_inside_a_borrowed_one_is_refused_and_nothing_changes() {
        let profile = profile("refused");
        let (models, _) = models(&profile);
        models.write_scan(&[]).expect("cleared");
        models
            .add_scan(PathBuf::from("D:/lmstudio"))
            .expect("added");
        let before = models.folders();

        assert!(models
            .set_download(Some(PathBuf::from("D:/lmstudio/demido")))
            .is_err());
        assert_eq!(models.folders(), before);
    }

    #[test]
    fn a_profile_set_up_before_the_ladder_keeps_the_folders_it_confirmed() {
        let profile = profile("earlier");
        let (models, answers) = models(&profile);
        // What an earlier build wrote, byte for byte: this one cannot write it.
        std::fs::write(
            profile.join("setup.json"),
            r#"{"generation":1,"folders":["D:/confirmed"]}"#,
        )
        .expect("an earlier build wrote this");

        assert_eq!(models.folders().scan, vec![PathBuf::from("D:/confirmed")]);
        // Rewritten by any later gesture, and still there, because it is on
        // the ladder now rather than in the answers.
        answers
            .write(&demido_setup::Answers::default())
            .expect("a later gesture");
        assert_eq!(models.folders().scan, vec![PathBuf::from("D:/confirmed")]);
    }
}
