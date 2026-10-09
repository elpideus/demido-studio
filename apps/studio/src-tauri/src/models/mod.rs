//! Every model the person can talk to: GGUF files on disk and the models of configured cloud
//! providers, merged with the person's own settings for each (name, picture, system prompt,
//! sampling, context length, enabled) and with what each can do ([`capabilities`]).

pub mod capabilities;
pub mod downloads;
pub mod gguf;
pub mod hf;
mod probe;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::{MutexGuard, Notify};
use tokio_util::sync::CancellationToken;

use crate::bail_msg;
use crate::error::CmdResult;
use crate::llm::GenParams;
use crate::paths::AppPaths;
use crate::providers::{ProviderKind, ProviderStore};
use crate::runtime::LaunchSpec;
use crate::settings::SettingsStore;
use capabilities::{Capabilities, CloudCatalog, LocalChecks, LocalKey};

pub const CHANGED_EVENT: &str = "models://changed";

/// What the person changed about a model. `None` means "use the default".
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSettings {
    pub name: Option<String>,
    pub description: Option<String>,
    /// File name inside the avatars folder.
    pub avatar: Option<String>,
    pub enabled: Option<bool>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
    pub context_length: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub thinking: Option<bool>,
    /// The tokens at which a chat with this model is compacted, instead of the threshold for
    /// every model (`Settings::auto_compact_tokens`). Never above what its window allows.
    pub auto_compact_tokens: Option<u32>,
}

/// The settings in force for a model: its own, or the family defaults.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Effective {
    pub system_prompt: String,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
    /// `None` for a local model: llama.cpp sizes it to the GPU's free memory as the model loads,
    /// and the runtime status says what it took.
    pub context_length: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub thinking: Option<bool>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    Local,
    Gemini,
    OpenRouter,
}

impl ModelSource {
    fn of(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Gemini => ModelSource::Gemini,
            ProviderKind::OpenRouter => ModelSource::OpenRouter,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub source: ModelSource,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub name: String,
    pub default_name: String,
    pub description: Option<String>,
    pub avatar_path: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub path: Option<String>,
    pub size: Option<u64>,
    pub quant: Option<String>,
    pub architecture: Option<String>,
    pub parameters: Option<String>,
    pub max_context: Option<u64>,
    pub repo: Option<String>,
    /// File name of the projector found next to a local model, which lets it see pictures or
    /// hear sound once llama.cpp says it does.
    pub projector: Option<String>,
    /// Lives in a folder Demido manages, so it can be deleted from the app.
    pub removable: bool,
    pub capabilities: Capabilities,
    /// llama.cpp has yet to say what this local model can do.
    pub checking_capabilities: bool,
    pub settings: ModelSettings,
    pub effective: Effective,
}

/// A GGUF model found on disk.
#[derive(Clone, Debug)]
struct LocalFile {
    id: String,
    path: PathBuf,
    /// Every file of a split model, first part first.
    parts: Vec<PathBuf>,
    root: PathBuf,
    size: u64,
    /// Seconds since the Unix epoch.
    modified: Option<u64>,
    info: gguf::GgufInfo,
    quant: Option<String>,
    repo: Option<String>,
    mmproj: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Default)]
struct ModelsFile {
    models: BTreeMap<String, ModelSettings>,
}

pub struct ModelRegistry {
    paths: AppPaths,
    settings: Arc<SettingsStore>,
    providers: Arc<ProviderStore>,
    file: PathBuf,
    overrides: RwLock<BTreeMap<String, ModelSettings>>,
    local: RwLock<Vec<LocalFile>>,
    /// VRAM of the primary GPU (or budget), for default context lengths.
    memory_budget_gb: f64,
    /// llama.cpp sizes a local model's context to the GPU (see `AppState::context_fits_gpu`).
    context_fits_gpu: bool,
    local_checks: RwLock<LocalChecks>,
    cloud_catalog: RwLock<CloudCatalog>,
    /// Wakes [`Self::check_capabilities`] after a rescan.
    rescanned: Notify,
    /// Held while llama.cpp is asked about a model, and by [`Self::pause_checks`].
    check_lock: tokio::sync::Mutex<()>,
    /// Stops the check running now.
    check_cancel: Mutex<Option<CancellationToken>>,
}

impl ModelRegistry {
    pub fn new(
        paths: AppPaths,
        settings: Arc<SettingsStore>,
        providers: Arc<ProviderStore>,
        memory_budget_gb: f64,
        context_fits_gpu: bool,
    ) -> Self {
        let file = paths.models_file.clone();
        let overrides = demido_core::fsx::read_json::<ModelsFile>(&file)
            .map(|f| f.models)
            .unwrap_or_default();
        let local_checks = LocalChecks::load(paths.cache_dir.join("capabilities.json"));
        let cloud_catalog = CloudCatalog::load(paths.cache_dir.join("models-dev.json"));
        Self {
            paths,
            settings,
            providers,
            file,
            overrides: RwLock::new(overrides),
            local: RwLock::new(Vec::new()),
            memory_budget_gb,
            context_fits_gpu,
            local_checks: RwLock::new(local_checks),
            cloud_catalog: RwLock::new(cloud_catalog),
            rescanned: Notify::new(),
            check_lock: tokio::sync::Mutex::new(()),
            check_cancel: Mutex::new(None),
        }
    }

    pub(crate) fn model_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.paths.builtin_model_dirs();
        for extra in self.settings.get().extra_model_dirs {
            if !dirs.contains(&extra) {
                dirs.push(extra);
            }
        }
        dirs
    }

    fn managed_dirs(&self) -> Vec<PathBuf> {
        self.paths.builtin_model_dirs()
    }

    /// Rescans every model folder. Cheap: only GGUF headers are read.
    pub fn rescan(&self) {
        let mut found: Vec<LocalFile> = Vec::new();
        for root in self.model_dirs() {
            if !root.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&root)
                .max_depth(5)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                let path = entry.path();
                if !entry.file_type().is_file() || !is_model_file(path) {
                    continue;
                }
                match scan_file(&root, path) {
                    // Embedding and speech recognition models (the search and speech models among
                    // them) cannot chat.
                    Some(file) if file.info.is_embedding() || file.info.is_speech_recognition() => {}
                    Some(file) if !found.iter().any(|f| f.id == file.id) => found.push(file),
                    _ => {}
                }
            }
        }
        found.sort_by(|a, b| a.id.cmp(&b.id));
        *self.local.write() = found;
        self.rescanned.notify_one();
    }

    pub fn list(&self) -> Vec<ModelEntry> {
        let default = self.settings.get().default_model;
        let overrides = self.overrides.read().clone();
        let runtime = self.runtime_build();
        let mut out: Vec<ModelEntry> = self
            .local
            .read()
            .iter()
            .map(|f| self.local_entry(f, overrides.get(&f.id).cloned().unwrap_or_default(), runtime.as_deref()))
            .collect();

        let catalog = self.cloud_catalog.read();
        for provider in self.providers.configs() {
            if !provider.enabled {
                continue;
            }
            let source = ModelSource::of(provider.kind);
            let defaults_on = match provider.kind {
                ProviderKind::Gemini => default_enabled_gemini(&provider.models),
                // Nothing that costs money is switched on without the person choosing it.
                ProviderKind::OpenRouter => provider
                    .models
                    .iter()
                    .filter(|m| m.free)
                    .map(|m| m.id.clone())
                    .collect(),
            };
            for m in &provider.models {
                let id = format!("{}:{}:{}", provider.kind.id_prefix(), provider.id, m.id);
                let s = overrides.get(&id).cloned().unwrap_or_default();
                let enabled = s.enabled.unwrap_or_else(|| defaults_on.contains(&m.id));
                out.push(ModelEntry {
                    effective: effective(&s, source, None, None, self.default_context()),
                    id,
                    source,
                    provider_id: Some(provider.id.clone()),
                    provider_name: Some(provider.name.clone()),
                    name: s.name.clone().unwrap_or_else(|| m.display_name.clone()),
                    default_name: m.display_name.clone(),
                    description: s
                        .description
                        .clone()
                        .or_else(|| Some(m.description.clone()).filter(|d| !d.is_empty())),
                    avatar_path: self.avatar_path(&s),
                    enabled,
                    is_default: false,
                    path: None,
                    size: None,
                    quant: None,
                    architecture: None,
                    parameters: None,
                    max_context: Some(m.input_token_limit),
                    repo: None,
                    projector: None,
                    removable: false,
                    capabilities: m.capabilities.unwrap_or_else(|| catalog.gemini(&m.id, m.thinking)),
                    checking_capabilities: false,
                    settings: s,
                });
            }
        }
        drop(catalog);

        let default_id = default
            .filter(|d| out.iter().any(|m| &m.id == d && m.enabled))
            .or_else(|| self.fallback_default(&out));
        for m in &mut out {
            m.is_default = Some(&m.id) == default_id.as_ref();
        }
        out
    }

    /// Without an explicit default: the starter model, else the first enabled local model,
    /// else the first enabled cloud model.
    fn fallback_default(&self, models: &[ModelEntry]) -> Option<String> {
        let starter = self
            .paths
            .manifest
            .as_ref()
            .and_then(|m| m.starter_model.as_ref())
            .map(|s| s.path.clone());
        models
            .iter()
            .filter(|m| m.enabled)
            .find(|m| {
                starter
                    .as_ref()
                    .is_some_and(|p| m.path.as_deref() == Some(&*p.to_string_lossy()))
            })
            .or_else(|| models.iter().find(|m| m.enabled && m.source == ModelSource::Local))
            .or_else(|| models.iter().find(|m| m.enabled))
            .map(|m| m.id.clone())
    }

    pub fn get(&self, id: &str) -> Option<ModelEntry> {
        self.list().into_iter().find(|m| m.id == id)
    }

    pub fn default_model(&self) -> Option<ModelEntry> {
        self.list().into_iter().find(|m| m.is_default)
    }

    /// `runtime` is the llama.cpp build that answers capability checks, if there is one.
    fn local_entry(&self, f: &LocalFile, s: ModelSettings, runtime: Option<&str>) -> ModelEntry {
        let default_name = pretty_name(f);
        let checked = runtime.and_then(|r| self.local_checks.read().get(&f.path, &local_key(f, r)));
        let managed = self.managed_dirs().iter().any(|d| f.path.starts_with(d));
        ModelEntry {
            effective: effective(
                &s,
                ModelSource::Local,
                f.info.architecture.as_deref(),
                Some(f),
                self.default_context(),
            ),
            id: f.id.clone(),
            source: ModelSource::Local,
            provider_id: None,
            provider_name: None,
            name: s.name.clone().unwrap_or_else(|| default_name.clone()),
            default_name,
            description: s.description.clone(),
            avatar_path: self.avatar_path(&s),
            enabled: s.enabled.unwrap_or(true),
            is_default: false,
            path: Some(f.path.to_string_lossy().into_owned()),
            size: Some(f.size),
            quant: f.quant.clone(),
            architecture: f.info.architecture.clone(),
            parameters: f.info.size_label.clone(),
            max_context: f.info.context_length,
            repo: f.repo.clone(),
            projector: f
                .mmproj
                .as_deref()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned()),
            removable: managed,
            capabilities: checked.unwrap_or_default(),
            checking_capabilities: runtime.is_some() && checked.is_none(),
            settings: s,
        }
    }

    /// The llama.cpp build that answers capability checks, when the runtime is installed.
    fn runtime_build(&self) -> Option<String> {
        self.paths.llama_server()?;
        let runtime = self.paths.manifest.as_ref()?.runtime.as_ref()?;
        Some(runtime.release.clone())
    }

    /// Asks llama.cpp what each new or changed local model can do, one model at a time, and
    /// hands the updated list to `changed` after every answer. Runs as long as the app does;
    /// without a local runtime it returns at once.
    pub async fn check_capabilities(self: Arc<Self>, changed: impl Fn(Vec<ModelEntry>)) {
        let (Some(server), Some(runtime)) = (self.paths.llama_server(), self.runtime_build()) else {
            return;
        };
        loop {
            let guard = self.check_lock.lock().await;
            let next = self.local.read().iter().find_map(|f| {
                let key = local_key(f, &runtime);
                (!self.local_checks.read().is_current(&f.path, &key)).then(|| (f.clone(), key))
            });
            let Some((file, key)) = next else {
                let present: Vec<PathBuf> = self.local.read().iter().map(|f| f.path.clone()).collect();
                self.local_checks.write().forget_missing(&present);
                drop(guard);
                self.rescanned.notified().await;
                continue;
            };

            let cancel = CancellationToken::new();
            *self.check_cancel.lock() = Some(cancel.clone());
            let started = Instant::now();
            let result = probe::run(&server, &file.path, file.mmproj.as_deref(), &cancel).await;
            *self.check_cancel.lock() = None;
            drop(guard);
            if cancel.is_cancelled() {
                continue; // Asked again once whatever paused the checks is done.
            }
            match &result {
                Ok(c) => tracing::info!(
                    model = %file.id,
                    "llama.cpp reports {c:?} in {:.1}s",
                    started.elapsed().as_secs_f64()
                ),
                Err(e) => tracing::warn!(model = %file.id, "llama.cpp could not say what the model can do: {e}"),
            }
            let present: Vec<PathBuf> = self.local.read().iter().map(|f| f.path.clone()).collect();
            self.local_checks.write().record(&file.path, key, result, &present);
            changed(self.list());
        }
    }

    /// Stops the capability check running now, if any, and starts no other until the guard is
    /// dropped, so a model file can be deleted: Windows refuses while llama.cpp has it open.
    pub async fn pause_checks(&self) -> MutexGuard<'_, ()> {
        if let Some(cancel) = self.check_cancel.lock().as_ref() {
            cancel.cancel();
        }
        self.check_lock.lock().await
    }

    /// Reads models.dev again when Gemini is set up and the copy kept is a week old, or at once
    /// with `force` (an unchanged catalog is not downloaded twice). Returns whether what cloud
    /// models can do may have changed. OpenRouter's own list says what its models can do.
    pub async fn refresh_cloud_catalog(&self, http: &reqwest::Client, force: bool) -> bool {
        let gemini = self.providers.configs().iter().any(|p| p.kind == ProviderKind::Gemini);
        if !gemini || !(force || self.cloud_catalog.read().is_stale()) {
            return false;
        }
        let etag = self.cloud_catalog.read().etag();
        match capabilities::fetch_catalog(http, etag.as_deref()).await {
            Ok(fetched) => {
                let changed = fetched.is_some();
                self.cloud_catalog.write().update(fetched);
                changed
            }
            Err(e) => {
                tracing::warn!("could not read models.dev: {e}");
                false
            }
        }
    }

    fn avatar_path(&self, s: &ModelSettings) -> Option<String> {
        s.avatar
            .as_ref()
            .map(|a| self.paths.avatars_dir.join(a))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    }

    fn save(&self, map: &BTreeMap<String, ModelSettings>) -> CmdResult<()> {
        demido_core::fsx::write_json(&self.file, &ModelsFile { models: map.clone() })?;
        Ok(())
    }

    /// Replaces a model's settings.
    pub fn set_settings(&self, id: &str, settings: ModelSettings) -> CmdResult<()> {
        let mut map = self.overrides.write();
        let previous_avatar = map.get(id).and_then(|s| s.avatar.clone());
        if previous_avatar.is_some()
            && previous_avatar != settings.avatar
            && let Some(old) = previous_avatar
        {
            let _ = std::fs::remove_file(self.paths.avatars_dir.join(old));
        }
        if settings == ModelSettings::default() {
            map.remove(id);
        } else {
            map.insert(id.to_string(), settings);
        }
        self.save(&map)
    }

    pub fn settings_of(&self, id: &str) -> ModelSettings {
        self.overrides.read().get(id).cloned().unwrap_or_default()
    }

    /// Turns several models on or off in one write, keeping the rest of their settings. Ids that
    /// match no model are ignored, so a stale list cannot leave orphaned overrides behind.
    pub fn set_enabled(&self, ids: &[String], enabled: bool) -> CmdResult<()> {
        let known: Vec<String> = self.list().into_iter().map(|m| m.id).collect();
        let ids: Vec<&String> = ids.iter().filter(|id| known.contains(id)).collect();
        let mut map = self.overrides.write();
        set_enabled_in(&mut map, &ids, enabled);
        self.save(&map)
    }

    /// Copies an image into the avatars folder and returns its new file name.
    pub fn import_avatar(&self, source: &Path) -> CmdResult<String> {
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| ["png", "jpg", "jpeg", "webp", "gif", "svg"].contains(&e.as_str()));
        let Some(ext) = ext else {
            bail_msg!("Pick a PNG, JPG, WebP, GIF or SVG image.");
        };
        let meta = std::fs::metadata(source)?;
        if meta.len() > 8 * 1024 * 1024 {
            bail_msg!("That image is larger than 8 MB.");
        }
        let name = format!("{}.{ext}", crate::db::new_id());
        std::fs::copy(source, self.paths.avatars_dir.join(&name))?;
        Ok(name)
    }

    /// Where a local model's projector goes: the model's Hugging Face repo (from its folders) and
    /// the folder it is in, where the projector is looked for.
    pub fn projector_target(&self, id: &str) -> CmdResult<(String, PathBuf)> {
        let file = self.local.read().iter().find(|f| f.id == id).cloned();
        let Some(file) = file else {
            bail_msg!("That model is no longer on disk.");
        };
        let (Some(repo), Some(dir)) = (file.repo, file.path.parent()) else {
            bail_msg!(
                "This model is not in a folder named after its Hugging Face repository, so its projector cannot be found."
            );
        };
        Ok((repo, dir.to_path_buf()))
    }

    /// Deletes a model's files from disk (managed folders only).
    pub fn delete_local(&self, id: &str) -> CmdResult<()> {
        let file = self.local.read().iter().find(|f| f.id == id).cloned();
        let Some(file) = file else {
            bail_msg!("That model is no longer on disk.");
        };
        if !self.managed_dirs().iter().any(|d| file.path.starts_with(d)) {
            bail_msg!("This model belongs to another app's folder. Disable it instead, or delete it from that app.");
        }
        for part in &file.parts {
            std::fs::remove_file(part)?;
        }
        // Remove the projector too when no other model in the folder uses it.
        if let (Some(mmproj), Some(dir)) = (&file.mmproj, file.path.parent()) {
            let others = std::fs::read_dir(dir)
                .map(|it| {
                    it.filter_map(Result::ok)
                        .any(|e| is_model_file(&e.path()) && e.path() != file.path)
                })
                .unwrap_or(false);
            if !others {
                let _ = std::fs::remove_file(mmproj);
            }
        }
        // Tidy empty repo folders.
        let mut dir = file.path.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if d == file.root || std::fs::remove_dir(&d).is_err() {
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
        let mut map = self.overrides.write();
        if map.remove(id).is_some() {
            self.save(&map)?;
        }
        drop(map);
        self.rescan();
        Ok(())
    }

    fn default_context(&self) -> DefaultContext {
        if self.context_fits_gpu {
            DefaultContext::FitGpu
        } else {
            DefaultContext::Budget(self.memory_budget_gb)
        }
    }

    /// Launch settings for a local model.
    pub fn launch_spec(&self, id: &str) -> CmdResult<LaunchSpec> {
        let entry = self.get(id);
        let file = self.local.read().iter().find(|f| f.id == id).cloned();
        let (Some(entry), Some(file)) = (entry, file) else {
            bail_msg!("That model is no longer available.");
        };
        // The projector lets the model read attached images and sound. It is loaded only once
        // llama.cpp has said the model reads them with it, so a projector it refuses never keeps
        // the model from starting.
        let reads_media = entry.capabilities.vision == Some(true) || entry.capabilities.audio == Some(true);
        Ok(LaunchSpec {
            model_id: entry.id.clone(),
            model_name: entry.name.clone(),
            path: file.path.clone(),
            context_length: entry.effective.context_length,
            gpu_layers: entry.effective.gpu_layers,
            mmproj: file.mmproj.clone().filter(|_| reads_media),
        })
    }

    pub fn gen_params(entry: &ModelEntry) -> GenParams {
        let e = &entry.effective;
        GenParams {
            temperature: e.temperature,
            top_p: e.top_p,
            top_k: e.top_k,
            min_p: e.min_p,
            repeat_penalty: e.repeat_penalty,
            max_tokens: e.max_tokens,
            seed: None,
            thinking: if entry.capabilities.thinking == Some(false) {
                None
            } else {
                e.thinking
            },
        }
    }

    /// Provider id and remote model name for a cloud model id (`<kind>:<provider>:<model>`). The
    /// model name may have colons of its own (`google/gemma-4-31b-it:free`).
    pub fn parse_cloud_id(id: &str) -> Option<(ProviderKind, &str, &str)> {
        let (prefix, rest) = id.split_once(':')?;
        let kind = [ProviderKind::Gemini, ProviderKind::OpenRouter]
            .into_iter()
            .find(|k| k.id_prefix() == prefix)?;
        let (provider, model) = rest.split_once(':')?;
        Some((kind, provider, model))
    }
}

fn is_model_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gguf")
        && !lower.starts_with("mmproj")
        && !lower.starts_with("mtp-")
        && !lower.starts_with("dflash-")
        && !lower.contains("imatrix")
        && !lower.contains(".mmproj")
        // Only the first part of a split model stands for the whole.
        && !regex::Regex::new(r"-0000[2-9]-of-|-000[1-9]\d-of-")
            .expect("valid regex")
            .is_match(&lower)
}

fn scan_file(root: &Path, path: &Path) -> Option<LocalFile> {
    let rel = path.strip_prefix(root).ok()?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let name = path.file_name()?.to_string_lossy().to_string();
    let info = gguf::read(path).ok()?;
    let mut parts = vec![path.to_path_buf()];
    if let Some(caps) = regex::Regex::new(r"^(.*)-00001-of-(\d{5})\.gguf$")
        .expect("valid regex")
        .captures(&name)
    {
        let stem = caps.get(1)?.as_str();
        let total: usize = caps.get(2)?.as_str().parse().ok()?;
        for i in 2..=total {
            parts.push(path.with_file_name(format!("{stem}-{i:05}-of-{total:05}.gguf")));
        }
        if parts.iter().any(|p| !p.is_file()) {
            return None; // Incomplete download.
        }
    }
    let size = parts
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let components: Vec<&str> = rel_str.split('/').collect();
    let repo = (components.len() >= 3).then(|| format!("{}/{}", components[0], components[1]));
    let mmproj = path.parent().and_then(projector_in);
    Some(LocalFile {
        id: format!("local:{rel_str}"),
        path: path.to_path_buf(),
        parts,
        root: root.to_path_buf(),
        size,
        modified,
        quant: gguf::quant_label(&name, info.file_type),
        info,
        repo,
        mmproj,
    })
}

/// The projector a model in `dir` is loaded with: of the `mmproj` files there, the one
/// [`hf::preferred_projector`] chooses.
fn projector_in(dir: &Path) -> Option<PathBuf> {
    let found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.to_ascii_lowercase().contains("mmproj") && n.ends_with(".gguf"))
        })
        .collect();
    hf::preferred_projector(found, |p| p.file_name().and_then(|n| n.to_str()).unwrap_or_default())
}

/// What llama.cpp's answer about `f` depends on.
fn local_key(f: &LocalFile, runtime: &str) -> LocalKey {
    LocalKey {
        size: f.size,
        modified: f.modified,
        projector: f.mmproj.clone(),
        runtime: runtime.to_string(),
    }
}

/// `Qwen3.5-9B-UD-Q6_K_XL.gguf` → `Qwen 3.5 9B`.
fn pretty_name(f: &LocalFile) -> String {
    let raw = f.info.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
        f.path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    let mut name = raw.replace(['_', '-'], " ");
    if let Some(q) = &f.quant {
        name = name.replace(&q.replace(['_', '-'], " "), "");
    }
    // `Qwen3.5` → `Qwen 3.5`, `gemma 4` → `Gemma 4`.
    let spaced = regex::Regex::new(r"(?i)\b(qwen|gemma|llama|mistral|phi|glm)(\d)")
        .expect("valid regex")
        .replace_all(&name, "$1 $2")
        .to_string();
    let mut words: Vec<String> = spaced
        .split_whitespace()
        .filter(|w| !w.eq_ignore_ascii_case("gguf") && !w.eq_ignore_ascii_case("it"))
        .map(|w| match w.to_ascii_lowercase().as_str() {
            "qat" => "QAT".to_string(),
            "moe" => "MoE".to_string(),
            "instruct" => "Instruct".to_string(),
            _ => {
                let mut c = w.chars();
                match c.next() {
                    Some(first) if w.chars().all(|ch| ch.is_ascii_lowercase()) => {
                        first.to_uppercase().collect::<String>() + c.as_str()
                    }
                    _ => w.to_string(),
                }
            }
        })
        .collect();
    words.dedup();
    let joined = words.join(" ");
    if joined.is_empty() { raw } else { joined }
}

/// How a local model's context is sized when the person has not chosen one.
#[derive(Clone, Copy, Debug)]
enum DefaultContext {
    /// llama.cpp fits it to the GPU's free memory as the model loads.
    FitGpu,
    /// From this much memory (GiB) less the model's size, in steps: there is no GPU memory of
    /// its own for llama.cpp to fit it to.
    Budget(f64),
}

/// Family defaults, as each vendor recommends them.
fn effective(
    s: &ModelSettings,
    source: ModelSource,
    architecture: Option<&str>,
    file: Option<&LocalFile>,
    default_context: DefaultContext,
) -> Effective {
    let arch = architecture.unwrap_or_default();
    let (temperature, top_p, top_k, min_p) = match source {
        ModelSource::Gemini | ModelSource::OpenRouter => (None, None, None, None),
        ModelSource::Local if arch.starts_with("qwen") => (Some(0.6), Some(0.95), Some(20), Some(0.0)),
        ModelSource::Local if arch.starts_with("gemma") => (Some(1.0), Some(0.95), Some(64), Some(0.0)),
        ModelSource::Local => (Some(0.7), Some(0.9), Some(40), Some(0.05)),
    };
    let context_length = match (source, file) {
        (ModelSource::Local, Some(f)) => {
            let trained = f.info.context_length.unwrap_or(32768).min(u32::MAX as u64) as u32;
            let chosen = match (s.context_length, default_context) {
                (Some(c), _) => Some(c),
                (None, DefaultContext::FitGpu) => None,
                (None, DefaultContext::Budget(budget_gb)) => {
                    let headroom = budget_gb - f.size as f64 / demido_hardware::GIB as f64;
                    Some(if headroom >= 4.0 {
                        32768
                    } else if headroom >= 2.0 {
                        16384
                    } else {
                        8192
                    })
                }
            };
            chosen.map(|c| c.min(trained.max(2048)))
        }
        _ => s.context_length,
    };
    Effective {
        system_prompt: s.system_prompt.clone().unwrap_or_default(),
        temperature: s.temperature.or(temperature),
        top_p: s.top_p.or(top_p),
        top_k: s.top_k.or(top_k),
        min_p: s.min_p.or(min_p),
        repeat_penalty: s.repeat_penalty,
        max_tokens: s.max_tokens,
        context_length,
        gpu_layers: s.gpu_layers,
        thinking: s.thinking.or(Some(true)),
    }
}

/// Gemini lists dozens of versions; only the stable families are on by default.
fn default_enabled_gemini(models: &[crate::llm::CloudModel]) -> Vec<String> {
    let stable = regex::Regex::new(r"^gemini-(\d+(\.\d+)?-)?(pro|flash|flash-lite)(-latest)?$").expect("valid regex");
    let picked: Vec<String> = models
        .iter()
        .filter(|m| stable.is_match(&m.id))
        .map(|m| m.id.clone())
        .collect();
    if picked.is_empty() {
        models.iter().take(3).map(|m| m.id.clone()).collect()
    } else {
        picked
    }
}

fn set_enabled_in(map: &mut BTreeMap<String, ModelSettings>, ids: &[&String], enabled: bool) {
    for id in ids {
        map.entry((*id).clone()).or_default().enabled = Some(enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_enable_keeps_other_settings() {
        let mut map = BTreeMap::new();
        map.insert(
            "a".to_string(),
            ModelSettings {
                name: Some("Mine".into()),
                enabled: Some(true),
                ..Default::default()
            },
        );
        let (a, b) = ("a".to_string(), "b".to_string());
        set_enabled_in(&mut map, &[&a, &b], false);
        assert_eq!(map["a"].name.as_deref(), Some("Mine"));
        assert_eq!(map["a"].enabled, Some(false));
        assert_eq!(map["b"].enabled, Some(false));
    }

    fn file(name: &str, gguf_name: Option<&str>, quant: Option<&str>) -> LocalFile {
        LocalFile {
            id: format!("local:{name}"),
            path: PathBuf::from(name),
            parts: vec![],
            root: PathBuf::new(),
            size: 5_000_000_000,
            modified: None,
            info: gguf::GgufInfo {
                name: gguf_name.map(str::to_string),
                architecture: Some("qwen35".into()),
                context_length: Some(262_144),
                ..Default::default()
            },
            quant: quant.map(str::to_string),
            repo: None,
            mmproj: None,
        }
    }

    #[test]
    fn pretty_names_read_well() {
        assert_eq!(
            pretty_name(&file("Qwen3.5-9B-UD-Q6_K_XL.gguf", None, Some("UD-Q6_K_XL"))),
            "Qwen 3.5 9B"
        );
        assert_eq!(pretty_name(&file("x.gguf", Some("Qwen3.5-9B"), None)), "Qwen 3.5 9B");
        assert_eq!(
            pretty_name(&file("gemma-4-12B-it-qat-UD-Q4_K_XL.gguf", None, Some("UD-Q4_K_XL"))),
            "Gemma 4 12B QAT"
        );
    }

    #[test]
    fn model_file_filter_skips_companions_and_later_parts() {
        assert!(is_model_file(Path::new("a/Qwen3.5-9B-Q4_K_M.gguf")));
        assert!(is_model_file(Path::new("a/big-00001-of-00003.gguf")));
        assert!(!is_model_file(Path::new("a/big-00002-of-00003.gguf")));
        assert!(!is_model_file(Path::new("a/mmproj-F16.gguf")));
        assert!(!is_model_file(Path::new("a/mtp-gemma-4-E4B-it.gguf")));
        assert!(!is_model_file(Path::new("a/imatrix_unsloth.gguf")));
    }

    #[test]
    fn context_follows_memory_headroom_and_training() {
        let f = file("m.gguf", None, None);
        let local = |s: &ModelSettings, d| effective(s, ModelSource::Local, Some("qwen35"), Some(&f), d);
        let e = local(&ModelSettings::default(), DefaultContext::Budget(12.0));
        assert_eq!(e.context_length, Some(32768));
        assert_eq!(e.temperature, Some(0.6));
        let e = local(&ModelSettings::default(), DefaultContext::Budget(6.0));
        assert_eq!(e.context_length, Some(8192));
        let custom = ModelSettings {
            context_length: Some(65536),
            temperature: Some(0.2),
            ..Default::default()
        };
        let e = local(&custom, DefaultContext::Budget(6.0));
        assert_eq!(e.context_length, Some(65536));
        assert_eq!(e.temperature, Some(0.2));
    }

    #[test]
    fn context_is_left_to_llama_cpp_on_a_gpu_unless_chosen() {
        let f = file("m.gguf", None, None);
        let local =
            |s: &ModelSettings| effective(s, ModelSource::Local, Some("qwen35"), Some(&f), DefaultContext::FitGpu);
        assert_eq!(local(&ModelSettings::default()).context_length, None);
        let chosen = |c| ModelSettings {
            context_length: Some(c),
            ..Default::default()
        };
        assert_eq!(local(&chosen(49152)).context_length, Some(49152));
        // Never more than the model was trained on.
        assert_eq!(local(&chosen(1_048_576)).context_length, Some(262_144));
    }

    #[test]
    fn cloud_ids_name_their_provider() {
        assert_eq!(
            ModelRegistry::parse_cloud_id("gemini:p1:gemini-2.5-flash"),
            Some((ProviderKind::Gemini, "p1", "gemini-2.5-flash"))
        );
        assert_eq!(
            ModelRegistry::parse_cloud_id("openrouter:p2:google/gemma-4-31b-it:free"),
            Some((ProviderKind::OpenRouter, "p2", "google/gemma-4-31b-it:free"))
        );
        assert_eq!(ModelRegistry::parse_cloud_id("local:C:/models/a.gguf"), None);
    }

    #[test]
    fn stable_gemini_models_are_on_by_default() {
        let m = |id: &str| crate::llm::CloudModel {
            id: id.into(),
            display_name: id.into(),
            description: String::new(),
            input_token_limit: 0,
            output_token_limit: 0,
            thinking: true,
            always_thinks: false,
            capabilities: None,
            free: false,
        };
        let on = default_enabled_gemini(&[
            m("gemini-2.5-flash"),
            m("gemini-2.5-pro"),
            m("gemini-2.5-flash-preview-05-20"),
            m("gemini-flash-latest"),
        ]);
        assert_eq!(on, vec!["gemini-2.5-flash", "gemini-2.5-pro", "gemini-flash-latest"]);
    }
}
