//! Hugging Face GGUF discovery + download.
//!
//! Phase 1: user pastes a repo URL, we list the GGUF quants available in that repo
//! (with sizes), they pick one, we stream-download it into app-data. A quant may be
//! split across multiple part files (`...-00001-of-00003.gguf`) — those are grouped
//! and all downloaded together.

use crate::db::local_models::LocalModel;
use futures_util::StreamExt;
use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// One selectable quantization for a repo, e.g. `Q4_K_M`, possibly multi-part.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuantOption {
    pub quant: String,
    /// Relative file paths within the repo, in part order.
    pub files: Vec<String>,
    /// Total bytes across all parts.
    pub size: i64,
}

/// A vision-projector (`mmproj-*.gguf`): its basename starts with `mmproj`. Not a runnable
/// model — llama-server loads it alongside the real model via `--mmproj` to enable vision,
/// so it's kept out of the quant list and fetched automatically with whichever quant.
fn is_mmproj(filename: &str) -> bool {
    filename.rsplit('/').next().unwrap_or(filename).to_lowercase().starts_with("mmproj")
}

/// A Hugging Face model repo, for the trending/search browser.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HfModel {
    pub id: String,
    pub downloads: i64,
    pub likes: i64,
    pub updated: String,
    pub pipeline_tag: Option<String>,
    pub gated: bool,
    /// HF tags — the UI derives capability chips (vision/tools/reasoning) from these.
    pub tags: Vec<String>,
}

fn parse_models(v: &serde_json::Value) -> Vec<HfModel> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| HfModel {
                    id: m.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    downloads: m.get("downloads").and_then(|x| x.as_i64()).unwrap_or(0),
                    likes: m.get("likes").and_then(|x| x.as_i64()).unwrap_or(0),
                    updated: m
                        .get("lastModified")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    pipeline_tag: m
                        .get("pipeline_tag")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    gated: !matches!(m.get("gated"), Some(serde_json::Value::Bool(false)) | None),
                    tags: m
                        .get("tags")
                        .and_then(|t| t.as_array())
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
                .filter(|m| !m.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

async fn query_models(client: &reqwest::Client, query: &str) -> Result<Vec<HfModel>, String> {
    let url = format!("https://huggingface.co/api/models?{}", query);
    let resp = client
        .get(&url)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("HF request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HF API returned HTTP {}", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("Bad HF response: {}", e))?;
    Ok(parse_models(&v))
}

/// Trending GGUF-bearing models (the default browse list).
pub async fn trending_models(client: &reqwest::Client) -> Result<Vec<HfModel>, String> {
    query_models(client, "filter=gguf&sort=trendingScore&direction=-1&limit=40").await
}

/// Search GGUF-bearing models by name.
pub async fn search_models(client: &reqwest::Client, q: &str) -> Result<Vec<HfModel>, String> {
    let enc = q.replace(' ', "+");
    query_models(
        client,
        &format!("search={}&filter=gguf&sort=downloads&direction=-1&limit=40", enc),
    )
    .await
}

/// Fetch a repo's model card (README markdown), stripping YAML frontmatter.
pub async fn model_card(client: &reqwest::Client, repo: &str) -> Result<String, String> {
    let url = format!("https://huggingface.co/{}/raw/main/README.md", repo);
    let resp = client
        .get(&url)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("HF request failed: {}", e))?;
    if !resp.status().is_success() {
        return Ok(String::new()); // no card is fine
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    // Strip leading `---\n...\n---` frontmatter block.
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            return Ok(rest[end + 4..].trim_start().to_string());
        }
    }
    Ok(text)
}

/// Best-effort capability read straight from Hugging Face — used when models.dev hasn't
/// indexed this repo (small / new / community GGUF repackagers). GGUF repackage repos don't
/// carry `config.json`/`tokenizer_config.json` (those belong to the original safetensors
/// repo), but the model-info API extracts a `gguf.chat_template` straight from the GGUF's own
/// metadata — same jinja heuristic `caps_from_props` runs against a live llama-server probe.
/// `pipeline_tag`/`tags` cover vision, tool-calling and reasoning the repo advertises.
pub async fn caps_from_repo(client: &reqwest::Client, repo: &str) -> crate::caps::PartialCaps {
    let url = format!("https://huggingface.co/api/models/{}", repo);
    let Ok(resp) = client.get(&url).header("User-Agent", "DemidoStudio").send().await else {
        return crate::caps::PartialCaps::default();
    };
    if !resp.status().is_success() {
        return crate::caps::PartialCaps::default();
    }
    let Ok(info) = resp.json::<serde_json::Value>().await else {
        return crate::caps::PartialCaps::default();
    };

    let tags: Vec<String> = info["tags"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_lowercase())).collect())
        .unwrap_or_default();
    let has_tag = |t: &str| tags.iter().any(|x| x == t);
    let pipeline = info["pipeline_tag"].as_str().unwrap_or("").to_lowercase();
    let template = info["gguf"]["chat_template"].as_str();

    let vision = pipeline == "image-text-to-text" || has_tag("vision") || has_tag("multimodal");
    let tools = has_tag("function-calling")
        || has_tag("tool-calling")
        || template.is_some_and(|t| t.contains("tool_call") || t.contains(".tools"));
    let reasoning = has_tag("reasoning")
        || template.is_some_and(|t| {
            t.contains("enable_thinking") || t.contains("<think>") || t.contains("reasoning_content")
        });

    crate::caps::PartialCaps {
        vision: Some(vision),
        tools: Some(tools),
        reasoning: Some(reasoning),
    }
}

/// Parse `owner/name` out of a Hugging Face model URL or a bare `owner/name`.
pub fn parse_repo(input: &str) -> Result<String, String> {
    let s = input.trim().trim_end_matches('/');
    // Full URL form: https://huggingface.co/<owner>/<name>[/...]
    if let Some(idx) = s.find("huggingface.co/") {
        let rest = &s[idx + "huggingface.co/".len()..];
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            return Ok(format!("{}/{}", parts[0], parts[1]));
        }
        return Err("URL missing owner/name".into());
    }
    // Bare owner/name
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() == 2 {
        return Ok(format!("{}/{}", parts[0], parts[1]));
    }
    Err("Expected a huggingface.co model URL or 'owner/name'".into())
}

/// Extract the quant tag (and strip any multi-part suffix) from a gguf filename.
/// Returns None for non-gguf files. `foo-Q4_K_M.gguf` -> `Q4_K_M`,
/// `foo-Q4_K_M-00001-of-00002.gguf` -> `Q4_K_M`.
fn quant_of(filename: &str) -> Option<String> {
    // ponytail: one regex over the filename; covers Q*/IQ*/F16/F32/BF16 + split suffix.
    let re = Regex::new(
        r"(?i)[.-]((?:IQ|Q)\d[A-Z0-9_]*|F16|F32|BF16)(?:-\d+-of-\d+)?\.gguf$",
    )
    .ok()?;
    let caps = re.captures(filename)?;
    Some(caps.get(1)?.as_str().to_uppercase())
}

/// Fetch all `.gguf` files in a repo as (path, size) via the HF tree API.
async fn fetch_gguf_tree(
    client: &reqwest::Client,
    repo: &str,
) -> Result<Vec<(String, i64)>, String> {
    let url = format!(
        "https://huggingface.co/api/models/{}/tree/main?recursive=true",
        repo
    );
    let resp = client
        .get(&url)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("HF request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HF API returned HTTP {}", resp.status().as_u16()));
    }
    let entries: Vec<serde_json::Value> =
        resp.json().await.map_err(|e| format!("Bad HF response: {}", e))?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let path = e.get("path").and_then(|v| v.as_str())?.to_string();
            if !path.to_lowercase().ends_with(".gguf") {
                return None;
            }
            // LFS size lives under lfs.size; plain size otherwise.
            let size = e
                .get("lfs")
                .and_then(|l| l.get("size"))
                .and_then(|v| v.as_i64())
                .or_else(|| e.get("size").and_then(|v| v.as_i64()))
                .unwrap_or(0);
            Some((path, size))
        })
        .collect())
}

/// Group non-mmproj gguf files by quant, summing sizes across multi-part files.
fn group_quants(files: &[(String, i64)]) -> Vec<QuantOption> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, (Vec<String>, i64)> = BTreeMap::new();
    for (path, size) in files {
        if is_mmproj(path) {
            continue;
        }
        let Some(q) = quant_of(path) else { continue };
        let entry = groups.entry(q).or_insert_with(|| (Vec::new(), 0));
        entry.0.push(path.clone());
        entry.1 += size;
    }
    let mut out: Vec<QuantOption> = groups
        .into_iter()
        .map(|(quant, (mut files, size))| {
            files.sort();
            QuantOption { quant, files, size }
        })
        .collect();
    out.sort_by_key(|q| q.size);
    out
}

/// The repo's mmproj file (path, size), if it ships one. Prefers f16 over other precisions.
fn find_mmproj(files: &[(String, i64)]) -> Option<(String, i64)> {
    let mut projs: Vec<&(String, i64)> = files.iter().filter(|(p, _)| is_mmproj(p)).collect();
    projs.sort_by_key(|(p, _)| !p.to_lowercase().contains("f16")); // f16 first
    projs.first().map(|(p, s)| (p.clone(), *s))
}

/// List quants in a repo (excluding mmproj projectors).
pub async fn list_quants(
    client: &reqwest::Client,
    repo: &str,
) -> Result<Vec<QuantOption>, String> {
    let files = fetch_gguf_tree(client, repo).await?;
    let quants = group_quants(&files);
    if quants.is_empty() {
        return Err("No .gguf model files found in that repo".into());
    }
    Ok(quants)
}

/// Both the quants and the repo's mmproj (single tree fetch), for the download flow.
pub async fn quants_and_mmproj(
    client: &reqwest::Client,
    repo: &str,
) -> Result<(Vec<QuantOption>, Option<(String, i64)>), String> {
    let files = fetch_gguf_tree(client, repo).await?;
    let quants = group_quants(&files);
    if quants.is_empty() {
        return Err("No .gguf model files found in that repo".into());
    }
    Ok((quants, find_mmproj(&files)))
}

/// Scan a models directory laid out `<owner>/<name>/*.gguf` (LM-Studio style) and derive
/// the local models present on disk, grouping multi-part files and attaching any mmproj.
/// Used to detect manually-added models and models under a user-chosen folder.
pub fn scan_models_dir(base: &Path) -> Vec<LocalModel> {
    let mut out = Vec::new();
    let Ok(owners) = std::fs::read_dir(base) else { return out };
    for owner_e in owners.flatten() {
        if !owner_e.path().is_dir() {
            continue;
        }
        let owner = owner_e.file_name().to_string_lossy().to_string();
        let Ok(names) = std::fs::read_dir(owner_e.path()) else { continue };
        for name_e in names.flatten() {
            let repo_dir = name_e.path();
            if !repo_dir.is_dir() {
                continue;
            }
            let name = name_e.file_name().to_string_lossy().to_string();
            let repo = format!("{}/{}", owner, name);

            // Collect the repo dir's gguf files as (filename, size).
            let mut files: Vec<(String, i64)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&repo_dir) {
                for f in entries.flatten() {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if !fname.to_lowercase().ends_with(".gguf") {
                        continue;
                    }
                    let size = f.metadata().map(|m| m.len() as i64).unwrap_or(0);
                    files.push((fname, size));
                }
            }
            if files.is_empty() {
                continue;
            }

            let mmproj = find_mmproj(&files)
                .map(|(f, _)| repo_dir.join(f).to_string_lossy().to_string());
            for q in group_quants(&files) {
                out.push(LocalModel {
                    id: format!("{}::{}", repo, q.quant),
                    repo: repo.clone(),
                    file_path: repo_dir.join(&q.files[0]).to_string_lossy().to_string(),
                    quant: q.quant,
                    size: q.size,
                    mmproj_path: mmproj.clone(),
                    // Probed from llama-server /props once the model is actually loaded.
                    caps_vision: None,
                    caps_tools: None,
                    caps_reasoning: None,
                });
            }
        }
    }
    out
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    id: String,
    downloaded: i64,
    total: i64,
}

/// Download every part file of a quant into `dest_dir`, emitting `local_download_progress`
/// events keyed by `id`. Returns the primary (first) file's path and the total bytes.
/// Simple resume: if a part already exists at full expected size it is skipped.
pub async fn download_quant(
    app: &AppHandle,
    client: &reqwest::Client,
    repo: &str,
    files: &[String],
    total: i64,
    dest_dir: &Path,
    id: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let mut first_path: Option<PathBuf> = None;
    let mut downloaded_all: i64 = 0;

    for rel in files {
        let filename = rel.rsplit('/').next().unwrap_or(rel);
        let dest = dest_dir.join(filename);
        if first_path.is_none() {
            first_path = Some(dest.clone());
        }

        let url = format!("https://huggingface.co/{}/resolve/main/{}", repo, rel);
        let resp = client
            .get(&url)
            .header("User-Agent", "DemidoStudio")
            .send()
            .await
            .map_err(|e| format!("Download request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Download HTTP {} for {}", resp.status().as_u16(), filename));
        }

        let tmp = dest.with_extension("gguf.part");
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut stream = resp.bytes_stream();
        use std::io::Write;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            downloaded_all += chunk.len() as i64;
            let _ = app.emit(
                "local_download_progress",
                DownloadProgress { id: id.to_string(), downloaded: downloaded_all, total },
            );
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    }

    first_path.ok_or_else(|| "No files to download".into())
}
