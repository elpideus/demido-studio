//! Browsing Hugging Face for GGUF models. unsloth's quantizations are ranked first.

use serde::Serialize;
use serde_json::Value;

const API: &str = "https://huggingface.co/api";
pub const PREFERRED_AUTHOR: &str = "unsloth";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfRepo {
    pub id: String,
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
    pub preferred: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Fits in GPU memory with room for context.
    Fits,
    /// Fits, but leaves little room for context.
    Tight,
    /// Larger than GPU memory: runs partly on the CPU, slowly.
    Large,
    Unknown,
}

/// One downloadable model (a single GGUF, or all parts of a split one).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModelFile {
    pub name: String,
    /// Repository paths, first part first.
    pub paths: Vec<String>,
    pub sizes: Vec<u64>,
    pub sha256: Vec<Option<String>>,
    pub size: u64,
    pub quant: Option<String>,
    pub fit: Fit,
    pub recommended: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfRepoFiles {
    pub repo: String,
    pub gated: bool,
    pub files: Vec<HfModelFile>,
}

fn with_token(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(t) if !t.is_empty() => req.bearer_auth(t),
        _ => req,
    }
}

pub async fn search(http: &reqwest::Client, query: &str, token: Option<&str>) -> anyhow::Result<Vec<HfRepo>> {
    let req = http.get(format!("{API}/models")).query(&[
        ("search", query),
        ("filter", "gguf"),
        ("sort", "downloads"),
        ("direction", "-1"),
        ("limit", "60"),
    ]);
    let resp = with_token(req, token).send().await?.error_for_status()?;
    let list: Vec<Value> = resp.json().await?;
    let mut repos: Vec<HfRepo> = list
        .into_iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let author = id.split('/').next().unwrap_or_default().to_string();
            Some(HfRepo {
                preferred: author.eq_ignore_ascii_case(PREFERRED_AUTHOR),
                downloads: m["downloads"].as_u64().unwrap_or(0),
                likes: m["likes"].as_u64().unwrap_or(0),
                last_modified: m["lastModified"].as_str().map(str::to_string),
                author,
                id,
            })
        })
        .collect();
    // Stable sort: unsloth first, popularity order otherwise.
    repos.sort_by_key(|r| !r.preferred);
    Ok(repos)
}

pub async fn repo_files(
    http: &reqwest::Client,
    repo: &str,
    token: Option<&str>,
    vram_gb: Option<f64>,
) -> anyhow::Result<HfRepoFiles> {
    let meta: Value = with_token(http.get(format!("{API}/models/{repo}")), token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let gated = !matches!(meta["gated"], Value::Bool(false) | Value::Null);

    let tree: Vec<Value> = with_token(
        http.get(format!("{API}/models/{repo}/tree/main"))
            .query(&[("recursive", "true")]),
        token,
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;

    let split = regex::Regex::new(r"^(.*)-(\d{5})-of-(\d{5})\.gguf$").expect("valid regex");
    let mut groups: std::collections::BTreeMap<String, Vec<(String, u64, Option<String>)>> = Default::default();
    for entry in &tree {
        if entry["type"] != "file" {
            continue;
        }
        let Some(path) = entry["path"].as_str() else {
            continue;
        };
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let lower = file_name.to_ascii_lowercase();
        if !lower.ends_with(".gguf")
            || lower.contains("mmproj")
            || lower.starts_with("mtp-")
            || lower.starts_with("dflash-")
            || lower.contains("imatrix")
        {
            continue;
        }
        let size = entry["lfs"]["size"]
            .as_u64()
            .or_else(|| entry["size"].as_u64())
            .unwrap_or(0);
        let sha = entry["lfs"]["oid"].as_str().map(str::to_string);
        let key = match split.captures(path) {
            Some(c) => format!("{}.gguf", &c[1]),
            None => path.to_string(),
        };
        groups.entry(key).or_default().push((path.to_string(), size, sha));
    }

    let mut files: Vec<HfModelFile> = groups
        .into_iter()
        .map(|(key, mut parts)| {
            parts.sort_by(|a, b| a.0.cmp(&b.0));
            let size = parts.iter().map(|p| p.1).sum();
            let name = key.rsplit('/').next().unwrap_or(&key).to_string();
            HfModelFile {
                quant: super::gguf::quant_label(&name, None),
                fit: fit(size, vram_gb),
                recommended: false,
                name,
                paths: parts.iter().map(|p| p.0.clone()).collect(),
                sizes: parts.iter().map(|p| p.1).collect(),
                sha256: parts.iter().map(|p| p.2.clone()).collect(),
                size,
            }
        })
        .collect();
    files.sort_by_key(|f| f.size);

    // Recommend the largest file that comfortably fits, preferring unsloth's dynamic quants.
    let best = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.fit == Fit::Fits)
        .max_by_key(|(_, f)| {
            let dynamic = f.quant.as_deref().is_some_and(|q| q.starts_with("UD-"));
            let not_huge = f
                .quant
                .as_deref()
                .is_none_or(|q| !q.contains("BF16") && !q.contains("F32"));
            (not_huge, f.size, dynamic)
        })
        .map(|(i, _)| i);
    if let Some(i) = best {
        files[i].recommended = true;
    }
    Ok(HfRepoFiles {
        repo: repo.to_string(),
        gated,
        files,
    })
}

fn fit(size: u64, vram_gb: Option<f64>) -> Fit {
    let Some(vram) = vram_gb.filter(|v| *v > 0.0) else {
        return Fit::Unknown;
    };
    let gb = size as f64 / (1u64 << 30) as f64;
    if gb <= vram * 0.8 {
        Fit::Fits
    } else if gb <= vram * 0.97 {
        Fit::Tight
    } else {
        Fit::Large
    }
}

/// Download URL of a repository file.
pub fn file_url(repo: &str, path: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_bands() {
        let gib = (1u64 << 30) as f64;
        assert_eq!(fit((8.0 * gib) as u64, Some(12.0)), Fit::Fits);
        assert_eq!(fit((11.0 * gib) as u64, Some(12.0)), Fit::Tight);
        assert_eq!(fit((13.0 * gib) as u64, Some(12.0)), Fit::Large);
        assert_eq!(fit(1, None), Fit::Unknown);
    }
}
