//! What setup puts in the install folder, and which folders are safe to install into.
//!
//! The uninstaller removes only what setup created, and setup refuses folders that already hold
//! other files or overlap the person's data, so removing Demido Studio never takes anything else
//! with it.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use demido_core::InstallManifest;
use demido_core::brand::UNINSTALLER_BIN;
use demido_core::platform::exe;
use serde::{Deserialize, Serialize};

/// Written into the install folder before setup creates anything else there, and kept up to date
/// with what it created until `install.json` records that. Without an `install.json`, a folder
/// holding it is an installation of setup's own that did not finish; next to one, it lists what
/// an update that did not finish added. Either way a retry, and the uninstaller, know which of
/// the leftovers are setup's.
pub(crate) const MARKER: &str = ".demido-setup";

/// Setup's records of what it created in the install folder.
pub(crate) const RECORDS: [&str; 2] = [InstallManifest::FILE_NAME, MARKER];

/// The marker's contents.
#[derive(Serialize, Deserialize)]
struct Marker {
    created: Vec<String>,
}

/// What setup owns in the install folder during a run. Each name is claimed before setup creates
/// it, so a run that stops halfway still knows what was its own. Entries that were in the folder
/// before setup came to it, and that setup's own records do not list, are foreign: never claimed,
/// so the uninstaller never removes them.
pub(crate) struct Claim {
    dir: PathBuf,
    /// Names that were in the folder before and are not setup's, in lower case: on Windows and
    /// macOS another program's `Logs` is the `logs` setup would create.
    foreign: BTreeSet<String>,
    created: BTreeSet<String>,
    /// Set once `install.json` records what setup created: the marker is gone for good.
    finished: bool,
}

impl Claim {
    /// Takes stock of `dir` (which must exist) before setup creates anything in it, then writes
    /// the marker. What setup created there earlier comes only from `previous`, the folder's
    /// `install.json`, and from the marker of an unfinished run. A name setup would create is not
    /// setup's merely for sitting next to its `install.json`: an earlier run may have left it out
    /// as foreign, or another program may have added it since.
    pub(crate) fn begin(dir: &Path, previous: Option<&InstallManifest>) -> anyhow::Result<Self> {
        let before = std::fs::read_dir(dir)
            .and_then(|entries| {
                entries
                    .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect::<std::io::Result<BTreeSet<String>>>()
            })
            .with_context(|| format!("reading {}", dir.display()))?;
        let mut ours: BTreeSet<String> = previous
            .and_then(created_entries)
            .unwrap_or_default()
            .into_iter()
            .collect();
        ours.extend(marker_entries(dir));
        let setups: BTreeSet<String> = ours.iter().map(|n| n.to_lowercase()).collect();
        let mut claim = Self {
            dir: dir.to_path_buf(),
            foreign: before
                .into_iter()
                .map(|n| n.to_lowercase())
                .filter(|n| !setups.contains(n))
                .collect(),
            created: ours,
            finished: false,
        };
        claim.add([MARKER.to_string()])?;
        Ok(claim)
    }

    /// Records `names` as created by setup, unless another program's is there: in the folder
    /// before setup came, or put there since. Call before creating them.
    pub(crate) fn add(&mut self, names: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
        let names: Vec<String> = names.into_iter().filter(|n| self.claimable(n)).collect();
        self.created.extend(names);
        self.save()
    }

    /// Claims `names` for a step about to write into them, as [`Claim::add`] does, but fails
    /// without claiming any of them when one is another program's: setup never writes into,
    /// overwrites or replaces what it did not create.
    pub(crate) fn take<S: AsRef<str>>(&mut self, names: &[S]) -> anyhow::Result<()> {
        if let Some(theirs) = names
            .iter()
            .map(AsRef::as_ref)
            .find(|n| !self.owns(n) && !self.claimable(n))
        {
            anyhow::bail!(
                "Setup did not create {} and will not change it. Move it out of the folder and try again.",
                self.dir.join(theirs).display()
            );
        }
        self.add(names.iter().map(|n| n.as_ref().to_string()))
    }

    /// Stops recording what setup claimed but is not in the folder: never created, or removed
    /// since. Whatever takes one of those names later is not setup's.
    pub(crate) fn forget_missing(&mut self) -> anyhow::Result<()> {
        let dir = &self.dir;
        self.created.retain(|name| present(&dir.join(name)));
        self.save()
    }

    /// Whether setup created `name` in the folder.
    pub(crate) fn owns(&self, name: &str) -> bool {
        self.created.contains(name)
    }

    /// Whether setup may claim `name` it does not own yet: it was not in the folder before setup
    /// came, and nothing has put it there since. Another program can add to the folder while a
    /// run is under way, and what it adds is its own by the time a step reaches that name.
    fn claimable(&self, name: &str) -> bool {
        is_plain_name(name) && !self.foreign.contains(&name.to_lowercase()) && !present(&self.dir.join(name))
    }

    fn save(&self) -> anyhow::Result<()> {
        // Written again now, the marker would only be left behind next to `install.json`.
        if self.finished {
            return Ok(());
        }
        let marker = Marker { created: self.names() };
        demido_core::fsx::write_json(&self.dir.join(MARKER), &marker)
            .with_context(|| format!("writing {}", self.dir.join(MARKER).display()))
    }

    /// Everything setup created in the folder, for `install.json`.
    pub(crate) fn names(&self) -> Vec<String> {
        self.created.iter().cloned().collect()
    }

    /// Call once `install.json` records what setup created: the marker is no longer needed. It
    /// stays listed there, so the uninstaller removes it should deleting it fail now.
    pub(crate) fn finish(&mut self) {
        self.finished = true;
        if self.owns(MARKER) {
            let _ = std::fs::remove_file(self.dir.join(MARKER));
        }
    }
}

/// The folder's `install.json`, when it is a record setup can act on: `None` when there is none,
/// or it is damaged, a newer setup's, or does not record what setup created.
pub fn installation(dir: &Path) -> Option<InstallManifest> {
    InstallManifest::load(dir).ok().filter(|m| m.created.is_some())
}

/// The top-level names the payload unpacks into the install folder.
pub fn payload_entries(zip: &[u8]) -> anyhow::Result<Vec<String>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip))?;
    let mut names = BTreeSet::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        if let Some(path) = entry.enclosed_name()
            && let Some(Component::Normal(first)) = path.components().next()
        {
            names.insert(first.to_string_lossy().into_owned());
        }
    }
    Ok(names.into_iter().collect())
}

/// The names setup created in an install folder with this manifest. `None` when it does not
/// record them (only builds that were never released wrote such a manifest): like an
/// `install.json` setup cannot read, it says nothing about which names are setup's.
pub fn created_entries(manifest: &InstallManifest) -> Option<Vec<String>> {
    let created = manifest.created.as_ref()?;
    Some(created.iter().filter(|name| is_plain_name(name)).cloned().collect())
}

/// The names the uninstaller removes from `dir`, whose `install.json` is `manifest`: what that
/// records, and what a run that did not finish (an update that failed, say) listed in its marker.
/// When the manifest cannot be read (`None`: damaged, or a newer setup's) or does not record what
/// setup created, nothing says which names are setup's rather than another program's, so only
/// the marker's list goes or, without one, the uninstaller alone.
pub fn removable_entries(dir: &Path, manifest: Option<&InstallManifest>) -> Vec<String> {
    removable_entries_with(dir, manifest, std::env::current_exe().ok().as_deref())
}

/// [`removable_entries`], with `current_exe` the executable that is running.
pub(crate) fn removable_entries_with(
    dir: &Path,
    manifest: Option<&InstallManifest>,
    current_exe: Option<&Path>,
) -> Vec<String> {
    let marker = marker_entries(dir);
    let mut names: BTreeSet<String> = match manifest.and_then(created_entries) {
        Some(created) => created.into_iter().collect(),
        None if marker.is_empty() => running_uninstaller(dir, current_exe).into_iter().collect(),
        None => BTreeSet::new(),
    };
    names.extend(marker);
    names.into_iter().collect()
}

/// The uninstaller's name, when the one in `dir` is `current_exe`: the uninstaller that was
/// started. Nothing says any other file of that name there is setup's rather than another
/// program's.
fn running_uninstaller(dir: &Path, current_exe: Option<&Path>) -> Option<String> {
    let name = exe(UNINSTALLER_BIN);
    let here = dir.join(&name).canonicalize().ok()?;
    (current_exe?.canonicalize().ok()? == here).then_some(name)
}

/// After an uninstall that could not remove all of `created` from `dir`: rewrites the records
/// among them that are still there to list only `left`, and themselves. Uninstalling again, or
/// an update, then finishes the job, and never takes what fills a name already removed for
/// setup's.
pub(crate) fn keep_records(dir: &Path, created: &[String], left: &[String]) -> anyhow::Result<()> {
    let kept: Vec<&str> = RECORDS
        .into_iter()
        .filter(|record| created.iter().any(|n| n == record) && dir.join(record).is_file())
        .collect();
    let mut listed: BTreeSet<String> = left.iter().cloned().collect();
    listed.extend(kept.iter().map(|record| record.to_string()));
    let listed: Vec<String> = listed.into_iter().collect();
    if kept.contains(&MARKER) {
        demido_core::fsx::write_json(
            &dir.join(MARKER),
            &Marker {
                created: listed.clone(),
            },
        )
        .with_context(|| format!("writing {}", dir.join(MARKER).display()))?;
    }
    if kept.contains(&InstallManifest::FILE_NAME)
        && let Some(mut manifest) = installation(dir)
    {
        manifest.created = Some(listed);
        manifest.save(dir).context("writing install.json")?;
    }
    Ok(())
}

/// Whether anything is at `path`, a broken link included.
pub(crate) fn present(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

/// What the marker in `dir` lists, the marker included; nothing when there is none.
fn marker_entries(dir: &Path) -> Vec<String> {
    let marker = dir.join(MARKER);
    if !marker.is_file() {
        return Vec::new();
    }
    // Only setup writes this name; even unreadable, it is setup's.
    let mut names = vec![MARKER.to_string()];
    if let Ok(m) = demido_core::fsx::read_json::<Marker>(&marker) {
        names.extend(m.created.into_iter().filter(|n| is_plain_name(n)));
    }
    names
}

/// A single file or folder name, so joining it to the install folder never reaches outside it.
fn is_plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None))
}

/// What the wizard needs to know about a folder before installing into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FolderCheck {
    /// Holds a Demido Studio installation (`install.json`), which installing updates.
    pub has_install: bool,
    /// Holds an `install.json` setup cannot read: another program's, a newer setup's, a damaged
    /// one, or one that does not record what setup created. Setup must not overwrite it.
    pub unreadable_install: bool,
    /// Holds files, and neither an installation nor the marker of setup's own unfinished one.
    pub other_files: bool,
    /// Is, holds or sits inside the folder with the person's chats and models.
    pub overlaps_user_data: bool,
}

impl FolderCheck {
    /// Why setup must not install here, in the words the wizard shows.
    pub fn problem(&self) -> Option<&'static str> {
        if self.overlaps_user_data {
            Some("This is where Demido Studio keeps your chats and models. Choose another folder.")
        } else if self.unreadable_install {
            Some(
                "This folder has an install.json that Setup can't read, so it may belong to another program or \
                 a newer Demido Studio. Choose another folder.",
            )
        } else if self.other_files {
            Some("This folder already contains other files. Choose an empty or new folder.")
        } else {
            None
        }
    }
}

/// Checks `dir` as an install folder. `user_data` is the per-user data folder.
pub fn check(dir: &Path, user_data: &Path) -> FolderCheck {
    if dir.as_os_str().is_empty() {
        return FolderCheck::default();
    }
    // Another program's `install.json` is not an installation to update, and not setup's to
    // overwrite, even next to a marker. Nor is a newer setup's (`load` refuses its schema), nor
    // one that does not say what setup created there.
    let has_install = installation(dir).is_some();
    let unreadable_install = !has_install && dir.join(InstallManifest::FILE_NAME).exists();
    // Setup writes its marker before anything else, so the rest is what an unfinished run left.
    // Names alone prove nothing: another program's `logs` folder is not setup's.
    let unfinished = dir.join(MARKER).is_file();
    let other_files =
        !has_install && !unfinished && std::fs::read_dir(dir).is_ok_and(|mut entries| entries.any(|e| e.is_ok()));
    let (dir, data) = (comparable(dir), comparable(user_data));
    FolderCheck {
        has_install,
        unreadable_install,
        other_files,
        overlaps_user_data: dir.starts_with(&data) || data.starts_with(&dir),
    }
}

/// `path` in a form two paths can be compared in. The deepest part that exists is resolved, so
/// links and `..` cannot hide an overlap. The rest is normalised the way the OS does when it
/// creates those folders: `.` and `..` resolved and, on Windows, trailing dots and spaces dropped
/// from each name. Case is folded on Windows.
fn comparable(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut probe = path;
    let mut out = loop {
        // A relative path is relative to the current folder.
        let real = if probe.as_os_str().is_empty() {
            Path::new(".").canonicalize()
        } else {
            probe.canonicalize()
        };
        if let Ok(real) = real {
            break real;
        }
        match (probe.parent(), probe.components().next_back()) {
            (Some(parent), Some(last)) => {
                missing.push(last);
                probe = parent;
            }
            // Not even the root exists.
            _ => break probe.to_path_buf(),
        }
    };
    for part in missing.iter().rev() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) if cfg!(windows) => {
                let name = name.to_string_lossy();
                let name = name.trim_end_matches(['.', ' ']);
                if !name.is_empty() {
                    out.push(name);
                }
            }
            Component::Normal(name) => out.push(name),
            _ => {}
        }
    }
    if cfg!(windows) {
        PathBuf::from(out.to_string_lossy().to_lowercase())
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn lists_the_payloads_top_level_names() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for name in [
            "demido-studio.exe",
            "resources/skills/a.md",
            "resources/sidecars/market.mjs",
            "LICENSE",
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.add_directory("empty/", options).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(
            payload_entries(&bytes).unwrap(),
            ["LICENSE", "demido-studio.exe", "empty", "resources"]
        );
    }

    const UNREADABLE: Option<&str> = Some(
        "This folder has an install.json that Setup can't read, so it may belong to another program or a newer \
         Demido Studio. Choose another folder.",
    );

    /// Only builds that were never released wrote a manifest without `created`. Nothing in it
    /// says which names in the folder are setup's, so it is treated like one setup cannot read.
    #[test]
    fn a_manifest_that_does_not_record_what_setup_created_is_unreadable() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        let mut m = InstallManifest::new(demido_core::InstallScope::User);
        m.created = None;
        m.save(&dir).unwrap();
        touch(&dir.join("logs").join("install.log"));
        touch(&dir.join(exe(UNINSTALLER_BIN)));
        assert!(InstallManifest::load(&dir).is_ok());
        assert_eq!(installation(&dir), None);
        assert_eq!(created_entries(&m), None);

        let found = check(&dir, &root.path().join("data"));
        assert!(!found.has_install);
        assert!(found.unreadable_install);
        assert_eq!(found.problem(), UNREADABLE);

        // The uninstaller removes only itself, when it is the one running, or, after a run that
        // did not finish, its marker's list.
        assert_eq!(removable_entries(&dir, Some(&m)), Vec::<String>::new());
        let started = dir.join(exe(UNINSTALLER_BIN));
        assert_eq!(
            removable_entries_with(&dir, Some(&m), Some(&started)),
            [exe(UNINSTALLER_BIN)]
        );
        demido_core::fsx::write_json(
            &dir.join(MARKER),
            &Marker {
                created: vec!["locales".into()],
            },
        )
        .unwrap();
        assert_eq!(removable_entries(&dir, Some(&m)), [MARKER, "locales"]);
        assert_eq!(check(&dir, &root.path().join("data")).problem(), UNREADABLE);
    }

    #[test]
    fn without_a_readable_manifest_only_the_marker_says_what_is_setups() {
        let root = tempfile::tempdir().unwrap();
        // A damaged or newer `install.json` in a folder setup was forced into: the known names
        // there may be another program's.
        std::fs::write(root.path().join("install.json"), b"{\"schema\": 1, \"vers").unwrap();
        touch(&root.path().join("logs").join("server.log"));
        touch(&root.path().join(exe(UNINSTALLER_BIN)));
        let started = root.path().join(exe(UNINSTALLER_BIN));
        assert_eq!(
            removable_entries_with(root.path(), None, Some(&started)),
            [exe(UNINSTALLER_BIN)]
        );
        demido_core::fsx::write_json(
            &root.path().join(MARKER),
            &Marker {
                created: vec!["locales".into()],
            },
        )
        .unwrap();
        assert_eq!(removable_entries(root.path(), None), [MARKER, "locales"]);
    }

    #[test]
    fn recorded_names_that_would_leave_the_folder_are_ignored() {
        let mut m = InstallManifest::new(demido_core::InstallScope::User);
        m.created = Some(vec![
            "runtime".into(),
            "..".into(),
            "../elsewhere".into(),
            "a/b".into(),
            String::new(),
            ".".into(),
        ]);
        assert_eq!(created_entries(&m).unwrap(), ["runtime"]);
    }

    #[test]
    fn empty_and_new_folders_are_fine() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let data = elsewhere.path().join("data");
        assert_eq!(check(root.path(), &data), FolderCheck::default());
        assert_eq!(
            check(&root.path().join("new").join("deeper"), &data),
            FolderCheck::default()
        );
    }

    #[test]
    fn folders_with_other_files_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Tools");
        touch(&dir.join("other-program.exe"));
        let found = check(&dir, &root.path().join("data"));
        assert!(found.other_files);
        assert!(!found.has_install);
        assert_eq!(
            found.problem(),
            Some("This folder already contains other files. Choose an empty or new folder.")
        );
    }

    #[test]
    fn an_existing_installation_can_be_updated() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        InstallManifest::new(demido_core::InstallScope::User)
            .save(&dir)
            .unwrap();
        touch(&dir.join("something-the-person-added.txt"));
        let found = check(&dir, &root.path().join("data"));
        assert!(found.has_install);
        assert_eq!(found.problem(), None);
    }

    #[test]
    fn an_install_json_setup_cannot_read_is_refused_for_what_it_is() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        // Another program's, a newer setup's (whether or not this build could parse it), a
        // damaged one, one that does not record what setup created.
        let mut newer = serde_json::to_value(InstallManifest::new(demido_core::InstallScope::User)).unwrap();
        newer["schema"] = (InstallManifest::SCHEMA + 1).into();
        let mut unrecorded = serde_json::to_value(InstallManifest::new(demido_core::InstallScope::User)).unwrap();
        unrecorded.as_object_mut().unwrap().remove("created");
        let contents: [Vec<u8>; 5] = [
            br#"{"product": "other"}"#.to_vec(),
            br#"{"schema": 2, "release": "9.0.0"}"#.to_vec(),
            serde_json::to_vec(&newer).unwrap(),
            b"{\"schema\": 1, \"vers".to_vec(),
            serde_json::to_vec(&unrecorded).unwrap(),
        ];
        for (i, content) in contents.into_iter().enumerate() {
            let dir = root.path().join(format!("Folder {i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("install.json"), content).unwrap();
            let found = check(&dir, &data);
            assert!(!found.has_install);
            assert!(found.unreadable_install);
            assert_eq!(found.problem(), UNREADABLE);
            // Not even the marker of an unfinished run makes it setup's to overwrite.
            touch(&dir.join(MARKER));
            assert_eq!(check(&dir, &data).problem(), UNREADABLE);
        }
        // The folder with the person's data still says so first.
        let found = check(&root.path().join("Folder 0"), &root.path().join("Folder 0"));
        assert_eq!(
            found.problem(),
            Some("This is where Demido Studio keeps your chats and models. Choose another folder.")
        );
        // Only an `install.json` that is there counts.
        let found = check(&root.path().join("Empty"), &data);
        assert!(!found.unreadable_install);
        assert!(!check(&root.path().join("Folder 0").join("nested"), &data).unreadable_install);
    }

    #[test]
    fn names_setup_uses_do_not_make_a_folder_setups() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        // Another program's folder that happens to hold only a `logs` folder, say D:\Server.
        let server = root.path().join("Server");
        touch(&server.join("logs").join("server.log"));
        assert!(check(&server, &data).other_files);
        // An unfinished install of setup's own is recognised by its marker, whatever it left.
        let unfinished = root.path().join("Demido Studio");
        touch(&unfinished.join("logs").join("install.log"));
        touch(&unfinished.join("resources").join("skills").join("a.md"));
        touch(&unfinished.join(MARKER));
        assert_eq!(check(&unfinished, &data), FolderCheck::default());
    }

    fn created(dir: &Path) -> Vec<String> {
        demido_core::fsx::read_json::<Marker>(&dir.join(MARKER))
            .unwrap()
            .created
    }

    /// The top-level names the engine itself creates.
    fn engine_entries() -> Vec<String> {
        ["runtime", "logs", ".downloads", InstallManifest::FILE_NAME]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn a_fresh_folder_is_claimed_through_the_marker() {
        let root = tempfile::tempdir().unwrap();
        let mut claim = Claim::begin(root.path(), None).unwrap();
        // The marker is there, listing itself, before anything else.
        assert_eq!(created(root.path()), [MARKER]);
        claim.add(["logs".to_string(), "runtime".into()]).unwrap();
        assert_eq!(created(root.path()), [MARKER, "logs", "runtime"]);
        assert!(claim.owns("logs"));
        claim.finish();
        assert!(!root.path().join(MARKER).exists());
        assert_eq!(claim.names(), [MARKER, "logs", "runtime"]);
        // A run that stops after that (cancelled at the very end) does not bring it back.
        claim.forget_missing().unwrap();
        assert!(!root.path().join(MARKER).exists());
    }

    #[test]
    fn what_was_in_the_folder_before_is_never_claimed() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("logs").join("server.log"));
        touch(&root.path().join(".downloads").join("keep.bin"));
        // On Windows and macOS this is the `resources` folder the app would unpack into.
        touch(&root.path().join("Resources").join("theirs.txt"));
        // Not even the names only setup uses are taken.
        let mut claim = Claim::begin(root.path(), None).unwrap();
        claim.add(engine_entries()).unwrap();
        claim.add(["resources".to_string(), "LICENSE".into()]).unwrap();
        assert!(!claim.owns("logs"));
        assert!(!claim.owns(".downloads"));
        assert!(!claim.owns("resources"));
        assert_eq!(claim.names(), [MARKER, "LICENSE", "install.json", "runtime"]);
        assert_eq!(created(root.path()), claim.names());

        // Nor by the next run, now that setup's `install.json` sits next to them.
        let mut manifest = InstallManifest::new(demido_core::InstallScope::User);
        manifest.created = Some(claim.names());
        manifest.save(root.path()).unwrap();
        claim.finish();
        let mut next = Claim::begin(root.path(), Some(&manifest)).unwrap();
        next.add(engine_entries()).unwrap();
        next.add(["resources".to_string(), "LICENSE".into()]).unwrap();
        assert_eq!(next.names(), [MARKER, "LICENSE", "install.json", "runtime"]);
        assert_eq!(created(root.path()), next.names());
    }

    #[test]
    fn a_step_never_takes_what_is_another_programs() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("Resources").join("theirs.txt"));
        let mut manifest = InstallManifest::new(demido_core::InstallScope::User);
        manifest.created = Some(vec!["runtime".into()]);
        manifest.save(root.path()).unwrap();
        let mut claim = Claim::begin(root.path(), Some(&manifest)).unwrap();
        // What setup created, and names nothing holds yet, are its to write into.
        claim.take(&["runtime", ".downloads"]).unwrap();
        assert_eq!(claim.names(), [MARKER, ".downloads", "runtime"]);
        // Another program's, whatever its case, fails the step, and nothing else is claimed.
        let err = claim.take(&["locales", "resources"]).unwrap_err().to_string();
        assert!(err.starts_with("Setup did not create "), "{err}");
        assert!(err.contains("resources"), "{err}");
        assert!(!claim.owns("locales"));
        assert_eq!(created(root.path()), claim.names());
    }

    #[test]
    fn what_another_program_adds_while_setup_runs_is_never_claimed() {
        let root = tempfile::tempdir().unwrap();
        let mut claim = Claim::begin(root.path(), None).unwrap();
        claim.add(["logs".to_string()]).unwrap();
        touch(&root.path().join("logs").join("install.log"));
        // Added after setup took stock of the folder.
        touch(&root.path().join("runtime").join("theirs.txt"));
        touch(&root.path().join("locales").join("theirs.pak"));
        let err = claim.take(&[".downloads", "runtime"]).unwrap_err().to_string();
        assert!(err.starts_with("Setup did not create "), "{err}");
        assert!(err.contains("runtime"), "{err}");
        assert!(!claim.owns(".downloads"));
        claim.add(["locales".to_string(), "LICENSE".into()]).unwrap();
        // What setup claimed before creating it stays its own.
        claim.take(&["logs"]).unwrap();
        assert_eq!(claim.names(), [MARKER, "LICENSE", "logs"]);
        assert_eq!(created(root.path()), claim.names());
    }

    /// Without a record of what setup created, the uninstaller in the folder is setup's only
    /// when it is the one that was started.
    #[test]
    fn without_a_record_only_the_running_uninstaller_is_removable() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Tools");
        let uninstaller = dir.join(exe(UNINSTALLER_BIN));
        touch(&uninstaller);
        // Another program's, or one nothing started: these tests are what runs.
        assert_eq!(removable_entries(&dir, None), Vec::<String>::new());
        let elsewhere = root.path().join("Elsewhere").join(exe(UNINSTALLER_BIN));
        touch(&elsewhere);
        assert_eq!(
            removable_entries_with(&dir, None, Some(&elsewhere)),
            Vec::<String>::new()
        );
        // The one that was started, however its path was written.
        assert_eq!(
            removable_entries_with(&dir, None, Some(&uninstaller)),
            [exe(UNINSTALLER_BIN)]
        );
        let roundabout = root
            .path()
            .join("Elsewhere")
            .join("..")
            .join("Tools")
            .join(exe(UNINSTALLER_BIN));
        assert_eq!(
            removable_entries_with(&dir, None, Some(&roundabout)),
            [exe(UNINSTALLER_BIN)]
        );
        // Nor is one there when nothing is.
        std::fs::remove_file(&uninstaller).unwrap();
        assert_eq!(
            removable_entries_with(&dir, None, Some(&uninstaller)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn what_is_no_longer_there_is_no_longer_setups() {
        let root = tempfile::tempdir().unwrap();
        let mut claim = Claim::begin(root.path(), None).unwrap();
        claim
            .add(["logs".to_string(), ".downloads".into(), "runtime".into()])
            .unwrap();
        touch(&root.path().join("logs").join("install.log"));
        touch(&root.path().join(".downloads").join("partial.bin"));
        // `runtime` was never created; the downloads are removed once the run succeeds.
        std::fs::remove_dir_all(root.path().join(".downloads")).unwrap();
        claim.forget_missing().unwrap();
        assert_eq!(claim.names(), [MARKER, "logs"]);
        assert_eq!(created(root.path()), claim.names());

        // So whatever takes those names later is another program's.
        touch(&root.path().join(".downloads").join("theirs.bin"));
        let mut manifest = InstallManifest::new(demido_core::InstallScope::User);
        manifest.created = Some(claim.names());
        manifest.save(root.path()).unwrap();
        claim.finish();
        let mut next = Claim::begin(root.path(), Some(&manifest)).unwrap();
        next.add([".downloads".to_string()]).unwrap();
        assert!(!next.owns(".downloads"));
    }

    #[test]
    fn a_retry_takes_back_what_the_unfinished_run_created() {
        let root = tempfile::tempdir().unwrap();
        let mut first = Claim::begin(root.path(), None).unwrap();
        first.add(["logs".to_string(), "resources".into()]).unwrap();
        touch(&root.path().join("logs").join("install.log"));
        touch(&root.path().join("resources").join("a.md"));
        // The person drops a file in before trying again.
        touch(&root.path().join("notes.txt"));

        let mut retry = Claim::begin(root.path(), None).unwrap();
        retry.add(["notes.txt".to_string(), "LICENSE".into()]).unwrap();
        assert_eq!(retry.names(), [MARKER, "LICENSE", "logs", "resources"]);
    }

    #[test]
    fn an_unreadable_marker_is_still_setups() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join(MARKER));
        touch(&root.path().join("logs").join("install.log"));
        let claim = Claim::begin(root.path(), None).unwrap();
        // What it listed is lost, so the leftovers stay; the marker itself is setup's.
        assert_eq!(claim.names(), [MARKER]);
        assert_eq!(created(root.path()), [MARKER]);
        assert!(removable_entries(root.path(), None).contains(&MARKER.to_string()));
    }

    #[test]
    fn an_update_builds_on_what_the_installation_created() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("old-payload-folder").join("file"));
        touch(&root.path().join("notes.txt"));
        let mut manifest = InstallManifest::new(demido_core::InstallScope::User);
        manifest.created = Some(vec!["install.json".into(), "old-payload-folder".into()]);
        manifest.save(root.path()).unwrap();
        assert_eq!(
            removable_entries(root.path(), Some(&manifest)),
            ["install.json", "old-payload-folder"]
        );

        let mut claim = Claim::begin(root.path(), Some(&manifest)).unwrap();
        claim
            .add(["notes.txt".to_string(), "logs".into(), "locales".into()])
            .unwrap();
        assert_eq!(
            claim.names(),
            [MARKER, "install.json", "locales", "logs", "old-payload-folder"]
        );
        // `install.json` is rewritten only at the end, so until then the marker keeps the record:
        // an update that stops here still leaves `locales` to the uninstaller.
        assert_eq!(created(root.path()), claim.names());
        assert_eq!(removable_entries(root.path(), Some(&manifest)), claim.names());
        claim.finish();
        assert!(!root.path().join(MARKER).exists());

        // A manifest that does not record what setup created makes nothing there setup's.
        manifest.created = None;
        let mut claim = Claim::begin(root.path(), Some(&manifest)).unwrap();
        claim
            .add(["install.json".to_string(), "old-payload-folder".into()])
            .unwrap();
        assert_eq!(claim.names(), [MARKER]);
    }

    /// Only builds that were never released wrote a `created` that leaves out some of what setup
    /// made. What such a list leaves out stays foreign: setup cannot tell its own `logs` from
    /// another program's, and leaving one of its folders behind is the safe mistake.
    #[test]
    fn what_a_partial_manifest_leaves_out_is_not_taken_back() {
        let root = tempfile::tempdir().unwrap();
        // What an installation left, with a manifest that lists only the app.
        let mut manifest = InstallManifest::new(demido_core::InstallScope::User);
        manifest.created = Some(vec!["demido-studio.exe".into()]);
        manifest.save(root.path()).unwrap();
        for file in [
            "demido-studio.exe",
            "logs/install.log",
            "runtime/llama/llama-server.exe",
            "resources/skills/a.md",
            ".downloads/partial.bin",
        ] {
            touch(&root.path().join(file));
        }
        // And what the person keeps there, which the manifest does not list either.
        touch(&root.path().join("notes.txt"));
        touch(&root.path().join("Other App").join("other.exe"));

        let app = ["demido-studio.exe", "resources", "LICENSE"];
        let mut claim = Claim::begin(root.path(), Some(&manifest)).unwrap();
        claim.add(engine_entries()).unwrap();
        claim.add(app.map(String::from)).unwrap();
        claim.add(["notes.txt".to_string(), "Other App".into()]).unwrap();
        // Only what it lists, and what this run creates anew.
        assert_eq!(claim.names(), [MARKER, "LICENSE", "demido-studio.exe"]);
        assert_eq!(created(root.path()), claim.names());
        for left_out in [".downloads", "install.json", "logs", "resources", "runtime"] {
            assert!(!claim.owns(left_out), "{left_out} was taken back");
        }
    }

    #[test]
    fn the_user_data_folder_and_its_surroundings_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("Local").join("Demido Studio");
        std::fs::create_dir_all(&data).unwrap();
        // Is it, holds it, sits inside it (even before that subfolder exists).
        for dir in [data.clone(), root.path().join("Local"), data.join("Programs")] {
            let found = check(&dir, &data);
            assert!(
                found.overlaps_user_data,
                "{} overlaps {}",
                dir.display(),
                data.display()
            );
            assert_eq!(
                found.problem(),
                Some("This is where Demido Studio keeps your chats and models. Choose another folder.")
            );
        }
        // A sibling whose name merely starts the same way is fine.
        let sibling = root.path().join("Local").join("Demido Studio 2");
        assert!(!check(&sibling, &data).overlaps_user_data);
        // `..` does not hide it, nor does a data folder that does not exist yet.
        std::fs::create_dir_all(root.path().join("Local").join("x")).unwrap();
        let sneaky = root.path().join("Local").join("x").join("..").join("Demido Studio");
        assert!(check(&sneaky, &data).overlaps_user_data);
        let fresh = root.path().join("Fresh").join("Demido Studio");
        assert!(check(&fresh, &fresh).overlaps_user_data);
    }

    #[test]
    fn dots_in_folders_that_do_not_exist_yet_are_resolved() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("Local");
        std::fs::create_dir_all(&local).unwrap();
        // Neither the data folder nor anything typed below it exists yet.
        let data = local.join("Demido Studio");
        for dir in [
            data.join("x").join(".."),
            data.join("x").join("y").join("..").join(".."),
            local.join("new").join("..").join("Demido Studio"),
            data.join(".").join("x"),
            local.join("new").join("..").join("..").join("Local"),
        ] {
            assert!(check(&dir, &data).overlaps_user_data, "{} overlaps", dir.display());
        }
        for dir in [
            data.join("..").join("Elsewhere"),
            local.join("new").join("..").join("Demido Studio 2"),
        ] {
            assert!(
                !check(&dir, &data).overlaps_user_data,
                "{} does not overlap",
                dir.display()
            );
        }
    }

    #[test]
    fn the_default_install_folders_are_not_the_user_data_folder() {
        let data = demido_core::paths::user_data_dir();
        for scope in [demido_core::InstallScope::User, demido_core::InstallScope::Machine] {
            let dir = demido_core::paths::default_install_dir(scope);
            assert!(!check(&dir, &data).overlaps_user_data, "{} overlaps", dir.display());
        }
    }

    #[cfg(windows)]
    #[test]
    fn case_does_not_hide_the_user_data_folder_on_windows() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("Demido Studio");
        std::fs::create_dir_all(&data).unwrap();
        let shouted = PathBuf::from(data.to_string_lossy().to_uppercase());
        assert!(check(&shouted, &data).overlaps_user_data);
    }

    /// Windows drops trailing dots and spaces from each name when it creates a folder, so
    /// `Demido Studio.` is the data folder.
    #[cfg(windows)]
    #[test]
    fn trailing_dots_and_spaces_do_not_hide_the_user_data_folder_on_windows() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("Local");
        std::fs::create_dir_all(&local).unwrap();
        let data = local.join("Demido Studio");
        let typed = |names: &[&str]| names.iter().fold(local.clone(), |path, name| path.join(name));
        let cases = [
            typed(&["Demido Studio."]),
            typed(&["Demido Studio "]),
            typed(&["Demido Studio. . "]),
            typed(&["Demido Studio.", "Programs"]),
            typed(&["new.", "..", "Demido Studio.."]),
        ];
        // Before the data folder exists, then once it does.
        for exists in [false, true] {
            if exists {
                std::fs::create_dir_all(&data).unwrap();
            }
            for dir in &cases {
                assert!(check(dir, &data).overlaps_user_data, "{} overlaps", dir.display());
            }
        }
        assert!(!check(&typed(&["Demido Studio.2"]), &data).overlaps_user_data);
    }

    #[cfg(unix)]
    #[test]
    fn trailing_dots_are_part_of_the_name_elsewhere() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("demido-studio");
        assert!(!check(&root.path().join("demido-studio."), &data).overlaps_user_data);
    }
}
