//! Cloud model providers: Google Gemini, OpenRouter, Anthropic (an API key) and Claude Code (a
//! Claude subscription, through the official CLI). Configuration lives in `providers.json`; API
//! keys live in the OS credential store. Claude Code keeps its own sign-in (see
//! [`crate::claude_code`]).

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::bail_msg;
use crate::db::{new_id, now_ms};
use crate::error::CmdResult;
use crate::llm::{Allowance, CloudModel, anthropic, claude_code, gemini, openrouter};

/// What a saved list of models records. A list saved by an older version, before each model's
/// effort levels were kept, is listed again at start.
const LISTING: u32 = 1;
use crate::secrets::{Secrets, provider_key};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gemini,
    OpenRouter,
    Anthropic,
    #[serde(rename = "claudecode")]
    ClaudeCode,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::ClaudeCode => "Anthropic",
        }
    }

    /// Signs in with something other than an API key.
    pub fn keyless(self) -> bool {
        self == ProviderKind::ClaudeCode
    }

    /// Starts the ids of the provider's models: `<prefix>:<provider id>:<model>`.
    pub fn id_prefix(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "gemini",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::ClaudeCode => "claude-code",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Gemini => gemini::DEFAULT_BASE_URL,
            ProviderKind::OpenRouter => openrouter::DEFAULT_BASE_URL,
            ProviderKind::Anthropic => anthropic::DEFAULT_BASE_URL,
            ProviderKind::ClaudeCode => "",
        }
    }
}

/// Which of a provider's models are offered. Only OpenRouter, which lists paid models next to
/// free ones, lets the person choose.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelGroup {
    #[default]
    All,
    Free,
}

impl ModelGroup {
    fn admits(self, model: &CloudModel) -> bool {
        match self {
            ModelGroup::All => true,
            ModelGroup::Free => model.free,
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
    pub model_group: ModelGroup,
    /// The models of `model_group`.
    #[serde(default)]
    pub models: Vec<CloudModel>,
    pub models_fetched_at: Option<i64>,
    /// The [`LISTING`] its models were listed with.
    #[serde(default)]
    pub listing: u32,
    pub created_at: i64,
    /// Claude Code's own tools this provider lets the model use (`WebSearch`, say), from
    /// [`crate::claude_code::BUILTIN_TOOLS`]. None by default: Demido's tools come first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claude_code_tools: Vec<String>,
    /// The Claude Code profile a Claude subscription is signed in with (`demido_studio_2`, say):
    /// its own config folder, so each subscription is a separate account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_code_profile: Option<String>,
}

impl ProviderConfig {
    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| self.kind.default_base_url().to_string())
    }

    /// The Claude Code profile this provider runs with.
    pub fn claude_code_profile(&self) -> &str {
        self.claude_code_profile
            .as_deref()
            .unwrap_or(crate::claude_code::DEFAULT_PROFILE)
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

/// What a provider says one of its keys has left.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider_id: String,
    #[serde(flatten)]
    pub report: UsageReport,
    pub checked_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum UsageReport {
    Reported {
        allowances: Vec<Allowance>,
    },
    /// The provider cannot be asked: Google shows Gemini's quotas only in AI Studio.
    Unreported,
    Failed {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_group: Option<ModelGroup>,
    pub claude_code_tools: Option<Vec<String>>,
}

/// Longest name a provider can be given; it is shown in one line of the model picker.
const MAX_NAME_CHARS: usize = 60;

/// A name as the person typed it, trimmed and shortened; `None` when nothing is left.
fn clean_name(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end();
    (!name.is_empty()).then(|| name.to_string())
}

/// The built-in Claude Code tools of `tools` that may be turned on, each once, in their usual order.
fn clean_tools(tools: &[String]) -> Vec<String> {
    crate::claude_code::BUILTIN_TOOLS
        .iter()
        .filter(|t| tools.iter().any(|x| x == *t))
        .map(|t| t.to_string())
        .collect()
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
        let mut list = demido_core::fsx::read_json::<ProvidersFile>(&path)
            .map(|f| f.providers)
            .unwrap_or_default();
        // Google's list leaves out how hard each model can think; it is known here instead.
        for config in list.iter_mut().filter(|c| c.kind == ProviderKind::Gemini) {
            config.models.iter_mut().for_each(gemini::fill_thinking);
        }
        for config in list.iter_mut().filter(|c| c.kind == ProviderKind::ClaudeCode) {
            // Claude Code's models are known, not listed: a saved list may predate what is known.
            config.models = claude_code::models();
            // A Claude subscription added before there could be several used the first profile.
            if !config
                .claude_code_profile
                .as_deref()
                .is_some_and(crate::claude_code::valid_profile)
            {
                config.claude_code_profile = Some(crate::claude_code::DEFAULT_PROFILE.to_string());
            }
        }
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

    fn claude_code_profile_used(&self, profile: &str) -> bool {
        self.list
            .read()
            .iter()
            .any(|c| c.kind == ProviderKind::ClaudeCode && c.claude_code_profile() == profile)
    }

    /// The first new Claude Code profile no provider uses, to sign the next subscription in with.
    pub fn free_claude_code_profile(&self) -> String {
        (1..)
            .map(crate::claude_code::new_profile)
            .find(|p| !self.claude_code_profile_used(p))
            .expect("a free profile")
    }

    /// The profile name for an account's email that no provider or folder has yet:
    /// `demido_studio_jane`, else `demido_studio_jane_2`...
    fn claude_code_profile_for(&self, email: &str) -> String {
        let base = crate::claude_code::profile_for_email(email);
        (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}_{n}") })
            .find(|p| !self.claude_code_profile_used(p) && !crate::claude_code::config_dir(p).exists())
            .expect("a free profile name")
    }

    /// Names each Claude subscription's profile after its account, when it is not yet (one added
    /// before profiles were named, or one whose renaming failed). Run at start, before any chat.
    pub async fn name_claude_code_profiles(&self) {
        let unnamed: Vec<(String, String)> = self
            .configs()
            .into_iter()
            .filter(|c| c.kind == ProviderKind::ClaudeCode)
            .map(|c| (c.id.clone(), c.claude_code_profile().to_string()))
            .filter(|(_, p)| !crate::claude_code::named_after_account(p))
            .collect();
        if unnamed.is_empty() || crate::claude_code::find_binary().is_none() {
            return;
        }
        for (id, profile) in unnamed {
            let account = crate::claude_code::status(&profile).await.account;
            let Some(email) = account.email.filter(|_| account.logged_in) else {
                continue;
            };
            let named = self.claude_code_profile_for(&email);
            if crate::claude_code::rename_profile(&profile, &named).await {
                tracing::info!("named Claude Code profile {profile} {named}");
                let _ = self.edit(&id, |c| c.claude_code_profile = Some(named));
            }
        }
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

    /// Adds a provider after checking the key works. Claude Code needs no key, only the CLI
    /// signed in with `claude_code_profile` (the first free one when not given).
    pub async fn add(
        &self,
        kind: ProviderKind,
        name: Option<String>,
        api_key: String,
        model_group: ModelGroup,
        claude_code_profile: Option<String>,
    ) -> CmdResult<ProviderConfig> {
        let key = api_key.trim().to_string();
        let mut profile = None;
        if kind.keyless() {
            let wanted = claude_code_profile.unwrap_or_else(|| self.free_claude_code_profile());
            let wanted = crate::claude_code::checked_profile(&wanted)?.to_string();
            let others: Vec<String> = self
                .configs()
                .iter()
                .filter(|c| c.kind == kind)
                .map(|c| c.claude_code_profile().to_string())
                .collect();
            if others.contains(&wanted) {
                bail_msg!("This Claude account is already added.");
            }
            let account = crate::claude_code::require_ready(&wanted).await?.account;
            // The same account twice would only share its plan's limits.
            let accounts = futures_util::future::join_all(others.iter().map(|p| crate::claude_code::status(p))).await;
            if account.email.is_some()
                && accounts
                    .iter()
                    .any(|s| s.account.email == account.email && s.account.auth_method == account.auth_method)
            {
                bail_msg!(
                    "{} is already added. Sign in with another Claude account.",
                    account.email.as_deref().unwrap_or("This account")
                );
            }
            profile = Some((wanted, account.email));
        } else if key.is_empty() {
            bail_msg!("Enter an API key.");
        }
        let models = self
            .fetch_models(kind, kind.default_base_url(), &key, model_group)
            .await?;
        // The profile signed in with takes its account's name, when it can.
        let profile = match profile {
            Some((wanted, Some(email))) => {
                let named = self.claude_code_profile_for(&email);
                let moved = crate::claude_code::rename_profile(&wanted, &named).await;
                Some(if moved { named } else { wanted })
            }
            Some((wanted, None)) => Some(wanted),
            None => None,
        };
        let config = ProviderConfig {
            id: new_id(),
            kind,
            name: name
                .as_deref()
                .and_then(clean_name)
                .unwrap_or_else(|| kind.label().to_string()),
            enabled: true,
            base_url: None,
            model_group,
            models,
            models_fetched_at: Some(now_ms()),
            listing: LISTING,
            created_at: now_ms(),
            claude_code_tools: Vec::new(),
            claude_code_profile: profile,
        };
        if !kind.keyless() {
            self.secrets.set(&provider_key(&config.id), &key);
        }
        let mut list = self.list.write();
        list.push(config.clone());
        self.save(&list)?;
        Ok(config)
    }

    pub async fn update(&self, id: &str, patch: ProviderPatch) -> CmdResult<ProviderConfig> {
        let Some(config) = self.get(id) else {
            bail_msg!("That provider no longer exists.");
        };
        let key = patch.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty());
        let group = patch.model_group.filter(|g| *g != config.model_group);
        // A new key is checked by listing the models with it before anything is saved; another
        // group of models is listed afresh.
        let models = if config.kind.keyless() {
            None
        } else if key.is_some() || group.is_some() {
            let Some(listing_key) = key.map(str::to_string).or_else(|| self.api_key(id)) else {
                bail_msg!("This provider has no API key.");
            };
            let base = patch
                .base_url
                .clone()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| config.base_url());
            let group = group.unwrap_or(config.model_group);
            Some(self.fetch_models(config.kind, &base, &listing_key, group).await?)
        } else {
            None
        };
        let updated = self.edit(id, |config| {
            if let Some(models) = models {
                config.models = models;
                config.models_fetched_at = Some(now_ms());
                config.listing = LISTING;
            }
            if let Some(group) = group {
                config.model_group = group;
            }
            if let Some(name) = patch.name.as_deref().and_then(clean_name) {
                config.name = name;
            }
            if let Some(enabled) = patch.enabled {
                config.enabled = enabled;
            }
            if let Some(base) = patch.base_url {
                config.base_url = Some(base).filter(|b| !b.trim().is_empty());
            }
            if let Some(tools) = patch.claude_code_tools {
                config.claude_code_tools = clean_tools(&tools);
            }
        })?;
        if let Some(key) = key.filter(|_| !config.kind.keyless()) {
            self.secrets.set(&provider_key(id), key);
        }
        Ok(updated)
    }

    pub async fn refresh_models(&self, id: &str) -> CmdResult<ProviderConfig> {
        let Some(config) = self.get(id) else {
            bail_msg!("That provider no longer exists.");
        };
        let key = if config.kind.keyless() {
            String::new()
        } else {
            let Some(key) = self.api_key(id) else {
                bail_msg!("This provider has no API key.");
            };
            key
        };
        let models = self
            .fetch_models(config.kind, &config.base_url(), &key, config.model_group)
            .await?;
        self.edit(id, |config| {
            config.models = models;
            config.models_fetched_at = Some(now_ms());
            config.listing = LISTING;
        })
    }

    /// Lists again the models of each provider whose list an older version saved. Run at start;
    /// a provider that cannot be reached keeps its list and is tried at the next start. Says
    /// whether any list changed.
    pub async fn refresh_old_listings(&self) -> bool {
        let old: Vec<String> = self
            .configs()
            .into_iter()
            .filter(|c| c.enabled && !c.kind.keyless() && c.listing < LISTING)
            .map(|c| c.id)
            .collect();
        let mut changed = false;
        for id in old {
            match self.refresh_models(&id).await {
                Ok(_) => changed = true,
                Err(e) => tracing::warn!("could not list the models of provider {id} again: {e}"),
            }
        }
        changed
    }

    /// Changes a provider as it is now, not as it was before a request: a rename made while its
    /// models were being fetched must survive the fetch.
    fn edit(&self, id: &str, change: impl FnOnce(&mut ProviderConfig)) -> CmdResult<ProviderConfig> {
        let mut list = self.list.write();
        let Some(slot) = list.iter_mut().find(|p| p.id == id) else {
            bail_msg!("That provider no longer exists.");
        };
        change(slot);
        let config = slot.clone();
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

    /// What every provider with a key has left, asked of them all at once.
    pub async fn usage(&self) -> Vec<ProviderUsage> {
        let asks = self.configs().into_iter().filter_map(|config| {
            let key = self.api_key(&config.id)?;
            Some(async move {
                let report = match config.kind {
                    ProviderKind::Gemini | ProviderKind::Anthropic | ProviderKind::ClaudeCode => {
                        UsageReport::Unreported
                    }
                    ProviderKind::OpenRouter => {
                        match openrouter::allowances(&self.http, &config.base_url(), &key).await {
                            Ok(allowances) => UsageReport::Reported { allowances },
                            Err(e) => UsageReport::Failed { message: e.to_string() },
                        }
                    }
                };
                ProviderUsage {
                    provider_id: config.id,
                    report,
                    checked_at: now_ms(),
                }
            })
        });
        futures_util::future::join_all(asks).await
    }

    async fn fetch_models(
        &self,
        kind: ProviderKind,
        base: &str,
        key: &str,
        group: ModelGroup,
    ) -> CmdResult<Vec<CloudModel>> {
        let listed = match kind {
            ProviderKind::Gemini => gemini::list_models(&self.http, base, key).await,
            ProviderKind::OpenRouter => openrouter::list_models(&self.http, base, key).await,
            ProviderKind::Anthropic => anthropic::list_models(&self.http, base, key).await,
            ProviderKind::ClaudeCode => Ok(claude_code::models()),
        };
        let models =
            listed.map_err(|e| crate::error::AppError::msg(format!("{} rejected the key: {e}", kind.label())))?;
        Ok(models.into_iter().filter(|m| group.admits(m)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_capped() {
        assert_eq!(clean_name("  Work Gemini  ").as_deref(), Some("Work Gemini"));
        assert_eq!(clean_name("   "), None);
        assert_eq!(
            clean_name(&"x".repeat(200)).map(|n| n.chars().count()),
            Some(MAX_NAME_CHARS)
        );
    }

    #[test]
    fn providers_saved_before_openrouter_still_load() {
        let saved = r#"{"providers": [{"id": "p1", "kind": "gemini", "name": "Google Gemini", "enabled": true,
            "baseUrl": null, "modelsFetchedAt": 1, "createdAt": 1, "models": [{"id": "gemini-2.5-flash",
            "displayName": "Gemini 2.5 Flash", "description": "", "inputTokenLimit": 1048576,
            "outputTokenLimit": 65536, "thinking": true}]}]}"#;
        let file: ProvidersFile = serde_json::from_str(saved).unwrap();
        let p = &file.providers[0];
        assert_eq!(p.model_group, ModelGroup::All);
        assert!(!p.models[0].free && !p.models[0].always_thinks && p.models[0].capabilities.is_none());
    }

    #[test]
    fn only_known_claude_code_tools_are_kept() {
        let asked = ["WebFetch", "WebSearch", "Bash", "WebSearch", "Nonsense"].map(String::from);
        assert_eq!(clean_tools(&asked), vec!["WebSearch", "Bash"]);
    }

    #[test]
    fn the_free_group_admits_only_free_models() {
        let m = |free| CloudModel {
            id: "m".into(),
            display_name: "m".into(),
            description: String::new(),
            input_token_limit: 0,
            output_token_limit: 0,
            thinking: false,
            always_thinks: false,
            capabilities: None,
            free,
            effort: false,
            efforts: Vec::new(),
            default_effort: None,
        };
        assert!(ModelGroup::Free.admits(&m(true)) && !ModelGroup::Free.admits(&m(false)));
        assert!(ModelGroup::All.admits(&m(false)));
    }
}
