//! Reading what llama.cpp publishes, and matching it to this computer.
//!
//! llama.cpp marks a stable release (`v1.0.0`). That release carries one small file, `nightly-tag.txt`,
//! naming the per-commit build it stands for (`b12359`). The build is where the files are: one archive per
//! system and kind of graphics card, each listed with its size and its SHA-256.
//!
//! Asking costs: GitHub allows sixty questions an hour from one address, and every question is time. So the
//! answer is kept (on disk, by `mod.rs`) and asking again is made cheap:
//!
//! - Within the hour, nothing is asked at all.
//! - After that, one question: "has the stable release changed since this mark?" (`If-None-Match`).
//!   Usually the answer is "no", with no body, and everything kept stays good.
//! - A build's list of files does not change once published. If the stable release still points at the
//!   build that is kept, that list is not asked for again.
//!
//! So: three questions the first time, none within the hour, one small one after it.

use reqwest::{Client, Response, StatusCode, header};
use serde::{Deserialize, Serialize};

use super::{Offer, RemoteFile, Source};
use crate::system::Advice;
use crate::{Error, Result, now_ms};

/// What the publisher lists, as last read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Published {
    /// The stable release, for example `v1.0.0`.
    pub release: String,
    /// The build it stands for, for example `b12359`.
    pub build: String,
    /// Every file of that build.
    pub files: Vec<RemoteFile>,
    /// The publisher's mark for the stable release as it was read. Sent back when asking again.
    pub mark: Option<String>,
    /// When the publisher was last asked, in milliseconds since 1970.
    pub asked_at: i64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    /// `sha256:…`. GitHub computes it when the file is uploaded.
    digest: Option<String>,
    browser_download_url: String,
}

/// A build tag goes into an address, so it may only be what a tag looks like.
fn plain(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 40 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn host(address: &str) -> &str {
    address.split("://").nth(1).unwrap_or(address).split('/').next().unwrap_or(address)
}

/// Turns a refusal into words a person can act on.
async fn answered(asked: &str, sent: reqwest::Result<Response>) -> Result<Response> {
    let res = sent.map_err(|e| Error::Net(format!("Could not reach {}. Check the connection and try again. ({})", host(asked), root_cause(&e))))?;
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    let out_of_questions = res.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0");
    Err(if out_of_questions || status == StatusCode::TOO_MANY_REQUESTS {
        Error::Net(format!("{} allows only so many questions an hour from one address, and they are used up. Try again in an hour.", host(asked)))
    } else if status == StatusCode::NOT_FOUND {
        Error::Net(format!("{} has nothing at {asked}. The runtime's releases may have moved.", host(asked)))
    } else {
        Error::Net(format!("{} answered {status} for {asked}.", host(asked)))
    })
}

/// The last link in the chain of causes: "connection refused" rather than "error sending request".
pub(super) fn root_cause(e: &dyn std::error::Error) -> String {
    let mut cause = e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause.to_string()
}

/// Asks for one release. With `unless`, asks "only if it changed since this mark" and gives `None` when it did not.
async fn release(http: &Client, source: &Source, which: &str, unless: Option<&str>) -> Result<Option<(Release, Option<String>)>> {
    let address = format!("{}/repos/{}/releases/{which}", source.api, source.repo);
    let mut ask = http.get(&address).header("accept", "application/vnd.github+json").header("x-github-api-version", "2022-11-28");
    if let Some(mark) = unless {
        ask = ask.header(header::IF_NONE_MATCH, mark);
    }
    let sent = ask.send().await;
    if let Ok(res) = &sent
        && res.status() == StatusCode::NOT_MODIFIED
    {
        return Ok(None);
    }
    let res = answered(&address, sent).await?;
    let mark = res.headers().get(header::ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
    let release = res.json::<Release>().await.map_err(|e| Error::Net(format!("{} answered, but not with a release list Demido can read. ({})", host(&address), root_cause(&e))))?;
    Ok(Some((release, mark)))
}

fn files_of(release: Release) -> Vec<RemoteFile> {
    release
        .assets
        .into_iter()
        .map(|a| RemoteFile {
            sha256: a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).map(str::to_ascii_lowercase),
            name: a.name,
            url: a.browser_download_url,
            size: a.size,
        })
        .collect()
}

/// Asks the publisher: the stable release, the build it stands for, and that build's files.
/// `known` is the last answer, if there is one. What has not changed is not asked for again.
pub(super) async fn read(http: &Client, source: &Source, known: Option<&Published>) -> Result<Published> {
    let Some((stable, mark)) = release(http, source, "latest", known.and_then(|k| k.mark.as_deref())).await? else {
        // "Not changed since your mark." Everything known stays good.
        let known = known.expect("only asked with a mark when there is a known answer");
        return Ok(Published { asked_at: now_ms(), ..known.clone() });
    };
    let Some(pointer) = stable.assets.iter().find(|a| a.name == "nightly-tag.txt") else {
        // No pointer: the stable release holds the files itself.
        return Ok(Published { release: stable.tag_name.clone(), build: stable.tag_name.clone(), files: files_of(stable), mark, asked_at: now_ms() });
    };
    source.allows(&pointer.browser_download_url)?;
    let text = answered(&pointer.browser_download_url, http.get(&pointer.browser_download_url).send().await).await?.text().await.map_err(|e| Error::Net(root_cause(&e)))?;
    let tag = text.trim().to_string();
    if !plain(&tag) {
        return Err(Error::Net(format!("The stable release {} points at a build with a name Demido does not accept. Nothing was downloaded.", stable.tag_name)));
    }
    // A build's files do not change once published: the list that is kept for this build is still right.
    if let Some(known) = known.filter(|k| k.build == tag && !k.files.is_empty()) {
        return Ok(Published { release: stable.tag_name, mark, asked_at: now_ms(), ..known.clone() });
    }
    let (build, _) = release(http, source, &format!("tags/{tag}"), None).await?.expect("asked without a mark, so there is an answer");
    Ok(Published { release: stable.tag_name, build: build.tag_name.clone(), files: files_of(build), mark, asked_at: now_ms() })
}

/// Which of the published files suit this computer, in the order of the advice: the recommended one first.
/// A build the advice names but the release does not have is left out.
pub(super) fn offers(advice: &Advice, build: &str, files: &[RemoteFile]) -> Vec<Offer> {
    std::iter::once((&advice.recommended, true))
        .chain(advice.others.iter().map(|b| (b, false)))
        .filter_map(|(b, recommended)| {
            let name = b.file.replace("{build}", build);
            let main = files.iter().find(|f| f.name == name)?.clone();
            // The CUDA builds need NVIDIA's own libraries. llama.cpp publishes them beside the build, in a
            // file named after the same system: "cudart-…-bin-ubuntu-cuda-13.5-x64.tar.gz".
            let tail = name.split_once("-bin-").map(|(_, tail)| format!("-bin-{tail}"));
            let extra: Vec<RemoteFile> = match (&tail, b.id.starts_with("cuda")) {
                (Some(tail), true) => files.iter().filter(|f| f.name.starts_with("cudart-") && f.name.ends_with(tail.as_str())).cloned().collect(),
                _ => Vec::new(),
            };
            Some(Offer {
                id: b.id.clone(),
                name: b.name.clone(),
                why: b.why.clone(),
                recommended,
                size: main.size,
                extra_size: extra.iter().map(|f| f.size).sum(),
                needs_extra: false,
                file: main,
                extra,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Build;

    fn file(name: &str, size: u64) -> RemoteFile {
        RemoteFile { name: name.into(), url: format!("https://github.com/ggml-org/llama.cpp/releases/download/b12359/{name}"), size, sha256: None }
    }
    fn build(id: &str, file: &str) -> Build {
        Build { id: id.into(), name: id.into(), file: file.into(), why: String::new() }
    }

    #[test]
    fn the_real_file_names_are_matched() {
        // Names as published for b12359, read from GitHub on 2026-11-21.
        let files = [
            file("llama-b12359-bin-ubuntu-cuda-13.5-x64.tar.gz", 170),
            file("cudart-llama-b12359-bin-ubuntu-cuda-13.5-x64.tar.gz", 423),
            file("cudart-llama-b12359-bin-ubuntu-cuda-13.5-arm64.tar.gz", 533),
            file("llama-b12359-bin-ubuntu-vulkan-x64.tar.gz", 34),
            file("llama-b12359-bin-ubuntu-x64.tar.gz", 22),
            file("llama-b12359-bin-win-cuda-13.5-x64.zip", 171),
            file("cudart-llama-bin-win-cuda-13.5-x64.zip", 406),
        ];
        let advice = Advice {
            runtime: "llama.cpp".into(),
            source: String::new(),
            recommended: build("cuda-13.5", "llama-{build}-bin-ubuntu-cuda-13.5-x64.tar.gz"),
            others: vec![
                build("cuda-12.8", "llama-{build}-bin-ubuntu-cuda-12.8-x64.tar.gz"),
                build("vulkan", "llama-{build}-bin-ubuntu-vulkan-x64.tar.gz"),
                build("cpu", "llama-{build}-bin-ubuntu-x64.tar.gz"),
            ],
        };
        let found = offers(&advice, "b12359", &files);
        // CUDA 12.8 is not among the files, so it is not offered.
        assert_eq!(found.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["cuda-13.5", "vulkan", "cpu"]);
        assert!(found[0].recommended && !found[1].recommended);
        // The CUDA libraries for the same system, and only those.
        assert_eq!(found[0].extra.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["cudart-llama-b12359-bin-ubuntu-cuda-13.5-x64.tar.gz"]);
        assert_eq!((found[0].size, found[0].extra_size), (170, 423));
        assert!(found[1].extra.is_empty() && found[2].extra.is_empty());

        // Windows names its CUDA libraries without the build tag.
        let windows = Advice { runtime: "llama.cpp".into(), source: String::new(), recommended: build("cuda-13.5", "llama-{build}-bin-win-cuda-13.5-x64.zip"), others: vec![] };
        assert_eq!(offers(&windows, "b12359", &files)[0].extra[0].name, "cudart-llama-bin-win-cuda-13.5-x64.zip");
    }

    #[test]
    fn a_build_tag_is_only_what_a_tag_looks_like() {
        assert!(plain("b12359") && plain("v1.0.0"));
        for bad in ["", "../etc", "b1 2", "b12359/assets", "b12359?x=1", &"b".repeat(41)] {
            assert!(!plain(bad), "{bad:?} was accepted");
        }
    }
}
