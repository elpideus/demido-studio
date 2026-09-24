//! Cloud model providers (Gemini for now). Configuration lives in `providers.json`; API keys live
//! in the OS credential store.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::bail_msg;
use crate::db::{new_id, now_ms};
use crate::error::CmdResult;
use crate::llm::gemini::{self, GeminiModel};
use crate::secrets::{Secrets, provider_key};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gemini,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "Google Gemini",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Gemini => gemini::DEFAULT_BASE_URL,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    pub enabled: bool,
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<GeminiModel>,
    pub models_fetched_at: Option<i64>,
    pub created_at: i64,
}

impl ProviderConfig {
    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| self.kind.default_base_url().to_string())
    }
}

/// A provider as the Settings window shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    pub config: ProviderConfig,
    pub has_key: bool,
    /// Last four characters of the key, for recognition.
    pub key_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct ProvidersFile {
    providers: Vec<ProviderConfig>,
}

pub struct ProviderStore {
    path: PathBuf,
    list: RwLock<Vec<ProviderConfig>>,
    secrets: Arc<Secrets>,
    http: reqwest::Client,
}

impl ProviderStore {
    pub fn load(path: PathBuf, secrets: Arc<Secrets>, http: reqwest::Client) -> Self {
        let list = demido_core::fsx::read_json::<ProvidersFile>(&path)
            .map(|f| f.providers)
            .unwrap_or_default();
        Self {
            path,
            list: RwLock::new(list),
            secrets,
            http,
        }
    }

    fn save(&self, list: &[ProviderConfig]) -> CmdResult<()> {
        demido_core::fsx::write_json(
            &self.path,
            &ProvidersFile {
                providers: list.to_vec(),
            },
        )?;
        Ok(())
    }

    pub fn configs(&self) -> Vec<ProviderConfig> {
        self.list.read().clone()
    }

    pub fn get(&self, id: &str) -> Option<ProviderConfig> {
        self.list.read().iter().find(|p| p.id == id).cloned()
    }

    pub fn api_key(&self, id: &str) -> Option<String> {
        self.secrets.get(&provider_key(id))
    }

    pub fn views(&self) -> Vec<ProviderView> {
        self.configs()
            .into_iter()
            .map(|config| {
                let key = self.api_key(&config.id);
                ProviderView {
                    has_key: key.is_some(),
                    key_hint: key.map(|k| {
                        let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
                        format!("••••{tail}")
                    }),
                    config,
                }
            })
            .collect()
    }

    /// Adds a provider after checking the key works.
    pub async fn add(&self, kind: ProviderKind, name: Option<String>, api_key: String) -> CmdResult<ProviderConfig> {
        let key = api_key.trim().to_string();
        if key.is_empty() {
            bail_msg!("Enter an API key.");
        }
        let models = self.fetch_models(kind, kind.default_base_url(), &key).await?;
        let config = ProviderConfig {
            id: new_id(),
            kind,
            name: name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| kind.label().to_string()),
            enabled: true,
            base_url: None,
            models,
            models_fetched_at: Some(now_ms()),
            created_at: now_ms(),
        };
        self.secrets.set(&provider_key(&config.id), &key);
        let mut list = self.list.write();
        list.push(config.clone());
        self.save(&list)?;
        Ok(config)
    }

    pub async fn update(&self, id: &str, patch: ProviderPatch) -> CmdResult<ProviderConfig> {
        let Some(mut config) = self.get(id) else {
            bail_msg!("That provider no longer exists.");
        };
        if let Some(key) = patch.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            let base = patch.base_url.clone().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| config.base_url());
            config.models = self.fetch_models(config.kind, &base, key).await?;
            config.models_fetched_at = Some(now_ms());
            self.secrets.set(&provider_key(id), key);
        }
        if let Some(name) = patch.name.filter(|n| !n.trim().is_empty()) {
            config.name = name;
        }
        if let Some(enabled) = patch.enabled {
            config.enabled = enabled;
        }
        if let Some(base) = patch.base_url {
            config.base_url = Some(base).filter(|b| !b.trim().is_empty());
        }
        let mut list = self.list.write();
        if let Some(slot) = list.iter_mut().find(|p| p.id == id) {
            *slot = config.clone();
        }
        self.save(&list)?;
        Ok(config)
    }

    pub async fn refresh_models(&self, id: &str) -> CmdResult<ProviderConfig> {
        let Some(mut config) = self.get(id) else {
            bail_msg!("That provider no longer exists.");
        };
        let Some(key) = self.api_key(id) else {
            bail_msg!("This provider has no API key.");
        };
        config.models = self.fetch_models(config.kind, &config.base_url(), &key).await?;
        config.models_fetched_at = Some(now_ms());
        let mut list = self.list.write();
        if let Some(slot) = list.iter_mut().find(|p| p.id == id) {
            *slot = config.clone();
        }
        self.save(&list)?;
        Ok(config)
    }

    pub fn remove(&self, id: &str) -> CmdResult<()> {
        let mut list = self.list.write();
        list.retain(|p| p.id != id);
        self.save(&list)?;
        self.secrets.delete(&provider_key(id));
        Ok(())
    }

    async fn fetch_models(&self, kind: ProviderKind, base: &str, key: &str) -> CmdResult<Vec<GeminiModel>> {
        match kind {
            ProviderKind::Gemini => gemini::list_models(&self.http, base, key)
                .await
                .map_err(|e| crate::error::AppError::msg(format!("Gemini rejected the key: {e}"))),
        }
    }
}
