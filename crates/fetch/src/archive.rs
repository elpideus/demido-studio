use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// Extracts a `.zip`, `.tar.gz` or `.tgz` archive into `dest`.
///
/// When every entry sits under one top-level folder (as in `node-v24-win-x64/...`), that folder
/// is stripped so the contents land directly in `dest`. Entries that would escape `dest` are
/// rejected.
pub fn extract(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    std::fs::create_dir_all(dest)?;
    let staging = dest.join(".extracting");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    if name.ends_with(".zip") {
        extract_zip(archive, &staging)?;
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, &staging)?;
    } else {
        bail!("unsupported archive format: {name}");
    }

    let root = single_root(&staging)?.unwrap_or_else(|| staging.clone());
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if target.exists() {
            if target.is_dir() {
                std::fs::remove_dir_all(&target)?;
            } else {
                std::fs::remove_file(&target)?;
            }
        }
        std::fs::rename(entry.path(), &target)
            .with_context(|| format!("moving {} into place", target.display()))?;
    }
    std::fs::remove_dir_all(&staging)?;
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("archive entry {} escapes the target folder", entry.name());
        };
        let out = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = File::create(&out)?;
        std::io::copy(&mut entry, &mut writer)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    for entry in tar.entries()? {
        let mut entry = entry?;
        // `unpack_in` refuses paths that would leave `dest`.
        if !entry.unpack_in(dest)? {
            bail!("archive entry escapes the target folder");
        }
    }
    Ok(())
}

/// The only child of `dir`, when it is a directory.
fn single_root(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let children: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    if children.len() == 1 && children[0].file_type()?.is_dir() {
        Ok(Some(children[0].path()))
    } else {
        Ok(None)
    }
}

/// Finds a file by name anywhere under `root` (shallowest match first).
pub fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut queue = std::collections::VecDeque::from([root.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
                return Some(path);
            }
        }
        subdirs.sort();
        queue.extend(subdirs);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn strips_a_single_root_folder() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("node.zip");
        make_zip(
            &archive,
            &[("node-v1/node.exe", "bin"), ("node-v1/lib/x.js", "js")],
        );
        let dest = dir.path().join("out");
        extract(&archive, &dest).unwrap();
        assert!(dest.join("node.exe").is_file());
        assert!(dest.join("lib/x.js").is_file());
        assert!(!dest.join(".extracting").exists());
    }

    #[test]
    fn keeps_flat_archives_flat_and_finds_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("llama.zip");
        make_zip(
            &archive,
            &[("llama-server.exe", "a"), ("ggml.dll", "b"), ("sub/readme", "c")],
        );
        let dest = dir.path().join("out");
        extract(&archive, &dest).unwrap();
        assert!(dest.join("llama-server.exe").is_file());
        assert_eq!(
            find_file(&dest, "LLAMA-SERVER.EXE"),
            Some(dest.join("llama-server.exe"))
        );
        assert_eq!(find_file(&dest, "readme"), Some(dest.join("sub").join("readme")));
    }
}
