//! Downloads and archives.
//!
//! [`Downloader::download`] streams a URL into `<dest>.part`, resumes from an existing partial
//! file with an HTTP range request, retries transient failures, verifies size and SHA-256, and
//! only then renames the file into place. Downloads that share a [`RateLimit`] split it. A cancelled download keeps its `.part` file so the
//! next attempt continues where it stopped.
//!
//! With the `xet` feature, [`Downloader::with_xet`] reads Hugging Face files through Xet, the
//! Hub's chunked storage, instead; the `.part` file, resume and checks stay the same.

mod archive;
mod download;
mod limit;
#[cfg(feature = "xet")]
mod xet;

pub use archive::{extract, find_file};
pub use download::{DownloadRequest, Downloader, FetchError, Progress, sha256_file};
pub use limit::RateLimit;
pub use tokio_util::sync::CancellationToken;
