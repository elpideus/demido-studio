//! Browsing Hugging Face for GGUF models: the search the Models window lists, and a repository's
//! files and facts for its detail pane. unsloth's quantizations are ranked first when the person
//! sorts by what Demido recommends.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::capabilities::Capabilities;

const API: &str = "https://huggingface.co/api";
pub const PREFERRED_AUTHOR: &str = "unsloth";

/// What the search and a repository's page read of each model, beyond its files.
const EXPAND: [&str; 9] = [
    "author",
    "downloads",
    "likes",
    "lastModified",
    "gated",
    "gguf",
    "pipeline_tag",
    "tags",
    "cardData",
];

/// Results a search lists.
const SEARCH_LIMIT: &str = "80";

/// How a search is ordered.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SearchSort {
    /// Most downloaded, with unsloth's first.
    #[default]
    Recommended,
    Downloads,
    Likes,
    /// Most recently updated.
    Updated,
    /// What Hugging Face says is popular this week.
    Trending,
}

impl SearchSort {
    fn api(self) -> &'static str {
        match self {
            SearchSort::Recommended | SearchSort::Downloads => "downloads",
            SearchSort::Likes => "likes",
            SearchSort::Updated => "lastModified",
            SearchSort::Trending => "trendingScore",
        }
    }
}

/// A model repository as the search lists it, with what its GGUF header and model card state.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfRepo {
    pub id: String,
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
    pub preferred: bool,
    /// Needs an account that accepted the publisher's terms.
    pub gated: bool,
    /// Parameters, from the GGUF header.
    pub parameters: Option<u64>,
    pub architecture: Option<String>,
    /// The context it was trained for, in tokens.
    pub context_length: Option<u64>,
    pub license: Option<String>,
    /// Hugging Face's task, e.g. `image-text-to-text`.
    pub pipeline: Option<String>,
    /// The model it was made from, when the card names one.
    pub base_model: Option<String>,
    /// As the publisher states them: the chat template, the task and the tags.
    pub capabilities: Capabilities,
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
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfRepoFiles {
    pub repo: String,
    pub gated: bool,
    pub files: Vec<HfModelFile>,
    /// What lets the repo's models see pictures or hear sound, downloaded with any of them.
    pub projector: Option<HfProjector>,
    /// The repository's facts, read again with its files.
    pub info: HfRepo,
}

/// A repo's `mmproj` file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HfProjector {
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
}

fn with_token(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token {
        Some(t) if !t.is_empty() => req.bearer_auth(t),
        _ => req,
    }
}

fn expand() -> Vec<(&'static str, &'static str)> {
    EXPAND.iter().map(|e| ("expand[]", *e)).collect()
}

pub async fn search(
    http: &reqwest::Client,
    query: &str,
    sort: SearchSort,
    token: Option<&str>,
) -> anyhow::Result<Vec<HfRepo>> {
    let mut params = vec![
        ("filter", "gguf"),
        ("sort", sort.api()),
        ("direction", "-1"),
        ("limit", SEARCH_LIMIT),
    ];
    if !query.is_empty() {
        params.push(("search", query));
    }
    params.extend(expand());
    let req = http.get(format!("{API}/models")).query(&params);
    let resp = with_token(req, token).send().await?.error_for_status()?;
    let list: Vec<Value> = resp.json().await?;
    let mut repos: Vec<HfRepo> = list.iter().filter_map(repo_info).collect();
    if sort == SearchSort::Recommended {
        // Stable sort: unsloth first, popularity order otherwise.
        repos.sort_by_key(|r| !r.preferred);
    }
    Ok(repos)
}

/// A repository as the API describes it, in a search result or on its own.
fn repo_info(m: &Value) -> Option<HfRepo> {
    let id = m["id"].as_str()?.to_string();
    let author = m["author"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| id.split('/').next().unwrap_or_default().to_string());
    let tags: Vec<&str> = m["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let card = &m["cardData"];
    let gguf = &m["gguf"];
    let pipeline = m["pipeline_tag"]
        .as_str()
        .or_else(|| card["pipeline_tag"].as_str())
        .map(str::to_string);
    let license = card["license"]
        .as_str()
        .map(str::to_string)
        .or_else(|| tags.iter().find_map(|t| t.strip_prefix("license:")).map(str::to_string))
        .filter(|l| !l.is_empty());
    let base_model = match &card["base_model"] {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    Some(HfRepo {
        preferred: author.eq_ignore_ascii_case(PREFERRED_AUTHOR),
        downloads: m["downloads"].as_u64().unwrap_or(0),
        likes: m["likes"].as_u64().unwrap_or(0),
        last_modified: m["lastModified"].as_str().map(str::to_string),
        gated: !matches!(m["gated"], Value::Bool(false) | Value::Null),
        parameters: gguf["total"].as_u64().filter(|p| *p > 0),
        architecture: gguf["architecture"].as_str().map(str::to_string),
        context_length: gguf["context_length"].as_u64().filter(|c| *c > 0),
        capabilities: stated_capabilities(gguf["chat_template"].as_str(), pipeline.as_deref(), &tags),
        license,
        pipeline,
        base_model,
        author,
        id,
    })
}

/// What a repository says its model can do. A chat template that mentions tools takes them; one
/// that opens a thinking block thinks. Seeing and hearing come from the task and the tags, and the
/// template's image or audio markers. Without a template, tools and thinking are unknown.
fn stated_capabilities(template: Option<&str>, pipeline: Option<&str>, tags: &[&str]) -> Capabilities {
    let template = template.map(str::to_ascii_lowercase);
    let has = |needles: &[&str]| template.as_deref().map(|t| needles.iter().any(|n| t.contains(n)));
    let tagged = |names: &[&str]| tags.iter().any(|t| names.contains(&t.to_ascii_lowercase().as_str()));
    let task = |names: &[&str]| pipeline.is_some_and(|p| names.contains(&p));
    let sees = task(&["image-text-to-text", "any-to-any", "visual-question-answering"])
        || tagged(&["vision", "image-text-to-text", "multimodal"])
        || has(&[
            "<start_of_image>",
            "<|image_pad|>",
            "<|vision_start|>",
            "[img]",
            "<image>",
        ]) == Some(true);
    let hears = task(&["audio-text-to-text", "any-to-any"])
        || tagged(&["audio", "audio-text-to-text"])
        || has(&["<|audio_bos|>", "<start_of_audio>", "<|audio|>"]) == Some(true);
    Capabilities {
        vision: Some(sees),
        audio: Some(hears),
        tools: has(&["tools", "tool_call", "function_call"]),
        thinking: has(&[
            "<think>",
            "enable_thinking",
            "reasoning_content",
            "reasoning_effort",
            "thinking",
        ]),
    }
}

pub async fn repo_files(http: &reqwest::Client, repo: &str, token: Option<&str>) -> anyhow::Result<HfRepoFiles> {
    let meta = async {
        with_token(http.get(format!("{API}/models/{repo}")).query(&expand()), token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    let tree = async {
        with_token(
            http.get(format!("{API}/models/{repo}/tree/main"))
                .query(&[("recursive", "true")]),
            token,
        )
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Value>>()
        .await
    };
    let (meta, tree) = tokio::join!(meta, tree);
    let (meta, tree) = (meta?, tree?);
    let mut info = repo_info(&meta).unwrap_or_else(|| HfRepo {
        id: repo.to_string(),
        ..HfRepo::default()
    });

    let split = regex::Regex::new(r"^(.*)-(\d{5})-of-(\d{5})\.gguf$").expect("valid regex");
    let mut groups: std::collections::BTreeMap<String, Vec<(String, u64, Option<String>)>> = Default::default();
    let mut projectors: Vec<HfProjector> = Vec::new();
    for entry in &tree {
        if entry["type"] != "file" {
            continue;
        }
        let Some(path) = entry["path"].as_str() else {
            continue;
        };
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let lower = file_name.to_ascii_lowercase();
        let size = entry["lfs"]["size"]
            .as_u64()
            .or_else(|| entry["size"].as_u64())
            .unwrap_or(0);
        let sha = entry["lfs"]["oid"].as_str().map(str::to_string);
        if lower.ends_with(".gguf") && lower.contains("mmproj") {
            projectors.push(HfProjector {
                path: path.to_string(),
                size,
                sha256: sha,
            });
            continue;
        }
        if !lower.ends_with(".gguf")
            || lower.starts_with("mtp-")
            || lower.starts_with("dflash-")
            || lower.contains("imatrix")
        {
            continue;
        }
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
                name,
                paths: parts.iter().map(|p| p.0.clone()).collect(),
                sizes: parts.iter().map(|p| p.1).collect(),
                sha256: parts.iter().map(|p| p.2.clone()).collect(),
                size,
            }
        })
        .collect();
    files.sort_by_key(|f| f.size);

    let projector = preferred_projector(projectors, |p| p.path.rsplit('/').next().unwrap_or(&p.path));
    // A projector is how a GGUF sees (or hears, for a model whose task says so).
    if projector.is_some() && info.capabilities.audio != Some(true) {
        info.capabilities.vision = Some(true);
    }
    Ok(HfRepoFiles {
        repo: repo.to_string(),
        gated: info.gated,
        files,
        projector,
        info,
    })
}

/// A repository's model card (its `README.md`) as Markdown, without the YAML front matter the Hub
/// reads its tags from. None when it has none, or it could not be read: the card is a nicety.
pub async fn model_card(http: &reqwest::Client, repo: &str, token: Option<&str>) -> Option<String> {
    let resp = with_token(http.get(file_url(repo, "README.md")), token)
        .send()
        .await
        .ok()?;
    let text = resp.error_for_status().ok()?.text().await.ok()?;
    let body = without_front_matter(&text).trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// A card's text after its front matter, the block between two `---` lines that opens it.
fn without_front_matter(readme: &str) -> &str {
    let Some(rest) = readme.trim_start_matches('\u{feff}').strip_prefix("---") else {
        return readme;
    };
    match rest.find("\n---") {
        Some(end) => rest[end + 4..]
            .trim_start_matches(['-'])
            .trim_start_matches(['\r', '\n']),
        None => readme,
    }
}

/// The projector to use of several: BF16 first, the precision these models were trained in (see
/// `catalog/models.json`), then F16, F32, and quantized ones last. Ties go to the shorter name.
pub fn preferred_projector<T>(candidates: Vec<T>, name: impl Fn(&T) -> &str) -> Option<T> {
    fn rank(name: &str) -> u8 {
        let lower = name.to_ascii_lowercase();
        if lower.contains("bf16") {
            0
        } else if lower.contains("f16") {
            1
        } else if lower.contains("f32") {
            2
        } else {
            3
        }
    }
    candidates.into_iter().min_by(|a, b| {
        let (a, b) = (name(a), name(b));
        (rank(a), a.len(), a).cmp(&(rank(b), b.len(), b))
    })
}

/// Download URL of a repository file.
pub fn file_url(repo: &str, path: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bf16_projector_is_chosen_over_others() {
        let pick = |names: &[&str]| preferred_projector(names.to_vec(), |n| *n).map(str::to_string);
        assert_eq!(
            pick(&["mmproj-F16.gguf", "mmproj-F32.gguf", "mmproj-BF16.gguf"]).as_deref(),
            Some("mmproj-BF16.gguf")
        );
        assert_eq!(
            pick(&[
                "mmproj-model-Q8_0.gguf",
                "mmproj-model-f32.gguf",
                "mmproj-model-f16.gguf"
            ])
            .as_deref(),
            Some("mmproj-model-f16.gguf")
        );
        assert_eq!(pick(&["x.mmproj-Q8_0.gguf"]).as_deref(), Some("x.mmproj-Q8_0.gguf"));
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn a_model_card_loses_only_its_front_matter() {
        let card =
            "---\nlicense: apache-2.0\ntags:\n- gguf\n---\n\n# Qwen3-8B\n\nText --- with a rule.\n\n---\n\nMore.";
        assert_eq!(
            without_front_matter(card),
            "# Qwen3-8B\n\nText --- with a rule.\n\n---\n\nMore."
        );
        assert_eq!(without_front_matter("# No front matter"), "# No front matter");
        assert_eq!(without_front_matter("---\nnever closed"), "---\nnever closed");
    }

    #[test]
    fn capabilities_are_read_from_the_template_task_and_tags() {
        let caps = stated_capabilities(Some("{% if tools %}...<think>"), Some("image-text-to-text"), &["gguf"]);
        assert_eq!(caps.tools, Some(true));
        assert_eq!(caps.thinking, Some(true));
        assert_eq!(caps.vision, Some(true));
        assert_eq!(caps.audio, Some(false));
        let plain = stated_capabilities(None, Some("text-generation"), &["audio"]);
        assert_eq!(plain.tools, None);
        assert_eq!(plain.thinking, None);
        assert_eq!(plain.vision, Some(false));
        assert_eq!(plain.audio, Some(true));
    }

    #[test]
    fn a_search_result_reads_the_gguf_header_and_the_card() {
        let m = serde_json::json!({
            "id": "unsloth/Qwen3-8B-GGUF",
            "downloads": 10, "likes": 2, "gated": "auto",
            "gguf": { "total": 8190735360u64, "architecture": "qwen3", "context_length": 40960, "chat_template": "tools" },
            "cardData": { "base_model": ["Qwen/Qwen3-8B"] },
            "tags": ["gguf", "license:apache-2.0"]
        });
        let r = repo_info(&m).unwrap();
        assert_eq!(r.author, "unsloth");
        assert!(r.preferred && r.gated);
        assert_eq!(r.parameters, Some(8190735360));
        assert_eq!(r.context_length, Some(40960));
        assert_eq!(r.license.as_deref(), Some("apache-2.0"));
        assert_eq!(r.base_model.as_deref(), Some("Qwen/Qwen3-8B"));
        assert_eq!(r.capabilities.tools, Some(true));
    }
}
