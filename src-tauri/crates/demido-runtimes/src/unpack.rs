//! Unzip a fetched archive into a row's managed directory.
//!
//! `llama-server.exe` resolves `cublasLt64_13.dll` next to itself, so a build
//! archive and its `cudart` companion are expanded into the **same** folder:
//! one deployable unit on disk, even though they are two rows in the manifest
//! and two rows in the ledger. That is also why `docs/rules/runtimes.md`
//! verifies both with one command.
//!
//! The zip is deleted once expanded. Only the expanded tree counts toward
//! `on_disk_mib`: keeping the archive after unpacking would double what the
//! ledger claims a row spent.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("opening the archive at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the archive is not a valid zip: {0}")]
    BadArchive(#[from] zip::result::ZipError),
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Expand `archive` into `dest_dir`, merging into whatever is already there.
/// Does not delete `archive`: the caller does that once every archive in the
/// selection has unpacked, so a failure partway through a multi archive row
/// never destroys evidence of what unpacked and what did not.
pub fn unpack(archive: &Path, dest_dir: &Path) -> Result<(), UnpackError> {
    let file = File::open(archive).map_err(|source| UnpackError::Open {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file)?;

    std::fs::create_dir_all(dest_dir).map_err(|source| UnpackError::Write {
        path: dest_dir.to_path_buf(),
        source,
    })?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|source| UnpackError::Write {
                path: out_path.clone(),
                source,
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| UnpackError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut out = File::create(&out_path).map_err(|source| UnpackError::Write {
            path: out_path.clone(),
            source,
        })?;
        // Streamed rather than read into a `Vec` first: `cudart` is 489 MiB on
        // disk in a handful of DLLs, and buffering one whole would be a
        // hundreds of megabytes allocation to copy a file.
        io::copy(&mut entry, &mut out).map_err(|source| UnpackError::Write {
            path: out_path,
            source,
        })?;
    }

    Ok(())
}

/// The size of everything under `dir`, in MiB. What the ledger's
/// `on_disk_mib` is measured from, rather than trusted from the manifest.
pub fn directory_size_mib(dir: &Path) -> io::Result<f64> {
    fn walk(dir: &Path, total: &mut u64) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                walk(&entry.path(), total)?;
            } else {
                *total += metadata.len();
            }
        }
        Ok(())
    }
    let mut total = 0u64;
    walk(dir, &mut total)?;
    Ok(total as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use std::io::Write as _;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-runtimes-unpack-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).expect("created");
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        for (name, contents) in files {
            writer.start_file(*name, options).expect("started");
            writer.write_all(contents).expect("wrote");
        }
        writer.finish().expect("finished");
    }

    #[test]
    fn two_archives_merge_into_one_directory() {
        let dir = scratch("merge");
        let build_zip = dir.join("build.zip");
        let cudart_zip = dir.join("cudart.zip");
        make_zip(&build_zip, &[("llama-server.exe", b"binary")]);
        make_zip(&cudart_zip, &[("cudart64_13.dll", b"dll")]);

        let dest = dir.join("llama-cpp").join("b10816");
        unpack(&build_zip, &dest).expect("unpacked build");
        unpack(&cudart_zip, &dest).expect("unpacked cudart");

        assert!(dest.join("llama-server.exe").exists());
        assert!(dest.join("cudart64_13.dll").exists());
    }

    #[test]
    fn directory_size_counts_every_file_under_it() {
        let dir = scratch("size");
        std::fs::write(dir.join("a.bin"), vec![0u8; 1024 * 1024]).expect("wrote");
        std::fs::create_dir_all(dir.join("sub")).expect("made subdir");
        std::fs::write(dir.join("sub").join("b.bin"), vec![0u8; 512 * 1024]).expect("wrote");

        let mib = directory_size_mib(&dir).expect("measured");
        assert!((mib - 1.5).abs() < 0.01, "expected ~1.5 MiB, got {mib}");
    }
}
