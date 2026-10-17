use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::RateLimit;

const MAX_ATTEMPTS: u32 = 6;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);
/// Received bytes are written, paced and reported in pieces no larger than this. Xet delivers
/// chunks of many megabytes at once.
const WRITE_SLICE: usize = 4 << 20;

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
    Size { file: String, expected: u64, actual: u64 },
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
            FetchError::Status { status, .. } => status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS,
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
    /// Every byte is here and the file is being checked against its SHA-256.
    pub verifying: bool,
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
    /// A transfer rate this download shares with others.
    pub limit: Option<RateLimit>,
    /// Read the file through Xet when the downloader has it and the server stores the file
    /// there. Anything else, and any Xet failure, uses plain HTTP.
    pub xet: bool,
}

impl DownloadRequest {
    pub fn new(url: impl Into<String>, dest: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            dest: dest.into(),
            expected_size: None,
            sha256: None,
            bearer: None,
            limit: None,
            xet: false,
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

    pub fn limit(mut self, limit: RateLimit) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn xet(mut self, enabled: bool) -> Self {
        self.xet = enabled;
        self
    }

    fn part_path(&self) -> PathBuf {
        let mut name = self.dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".part");
        self.dest.with_file_name(name)
    }
}

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    #[cfg(feature = "xet")]
    user_agent: String,
    #[cfg(feature = "xet")]
    xet: Option<std::sync::Arc<crate::xet::Xet>>,
}

impl Downloader {
    pub fn new(user_agent: &str) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            // HTTP/2's default flow-control window holds a single stream to about 30 MB/s
            // however fast the line is; HTTP/1.1 reaches the full speed of a gigabit connection.
            .http1_only()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            #[cfg(feature = "xet")]
            user_agent: user_agent.to_string(),
            #[cfg(feature = "xet")]
            xet: None,
        })
    }

    /// Lets requests marked [`DownloadRequest::xet`] read Hugging Face files through Xet.
    /// `cache_dir` holds the engine's working files.
    #[cfg(feature = "xet")]
    pub fn with_xet(mut self, cache_dir: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let xet = crate::xet::Xet::new(&self.user_agent, cache_dir.into())?;
        self.xet = Some(std::sync::Arc::new(xet));
        Ok(self)
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
        if let Ok(meta) = tokio::fs::metadata(&req.dest).await
            && req.sha256.is_some()
            && req.expected_size.is_none_or(|size| size == meta.len())
        {
            on_progress(Progress {
                downloaded: meta.len(),
                total: Some(meta.len()),
                bytes_per_second: 0.0,
                verifying: true,
            });
        }
        if already_complete(req).await? {
            let size = tokio::fs::metadata(&req.dest).await?.len();
            on_progress(Progress {
                downloaded: size,
                total: Some(size),
                bytes_per_second: 0.0,
                verifying: false,
            });
            return Ok(());
        }

        let part = req.part_path();
        #[cfg_attr(not(feature = "xet"), allow(unused_mut))]
        let mut xet = req.xet;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.attempt(req, &part, xet, cancel, &mut on_progress).await {
                Ok(()) => break,
                #[cfg(feature = "xet")]
                Err(Stop::Xet(err)) => {
                    tracing::warn!(url = %req.url, "Xet transfer failed: {err:#}; continuing over HTTP");
                    xet = false;
                    attempt -= 1;
                }
                Err(Stop::Fetch(err)) if err.is_transient() && attempt < MAX_ATTEMPTS => {
                    let wait = Duration::from_millis(500 * 2u64.pow(attempt.min(5)));
                    tracing::warn!(url = %req.url, attempt, "download interrupted: {err}; retrying in {wait:?}");
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(FetchError::Cancelled),
                        _ = tokio::time::sleep(wait) => {}
                    }
                }
                Err(Stop::Fetch(err)) => return Err(err),
            }
        }

        let file = display_name(&req.dest);
        let size = tokio::fs::metadata(&part).await?.len();
        if let Some(expected) = req.expected_size
            && size != expected
        {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(FetchError::Size {
                file,
                expected,
                actual: size,
            });
        }
        if let Some(expected) = &req.sha256 {
            on_progress(Progress {
                downloaded: size,
                total: Some(size),
                bytes_per_second: 0.0,
                verifying: true,
            });
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
        xet: bool,
        cancel: &CancellationToken,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<(), Stop> {
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

        let (mut body, offset, total) = tokio::select! {
            _ = cancel.cancelled() => return Err(FetchError::Cancelled.into()),
            opened = self.open(req, part, offset, xet) => opened?,
        };
        let resumed = offset > 0;
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
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    file.flush().await?;
                    return Err(FetchError::Cancelled.into());
                }
                next = body.next() => next,
            };
            let mut chunk = match chunk {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(err) => {
                    file.flush().await?;
                    return Err(err);
                }
            };
            while !chunk.is_empty() {
                let slice = chunk.split_to(chunk.len().min(WRITE_SLICE));
                file.write_all(&slice).await?;
                downloaded += slice.len() as u64;
                if let Some(limit) = &req.limit {
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            file.flush().await?;
                            return Err(FetchError::Cancelled.into());
                        }
                        _ = limit.take(slice.len()) => {}
                    }
                }
                if last_emit.elapsed() >= PROGRESS_INTERVAL {
                    last_emit = Instant::now();
                    on_progress(Progress {
                        downloaded,
                        total,
                        bytes_per_second: rate.sample(downloaded),
                        verifying: false,
                    });
                }
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        on_progress(Progress {
            downloaded,
            total,
            bytes_per_second: rate.sample(downloaded),
            verifying: false,
        });
        Ok(())
    }

    /// Starts reading the file at `offset`. Returns the body, the offset it actually starts at
    /// (0 when the server ignored the range) and the total size when known.
    async fn open(
        &self,
        req: &DownloadRequest,
        part: &Path,
        offset: u64,
        xet: bool,
    ) -> Result<(Body, u64, Option<u64>), Stop> {
        #[cfg(feature = "xet")]
        if xet && let Some(engine) = &self.xet {
            match engine.open(&req.url, req.bearer.as_deref(), offset).await {
                Ok(Some(body)) => {
                    let total = req.expected_size.unwrap_or(body.size);
                    return Ok((Body::Xet(body), offset, Some(total)));
                }
                Ok(None) => {}
                Err(err) => return Err(Stop::Xet(err)),
            }
        }
        #[cfg(not(feature = "xet"))]
        let _ = xet;

        let mut builder = self.client.get(&req.url);
        if offset > 0 {
            builder = builder.header(RANGE, format!("bytes={offset}-"));
        }
        if let Some(token) = &req.bearer {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = builder.send().await?;

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
            return Err(FetchError::Restart.into());
        }
        if !status.is_success() {
            return Err(FetchError::Status {
                url: req.url.clone(),
                status,
            }
            .into());
        }
        let offset = if resumed { offset } else { 0 };
        let total = req
            .expected_size
            .or_else(|| response.content_length().map(|len| len + offset));
        Ok((Body::Http(response), offset, total))
    }
}

/// Why an attempt stopped short.
enum Stop {
    Fetch(FetchError),
    /// The Xet transfer failed; the rest of the file comes over HTTP.
    #[cfg(feature = "xet")]
    Xet(anyhow::Error),
}

impl From<FetchError> for Stop {
    fn from(err: FetchError) -> Self {
        Stop::Fetch(err)
    }
}

impl From<reqwest::Error> for Stop {
    fn from(err: reqwest::Error) -> Self {
        Stop::Fetch(err.into())
    }
}

impl From<std::io::Error> for Stop {
    fn from(err: std::io::Error) -> Self {
        Stop::Fetch(err.into())
    }
}

/// Where an attempt reads the file's bytes from.
enum Body {
    Http(reqwest::Response),
    #[cfg(feature = "xet")]
    Xet(crate::xet::XetBody),
}

impl Body {
    async fn next(&mut self) -> Result<Option<Bytes>, Stop> {
        match self {
            Body::Http(response) => Ok(response.chunk().await?),
            #[cfg(feature = "xet")]
            Body::Xet(body) => body.next().await.map_err(Stop::Xet),
        }
    }
}

async fn already_complete(req: &DownloadRequest) -> Result<bool, FetchError> {
    let Ok(meta) = tokio::fs::metadata(&req.dest).await else {
        return Ok(false);
    };
    if let Some(size) = req.expected_size
        && meta.len() != size
    {
        return Ok(false);
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

    /// Serves `body` over HTTP/1.1, honouring `Range: bytes=N-`, and records each range start.
    async fn serve(body: Vec<u8>) -> (String, std::sync::Arc<std::sync::Mutex<Vec<Option<usize>>>>) {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let body = std::sync::Arc::new(body);
        let starts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = starts.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = body.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = socket.read(&mut buf).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buf[..n]);
                    }
                    let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    let start = request
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                    seen.lock().unwrap().push(start);
                    let len = body.len();
                    let head = match start {
                        Some(s) => format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {s}-{}/{len}\r\n\r\n",
                            len - s,
                            len - 1
                        ),
                        None => format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n"),
                    };
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&body[start.unwrap_or(0)..]).await.unwrap();
                });
            }
        });
        (url, starts)
    }

    #[tokio::test]
    async fn a_partial_file_is_resumed_and_verified() {
        let body: Vec<u8> = (0..3_000_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let sha = hex::encode(Sha256::digest(&body));
        let (url, starts) = serve(body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.gguf");
        let req = DownloadRequest::new(url, &dest).size(body.len() as u64).sha256(sha);
        tokio::fs::write(req.part_path(), &body[..1_200_000]).await.unwrap();

        let dl = Downloader::new("test").unwrap();
        let mut last = None;
        dl.download(&req, &CancellationToken::new(), |p| last = Some(p))
            .await
            .unwrap();
        assert_eq!(*starts.lock().unwrap(), vec![Some(1_200_000)]);
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);
        assert!(!tokio::fs::try_exists(req.part_path()).await.unwrap());
        assert_eq!(last.unwrap().downloaded, body.len() as u64);
    }

    #[cfg(feature = "xet")]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "downloads 19 MB from huggingface.co"]
    async fn a_hub_file_downloads_and_resumes_through_xet() {
        const URL: &str = "https://huggingface.co/ggml-org/tiny-llamas/resolve/main/stories15M-q4_0.gguf";
        const SHA: &str = "6151b1929d7f5aa3385d9ddef3393e55587c0a55de661562322bc51dfda93a04";
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("stories.gguf");
        let dl = Downloader::new("test")
            .unwrap()
            .with_xet(dir.path().join("xet"))
            .unwrap();
        let req = DownloadRequest::new(URL, &dest).size(19_077_344).sha256(SHA).xet(true);
        dl.download(&req, &CancellationToken::new(), |_| {}).await.unwrap();

        let bytes = tokio::fs::read(&dest).await.unwrap();
        tokio::fs::write(req.part_path(), &bytes[..7_000_000]).await.unwrap();
        tokio::fs::remove_file(&dest).await.unwrap();
        dl.download(&req, &CancellationToken::new(), |_| {}).await.unwrap();
        assert_eq!(sha256_file(&dest).await.unwrap(), SHA);
    }
}
