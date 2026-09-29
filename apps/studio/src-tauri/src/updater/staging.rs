//! The updates folder: a verified installer waiting to run, and `pending.json` describing it.
//!
//! A download lands in `<data>/updates/Demido-Studio-Setup-<version>.exe`; its `.part` file lets
//! an interrupted download continue. Once the signature checks out, `pending.json` records it and
//! the update is "ready": installed with a click, or at the next launch in automatic mode. The
//! signature is also written beside the installer as `<installer>.sig`: setup, running elevated
//! for a machine-wide installation, checks the copy of itself it leaves as the uninstaller
//! against it. At every launch [`reconcile`] looks at what is there, decides what it means and
//! tidies up; an installer and its `.sig` stay or go together.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};
use demido_core::brand;
use semver::Version;
use serde::{Deserialize, Serialize};

use super::feed::ReleaseInfo;

pub const PENDING_FILE: &str = "pending.json";

/// A verified download waiting to be installed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub version: String,
    /// The installer's file name, inside the updates folder.
    pub file: String,
    /// The release's `.sig` text, checked again right before the installer runs.
    pub signature: String,
    pub release: ReleaseInfo,
    /// Times the installer was started for this update. It is counted before the installer
    /// starts, so a count above zero at launch means an attempt did not finish.
    #[serde(default)]
    pub attempts: u32,
    pub downloaded_at: DateTime<Utc>,
}

/// The updates folder. Everything in it belongs to the updater.
#[derive(Clone, Debug)]
pub struct Staging {
    dir: PathBuf,
}

impl Staging {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where a file of the updates folder lives.
    pub fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    pub fn installer(&self, pending: &Pending) -> PathBuf {
        self.path(&pending.file)
    }

    /// The installer's signature, beside it.
    pub fn signature(&self, pending: &Pending) -> PathBuf {
        self.path(&signature_file(&pending.file))
    }

    fn read(&self) -> Read {
        let path = self.path(PENDING_FILE);
        if !path.exists() {
            return Read::Missing;
        }
        match demido_core::fsx::read_json::<Pending>(&path) {
            Ok(pending) => Read::Found(pending),
            Err(err) => Read::Unreadable(format!("{err:#}")),
        }
    }

    /// Writes the record (again, as when an install attempt is counted).
    pub fn save(&self, pending: &Pending) -> anyhow::Result<()> {
        demido_core::fsx::write_json(&self.path(PENDING_FILE), pending)
    }

    /// Records a verified download as ready: its signature beside the installer, then the record,
    /// so a record never points at an installer without its `.sig`.
    pub fn stage(&self, pending: &Pending) -> anyhow::Result<()> {
        let sig = self.signature(pending);
        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        std::fs::write(&sig, &pending.signature).with_context(|| format!("writing {}", sig.display()))?;
        self.save(pending)
    }

    /// Forgets the pending update and deletes its installer and signature.
    pub fn discard(&self, pending: &Pending) {
        remove(&self.path(PENDING_FILE));
        remove(&self.installer(pending));
        remove(&self.signature(pending));
    }

    /// Deletes a download's partial file.
    pub fn discard_partial(&self, file: &str) {
        remove(&self.path(&format!("{file}.part")));
    }

    /// Deletes every file of the folder that `keep` does not claim.
    pub fn prune(&self, keep: impl Fn(&str) -> bool) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().is_ok_and(|t| t.is_file()) && !keep(&name) {
                remove(&entry.path());
            }
        }
    }

    /// After a new download is staged: nothing but it, its signature and its record stays.
    pub fn keep_only(&self, pending: &Pending) {
        self.prune(|name| belongs_to(name, pending));
    }
}

/// The name of an installer's signature file: `<installer>.sig`, as releases publish it.
pub fn signature_file(installer: &str) -> String {
    format!("{installer}.sig")
}

/// Whether `name` is one of the files of the staged `pending`: its record, installer or signature.
fn belongs_to(name: &str, pending: &Pending) -> bool {
    name == PENDING_FILE || name == pending.file || name == signature_file(&pending.file)
}

enum Read {
    Missing,
    Found(Pending),
    Unreadable(String),
}

fn remove(path: &Path) {
    if let Err(err) = std::fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("could not delete {}: {err}", path.display());
    }
}

/// The version in an installer's file name or its `.part`, by the pattern
/// [`brand::setup_file_name`] writes.
pub fn version_in(file_name: &str) -> Option<Version> {
    let name = file_name.strip_suffix(".part").unwrap_or(file_name);
    let pattern = brand::setup_file_name("{version}");
    let (prefix, suffix) = pattern.split_once("{version}")?;
    Version::parse(name.strip_prefix(prefix)?.strip_suffix(suffix)?).ok()
}

/// What [`reconcile`] found in the updates folder.
#[derive(Debug, PartialEq)]
pub enum Reconciled {
    /// Nothing is waiting.
    Nothing,
    /// The staged version is the one running now: the update worked.
    Installed(String),
    /// A verified update newer than this version. `failed_attempt`: the installer was started
    /// for it before and this is still the old version.
    Ready { pending: Pending, failed_attempt: bool },
    /// The staged update was dropped, for the reason given (for the log).
    Discarded(String),
}

/// Decides what the updates folder means for the version `running` and tidies it:
///
/// - the pending version is the running one: the update worked, and the folder is emptied;
/// - it is older: stale, deleted;
/// - it is newer: ready, when its installer is there and `verify` accepts it; deleted otherwise.
///
/// Then any other file goes, except the partial download of a newer version, which the next
/// check resumes instead of starting over.
pub fn reconcile(
    staging: &Staging,
    running: &Version,
    verify: impl Fn(&Path, &Pending) -> anyhow::Result<()>,
) -> Reconciled {
    let outcome = match staging.read() {
        Read::Missing => Reconciled::Nothing,
        Read::Unreadable(err) => {
            remove(&staging.path(PENDING_FILE));
            Reconciled::Discarded(format!("{PENDING_FILE} could not be read: {err}"))
        }
        Read::Found(pending) => judge(staging, running, pending, verify),
    };

    match &outcome {
        Reconciled::Installed(_) => staging.prune(|_| false),
        Reconciled::Ready { pending, .. } => {
            staging.prune(|name| belongs_to(name, pending) || newer_partial(name, running))
        }
        Reconciled::Nothing | Reconciled::Discarded(_) => staging.prune(|name| newer_partial(name, running)),
    }
    outcome
}

fn judge(
    staging: &Staging,
    running: &Version,
    pending: Pending,
    verify: impl Fn(&Path, &Pending) -> anyhow::Result<()>,
) -> Reconciled {
    // Until the record names a real installer, its `file` is not trusted as a path: only the record
    // goes, and the tidy-up that follows clears the rest of the folder.
    let Ok(version) = Version::parse(&pending.version) else {
        remove(&staging.path(PENDING_FILE));
        return Reconciled::Discarded(format!("{:?} is not a version", pending.version));
    };
    // The installer's name is part of what the signature covers, and the only file the updater
    // will ever run from here.
    if pending.file != brand::setup_file_name(&pending.version) {
        remove(&staging.path(PENDING_FILE));
        return Reconciled::Discarded(format!("{:?} is not the installer of {version}", pending.file));
    }
    match version.cmp_precedence(running) {
        Ordering::Equal => Reconciled::Installed(pending.version),
        Ordering::Less => {
            staging.discard(&pending);
            Reconciled::Discarded(format!("{version} is older than this version"))
        }
        Ordering::Greater => {
            let installer = staging.installer(&pending);
            let checked = if installer.is_file() {
                verify(&installer, &pending)
            } else {
                Err(anyhow::anyhow!("its installer is missing"))
            };
            match checked {
                Ok(()) => Reconciled::Ready {
                    failed_attempt: pending.attempts >= 1,
                    pending,
                },
                Err(err) => {
                    staging.discard(&pending);
                    Reconciled::Discarded(format!("{version}: {err:#}"))
                }
            }
        }
    }
}

fn newer_partial(name: &str, running: &Version) -> bool {
    name.ends_with(".part") && version_in(name).is_some_and(|v| v.cmp_precedence(running) == Ordering::Greater)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn pending(version: &str, attempts: u32) -> Pending {
        Pending {
            version: version.to_string(),
            file: brand::setup_file_name(version),
            signature: "sig".into(),
            release: ReleaseInfo {
                version: version.to_string(),
                notes: String::new(),
                published_at: None,
                url: "https://github.com/x".into(),
                size: 7,
                prerelease: false,
            },
            attempts,
            downloaded_at: Utc::now(),
        }
    }

    /// A folder holding `pending` (and its installer and signature, as a download stages them,
    /// when `with_installer`) plus loose files.
    fn folder(pending: Option<&Pending>, with_installer: bool, loose: &[&str]) -> (tempfile::TempDir, Staging) {
        let dir = tempfile::tempdir().unwrap();
        let staging = Staging::new(dir.path().join("updates"));
        std::fs::create_dir_all(staging.dir()).unwrap();
        if let Some(p) = pending {
            if with_installer {
                std::fs::write(staging.installer(p), b"installer").unwrap();
                staging.stage(p).unwrap();
            } else {
                staging.save(p).unwrap();
            }
        }
        for name in loose {
            std::fs::write(staging.path(name), b"x").unwrap();
        }
        (dir, staging)
    }

    fn files(staging: &Staging) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(staging.dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn accept(_: &Path, _: &Pending) -> anyhow::Result<()> {
        Ok(())
    }

    #[test]
    fn a_crafted_record_never_deletes_outside_the_updates_folder() {
        for (version, file) in [("0.5.0", "..\\outside.txt"), ("not a version", "../outside.txt")] {
            let mut crafted = pending("0.5.0", 0);
            crafted.version = version.to_string();
            crafted.file = file.to_string();
            let (dir, staging) = folder(None, false, &[]);
            staging.save(&crafted).unwrap();
            let outside = dir.path().join("outside.txt");
            std::fs::write(&outside, b"not the updater's").unwrap();
            let outcome = reconcile(&staging, &v("0.4.0"), accept);
            assert!(matches!(outcome, Reconciled::Discarded(_)), "{outcome:?}");
            assert!(outside.is_file(), "{file} was deleted");
            assert!(files(&staging).is_empty());
        }
    }

    fn reject(_: &Path, _: &Pending) -> anyhow::Result<()> {
        anyhow::bail!("bad signature")
    }

    fn part(version: &str) -> String {
        format!("{}.part", brand::setup_file_name(version))
    }

    fn sig(version: &str) -> String {
        format!("{}.sig", brand::setup_file_name(version))
    }

    /// The files a staged `p` leaves in the folder, plus `more`, sorted.
    fn staged_files(p: &Pending, more: &[String]) -> Vec<String> {
        let mut f = vec![PENDING_FILE.to_string(), p.file.clone(), sig(&p.version)];
        f.extend(more.iter().cloned());
        f.sort();
        f
    }

    #[test]
    fn a_newer_verified_update_stays_ready() {
        let p = pending("0.5.0", 0);
        let (_d, staging) = folder(Some(&p), true, &[]);
        let outcome = reconcile(&staging, &v("0.4.0"), accept);
        assert_eq!(
            outcome,
            Reconciled::Ready {
                pending: p.clone(),
                failed_attempt: false
            }
        );
        assert_eq!(files(&staging), staged_files(&p, &[]));
    }

    #[test]
    fn staging_writes_the_signature_beside_the_installer() {
        let p = pending("0.5.0", 0);
        let (_d, staging) = folder(Some(&p), true, &[]);
        let path = staging.signature(&p);
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            "Demido-Studio-Setup-0.5.0.exe.sig"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), p.signature);
        assert!(matches!(staging.read(), Read::Found(found) if found == p));

        // Discarding it takes the signature along.
        staging.discard(&p);
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn an_attempt_that_did_not_finish_is_remembered() {
        let p = pending("0.5.0", 1);
        let (_d, staging) = folder(Some(&p), true, &[]);
        let Reconciled::Ready { failed_attempt, .. } = reconcile(&staging, &v("0.4.0"), accept) else {
            panic!("expected ready");
        };
        assert!(failed_attempt);
    }

    #[test]
    fn the_running_version_means_the_update_worked_and_everything_goes() {
        let p = pending("0.5.0", 1);
        let (_d, staging) = folder(Some(&p), true, &[&part("0.6.0"), "stray.tmp"]);
        assert_eq!(
            reconcile(&staging, &v("0.5.0"), reject),
            Reconciled::Installed("0.5.0".into())
        );
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn an_older_update_is_stale() {
        let p = pending("0.4.1", 0);
        let (_d, staging) = folder(Some(&p), true, &[]);
        assert!(matches!(
            reconcile(&staging, &v("0.5.0"), accept),
            Reconciled::Discarded(_)
        ));
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn a_failing_signature_or_a_missing_installer_is_deleted() {
        let p = pending("0.5.0", 0);
        let (_d, staging) = folder(Some(&p), true, &[]);
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), reject),
            Reconciled::Discarded(_)
        ));
        assert!(files(&staging).is_empty());

        let (_d, staging) = folder(Some(&p), false, &[]);
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), accept),
            Reconciled::Discarded(_)
        ));
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn a_record_pointing_at_another_file_is_deleted_unverified() {
        let mut p = pending("0.5.0", 0);
        p.file = "..\\..\\evil.exe".into();
        let (_d, staging) = folder(Some(&p), false, &[]);
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), |_: &Path, _: &Pending| panic!("must not verify")),
            Reconciled::Discarded(_)
        ));
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn an_unreadable_record_is_deleted() {
        let (_d, staging) = folder(None, false, &[PENDING_FILE]);
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), accept),
            Reconciled::Discarded(_)
        ));
        assert!(files(&staging).is_empty());
    }

    #[test]
    fn leftovers_go_except_a_newer_partial_download() {
        let p = pending("0.5.0", 0);
        let loose = [
            part("0.6.0"),
            part("0.5.0"),
            part("0.4.0"),
            part("0.3.0"),
            brand::setup_file_name("0.4.5"),
            sig("0.4.5"),
            sig("0.6.0"),
            ".pending.json.123.tmp".to_string(),
            "notes.txt".to_string(),
        ];
        let loose: Vec<&str> = loose.iter().map(String::as_str).collect();
        let (_d, staging) = folder(Some(&p), true, &loose);
        reconcile(&staging, &v("0.4.0"), accept);
        // The staged installer keeps its signature; no other signature stays.
        assert_eq!(files(&staging), staged_files(&p, &[part("0.6.0"), part("0.5.0")]));

        let (_d, staging) = folder(None, false, &loose);
        assert_eq!(reconcile(&staging, &v("0.4.0"), accept), Reconciled::Nothing);
        let mut expected = vec![part("0.6.0"), part("0.5.0")];
        expected.sort();
        assert_eq!(files(&staging), expected);
    }

    #[test]
    fn a_missing_folder_is_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let staging = Staging::new(dir.path().join("never-created"));
        assert_eq!(reconcile(&staging, &v("0.4.0"), accept), Reconciled::Nothing);
    }

    #[test]
    fn versions_are_read_back_from_installer_names() {
        assert_eq!(version_in(&brand::setup_file_name("0.5.0")), Some(v("0.5.0")));
        assert_eq!(version_in(&part("0.6.0-beta.1")), Some(v("0.6.0-beta.1")));
        assert_eq!(version_in("pending.json"), None);
        assert_eq!(version_in(&brand::setup_file_name("latest")), None);
    }

    #[test]
    fn keep_only_leaves_the_new_download_its_signature_and_its_record() {
        let p = pending("0.6.0", 0);
        let (_d, staging) = folder(
            Some(&p),
            true,
            &[&part("0.5.0"), &brand::setup_file_name("0.5.0"), &sig("0.5.0")],
        );
        staging.keep_only(&p);
        assert_eq!(files(&staging), staged_files(&p, &[]));
    }

    /// The real check, end to end, with the test key's signature over a staged file.
    #[cfg(windows)]
    #[test]
    fn a_really_signed_installer_verifies_from_the_folder() {
        use demido_core::signature::fixtures::{TEST_DATA, TEST_PUBLIC_KEY, TEST_SIGNATURE, TEST_VERSION};
        let mut p = pending(TEST_VERSION, 0);
        p.signature = TEST_SIGNATURE.into();
        let (_d, staging) = folder(Some(&p), false, &[]);
        std::fs::write(staging.installer(&p), TEST_DATA).unwrap();
        let check = |path: &Path, p: &Pending| {
            crate::updater::verify::verify_file(path, &p.signature, TEST_PUBLIC_KEY, &p.file, &p.version)
        };
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), check),
            Reconciled::Ready { .. }
        ));
        std::fs::write(staging.installer(&p), b"tampered").unwrap();
        assert!(matches!(
            reconcile(&staging, &v("0.4.0"), check),
            Reconciled::Discarded(_)
        ));
    }
}
