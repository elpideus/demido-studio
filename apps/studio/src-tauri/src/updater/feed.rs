//! The list of releases the updater reads, and the choice of which one to offer.
//!
//! Releases are the GitHub Releases of [`brand::RELEASES_REPO`]. Each one carries the installer
//! (`Demido-Studio-Setup-<version>.exe`, which holds the whole app) and its signature (the same
//! name plus `.sig`). [`select`] is pure: from the list, the channel and the running version it
//! picks the newest release worth offering, so every rule about what gets offered is tested here.

use std::cmp::Ordering;
use std::time::Duration;

use demido_core::brand;
use parking_lot::Mutex;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, ETAG, IF_NONE_MATCH, RETRY_AFTER};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::settings::UpdateChannel;

/// Base URL that replaces GitHub's API, for testing against a local mock of the releases feed.
pub const FEED_ENV: &str = "DEMIDO_UPDATE_FEED";
/// Longest wait for the feed or a signature file; the installer itself goes through the
/// resumable downloader instead.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Where releases are listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedConfig {
    /// Base URL: `https://api.github.com/repos/<owner>/<name>` unless overridden.
    pub base: String,
    /// Set through [`FEED_ENV`]. A mock serves its files from wherever it runs, so asset links
    /// may then use any http(s) host instead of only GitHub.
    pub custom: bool,
}

impl FeedConfig {
    pub fn github() -> Self {
        Self {
            base: format!("https://api.github.com/repos/{}", brand::RELEASES_REPO),
            custom: false,
        }
    }

    /// GitHub, unless [`FEED_ENV`] names another base URL.
    pub fn from_env() -> Self {
        match std::env::var(FEED_ENV) {
            Ok(base) if !base.trim().is_empty() => Self {
                base: base.trim().trim_end_matches('/').to_string(),
                custom: true,
            },
            _ => Self::github(),
        }
    }

    pub fn releases_url(&self) -> String {
        format!("{}/releases?per_page=30", self.base)
    }
}

/// A release as GitHub's API lists it, with only the fields the updater reads.
#[derive(Clone, Debug, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    /// `sha256:<hex>`, on assets uploaded since GitHub started computing it.
    #[serde(default)]
    pub digest: Option<String>,
}

/// A newer version as the UI shows it and `pending.json` keeps it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    /// Release notes, in Markdown, as written on GitHub.
    pub notes: String,
    pub published_at: Option<String>,
    /// The release's page, for "Release notes" and "Download from GitHub".
    pub url: String,
    /// Installer size in bytes.
    pub size: u64,
    pub prerelease: bool,
}

/// A file of a release, ready to download.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256, when GitHub lists one.
    pub sha256: Option<String>,
}

/// The release chosen to update to, with the two files it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub version: Version,
    pub release: ReleaseInfo,
    pub setup: Asset,
    pub signature: Asset,
}

/// The newest release on `channel` that is newer than `running`, and complete: an installer and
/// its signature, both at an allowed address. Never the running version and never an older one.
/// `any_host` accepts asset links outside github.com (a custom feed, see [`FeedConfig`]).
pub fn select(releases: &[GhRelease], channel: UpdateChannel, running: &Version, any_host: bool) -> Option<Candidate> {
    releases
        .iter()
        .filter_map(|r| candidate(r, channel, any_host))
        .filter(|c| c.version.cmp_precedence(running) == Ordering::Greater)
        .max_by(|a, b| a.version.cmp_precedence(&b.version))
}

fn candidate(release: &GhRelease, channel: UpdateChannel, any_host: bool) -> Option<Candidate> {
    if release.draft {
        return None;
    }
    let version = parse_version(&release.tag_name)?;
    // Either marker makes it a pre-release: a tag like `v0.5.0-beta.1` published without GitHub's
    // checkbox is still not a stable release.
    let prerelease = release.prerelease || !version.pre.is_empty();
    if prerelease && channel == UpdateChannel::Release {
        return None;
    }
    let setup_name = brand::setup_file_name(&version.to_string());
    let setup = asset(release, &setup_name, any_host)?;
    let signature = asset(release, &format!("{setup_name}.sig"), any_host)?;
    let url = if allowed_url(&release.html_url, any_host) {
        release.html_url.clone()
    } else {
        format!(
            "https://github.com/{}/releases/tag/{}",
            brand::RELEASES_REPO,
            release.tag_name
        )
    };
    Some(Candidate {
        release: ReleaseInfo {
            version: version.to_string(),
            notes: release.body.clone().unwrap_or_default(),
            published_at: release.published_at.clone(),
            url,
            size: setup.size,
            prerelease,
        },
        version,
        setup,
        signature,
    })
}

fn asset(release: &GhRelease, name: &str, any_host: bool) -> Option<Asset> {
    let found = release.assets.iter().find(|a| a.name == name)?;
    if !allowed_url(&found.browser_download_url, any_host) {
        tracing::warn!(
            "ignoring update asset {name} at an unexpected address: {}",
            found.browser_download_url
        );
        return None;
    }
    Some(Asset {
        name: found.name.clone(),
        url: found.browser_download_url.clone(),
        size: found.size,
        sha256: found.digest.as_deref().and_then(parse_digest),
    })
}

/// The version a release tag names: `v0.5.0` and `0.5.0` are both 0.5.0.
pub fn parse_version(tag: &str) -> Option<Version> {
    let tag = tag.trim();
    Version::parse(tag.strip_prefix(['v', 'V']).unwrap_or(tag)).ok()
}

/// The lowercase hex SHA-256 in GitHub's `sha256:<hex>` digest, or `None` for any other form.
pub fn parse_digest(digest: &str) -> Option<String> {
    let hex = digest.trim().strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// Whether the updater may fetch `raw`: https on github.com (where release assets live), or any
/// http(s) address for a custom feed. Links with credentials in them are never followed.
pub fn allowed_url(raw: &str, any_host: bool) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if any_host {
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
    } else {
        url.scheme() == "https" && url.host_str() == Some("github.com")
    }
}

/// Why the releases list could not be read, in words for the person.
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("GitHub is limiting update checks right now. Try again in a while.")]
    RateLimited,
    #[error("Could not reach GitHub to check for updates. Check the internet connection and try again.")]
    Offline(#[source] reqwest::Error),
    #[error("GitHub answered {0} to the update check. Try again in a while.")]
    Status(StatusCode),
    #[error("GitHub sent a list of updates that Demido Studio could not read.")]
    Invalid(#[source] serde_json::Error),
}

/// GitHub's refusal for too many requests: 429, or 403 with the rate limit spent (or a
/// `Retry-After`, or a body that says so, for its secondary limits).
pub fn rate_limited(status: u16, remaining: Option<&str>, retry_after: bool, body: &str) -> bool {
    status == 429
        || (status == 403
            && (remaining.is_some_and(|r| r.trim() == "0")
                || retry_after
                || body.to_ascii_lowercase().contains("rate limit")))
}

/// Parses the releases list. An entry that does not parse is skipped rather than failing the
/// whole list, so one odd release cannot block updates.
pub fn parse_releases(bytes: &[u8]) -> Result<Vec<GhRelease>, serde_json::Error> {
    let list: Vec<serde_json::Value> = serde_json::from_slice(bytes)?;
    Ok(list
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

struct Cached {
    etag: String,
    releases: Vec<GhRelease>,
}

/// Reads the releases list, remembering its ETag: GitHub answers an unchanged list with 304, which
/// does not count against the 60 requests an hour it allows a computer without an account.
pub struct Feed {
    config: FeedConfig,
    cache: Mutex<Option<Cached>>,
}

impl Feed {
    pub fn new(config: FeedConfig) -> Self {
        Self {
            config,
            cache: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &FeedConfig {
        &self.config
    }

    pub async fn releases(&self, http: &reqwest::Client) -> Result<Vec<GhRelease>, FeedError> {
        let etag = self.cache.lock().as_ref().map(|c| c.etag.clone());
        let mut request = http
            .get(self.config.releases_url())
            .timeout(REQUEST_TIMEOUT)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(etag) = &etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(FeedError::Offline)?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED {
            return match self.cache.lock().as_ref() {
                Some(cached) => Ok(cached.releases.clone()),
                None => Err(FeedError::Status(status)),
            };
        }
        if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
            let headers = response.headers();
            let remaining = headers
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let retry_after = headers.contains_key(RETRY_AFTER);
            let body = response.text().await.unwrap_or_default();
            return Err(
                if rate_limited(status.as_u16(), remaining.as_deref(), retry_after, &body) {
                    FeedError::RateLimited
                } else {
                    FeedError::Status(status)
                },
            );
        }
        if !status.is_success() {
            return Err(FeedError::Status(status));
        }
        let new_etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = response.bytes().await.map_err(FeedError::Offline)?;
        let releases = parse_releases(&bytes).map_err(FeedError::Invalid)?;
        *self.cache.lock() = new_etag.map(|etag| Cached {
            etag,
            releases: releases.clone(),
        });
        Ok(releases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh_asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/elpideus/demido-studio/releases/download/x/{name}"),
            size: 1234,
            digest: None,
        }
    }

    /// A complete release: installer and signature, both on github.com.
    fn release(tag: &str, prerelease: bool) -> GhRelease {
        let version = tag.trim_start_matches('v');
        let setup = brand::setup_file_name(version);
        GhRelease {
            tag_name: tag.to_string(),
            body: Some(format!("Notes for {version}")),
            draft: false,
            prerelease,
            published_at: Some("2026-09-20T10:00:00Z".into()),
            html_url: format!("https://github.com/elpideus/demido-studio/releases/tag/{tag}"),
            assets: vec![gh_asset(&setup), gh_asset(&format!("{setup}.sig"))],
        }
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn picked(releases: &[GhRelease], channel: UpdateChannel, running: &str) -> Option<String> {
        select(releases, channel, &v(running), false).map(|c| c.release.version)
    }

    #[test]
    fn the_highest_newer_version_wins_whatever_the_order() {
        let list = [
            release("v0.4.1", false),
            release("v0.6.0", false),
            release("v0.5.0", false),
        ];
        assert_eq!(picked(&list, UpdateChannel::Release, "0.4.0").as_deref(), Some("0.6.0"));
    }

    #[test]
    fn the_release_channel_skips_pre_releases_by_flag_or_by_tag() {
        let list = [
            release("v0.4.1", false),
            release("v0.5.0", true),
            // Tagged as a pre-release but published without GitHub's checkbox.
            release("v0.6.0-beta.1", false),
        ];
        assert_eq!(picked(&list, UpdateChannel::Release, "0.4.0").as_deref(), Some("0.4.1"));
        assert_eq!(
            picked(&list, UpdateChannel::Prerelease, "0.4.0").as_deref(),
            Some("0.6.0-beta.1")
        );
    }

    #[test]
    fn a_pre_release_of_a_newer_version_beats_an_older_stable_only_on_that_channel() {
        let list = [release("v0.5.0", false), release("v0.6.0-rc.1", true)];
        assert_eq!(picked(&list, UpdateChannel::Release, "0.4.0").as_deref(), Some("0.5.0"));
        assert_eq!(
            picked(&list, UpdateChannel::Prerelease, "0.4.0").as_deref(),
            Some("0.6.0-rc.1")
        );
        // The stable version outranks its own pre-releases.
        let list = [release("v0.6.0-rc.1", true), release("v0.6.0", false)];
        assert_eq!(
            picked(&list, UpdateChannel::Prerelease, "0.5.0").as_deref(),
            Some("0.6.0")
        );
        assert_eq!(
            picked(&list, UpdateChannel::Prerelease, "0.6.0-beta.2").as_deref(),
            Some("0.6.0")
        );
    }

    #[test]
    fn never_offers_the_running_version_or_an_older_one() {
        let list = [
            release("v0.3.9", false),
            release("v0.4.0", false),
            release("v0.4.0-beta.1", true),
        ];
        assert_eq!(picked(&list, UpdateChannel::Prerelease, "0.4.0"), None);
        // A build differing only in metadata is the same version.
        let list = [release("v0.4.0+rebuild", false)];
        assert_eq!(picked(&list, UpdateChannel::Release, "0.4.0"), None);
        // Running a pre-release, its stable version is an update.
        let list = [release("v0.4.0", false)];
        assert_eq!(
            picked(&list, UpdateChannel::Release, "0.4.0-beta.3").as_deref(),
            Some("0.4.0")
        );
    }

    #[test]
    fn drafts_unparsable_tags_and_incomplete_releases_are_skipped() {
        let mut draft = release("v0.9.0", false);
        draft.draft = true;
        let unparsable = release("nightly-2026-09-20", false);
        let mut no_signature = release("v0.8.0", false);
        no_signature.assets.pop();
        let mut no_installer = release("v0.7.0", false);
        no_installer.assets.remove(0);
        let mut renamed = release("v0.6.0", false);
        renamed.assets[0].name = "Demido-Studio-Setup.exe".into();
        let list = [
            draft,
            unparsable,
            no_signature,
            no_installer,
            renamed,
            release("v0.5.0", false),
        ];
        assert_eq!(picked(&list, UpdateChannel::Release, "0.4.0").as_deref(), Some("0.5.0"));
    }

    #[test]
    fn a_candidate_carries_everything_the_download_and_the_ui_need() {
        let mut r = release("v0.5.0", false);
        r.assets[0].digest = Some(format!("sha256:{}", "AB".repeat(32)));
        r.assets[0].size = 25_000_000;
        let c = select(&[r], UpdateChannel::Release, &v("0.4.0"), false).unwrap();
        assert_eq!(c.setup.name, brand::setup_file_name("0.5.0"));
        assert_eq!(c.signature.name, format!("{}.sig", brand::setup_file_name("0.5.0")));
        assert_eq!(c.setup.sha256, Some("ab".repeat(32)));
        assert_eq!(c.signature.sha256, None);
        assert_eq!(c.release.size, 25_000_000);
        assert_eq!(c.release.notes, "Notes for 0.5.0");
        assert_eq!(c.release.published_at.as_deref(), Some("2026-09-20T10:00:00Z"));
        assert!(!c.release.prerelease);
        assert_eq!(
            c.release.url,
            "https://github.com/elpideus/demido-studio/releases/tag/v0.5.0"
        );
    }

    #[test]
    fn digests_are_read_only_in_githubs_sha256_form() {
        let hex = "0123456789abcdef".repeat(4);
        assert_eq!(parse_digest(&format!("sha256:{hex}")), Some(hex.clone()));
        assert_eq!(
            parse_digest(&format!("sha256:{}", hex.to_uppercase())),
            Some(hex.clone())
        );
        assert_eq!(parse_digest(&format!("sha512:{hex}")), None);
        assert_eq!(parse_digest(&hex), None);
        assert_eq!(parse_digest("sha256:abc"), None);
        assert_eq!(parse_digest(&format!("sha256:{}", "zz".repeat(32))), None);
    }

    #[test]
    fn assets_must_come_from_github_over_https_unless_the_feed_is_custom() {
        let gh = "https://github.com/elpideus/demido-studio/releases/download/v0.5.0/a.exe";
        assert!(allowed_url(gh, false));
        assert!(!allowed_url(&gh.replace("https", "http"), false));
        assert!(!allowed_url("https://evil.example/a.exe", false));
        assert!(!allowed_url("https://github.com.evil.example/a.exe", false));
        assert!(!allowed_url("https://user:pw@github.com/a.exe", false));
        assert!(!allowed_url("file:///C:/a.exe", false));
        assert!(!allowed_url("not a url", false));
        assert!(allowed_url("http://127.0.0.1:8080/a.exe", true));
        assert!(allowed_url(gh, true));
        assert!(!allowed_url("file:///C:/a.exe", true));
    }

    #[test]
    fn an_asset_elsewhere_than_github_makes_the_release_incomplete() {
        let mut r = release("v0.5.0", false);
        r.assets[1].browser_download_url = "https://evil.example/x.sig".into();
        assert!(select(std::slice::from_ref(&r), UpdateChannel::Release, &v("0.4.0"), false).is_none());
        assert!(select(&[r], UpdateChannel::Release, &v("0.4.0"), true).is_some());
    }

    #[test]
    fn an_odd_release_page_link_falls_back_to_the_tag_page() {
        let mut r = release("v0.5.0", false);
        r.html_url = "javascript:alert(1)".into();
        let c = select(&[r], UpdateChannel::Release, &v("0.4.0"), false).unwrap();
        assert_eq!(
            c.release.url,
            format!("https://github.com/{}/releases/tag/v0.5.0", brand::RELEASES_REPO)
        );
    }

    #[test]
    fn tags_parse_with_or_without_the_v() {
        assert_eq!(parse_version("v0.5.0"), Some(v("0.5.0")));
        assert_eq!(parse_version("0.5.0-beta.2"), Some(v("0.5.0-beta.2")));
        assert_eq!(parse_version("v0.5"), None);
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn rate_limits_are_told_apart_from_other_refusals() {
        assert!(rate_limited(429, None, false, ""));
        assert!(rate_limited(403, Some("0"), false, ""));
        assert!(rate_limited(403, None, true, ""));
        assert!(rate_limited(
            403,
            Some("12"),
            false,
            r#"{"message":"You have exceeded a secondary rate limit"}"#
        ));
        assert!(!rate_limited(403, Some("41"), false, r#"{"message":"Forbidden"}"#));
        assert!(!rate_limited(404, Some("0"), false, ""));
    }

    #[test]
    fn one_unreadable_entry_does_not_hide_the_rest() {
        let json = br#"[
            {"tag_name": "v0.5.0", "draft": false, "prerelease": false, "html_url": "https://github.com/x",
             "assets": [{"name": "a", "browser_download_url": "https://github.com/a", "size": 3, "digest": null}]},
            {"name": "no tag here"},
            {"tag_name": "v0.6.0", "assets": []}
        ]"#;
        let list = parse_releases(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].assets[0].size, 3);
        assert!(parse_releases(b"{\"message\": \"Not Found\"}").is_err());
    }

    #[test]
    fn the_feed_defaults_to_the_releases_repository() {
        let config = FeedConfig::github();
        assert!(!config.custom);
        assert_eq!(
            config.releases_url(),
            format!(
                "https://api.github.com/repos/{}/releases?per_page=30",
                brand::RELEASES_REPO
            )
        );
    }

    fn http_response(status: &str, headers: &[(&str, &str)], body: &str) -> String {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("\r\n");
        out.push_str(body);
        out
    }

    /// A local server answering one connection at a time with `responses`, in order. Returns its
    /// base URL and, once every response went out, the request heads it received.
    async fn serve(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/repos/owner/name", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut heads = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buf).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    head.extend_from_slice(&buf[..n]);
                }
                heads.push(String::from_utf8_lossy(&head).to_ascii_lowercase());
                socket.write_all(response.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
            heads
        });
        (base, task)
    }

    #[tokio::test]
    async fn the_feed_sends_githubs_headers_reuses_its_etag_and_names_rate_limits() {
        let list = r#"[{"tag_name": "v0.5.0", "assets": []}]"#;
        let (base, heads) = serve(vec![
            http_response("200 OK", &[("ETag", "\"v1\"")], list),
            http_response("304 Not Modified", &[], ""),
            http_response(
                "403 Forbidden",
                &[("X-RateLimit-Remaining", "0")],
                r#"{"message": "API rate limit exceeded"}"#,
            ),
            http_response("500 Internal Server Error", &[], ""),
        ])
        .await;
        let feed = Feed::new(FeedConfig { base, custom: true });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();

        let first = feed.releases(&http).await.unwrap();
        assert_eq!(first.len(), 1);
        // Unchanged: GitHub answers 304 and the list comes from memory.
        let second = feed.releases(&http).await.unwrap();
        assert_eq!(second[0].tag_name, "v0.5.0");
        let limited = feed.releases(&http).await.unwrap_err();
        assert!(matches!(limited, FeedError::RateLimited), "{limited:?}");
        assert_eq!(
            limited.to_string(),
            "GitHub is limiting update checks right now. Try again in a while."
        );
        let broken = feed.releases(&http).await.unwrap_err();
        assert!(
            matches!(broken, FeedError::Status(s) if s.as_u16() == 500),
            "{broken:?}"
        );

        let heads = heads.await.unwrap();
        assert!(
            heads[0].starts_with("get /repos/owner/name/releases?per_page=30 "),
            "{}",
            heads[0]
        );
        assert!(heads[0].contains("accept: application/vnd.github+json"));
        assert!(heads[0].contains("x-github-api-version: 2022-11-28"));
        assert!(!heads[0].contains("if-none-match"));
        assert!(heads[1].contains("if-none-match: \"v1\""), "{}", heads[1]);
    }
}
