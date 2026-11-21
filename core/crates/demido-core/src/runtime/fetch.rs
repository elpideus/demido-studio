//! Downloading one file: it can stop and go on, and it is checked before it is trusted.
//!
//! The bytes go to `<name>.part`. If that file is already there (an earlier try that was stopped, or a
//! connection that broke), the download goes on from where it ended. Only a complete file whose SHA-256 is
//! the one its publisher listed gets its real name.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::{Client, StatusCode, header};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::RemoteFile;
use super::catalog::root_cause;
use crate::{Error, Result};

fn part_of(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    to.with_file_name(name)
}

/// The SHA-256 of a file, as 64 small hex letters. Reads it in pieces: these files are hundreds of megabytes.
pub(super) fn sha256_of(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut piece = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut piece)?;
        if n == 0 {
            break;
        }
        hash.update(&piece[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads `file` to `to`. `progress` hears how many bytes are there, often.
/// Returns whether the file could be checked against a published SHA-256.
pub(crate) async fn download(http: &Client, file: &RemoteFile, to: &Path, stop: &AtomicBool, mut progress: impl FnMut(u64)) -> Result<bool> {
    let part = part_of(to);
    let stopped = || Error::Stopped(format!("Stopped. What was downloaded of {} is kept, so the next try goes on from there.", file.name));
    // Twice at most: the second time is a fresh start, when what was kept turns out to be unusable.
    for fresh in [false, true] {
        if fresh {
            tokio::fs::remove_file(&part).await.ok();
        }
        let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
        if file.size > 0 && have > file.size {
            continue; // more than the whole file: not ours
        }
        if !(file.size > 0 && have == file.size) {
            let mut ask = http.get(&file.url);
            if have > 0 {
                ask = ask.header(header::RANGE, format!("bytes={have}-"));
            }
            let mut res =
                ask.send().await.map_err(|e| Error::Net(format!("Could not download {}. Check the connection and try again; it goes on from where it stopped. ({})", file.name, root_cause(&e))))?;
            let going_on = match res.status() {
                StatusCode::PARTIAL_CONTENT => true,
                StatusCode::OK => false, // the server sent the whole file again
                StatusCode::RANGE_NOT_SATISFIABLE if !fresh => continue,
                other => return Err(Error::Net(format!("Could not download {}: the server answered {other}.", file.name))),
            };
            let mut out = tokio::fs::OpenOptions::new().create(true).write(true).append(going_on).truncate(!going_on).open(&part).await?;
            let mut done = if going_on { have } else { 0 };
            progress(done);
            loop {
                if stop.load(Ordering::Relaxed) {
                    out.flush().await.ok();
                    return Err(stopped());
                }
                match res.chunk().await {
                    Ok(Some(bytes)) => {
                        out.write_all(&bytes).await?;
                        done += bytes.len() as u64;
                        progress(done);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        out.flush().await.ok();
                        return Err(Error::Net(format!(
                            "The download of {} broke off after {} of {} MB. Try again; it goes on from there. ({})",
                            file.name,
                            done >> 20,
                            file.size >> 20,
                            root_cause(&e)
                        )));
                    }
                }
            }
            out.flush().await?;
        }
        // Complete, as far as the connection is concerned. Now check it is the file that was published.
        let (size, hash) = {
            let part = part.clone();
            tokio::task::spawn_blocking(move || -> Result<(u64, String)> { Ok((std::fs::metadata(&part)?.len(), sha256_of(&part)?)) }).await.map_err(|e| Error::Other(e.to_string()))??
        };
        let size_fits = file.size == 0 || size == file.size;
        let hash_fits = file.sha256.as_ref().is_none_or(|want| want.eq_ignore_ascii_case(&hash));
        if size_fits && hash_fits {
            tokio::fs::rename(&part, to).await?;
            return Ok(file.sha256.is_some());
        }
        if fresh {
            tokio::fs::remove_file(&part).await.ok();
            return Err(Error::Invalid(format!(
                "{} is not the file that was published: its checksum does not match, twice in a row. It was deleted and nothing was installed. If this keeps happening, something between this computer and the publisher is changing the file.",
                file.name
            )));
        }
        // What was kept from an earlier try may have been damaged. Start over once before giving up.
    }
    unreachable!("the second pass returns")
}
