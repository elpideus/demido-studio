//! One file, from a URL to a verified partial file beside its destination.
//!
//! The queue owns scheduling; this owns the transfer, which is where every
//! interesting failure lives: a connection reset half way, a proxy that
//! ignores `Range` and restarts the body, a server whose length is not the one
//! listed, a login page where a model should be, a disk that fills.
//!
//! Carried from v2's `demido-download/src/transfer.rs`, same author and license,
//! with three changes: the size the index listed is enforced rather than
//! trusted, a failure is a [`Failure`] with a cause rather than a sentence,
//! and a finished file is checked to be a GGUF as well as hashed.

use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::{Failure, Piece};

/// How often progress is reported. Every chunk would be thousands of events a
/// second on a fast link, each redrawing the same bar.
const REPORT_EVERY: Duration = Duration::from_millis(100);

/// How much arrives before it is written. `tokio::fs` is a blocking file
/// behind a thread pool, so writing every chunk as it landed spent most of a
/// transfer not reading the socket: measured in v2 against a real CDN, 21 MiB/s
/// out of an 87 MiB/s peak.
const WRITE_BUFFER: usize = 4 * 1024 * 1024;

/// Read size when hashing a finished file.
const HASH_CHUNK: usize = 1024 * 1024;

/// Why a transfer stopped short of a verified file.
#[derive(Debug)]
pub(crate) enum Stop {
    Failed(Failure),
    /// Somebody paused or cancelled. Which one is the queue's to know.
    Cancelled,
}

impl From<Failure> for Stop {
    fn from(failure: Failure) -> Self {
        Stop::Failed(failure)
    }
}

/// Bytes of this piece on disk: all of it if it is already at its
/// destination, otherwise however long its partial file is.
pub fn on_disk(piece: &Piece) -> u64 {
    if finished(piece) {
        return piece.bytes;
    }
    std::fs::metadata(piece.partial()).map_or(0, |meta| meta.len())
}

/// Already renamed into place by an earlier run, at the listed length.
fn finished(piece: &Piece) -> bool {
    !piece.partial().exists()
        && std::fs::metadata(&piece.destination).is_ok_and(|meta| meta.len() == piece.bytes)
}

/// What one transfer is given besides the piece.
#[derive(Clone, Copy)]
pub(crate) struct Run<'a> {
    pub client: &'a reqwest::Client,
    pub cancel: &'a CancellationToken,
    pub stall: Duration,
}

/// Called with a piece's bytes on disk as they grow.
pub(crate) type Report<'a> = &'a mut (dyn FnMut(u64) + Send);

/// Fetch `piece` into its partial file. Nothing is renamed into the library
/// here: the queue [`verify`]s each file, and an item's files are promoted
/// together, by [`promote`], once every one of them passed.
pub(crate) async fn fetch(piece: &Piece, run: Run<'_>, report: Report<'_>) -> Result<(), Stop> {
    if finished(piece) {
        report(piece.bytes);
        return Ok(());
    }
    if let Some(parent) = piece.destination.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(disk)?;
    }

    let partial = piece.partial();
    let mut from = tokio::fs::metadata(&partial)
        .await
        .map_or(0, |meta| meta.len());
    if from > piece.bytes {
        // Longer than the file it is part of: not a prefix of anything.
        remove(&partial).await;
        from = 0;
    }

    let mut restarted = false;
    while from < piece.bytes {
        match body(piece, from, run).await? {
            Body::Stream { response, from: at } => {
                from = write(piece, response, at, run, report).await?;
            }
            Body::Restart if !restarted => {
                tracing::warn!(url = %piece.url, from, "the host refused the resume point; starting over");
                remove(&partial).await;
                restarted = true;
                from = 0;
            }
            Body::Restart => return Err(Failure::Refused { status: 416 }.into()),
        }
    }

    report(piece.bytes);
    Ok(())
}

/// What the host agreed to send.
enum Body {
    /// A body to write at `from`: the resume point if the range was honoured,
    /// zero if the host sent the whole file anyway.
    Stream {
        response: reqwest::Response,
        from: u64,
    },
    /// The bytes on disk are not a prefix of what is served.
    Restart,
}

async fn body(piece: &Piece, from: u64, run: Run<'_>) -> Result<Body, Stop> {
    let mut request = run.client.get(&piece.url);
    if from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={from}-"));
    }
    let response = tokio::select! {
        biased;
        () = run.cancel.cancelled() => return Err(Stop::Cancelled),
        sent = tokio::time::timeout(run.stall, request.send()) => match sent {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(Failure::Unreachable { detail: chain(&error) }.into()),
            Err(_) => return Err(Failure::Stalled { seconds: run.stall.as_secs() }.into()),
        },
    };

    let status = response.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && from > 0 {
        return Ok(Body::Restart);
    }
    if let Some(failure) = refusal(status) {
        return Err(failure.into());
    }
    // A login page is a success with a body, and it is the one body that is
    // never a model. Refused before a byte of it is written.
    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.trim_start().starts_with("text/html"))
    {
        return Err(Failure::NotTheFile {
            content_type: content_type.to_owned(),
        }
        .into());
    }

    let length = response.content_length();
    let (at, stated) = if status == reqwest::StatusCode::PARTIAL_CONTENT && from > 0 {
        match content_range(&response) {
            // Honoured at the byte asked for.
            Some((start, total)) if start == from => (from, total.or(length.map(|l| from + l))),
            // A range, but not the one asked for: the bytes on disk and what
            // is coming do not line up.
            _ => return Ok(Body::Restart),
        }
    } else {
        // The whole file, whatever was asked. Appending it to what is on disk
        // would make a file of the right length and garbage inside, so it is
        // written from zero.
        (0, length)
    };

    if let Some(stated) = stated.filter(|stated| *stated != piece.bytes) {
        remove(&piece.partial()).await;
        return Err(Failure::WrongSize {
            stated,
            expected: piece.bytes,
        }
        .into());
    }
    Ok(Body::Stream { response, from: at })
}

/// A status that is a refusal, as the cause a person acts on.
fn refusal(status: reqwest::StatusCode) -> Option<Failure> {
    if status.is_success() {
        return None;
    }
    Some(match status.as_u16() {
        401 | 403 => Failure::Gated {
            status: status.as_u16(),
        },
        404 => Failure::Missing,
        429 => Failure::RateLimited,
        other => Failure::Refused { status: other },
    })
}

/// `Content-Range: bytes 20-99/100`, as the start and the total when stated.
fn content_range(response: &reqwest::Response) -> Option<(u64, Option<u64>)> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    let (range, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Write a body into the partial file starting at `from`, and hand back how
/// many bytes of the piece are on disk afterwards.
async fn write(
    piece: &Piece,
    response: reqwest::Response,
    from: u64,
    run: Run<'_>,
    report: Report<'_>,
) -> Result<u64, Stop> {
    let partial = piece.partial();
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(from > 0)
        .truncate(from == 0)
        .open(&partial)
        .await
        .map_err(disk)?;
    let mut file = tokio::io::BufWriter::with_capacity(WRITE_BUFFER, file);

    let mut received = from;
    let outcome = stream(&mut file, &mut received, piece, response, run, report).await;
    // Flushed on every way out, so the partial file's length is every byte
    // this transfer accepted: a pause, a reset and a stall all resume from it.
    let flushed = file.flush().await.map_err(disk);
    drop(file);

    if let Err(Stop::Failed(Failure::WrongSize { .. })) = &outcome {
        remove(&partial).await;
    }
    outcome?;
    flushed?;
    Ok(received)
}

async fn stream(
    file: &mut (impl tokio::io::AsyncWrite + Unpin),
    received: &mut u64,
    piece: &Piece,
    response: reqwest::Response,
    run: Run<'_>,
    report: Report<'_>,
) -> Result<(), Stop> {
    let mut body = response.bytes_stream();
    let mut last = Instant::now();
    report(*received);

    loop {
        let next = tokio::select! {
            biased;
            () = run.cancel.cancelled() => return Err(Stop::Cancelled),
            next = tokio::time::timeout(run.stall, body.next()) => next,
        };
        let chunk = match next {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(error))) => return Err(interrupted(&error, *received, piece).into()),
            Ok(None) => break,
            Err(_) => {
                return Err(Failure::Stalled {
                    seconds: run.stall.as_secs(),
                }
                .into())
            }
        };

        let after = received.saturating_add(chunk.len() as u64);
        if after > piece.bytes {
            return Err(Failure::WrongSize {
                stated: after,
                expected: piece.bytes,
            }
            .into());
        }
        file.write_all(&chunk).await.map_err(disk)?;
        *received = after;

        if last.elapsed() >= REPORT_EVERY {
            last = Instant::now();
            report(*received);
        }
    }

    if *received < piece.bytes {
        return Err(Failure::EndedEarly {
            received: *received,
            expected: piece.bytes,
        }
        .into());
    }
    Ok(())
}

/// A body that stopped with an error, as what happened to the connection.
fn interrupted(error: &reqwest::Error, received: u64, piece: &Piece) -> Failure {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) {
                return Failure::Reset { received };
            }
        }
        source = cause.source();
    }
    Failure::EndedEarly {
        received,
        expected: piece.bytes,
    }
}

/// Check the whole partial file: its digest where the index published one,
/// and that it is a GGUF as long as its own header says. A file that fails is
/// deleted, because every later resume would fail at the same place.
pub(crate) async fn verify(piece: &Piece, cancel: &CancellationToken) -> Result<(), Stop> {
    if finished(piece) {
        return Ok(());
    }
    let partial = piece.partial();
    if let Some(expected) = &piece.sha256 {
        let actual = sha256(&partial, cancel).await?;
        if !actual.eq_ignore_ascii_case(expected) {
            remove(&partial).await;
            return Err(Failure::Corrupt.into());
        }
    }
    let checked = partial.clone();
    let header = tokio::task::spawn_blocking(move || demido_models::gguf::verify(&checked))
        .await
        .map_err(|error| Failure::Disk {
            detail: error.to_string(),
        })?;
    if let Err(damage) = header {
        remove(&partial).await;
        return Err(Failure::Damaged { damage }.into());
    }
    Ok(())
}

/// Hashed by reading the finished file back, never as bytes arrive: a resumed
/// transfer never sees its earlier bytes, so a running digest would be right
/// on a clean run and wrong on exactly the runs a digest is for.
async fn sha256(path: &Path, cancel: &CancellationToken) -> Result<String, Stop> {
    let mut file = tokio::fs::File::open(path).await.map_err(disk)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        let read = file.read(&mut buffer).await.map_err(disk)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Rename a verified piece into place. The moment it becomes part of the
/// library, and atomic, so a scan never sees half of it.
pub(crate) async fn promote(piece: &Piece) -> Result<(), Failure> {
    if finished(piece) {
        return Ok(());
    }
    if tokio::fs::metadata(&piece.destination).await.is_ok() {
        tokio::fs::remove_file(&piece.destination)
            .await
            .map_err(disk_failure)?;
    }
    tokio::fs::rename(piece.partial(), &piece.destination)
        .await
        .map_err(disk_failure)
}

/// Delete a partial file, if there is one.
pub(crate) async fn remove(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "a partial download could not be deleted");
        }
    }
}

fn disk(error: std::io::Error) -> Stop {
    Stop::Failed(disk_failure(error))
}

/// A write that failed, as a full disk or as the disk's own words.
fn disk_failure(error: std::io::Error) -> Failure {
    // `ERROR_HANDLE_DISK_FULL` and `ERROR_DISK_FULL`, which is what Windows
    // answers a write with and what `StorageFull` is built from.
    const FULL: [i32; 2] = [39, 112];
    if error.kind() == std::io::ErrorKind::StorageFull
        || error
            .raw_os_error()
            .is_some_and(|code| FULL.contains(&code))
    {
        return Failure::DiskFull;
    }
    Failure::Disk {
        detail: error.to_string(),
    }
}

/// Every layer of a transport error, joined. `reqwest`'s own sentence names
/// the URL and never the reason, which is always a hop or two down.
fn chain(error: &reqwest::Error) -> String {
    let mut detail = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_disk_is_named_as_one() {
        assert_eq!(
            disk_failure(std::io::Error::from_raw_os_error(112)),
            Failure::DiskFull
        );
        assert!(matches!(
            disk_failure(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            Failure::Disk { .. }
        ));
    }

    #[test]
    fn refusals_are_told_apart() {
        use reqwest::StatusCode;
        assert_eq!(
            refusal(StatusCode::UNAUTHORIZED),
            Some(Failure::Gated { status: 401 })
        );
        assert_eq!(refusal(StatusCode::NOT_FOUND), Some(Failure::Missing));
        assert_eq!(
            refusal(StatusCode::TOO_MANY_REQUESTS),
            Some(Failure::RateLimited)
        );
        assert_eq!(refusal(StatusCode::OK), None);
    }
}
