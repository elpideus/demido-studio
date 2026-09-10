//! The fetcher. No trait to mock HTTP, no seam: a fetch failure partway
//! through a real archive on a real disk is the only interesting behaviour
//! this has, and a mock of it would test the mock.
//!
//! **Resume is the partial file on disk, and nothing else.** There is no
//! separate marker: `<name>.part` at less than the full size *is* "a fetch
//! was interrupted here", and its absence *is* "start fresh". Cancelling
//! (`Cancel::cancel`) stops the loop and leaves the `.part` file exactly
//! where it was, which is what makes the next call a resume rather than a
//! reinstall (`docs/rules/runtimes.md` acceptance: "a failed fetch is not a
//! reinstall").

use std::path::{Path, PathBuf};

use tokio::io::{AsyncSeekExt, AsyncWriteExt};

pub type Cancel = tokio_util::sync::CancellationToken;

/// The minimum an item needs to be fetched. Not `demido_catalog::Archive`
/// itself, so a model row a user pointed a URL at can be fetched by the same
/// function without depending on the pinned manifest
/// (`docs/rules/runtimes.md`: the required group is data, not a second code
/// path, and that has to hold here too).
pub trait Fetchable {
    fn name(&self) -> &str;
    fn url(&self) -> String;
}

impl Fetchable for demido_catalog::Archive {
    fn name(&self) -> &str {
        self.name
    }

    fn url(&self) -> String {
        demido_catalog::Archive::url(self)
    }
}

/// A reference is fetchable wherever the thing is, because the caller that
/// matters holds references: `demido_catalog::Selection::archives` yields
/// `&Archive` out of the manifest, and a row is fetched from exactly that.
impl<T: Fetchable + ?Sized> Fetchable for &T {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn url(&self) -> String {
        (**self).url()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub bytes: u64,
    pub total: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// Names which archive failed, so the caller can offer a retry of that
    /// row alone rather than the whole selection.
    #[error("fetching {archive} failed: {detail}")]
    Failed { archive: String, detail: String },

    #[error("fetching {archive} was cancelled")]
    Cancelled { archive: String },
}

impl FetchError {
    pub fn archive(&self) -> &str {
        match self {
            FetchError::Failed { archive, .. } | FetchError::Cancelled { archive } => archive,
        }
    }
}

/// Fetch one archive into `dest_dir`, resuming a `.part` file already there.
///
/// Returns the path to the finished, whole file. The `.part` file is removed
/// only once every byte has arrived; a process killed mid write leaves it
/// exactly as far as it got.
pub async fn fetch(
    item: &impl Fetchable,
    dest_dir: &Path,
    client: &reqwest::Client,
    mut on_progress: impl FnMut(Progress),
    cancel: &Cancel,
) -> Result<PathBuf, FetchError> {
    let name = item.name().to_owned();
    let failed = |detail: String| FetchError::Failed {
        archive: name.clone(),
        detail,
    };

    tokio::fs::create_dir_all(dest_dir)
        .await
        .map_err(|e| failed(format!("creating {}: {e}", dest_dir.display())))?;

    let finished = dest_dir.join(&name);
    if finished.exists() {
        return Ok(finished);
    }

    let part = dest_dir.join(format!("{name}.part"));
    let already = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut request = client.get(item.url());
    if already > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={already}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| failed(format!("requesting {}: {e}", item.url())))?;

    let status = response.status();
    if !status.is_success() {
        return Err(failed(format!("server answered {status}")));
    }

    // A pin is a permanent upstream URL (`docs/rules/runtimes.md` section 3),
    // so a 200 in place of the 206 we asked for means "this server does not
    // support ranges", never "the content changed underneath us". Restart
    // rather than append to a file whose offset the response does not agree
    // with.
    let resumed = already > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut downloaded = if resumed { already } else { 0 };

    let total = response
        .content_length()
        .map(|len| downloaded + len)
        .unwrap_or(downloaded);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resumed)
        .open(&part)
        .await
        .map_err(|e| failed(format!("opening {}: {e}", part.display())))?;
    if resumed {
        file.seek(std::io::SeekFrom::End(0))
            .await
            .map_err(|e| failed(format!("seeking {}: {e}", part.display())))?;
    }

    on_progress(Progress {
        bytes: downloaded,
        total,
    });

    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() {
            return Err(FetchError::Cancelled { archive: name });
        }
        let chunk = chunk.map_err(|e| failed(format!("reading the response: {e}")))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| failed(format!("writing {}: {e}", part.display())))?;
        downloaded += chunk.len() as u64;
        on_progress(Progress {
            bytes: downloaded,
            total,
        });
    }
    file.flush()
        .await
        .map_err(|e| failed(format!("flushing {}: {e}", part.display())))?;
    drop(file);

    // A stream that ends early and cleanly must not be renamed into place: the
    // next call would see a finished archive, skip the download, and hand a
    // truncated zip to `unpack`. The `.part` file is left exactly as far as it
    // got, which is what makes the retry a resume.
    if total > 0 && downloaded < total {
        return Err(failed(format!(
            "the response ended at {downloaded} of {total} bytes"
        )));
    }

    tokio::fs::rename(&part, &finished)
        .await
        .map_err(|e| failed(format!("renaming {}: {e}", part.display())))?;

    Ok(finished)
}
