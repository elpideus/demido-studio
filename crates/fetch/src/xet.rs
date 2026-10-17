//! Hugging Face's Xet storage, read with the official `hf-xet` engine.
//!
//! A Hub `resolve` URL answers a HEAD request with the file's Xet hash and the route that hands
//! out read tokens. The engine then fetches the file's chunks from the CAS servers over many
//! connections and returns its bytes in order, from any offset, so the caller writes, resumes
//! and verifies exactly as it does for plain HTTP.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context as _;
use bytes::Bytes;
use reqwest::header::{AUTHORIZATION, LINK};
use tokio::sync::OnceCell;
use xet::xet_session::{
    HeaderMap, HeaderValue, XetConfig, XetDownloadStream, XetDownloadStreamGroup, XetFileInfo, XetSession,
    XetSessionBuilder, header,
};

/// The engine buffers chunks it fetched ahead of the write position. Its defaults scale with
/// system memory (gigabytes on a desktop) and delay the first in-order bytes; these keep a
/// download at about 300 MB of memory without slowing it.
const BUFFERS: [(&str, &str); 4] = [
    ("reconstruction.download_buffer_size", "128mb"),
    ("reconstruction.download_buffer_perfile_size", "32mb"),
    ("reconstruction.download_buffer_limit", "256mb"),
    ("reconstruction.min_prefetch_buffer", "128mb"),
];

pub(crate) struct Xet {
    /// Does not follow redirects: the HEAD answer itself carries the Xet headers.
    client: reqwest::Client,
    cache_dir: PathBuf,
    session: OnceCell<XetSession>,
}

/// A file being read from Xet.
pub(crate) struct XetBody {
    stream: XetDownloadStream,
    _group: XetDownloadStreamGroup,
    pub size: u64,
}

impl XetBody {
    pub async fn next(&mut self) -> anyhow::Result<Option<Bytes>> {
        Ok(self.stream.next().await?)
    }
}

impl Drop for XetBody {
    /// Stops the chunk fetches still running ahead of the write position.
    fn drop(&mut self) {
        self.stream.cancel();
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadToken {
    access_token: String,
    exp: u64,
    cas_url: String,
}

impl Xet {
    pub fn new(user_agent: &str, cache_dir: PathBuf) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .http1_only()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            cache_dir,
            session: OnceCell::new(),
        })
    }

    /// Opens `url` at `offset`. `None` when the file is not stored in Xet.
    pub async fn open(&self, url: &str, bearer: Option<&str>, offset: u64) -> anyhow::Result<Option<XetBody>> {
        let auth = bearer.map(|t| format!("Bearer {t}"));
        let mut head = self.client.head(url);
        if let Some(auth) = &auth {
            head = head.header(AUTHORIZATION, auth);
        }
        let response = head.send().await?;
        let status = response.status();
        if !status.is_success() && !status.is_redirection() {
            anyhow::bail!("{url} answered {status}");
        }
        let headers = response.headers();
        let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let (Some(hash), Some(size), Some(token_url)) = (
            text("x-xet-hash"),
            text("x-linked-size").and_then(|s| s.parse::<u64>().ok()),
            headers
                .get_all(LINK)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .find_map(|v| link_target(v, "xet-auth")),
        ) else {
            return Ok(None);
        };
        if offset >= size {
            anyhow::bail!("resume point {offset} is past the end of the {size}-byte file");
        }

        let mut get = self.client.get(&token_url);
        if let Some(auth) = &auth {
            get = get.header(AUTHORIZATION, auth);
        }
        let token: ReadToken = get
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("reading the Xet token")?;

        let mut refresh = HeaderMap::new();
        if let Some(auth) = &auth {
            refresh.insert(header::AUTHORIZATION, HeaderValue::from_str(auth)?);
        }
        let group = self
            .session()
            .await?
            .new_download_stream_group()?
            .with_endpoint(token.cas_url)
            .with_token_info(token.access_token, token.exp)
            .with_token_refresh_url(token_url, refresh)
            .build()
            .await?;
        let mut stream = group
            .download_stream(XetFileInfo::new(hash.to_string(), size), Some(offset..size))
            .await?;
        stream.start();
        Ok(Some(XetBody {
            stream,
            _group: group,
            size,
        }))
    }

    async fn session(&self) -> anyhow::Result<&XetSession> {
        self.session
            .get_or_try_init(|| async {
                let mut config = XetConfig::new();
                for (path, value) in BUFFERS {
                    config = config.with_config(path, value)?;
                }
                // Downloads keep no chunk cache today; make sure a later version doesn't start one.
                config = config.with_config("chunk_cache.size_bytes", 0)?;
                config.telemetry.enabled = false;
                let session = XetSessionBuilder::new_with_config(config)
                    .with_cache_dir(self.cache_dir.clone())
                    .with_tokio_handle(tokio::runtime::Handle::current())
                    .build()?;
                anyhow::Ok(session)
            })
            .await
    }
}

/// The target of the `rel` entry in an HTTP `Link` header.
fn link_target(header: &str, rel: &str) -> Option<String> {
    header.split(',').find_map(|entry| {
        let (target, params) = entry.trim().split_once(';')?;
        let matches = params.split(';').any(|p| {
            p.trim()
                .strip_prefix("rel=")
                .is_some_and(|v| v.trim_matches('"') == rel)
        });
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        matches.then(|| target.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_token_route_in_the_link_header() {
        let header = "<https://huggingface.co/api/models/a/b/xet-read-token/9f64>; rel=\"xet-auth\", \
                      <https://cas-server.xethub.hf.co/v1/reconstructions/c2ed>; rel=\"xet-reconstruction-info\"";
        assert_eq!(
            link_target(header, "xet-auth").as_deref(),
            Some("https://huggingface.co/api/models/a/b/xet-read-token/9f64")
        );
        assert_eq!(
            link_target(header, "xet-reconstruction-info").as_deref(),
            Some("https://cas-server.xethub.hf.co/v1/reconstructions/c2ed")
        );
        assert_eq!(link_target(header, "next"), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "downloads 12 MB from huggingface.co"]
    async fn reads_a_hub_file_from_an_offset() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let xet = Xet::new("test", dir.path().to_path_buf()).unwrap();
        let url = "https://huggingface.co/ggml-org/tiny-llamas/resolve/main/stories15M-q4_0.gguf";
        let mut body = xet.open(url, None, 7_000_000).await.unwrap().expect("stored in Xet");
        assert_eq!(body.size, 19_077_344);
        let mut tail = Vec::new();
        while let Some(chunk) = body.next().await.unwrap() {
            tail.extend_from_slice(&chunk);
        }
        assert_eq!(tail.len(), 19_077_344 - 7_000_000);

        let plain = reqwest::get(url).await.unwrap().bytes().await.unwrap();
        assert_eq!(Sha256::digest(&tail), Sha256::digest(&plain[7_000_000..]));
        assert!(
            xet.open(
                "https://huggingface.co/ggml-org/tiny-llamas/resolve/main/README.md",
                None,
                0
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}
