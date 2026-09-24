//! Everything the app keeps alive, built once at startup.

use std::sync::Arc;

use demido_hardware::HardwareReport;
use tauri::AppHandle;

use crate::agent::Agent;
use crate::db::Db;
use crate::market::MarketService;
use crate::models::ModelRegistry;
use crate::models::downloads::DownloadManager;
use crate::paths::AppPaths;
use crate::providers::ProviderStore;
use crate::runtime::LocalRuntime;
use crate::secrets::Secrets;
use crate::settings::SettingsStore;
use crate::skills::SkillRegistry;

pub struct AppState {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub hardware: HardwareReport,
    pub settings: Arc<SettingsStore>,
    pub db: Arc<Db>,
    pub secrets: Arc<Secrets>,
    pub providers: Arc<ProviderStore>,
    pub models: Arc<ModelRegistry>,
    pub downloads: Arc<DownloadManager>,
    pub runtime: Arc<LocalRuntime>,
    pub skills: Arc<SkillRegistry>,
    pub market: Arc<MarketService>,
    pub agent: Agent,
    /// Client for the internet (Hugging Face, Gemini).
    pub http: reqwest::Client,
    /// Client for the local model server: no proxy, no timeout (answers can be long).
    pub local_http: reqwest::Client,
}

impl AppState {
    /// GPU memory the models are sized against, in GiB (system RAM share on CPU-only machines).
    pub fn memory_budget_gb(hardware: &HardwareReport) -> f64 {
        match hardware.primary_gpu() {
            Some(gpu) if !gpu.integrated => gpu.vram_gb(),
            _ => hardware.total_memory_gb() * 0.5,
        }
    }
}
