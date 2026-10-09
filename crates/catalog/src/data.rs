//! Typed mirror of `catalog/runtimes.json` and `catalog/models.json`.

use std::collections::BTreeMap;

use demido_core::{Arch, Backend, Os};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub runtimes: RuntimeCatalog,
    pub models: ModelCatalog,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCatalog {
    pub llama_cpp: LlamaCpp,
    pub node: ToolDist,
    pub uv: ToolDist,
    pub python: PythonSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaCpp {
    pub release: String,
    pub label: String,
    pub base_url: String,
    pub variants: Vec<RuntimeVariant>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVariant {
    pub id: String,
    pub backend: Backend,
    pub os: Os,
    pub arch: Arch,
    pub label: String,
    #[serde(default)]
    pub requires: Requirements,
    pub assets: Vec<Asset>,
}

impl RuntimeVariant {
    pub fn download_size(&self) -> u64 {
        self.assets.iter().map(|a| a.size).sum()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    pub nvidia_driver_min: Option<String>,
    pub compute_capability_min: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

impl LlamaCpp {
    pub fn asset_url(&self, asset: &Asset) -> String {
        format!("{}{}", self.base_url, asset.name)
    }

    pub fn variant(&self, id: &str) -> Option<&RuntimeVariant> {
        self.variants.iter().find(|v| v.id == id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDist {
    pub version: String,
    pub assets: Vec<ToolAsset>,
}

impl ToolDist {
    /// The build for this machine, if the tool ships one.
    pub fn for_platform(&self, os: Os, arch: Arch) -> Option<&ToolAsset> {
        self.assets.iter().find(|a| a.os == os && a.arch == arch)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAsset {
    pub os: Os,
    pub arch: Arch,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonSpec {
    pub version: String,
    pub estimated_size: u64,
    pub packages: Vec<String>,
    pub estimated_packages_size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub families: Vec<Family>,
    pub tiers: Vec<Tier>,
    pub cpu_tiers: Vec<CpuTier>,
    pub smoke_test: ModelPick,
    pub search: SearchCatalog,
}

/// Embedding models for searching attached files by meaning.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCatalog {
    /// Largest first: the first whose `min_vram_gb` a GPU's memory reaches is used; a CPU-only
    /// machine uses the last.
    pub models: Vec<SearchModel>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchModel {
    /// Stable name the vectors made with this model are stored under.
    pub id: String,
    pub name: String,
    pub min_vram_gb: f64,
    pub repo: String,
    pub file: String,
    pub quant: String,
    pub size: u64,
    pub sha256: String,
    /// Put before a question (the model card's prompt for search queries).
    pub query_prefix: String,
    /// Put before a passage.
    pub document_prefix: String,
    /// Similarity below which a passage counts as unrelated to a question.
    pub relevance: f32,
    /// Tokens llama.cpp decodes at once (`--ubatch-size`): the whole input for a model that
    /// reads both ways, less for one that reads left to right (see `catalog/models.json`).
    pub micro_batch: u32,
    /// GPU memory the model's server takes with the app's settings, in MiB (measured): a chat
    /// model leaves this much free for it.
    #[serde(default)]
    pub gpu_memory_mb: u32,
}

impl SearchModel {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/main/{}", self.repo, self.file)
    }
}

impl SearchCatalog {
    /// The model for a machine: by GPU memory (`gpu_memory_gb`, None without a usable GPU).
    pub fn for_memory(&self, gpu_memory_gb: Option<f64>) -> &SearchModel {
        gpu_memory_gb
            .and_then(|gb| self.models.iter().find(|m| gb >= m.min_vram_gb))
            .or_else(|| self.models.last())
            .expect("the catalog has search models")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    pub id: String,
    pub label: String,
    pub vendor: String,
    pub description: String,
    pub recommended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tier {
    pub id: String,
    pub min_vram_gb: f64,
    pub context_length: u32,
    pub models: BTreeMap<String, ModelPick>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTier {
    pub id: String,
    pub min_ram_gb: f64,
    pub context_length: u32,
    pub models: BTreeMap<String, ModelPick>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPick {
    pub name: String,
    pub repo: String,
    pub file: String,
    pub quant: String,
    pub size: u64,
    pub sha256: String,
}

impl ModelPick {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/main/{}", self.repo, self.file)
    }
}
