//! Downloads and archives.
//!
//! [`Downloader::download`] streams a URL into `<dest>.part`, resumes from an existing partial
//! file with an HTTP range request, retries transient failures, verifies size and SHA-256, and
//! only then renames the file into place. A cancelled download keeps its `.part` file so the
//! next attempt continues where it stopped.

mod archive;
mod download;

pub use archive::{extract, find_file};
pub use download::{DownloadRequest, Downloader, FetchError, Progress, sha256_file};
pub use tokio_util::sync::CancellationToken;
