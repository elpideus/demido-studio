//! Unpacking an archive without trusting it.
//!
//! An archive is a list of names chosen by whoever made it. A name like `../../.bashrc`, or a link that
//! points outside, would write where it must not. Every entry is checked before anything is written:
//! plain names only, links only to places inside the folder, and a ceiling on how much may come out
//! (a small archive can hold a very large one).

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Error, Result};

/// More than any runtime needs. The CUDA libraries, the largest, unpack to about 2 GB.
const MOST_BYTES: u64 = 16 << 30;
const MOST_ENTRIES: usize = 50_000;

fn refuse(archive: &Path, why: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("{} was not unpacked: {why}. Nothing from it was kept.", archive.file_name().unwrap_or_default().to_string_lossy()))
}

/// `a/b/c`, and nothing that climbs or starts from the root. A leading `./` is harmless and dropped.
fn inside(path: &Path) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => clean.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

/// Whether a link stored at `at` and pointing to `target` stays inside the folder.
/// `lib/libllama.so -> libllama.so.0` does. `lib/x -> ../../outside` and `x -> /etc/passwd` do not.
fn link_stays_inside(at: &Path, target: &Path) -> bool {
    let mut depth = at.components().count() as i64 - 1; // the folder the link sits in
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => return false, // from the root, or a Windows drive
        }
    }
    true
}

/// Counts what comes out, and stops at the ceiling.
struct Budget {
    bytes: u64,
    entries: usize,
}
impl Budget {
    fn take(&mut self, archive: &Path, bytes: u64) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries += 1;
        if self.bytes > MOST_BYTES || self.entries > MOST_ENTRIES {
            return Err(refuse(archive, "it holds far more than a runtime does"));
        }
        Ok(())
    }
}

/// Unpacks `archive` (`.tar.gz` or `.zip`) into `into`, which is made if missing. Returns the bytes written.
pub(super) fn unpack(archive: &Path, into: &Path, stop: &AtomicBool) -> Result<u64> {
    std::fs::create_dir_all(into)?;
    let name = archive.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    let mut budget = Budget { bytes: 0, entries: 0 };
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        tar_gz(archive, into, stop, &mut budget)?;
    } else if name.ends_with(".zip") {
        zip(archive, into, stop, &mut budget)?;
    } else {
        return Err(refuse(archive, "Demido unpacks .tar.gz and .zip, and this is neither"));
    }
    Ok(budget.bytes)
}

fn check_stop(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Relaxed) { Err(Error::Stopped("Stopped while unpacking. Nothing was installed.".into())) } else { Ok(()) }
}

fn tar_gz(archive: &Path, into: &Path, stop: &AtomicBool, budget: &mut Budget) -> Result<()> {
    use tar::EntryType as T;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(File::open(archive)?)));
    tar.set_preserve_permissions(true); // the "may be run" mark on the programs
    tar.set_overwrite(true);
    for entry in tar.entries().map_err(|e| refuse(archive, e))? {
        check_stop(stop)?;
        let mut entry = entry.map_err(|e| refuse(archive, format_args!("it is damaged ({e})")))?;
        let stored = entry.path().map_err(|e| refuse(archive, e))?.into_owned();
        let kind = entry.header().entry_type();
        // Notes that tar keeps for itself (long names and the like) are handled by the reader.
        if matches!(kind, T::XHeader | T::XGlobalHeader | T::GNULongName | T::GNULongLink) {
            continue;
        }
        let Some(path) = inside(&stored) else {
            return Err(refuse(archive, format_args!("it holds \"{}\", a name that leads outside the folder", stored.display())));
        };
        match kind {
            T::Regular | T::Continuous | T::Directory | T::GNUSparse => {}
            T::Symlink => {
                let target = entry.link_name().map_err(|e| refuse(archive, e))?.unwrap_or_default().into_owned();
                if !link_stays_inside(&path, &target) {
                    return Err(refuse(archive, format_args!("its link \"{}\" points outside the folder, to \"{}\"", path.display(), target.display())));
                }
            }
            T::Link => {
                let target = entry.link_name().map_err(|e| refuse(archive, e))?.unwrap_or_default().into_owned();
                if inside(&target).is_none() {
                    return Err(refuse(archive, format_args!("its link \"{}\" points outside the folder", path.display())));
                }
            }
            // Devices, pipes and anything unknown have no business in a runtime.
            _ => return Err(refuse(archive, format_args!("it holds \"{}\", which is neither a file, a folder nor a link", path.display()))),
        }
        budget.take(archive, entry.header().size().unwrap_or(0))?;
        // `unpack_in` checks once more, against the folder as it is on disk, and says false when it will not write.
        if !entry.unpack_in(into).map_err(|e| refuse(archive, format_args!("\"{}\" could not be written ({})", path.display(), crate::runtime::catalog::root_cause(&e))))? {
            return Err(refuse(archive, format_args!("\"{}\" would have been written outside the folder", path.display())));
        }
    }
    Ok(())
}

fn zip(archive: &Path, into: &Path, stop: &AtomicBool, budget: &mut Budget) -> Result<()> {
    let mut zip = ::zip::ZipArchive::new(BufReader::new(File::open(archive)?)).map_err(|e| refuse(archive, format_args!("it is damaged ({e})")))?;
    for i in 0..zip.len() {
        check_stop(stop)?;
        let mut entry = zip.by_index(i).map_err(|e| refuse(archive, format_args!("it is damaged ({e})")))?;
        let stored = entry.name().map(|n| n.into_owned()).unwrap_or_else(|_| "(a name that cannot be read)".into());
        let Some(path) = entry.enclosed_name().as_deref().and_then(inside) else {
            return Err(refuse(archive, format_args!("it holds \"{stored}\", a name that leads outside the folder")));
        };
        if entry.is_symlink() {
            return Err(refuse(archive, format_args!("it holds a link, \"{}\", and a .zip for Windows has none", path.display())));
        }
        let to = into.join(&path);
        if entry.is_dir() {
            budget.take(archive, 0)?;
            std::fs::create_dir_all(&to)?;
            continue;
        }
        budget.take(archive, entry.size())?;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mode = entry.unix_mode();
        let mut out = File::create(&to)?;
        // The size an entry declares is a claim. Never write more than it claimed (which the budget counted).
        let declared = entry.size();
        let written = io::copy(&mut (&mut entry).take(declared), &mut out)?;
        if written != declared || entry.read(&mut [0u8; 1])? != 0 {
            return Err(refuse(archive, format_args!("\"{}\" is not the size it says it is", path.display())));
        }
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode & 0o755))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::Write;

    pub(in crate::runtime) fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("demido-unpack-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One entry of a test archive: its name, its kind, its bytes, and where it points if it is a link.
    pub(in crate::runtime) type Entry<'a> = (&'a str, tar::EntryType, &'a [u8], Option<&'a str>);

    /// Builds a .tar.gz from raw headers, so that names a careful tool would refuse to write can be tested.
    pub(in crate::runtime) fn tar_with(to: &Path, entries: &[Entry]) {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(File::create(to).unwrap(), flate2::Compression::fast()));
        for (name, kind, body, link) in entries {
            let mut h = tar::Header::new_gnu();
            // Written straight into the header: `set_path` would refuse the names this test needs.
            let raw = h.as_old_mut();
            raw.name[..name.len()].copy_from_slice(name.as_bytes());
            if let Some(link) = link {
                raw.linkname[..link.len()].copy_from_slice(link.as_bytes());
            }
            h.set_entry_type(*kind);
            h.set_size(body.len() as u64);
            h.set_mode(if name.ends_with("llama-server") || *kind == tar::EntryType::Directory { 0o755 } else { 0o644 });
            h.set_cksum();
            b.append(&h, *body).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }

    static NO_STOP: AtomicBool = AtomicBool::new(false);

    #[test]
    fn an_ordinary_archive_comes_out_whole() {
        use tar::EntryType as T;
        let dir = temp("ok");
        let archive = dir.join("runtime.tar.gz");
        tar_with(
            &archive,
            &[
                ("build/bin/", T::Directory, b"", None),
                ("build/bin/llama-server", T::Regular, b"#!/bin/sh\n", None),
                ("build/bin/libllama.so.0", T::Regular, b"lib", None),
                ("build/bin/libllama.so", T::Symlink, b"", Some("libllama.so.0")),
            ],
        );
        let out = dir.join("out");
        assert_eq!(unpack(&archive, &out, &NO_STOP).unwrap(), 13);
        assert_eq!(std::fs::read(out.join("build/bin/libllama.so")).unwrap(), b"lib");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(out.join("build/bin/llama-server")).unwrap().permissions().mode() & 0o111, 0o111, "the program may be run");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_and_links_that_lead_outside_are_refused() {
        use tar::EntryType as T;
        let cases: [(&str, &[Entry]); 6] = [
            ("climbs", &[("../escaped.txt", T::Regular, b"x", None)]),
            ("climbs-later", &[("a/../../escaped.txt", T::Regular, b"x", None)]),
            ("from-root", &[("/tmp/demido-escaped.txt", T::Regular, b"x", None)]),
            ("link-out", &[("lib/evil", T::Symlink, b"", Some("../../outside"))]),
            ("link-root", &[("evil", T::Symlink, b"", Some("/etc/passwd"))]),
            ("device", &[("dev-null", T::Char, b"", None)]),
        ];
        for (name, entries) in cases {
            let dir = temp(name);
            let archive = dir.join("bad.tar.gz");
            tar_with(&archive, entries);
            let out = dir.join("inner").join("out");
            let err = unpack(&archive, &out, &NO_STOP).unwrap_err().to_string();
            assert!(err.contains("was not unpacked"), "{name}: {err}");
            assert!(!dir.join("escaped.txt").exists() && !dir.join("inner/escaped.txt").exists() && !dir.join("outside").exists(), "{name} wrote outside its folder");
            std::fs::remove_dir_all(dir).ok();
        }
    }

    #[test]
    fn a_zip_comes_out_and_a_climbing_zip_does_not() {
        let dir = temp("zip");
        let good = dir.join("runtime.zip");
        let mut z = ::zip::ZipWriter::new(File::create(&good).unwrap());
        let opts = ::zip::write::SimpleFileOptions::default();
        z.add_directory("bin/", opts).unwrap();
        z.start_file("bin/llama-server.exe", opts).unwrap();
        z.write_all(b"MZ-not-really").unwrap();
        z.finish().unwrap();
        let out = dir.join("out");
        assert_eq!(unpack(&good, &out, &NO_STOP).unwrap(), 13);
        assert_eq!(std::fs::read(out.join("bin/llama-server.exe")).unwrap(), b"MZ-not-really");

        let bad = dir.join("bad.zip");
        let mut z = ::zip::ZipWriter::new(File::create(&bad).unwrap());
        z.start_file("../escaped.txt", opts).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        assert!(unpack(&bad, &dir.join("inner/out"), &NO_STOP).is_err());
        assert!(!dir.join("escaped.txt").exists() && !dir.join("inner/escaped.txt").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn links_are_judged_by_where_they_end() {
        let p = Path::new;
        assert!(link_stays_inside(p("lib/libllama.so"), p("libllama.so.0")));
        assert!(link_stays_inside(p("a/b/c"), p("../../d")));
        assert!(!link_stays_inside(p("a/b/c"), p("../../../d")));
        assert!(!link_stays_inside(p("x"), p("/etc/passwd")));
        assert!(!link_stays_inside(p("x"), p("..")));
        assert_eq!(inside(p("./a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(inside(p("a/../b")), None);
        assert_eq!(inside(p("")), None);
    }
}
