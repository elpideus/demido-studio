//! What Hugging Face publishes, keyless, parsed from bytes.
//!
//! Brief B22: "Models Browser & Downloader"
//!
//! **Keyless.** A person should not need an account before they can run a
//! model. That is also the boundary of what this can promise: a gated
//! repository answers the search and refuses the file, so gating is read here,
//! at listing time, and carried to the browser, which says so before the
//! download rather than after it fails.
//!
//! **Parsing is a pure function over bytes.** [`parse_listing`] and
//! [`parse_files`] take what the server sent and nothing else, so a repository
//! whose file list confuses the browser is reproduced in a test by committing
//! the payload (`tests/fixtures/index/`), not by hoping it is published the
//! same way next week. Prior art is v2's `search.rs`, written this way for
//! exactly this reason.
//!
//! **Publisher fields are passed through and never interpreted.** The listing
//! carries no reliable statement of tool use or reasoning, and inventing one is
//! how a browser recommends a model that fails on the first turn. [`Repo`] has
//! no field that could hold a capability.
//!
//! **A network failure is not a broken window.** Every call answers with an
//! [`Answer`], never an error: an unreadable index is a stated condition the
//! browser renders beside a library that still works.
//!
//! **No seam.** The index is one host, and a second implementation would exist
//! only in tests (#36). The tests point this one at `127.0.0.1` instead.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where the index lives.
pub const HOST: &str = "https://huggingface.co";

/// Identifies the app, so a rate limit is one somebody can explain: the name,
/// the version, and where to find the people responsible.
pub const USER_AGENT: &str = concat!(
    "demido-studio/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/elpideus/demido-studio)"
);

/// How many repositories one search returns. A models browser is for
/// choosing, and past about this many a better query beats more scrolling.
pub const LISTING: usize = 30;

/// How long one request may take. A listing is a few kilobytes; a request that
/// has not answered in this long is a connection that is not going to.
const PATIENCE: Duration = Duration::from_secs(20);

/// A repository that publishes GGUF weights, as the listing describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    /// `unsloth/Qwen3-8B-GGUF`, which is what everything else keys on.
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Downloads over the last thirty days, as Hugging Face counts them. The
    /// listing's order.
    pub downloads: u64,
    pub likes: u64,
    /// Whether the files need an account and accepted terms. A keyless fetch
    /// of a gated file is refused, so this is known before anything is.
    pub gated: bool,
    /// The publisher's own fields, verbatim. `pipeline_tag`, `library_name`
    /// and every tag in the order sent, including the ones nobody reads:
    /// which of them a person sees is the window's call, and what they mean is
    /// nobody's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    pub tags: Vec<String>,
}

/// One `.gguf` in a repository.
///
/// Only the facts the server sends. Which file is weights, a projector or a
/// piece of a split model, and what its quantisation is, are read off this
/// list by #71.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    /// The path inside the repository, directory included: a large model is
    /// published as `Q4_K_M/model-00001-of-00002.gguf`, and the path is the URL.
    pub path: String,
    /// The object's size as the server records it, which is the number the
    /// download will actually cost.
    pub bytes: u64,
    /// SHA-256 as Git LFS names the object, for the queue to verify against.
    /// `None` where the server did not send one, which includes a gated
    /// repository masking it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// What asking the index got.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Answer<T> {
    Read {
        found: T,
    },
    /// The index could not be read, and why. The caller's own library is not
    /// involved, which is the point of this being a value.
    Unreadable {
        cause: Cause,
        reason: String,
    },
}

/// Why the index could not be read. Distinct causes, because a person does
/// different things about each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// Nothing answered: no connection, a DNS failure, a timeout.
    Offline,
    /// Hugging Face is limiting this address. It passes.
    RateLimited,
    /// No such public repository. Keyless, Hugging Face answers a private or
    /// absent repository with `401`, not `404`.
    Missing,
    /// Some other refusal, with its status.
    Answered,
    /// A success whose body is not what the index sends: a captive portal, a
    /// proxy's error page, a schema that moved.
    Malformed,
}

/// A body that is not the payload it claims to be.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Malformed(String);

/// The index, at one host.
#[derive(Debug, Clone)]
pub struct Index {
    client: reqwest::Client,
    host: String,
}

impl Default for Index {
    fn default() -> Self {
        Self::at(HOST)
    }
}

impl Index {
    /// The index at another host. Only the tests have one.
    pub fn at(host: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            host: host.trim_end_matches('/').to_owned(),
        }
    }

    /// Repositories publishing GGUF files whose id contains `query`, which is
    /// a name, an author, or `author/name`. Most downloaded first, capped at
    /// [`LISTING`].
    ///
    /// An empty query is the front page: what most people are running, which
    /// is a better first screen than a blank one asking somebody to guess.
    pub async fn search(&self, query: &str) -> Answer<Vec<Repo>> {
        let mut url = format!(
            "{}/api/models?filter=gguf&sort=downloads&direction=-1&limit={LISTING}",
            self.host
        );
        // Named, because without `expand` the listing leaves `gated` out, and
        // with it the listing sends exactly these and nothing else.
        for field in [
            "author",
            "downloads",
            "likes",
            "gated",
            "pipeline_tag",
            "library_name",
            "tags",
        ] {
            url.push_str("&expand[]=");
            url.push_str(field);
        }
        let query = query.trim();
        if !query.is_empty() {
            url.push_str("&search=");
            url.push_str(&encode(query));
        }
        self.read(&url, parse_listing).await
    }

    /// Every `.gguf` in a repository, subdirectories included.
    ///
    /// The id goes into a path, so one that is not `owner/name` is refused
    /// before anything is sent rather than allowed to become some other URL.
    pub async fn files(&self, repo: &str) -> Answer<Vec<File>> {
        if !is_repo_id(repo) {
            return Answer::Unreadable {
                cause: Cause::Missing,
                reason: format!("{repo:?} is not a repository id of the form owner/name"),
            };
        }
        let url = format!("{}/api/models/{repo}/tree/main?recursive=true", self.host);
        self.read(&url, parse_files).await
    }

    async fn read<T>(&self, url: &str, parse: fn(&[u8]) -> Result<T, Malformed>) -> Answer<T> {
        let unreadable = |cause, reason: String| Answer::Unreadable { cause, reason };

        let response = match self
            .client
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .timeout(PATIENCE)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, url, "the model index did not answer");
                return unreadable(
                    Cause::Offline,
                    format!("Hugging Face could not be reached: {error}"),
                );
            }
        };

        let status = response.status();
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(error) => {
                return unreadable(
                    Cause::Offline,
                    format!("Hugging Face stopped answering part way: {error}"),
                )
            }
        };

        match status.as_u16() {
            200..=299 => match parse(&body) {
                Ok(found) => Answer::Read { found },
                Err(error) => unreadable(
                    Cause::Malformed,
                    format!("Hugging Face answered with something that is not a listing: {error}"),
                ),
            },
            429 => unreadable(
                Cause::RateLimited,
                format!("Hugging Face is limiting requests from this address ({status}); it passes within minutes"),
            ),
            401 | 403 | 404 => unreadable(
                Cause::Missing,
                format!("Hugging Face has no public repository there ({status})"),
            ),
            _ => {
                let said: String = String::from_utf8_lossy(&body).chars().take(200).collect();
                unreadable(
                    Cause::Answered,
                    format!("Hugging Face answered {status}: {said}"),
                )
            }
        }
    }
}

/// Read a search payload.
///
/// Ordered by downloads whatever order it arrived in, and capped at
/// [`LISTING`] whatever `limit` the server honoured, so both promises hold at
/// this function rather than at a query string.
pub fn parse_listing(body: &[u8]) -> Result<Vec<Repo>, Malformed> {
    let listed: Vec<Listed> =
        serde_json::from_slice(body).map_err(|error| Malformed(error.to_string()))?;
    let mut repos: Vec<Repo> = listed
        .into_iter()
        .map(|item| Repo {
            id: item.id,
            author: item.author,
            downloads: item.downloads,
            likes: item.likes,
            // `false` when open, `"auto"` or `"manual"` when not, so anything
            // that is not the boolean false is a gate. An absent field is
            // open: that is what the listing sends for a repository nobody
            // gated.
            gated: item
                .gated
                .is_some_and(|gated| gated != serde_json::Value::Bool(false)),
            pipeline: item.pipeline_tag,
            library: item.library_name,
            tags: item.tags,
        })
        .collect();
    repos.sort_by_key(|repo| std::cmp::Reverse(repo.downloads));
    repos.truncate(LISTING);
    Ok(repos)
}

/// Read a repository's file tree, keeping the `.gguf` files.
///
/// A repository holds a README, a config, an importance matrix and often the
/// unquantised weights beside the GGUFs. A file llama.cpp cannot load is not a
/// choice anybody has, so it is dropped here rather than shown greyed.
pub fn parse_files(body: &[u8]) -> Result<Vec<File>, Malformed> {
    let entries: Vec<Entry> =
        serde_json::from_slice(body).map_err(|error| Malformed(error.to_string()))?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.kind == "file" && is_gguf(&entry.path))
        .map(|entry| File {
            // The LFS record is the object. `size` beside it is the same number
            // today, and was the pointer's 135 bytes on the payloads v2 read.
            bytes: entry.lfs.as_ref().map_or(entry.size, |lfs| lfs.size),
            sha256: entry
                .lfs
                .and_then(|lfs| lfs.oid)
                .filter(|oid| is_sha256(oid)),
            path: entry.path,
        })
        .collect())
}

/// `owner/name`, in the characters Hugging Face allows in either.
fn is_repo_id(id: &str) -> bool {
    let part = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    id.split_once('/')
        .is_some_and(|(owner, name)| part(owner) && part(name))
}

fn is_gguf(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("gguf"))
}

/// A gated repository sends sixty-four asterisks where the digest goes.
fn is_sha256(oid: &str) -> bool {
    oid.len() == 64 && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Percent-encode a search query. Only what a model name can contain needs
/// escaping, and a URL encoder for one field is a dependency for nothing.
fn encode(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// One row of a search payload. Every field the browser does not show is
/// ignored, so a schema change elsewhere in the row is not an outage.
#[derive(Deserialize)]
struct Listed {
    id: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    gated: Option<serde_json::Value>,
    #[serde(default)]
    pipeline_tag: Option<String>,
    #[serde(default)]
    library_name: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<Lfs>,
}

#[derive(Deserialize)]
struct Lfs {
    #[serde(default)]
    size: u64,
    #[serde(default)]
    oid: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_survives_the_characters_a_model_name_has() {
        assert_eq!(encode("qwen3 8b"), "qwen3+8b");
        assert_eq!(encode("unsloth/Qwen3"), "unsloth%2FQwen3");
        assert_eq!(encode("phi-4_v2.1~x"), "phi-4_v2.1~x");
        assert_eq!(encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn only_owner_slash_name_is_a_repository() {
        assert!(is_repo_id("unsloth/Qwen3-0.6B-GGUF"));
        assert!(is_repo_id("Qwen/Qwen3.5-9B_x"));
        for refused in [
            "", "unsloth", "a/b/c", "../etc", "a/..", "a/b?x=1", "a/b#frag", "a /b",
        ] {
            assert!(!is_repo_id(refused), "{refused}");
        }
    }

    #[test]
    fn only_a_digest_is_a_digest() {
        assert!(is_sha256(&"a".repeat(64)));
        assert!(!is_sha256(&"*".repeat(64)));
        assert!(!is_sha256("aa11"));
    }
}
