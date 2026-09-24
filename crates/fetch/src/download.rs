use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const MAX_ATTEMPTS: u32 = 6;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("download cancelled")]
    Cancelled,
    #[error("{url} answered {status}")]
    Status { url: String, status: StatusCode },
    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("size mismatch for {file}: expected {expected} bytes, got {actual}")]
    Size {
        file: String,
        expected: u64,
        actual: u64,
    },
    #[error("the server rejected the resume point; starting over")]
    Restart,
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl FetchError {
    /// Whether retrying the same request could succeed.
    fn is_transient(&self) -> bool {
        match self {
            FetchError::Http(e) => e.is_timeout() || e.is_connect() || e.is_body() || e.is_request(),
            FetchError::Status { status, .. } => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            FetchError::Restart => true,
            _ => false,
        }
    }
}

/// Where a download stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Smoothed transfer rate.
    pub bytes_per_second: f64,
}

impl Progress {
    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.downloaded as f64 / t as f64).clamp(0.0, 1.0))
    }
}

#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub url: String,
    pub dest: PathBuf,
    pub expected_size: Option<u64>,
    /// Lowercase hex SHA-256 of the complete file.
    pub sha256: Option<String>,
    /// Bearer token (Hugging Face gated repositories).
    pub bearer: Option<String>,
}

impl DownloadRequest {
    pub fn new(url: impl Into<String>, dest: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            dest: dest.into(),
            expected_size: None,
            sha256: None,
            bearer: None,
        }
    }

    pub fn size(mut self, size: u64) -> Self {
        self.expected_size = Some(size);
        self
    }

    pub fn sha256(mut self, sha: impl Into<String>) -> Self {
        self.sha256 = Some(sha.into().to_ascii_lowercase());
        self
    }

    pub fn bearer(mut self, token: Option<String>) -> Self {
        self.bearer = token.filter(|t| !t.is_empty());
        self
    }

    fn part_path(&self) -> PathBuf {
        let mut name = self
            .dest
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".part");
        self.dest.with_file_name(name)
    }
}

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
}

impl Downloader {
    pub fn new(user_agent: &str) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Downloads `req.url` to `req.dest`. Returns immediately when the destination already exists
    /// with the expected checksum (or size, when no checksum is known).
    pub async fn download(
        &self,
        req: &DownloadRequest,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<(), FetchError> {
        if let Some(parent) = req.dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if already_complete(req).await? {
            let size = tokio::fs::metadata(&req.dest).await?.len();
            on_progress(Progress {
                downloaded: size,
                total: Some(size),
                bytes_per_second: 0.0,
            });
            return Ok(());
        }

        let part = req.part_path();
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.attempt(req, &part, cancel, &mut on_progress).await {
                Ok(()) => break,
                Err(err) if err.is_transient() && attempt < MAX_ATTEMPTS => {
                    let wait = Duration::from_millis(500 * 2u64.pow(attempt.min(5)));
                    tracing::warn!(url = %req.url, attempt, "download interrupted: {err}; retrying in {wait:?}");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(FetchError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                }
                Err(err) => return Err(err),
            }
        }

        let file = display_name(&req.dest);
        let size = tokio::fs::metadata(&part).await?.len();
        if let Some(expected) = req.expected_size {
            if size != expected {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(FetchError::Size {
                    file,
                    expected,
                    actual: size,
                });
            }
        }
        if let Some(expected) = &req.sha256 {
            let actual = sha256_file(&part).await?;
            if &actual != expected {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(FetchError::Checksum {
                    file,
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        if tokio::fs::try_exists(&req.dest).await? {
            tokio::fs::remove_file(&req.dest).await?;
        }
        tokio::fs::rename(&part, &req.dest).await?;
        Ok(())
    }

    async fn attempt(
        &self,
        req: &DownloadRequest,
        part: &Path,
        cancel: &CancellationToken,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<(), FetchError> {
        let mut offset = match tokio::fs::metadata(part).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        if let Some(expected) = req.expected_size {
            if offset > expected {
                offset = 0;
            } else if offset == expected {
                return Ok(());
            }
        }

        let mut builder = self.client.get(&req.url);
        if offset > 0 {
            builder = builder.header(RANGE, format!("bytes={offset}-"));
        }
        if let Some(token) = &req.bearer {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(FetchError::Cancelled),
            r = builder.send() => r?,
        };

        let status = response.status();
        let resumed = status == StatusCode::PARTIAL_CONTENT
            && response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with(&format!("bytes {offset}-")));
        if status == StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
            // The partial file is stale or already complete; start over.
            tokio::fs::remove_file(part).await.ok();
            return Err(FetchError::Restart);
        }
        if !status.is_success() {
            return Err(FetchError::Status {
                url: req.url.clone(),
                status,
            });
        }
        if !resumed {
            offset = 0;
        }

        let total = req
            .expected_size
            .or_else(|| response.content_length().map(|len| len + offset));
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part)
            .await?;

        let mut downloaded = offset;
        let mut rate = RateMeter::new(downloaded);
        let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
        let mut stream = response.bytes_stream();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    file.flush().await?;
                    return Err(FetchError::Cancelled);
                }
                next = stream.next() => next,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed() >= PROGRESS_INTERVAL {
                last_emit = Instant::now();
                on_progress(Progress {
                    downloaded,
                    total,
                    bytes_per_second: rate.sample(downloaded),
                });
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        on_progress(Progress {
            downloaded,
            total,
            bytes_per_second: rate.sample(downloaded),
        });
        Ok(())
    }
}

async fn already_complete(req: &DownloadRequest) -> Result<bool, FetchError> {
    let Ok(meta) = tokio::fs::metadata(&req.dest).await else {
        return Ok(false);
    };
    if let Some(size) = req.expected_size {
        if meta.len() != size {
            return Ok(false);
        }
    }
    match &req.sha256 {
        Some(expected) => Ok(&sha256_file(&req.dest).await? == expected),
        None => Ok(req.expected_size.is_some()),
    }
}

/// Lowercase hex SHA-256 of a file, read in 1 MiB blocks.
pub async fn sha256_file(path: &Path) -> Result<String, FetchError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Exponentially smoothed bytes per second.
struct RateMeter {
    last_bytes: u64,
    last_time: Instant,
    rate: f64,
}

impl RateMeter {
    fn new(start: u64) -> Self {
        Self {
            last_bytes: start,
            last_time: Instant::now(),
            rate: 0.0,
        }
    }

    fn sample(&mut self, bytes: u64) -> f64 {
        let dt = self.last_time.elapsed().as_secs_f64();
        if dt >= 0.25 {
            let instant = (bytes.saturating_sub(self.last_bytes)) as f64 / dt;
            self.rate = if self.rate == 0.0 {
                instant
            } else {
                self.rate * 0.7 + instant * 0.3
            };
            self.last_bytes = bytes;
            self.last_time = Instant::now();
        }
        self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_keeps_the_extension_visible() {
        let req = DownloadRequest::new("http://x", "/tmp/model.gguf");
        assert_eq!(req.part_path(), PathBuf::from("/tmp/model.gguf.part"));
    }

    #[tokio::test]
    async fn an_existing_verified_file_is_not_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.bin");
        tokio::fs::write(&dest, b"hello").await.unwrap();
        let sha = sha256_file(&dest).await.unwrap();
        let req = DownloadRequest::new("http://127.0.0.1:9/never", &dest)
            .size(5)
            .sha256(sha);
        let dl = Downloader::new("test").unwrap();
        let mut seen = None;
        dl.download(&req, &CancellationToken::new(), |p| seen = Some(p))
            .await
            .unwrap();
        assert_eq!(seen.unwrap().downloaded, 5);
    }
}
