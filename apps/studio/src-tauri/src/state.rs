//! Everything the app keeps alive, built once at startup.

use std::sync::Arc;

use demido_hardware::HardwareReport;
use tauri::AppHandle;

use crate::agent::Agent;
use crate::attachments::meaning::Embedder;
use crate::db::Db;
use crate::mail::MailService;
use crate::market::MarketService;
use crate::models::ModelRegistry;
use crate::models::downloads::DownloadManager;
use crate::paths::AppPaths;
use crate::providers::ProviderStore;
use crate::runtime::LocalRuntime;
use crate::secrets::Secrets;
use crate::settings::SettingsStore;
use crate::skills::SkillRegistry;
use crate::speech::Transcriber;
use crate::updater::Updater;

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
    /// The search model, which indexes attached files by meaning.
    pub embedder: Arc<Embedder>,
    /// The speech model, which writes down what the person says for models that cannot hear.
    pub speech: Arc<Transcriber>,
    pub skills: Arc<SkillRegistry>,
    pub market: Arc<MarketService>,
    pub mail: Arc<MailService>,
    pub updater: Arc<Updater>,
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

    /// Whether llama.cpp sizes a local model's context to what the GPU has free when it loads
    /// (`--fit`): with a GPU runtime on a GPU with memory of its own. llama.cpp fits only GPU
    /// memory, so on the CPU, or a GPU sharing system memory, it would take the whole trained
    /// context instead.
    pub fn context_fits_gpu(hardware: &HardwareReport, backend: Option<demido_core::Backend>) -> bool {
        let catalog = demido_catalog::catalog();
        let choices = demido_catalog::backend_choices(hardware, catalog);
        let backend = backend.unwrap_or_else(|| demido_catalog::default_backend(&choices));
        choices
            .iter()
            .find(|c| c.backend == backend)
            .is_some_and(|c| c.backend.is_gpu() && !c.uses_system_memory)
    }
}
