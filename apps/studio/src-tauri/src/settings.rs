//! `settings.json`: small preferences that are not chats, models, providers or skills.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error::CmdResult;

/// Version of the safety notice. Bumping it shows the notice again to everyone.
pub const DISCLAIMER_VERSION: &str = "2026-09";

/// Which releases the updater offers.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Stable releases only.
    #[default]
    Release,
    /// Stable releases and pre-releases, whichever is newest.
    Prerelease,
}

/// When the assistant asks before it acts (see `tools::permissions`).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Asks before every tool call that reads or changes anything.
    Cautious,
    /// Asks only before what is risky: running code, sending email, deleting for good, saving to
    /// an online account, a long download.
    #[default]
    Balanced,
    /// Never asks.
    Autonomous,
}

/// Where market history downloads come from first.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MarketSource {
    /// Dukascopy, with MetaTrader as the fallback when the person allows it.
    #[default]
    Dukascopy,
    /// MetaTrader, with Dukascopy for what it does not have when the person allows it.
    Metatrader,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Which version of the safety notice was accepted.
    pub disclaimer_accepted: Option<String>,
    /// Model selected for new chats.
    pub default_model: Option<String>,
    /// Load the default local model when the app starts, so the first answer is fast.
    pub preload_default_model: bool,
    /// Built-in tool groups the assistant may use, by id.
    pub tool_groups: BTreeMap<String, bool>,
    /// When the assistant asks before it acts.
    pub permission_mode: PermissionMode,
    /// Tools the person chose to always allow without asking (in the Balanced mode).
    pub always_allowed_tools: BTreeSet<String>,
    /// Extra folders scanned for GGUF models (for example LM Studio's).
    pub extra_model_dirs: Vec<PathBuf>,
    /// Chat list panel visibility.
    pub chat_list_open: bool,
    /// Window layout owned by the UI (open windows and their geometry).
    pub window_layout: serde_json::Value,
    pub last_chat_id: Option<String>,
    /// A market download the assistant starts asks first when its estimate exceeds this.
    pub download_approval_seconds: u32,
    /// Model downloads that run at once (1 to 3). One gives the file being waited for the whole
    /// connection.
    pub download_concurrency: u8,
    /// Bytes per second all model downloads may take together; 0 is no limit.
    pub download_limit: u64,
    /// Fetch model files through Hugging Face's Xet engine rather than plain HTTP.
    pub download_xet: bool,
    /// Where market history downloads come from first (see `market::metatrader`).
    pub market_source: MarketSource,
    /// Downloads go on from MetaTrader while Dukascopy refuses or fails.
    pub metatrader_fallback: bool,
    /// With MetaTrader first, Dukascopy downloads what it does not have; off, MetaTrader is the only
    /// source and Dukascopy is never asked.
    pub dukascopy_fallback: bool,
    /// Which version of the MetaTrader notice was accepted: nothing touches MetaTrader without it.
    pub metatrader_consent: Option<String>,
    /// The MetaTrader terminal downloads use (its terminal64.exe).
    pub metatrader_terminal: Option<PathBuf>,
    /// The account they use, by its login and server.
    pub metatrader_login: Option<u64>,
    pub metatrader_server: Option<String>,
    /// The terminal may be logged into that account when it is on another one, and back after.
    pub metatrader_switch: bool,
    /// Which releases the updater offers.
    pub update_channel: UpdateChannel,
    /// Download new versions in the background and install them the next time the app starts.
    pub auto_update: bool,
    /// Summarize the older part of a chat when it nears the model's context window.
    pub auto_compact: bool,
    /// The tokens at which a chat is compacted, instead of the default share of the context
    /// window. Never above that share, whatever is set.
    pub auto_compact_tokens: Option<u32>,
    /// Load the images an email links to from the internet without asking, in the Mail window.
    pub mail_show_images: bool,
    /// Opening a message in the Mail window marks it as read.
    pub mail_mark_read: bool,
    /// The Mail window asks before it opens a link from an email in the browser.
    pub mail_confirm_links: bool,
    /// The microphone the composer records with (the browser's device id); `None` for the
    /// system's default.
    pub microphone: Option<String>,
    /// The speech model that writes down what the person says (a catalog id); `None` lets the
    /// app pick by the GPU's memory.
    pub speech_model: Option<String>,
    /// Send the person's recorded voice to models that can hear it, rather than a transcript.
    pub send_voice: bool,
    /// The person went on without the runtimes when setup first offered them (Linux), so the app
    /// does not start setup on its own again.
    pub local_setup_declined: bool,
    /// The app version that last started setup to bring the runtimes up to date (Linux): once a
    /// version, so an update that cannot finish (offline, say) does not run at every start.
    pub runtime_update_tried: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let tool_groups = ["market", "coding", "computer", "files", "skills"]
            .into_iter()
            .map(|g| (g.to_string(), true))
            .collect();
        Settings {
            disclaimer_accepted: None,
            default_model: None,
            preload_default_model: true,
            tool_groups,
            permission_mode: PermissionMode::Balanced,
            always_allowed_tools: BTreeSet::new(),
            extra_model_dirs: Vec::new(),
            chat_list_open: true,
            window_layout: serde_json::Value::Null,
            last_chat_id: None,
            download_approval_seconds: 60,
            download_concurrency: 1,
            download_limit: 0,
            download_xet: false,
            market_source: MarketSource::Dukascopy,
            metatrader_fallback: true,
            dukascopy_fallback: true,
            metatrader_consent: None,
            metatrader_terminal: None,
            metatrader_login: None,
            metatrader_server: None,
            metatrader_switch: true,
            update_channel: UpdateChannel::Release,
            auto_update: true,
            auto_compact: true,
            auto_compact_tokens: None,
            mail_show_images: false,
            mail_mark_read: true,
            mail_confirm_links: true,
            microphone: None,
            speech_model: None,
            send_voice: true,
            local_setup_declined: false,
            runtime_update_tried: None,
        }
    }
}

impl Settings {
    pub fn tool_group_enabled(&self, group: &str) -> bool {
        self.tool_groups.get(group).copied().unwrap_or(true)
    }

    /// Brings switches saved by an older version up to date. Python and the terminal became one
    /// group, Coding, which stays off if either was switched off; Pine scripts joined Market data.
    /// Then commands left Coding for Computer use, which is on where Coding was.
    fn upgrade(&mut self) {
        if !self.tool_groups.contains_key("coding") {
            let coding = ["python", "terminal"].iter().all(|g| self.tool_group_enabled(g));
            self.tool_groups.insert("coding".into(), coding);
        }
        if !self.tool_groups.contains_key("computer") {
            let computer = self.tool_group_enabled("coding");
            self.tool_groups.insert("computer".into(), computer);
        }
        for old in ["python", "terminal", "pine", "apps"] {
            self.tool_groups.remove(old);
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    value: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let mut value = demido_core::fsx::read_json::<Settings>(&path).unwrap_or_else(|err| {
            if path.exists() {
                tracing::warn!("settings.json could not be read, using defaults: {err:#}");
            }
            Settings::default()
        });
        value.upgrade();
        Self {
            path,
            value: RwLock::new(value),
        }
    }

    pub fn get(&self) -> Settings {
        self.value.read().clone()
    }

    /// Applies `f` and saves.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> CmdResult<Settings> {
        let mut guard = self.value.write();
        f(&mut guard);
        demido_core::fsx::write_json(&self.path, &*guard)?;
        Ok(guard.clone())
    }

    /// Merges a partial JSON object from the UI into the settings.
    pub fn patch(&self, patch: serde_json::Value) -> CmdResult<Settings> {
        let mut guard = self.value.write();
        let mut current = serde_json::to_value(&*guard)?;
        merge(&mut current, patch);
        *guard = serde_json::from_value(current)?;
        demido_core::fsx::write_json(&self.path, &*guard)?;
        Ok(guard.clone())
    }
}

/// Shallow-recursive JSON merge: objects merge key by key, everything else is replaced.
fn merge(target: &mut serde_json::Value, patch: serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(&k) {
                    Some(existing) if existing.is_object() && v.is_object() && k != "windowLayout" => {
                        merge(existing, v)
                    }
                    _ => {
                        t.insert(k, v);
                    }
                }
            }
        }
        (t, p) => *t = p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_merges_nested_maps_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path().join("settings.json"));
        store
            .patch(serde_json::json!({"toolGroups": {"coding": false}, "defaultModel": "local:x"}))
            .unwrap();
        let reloaded = SettingsStore::load(dir.path().join("settings.json")).get();
        assert_eq!(reloaded.default_model.as_deref(), Some("local:x"));
        assert!(!reloaded.tool_group_enabled("coding"));
        assert!(reloaded.tool_group_enabled("market"));
    }

    #[test]
    fn python_switched_off_before_coding_keeps_coding_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"toolGroups": {"market": true, "python": false, "files": true, "skills": true}}"#,
        )
        .unwrap();
        let loaded = SettingsStore::load(path.clone()).get();
        assert!(!loaded.tool_group_enabled("coding"));
        assert!(!loaded.tool_group_enabled("computer"), "commands were off with Python");
        assert!(!loaded.tool_groups.contains_key("python"));

        std::fs::write(
            &path,
            r#"{"toolGroups": {"python": true, "terminal": true, "pine": false}}"#,
        )
        .unwrap();
        let loaded = SettingsStore::load(path).get();
        assert!(loaded.tool_group_enabled("coding"));
        assert!(loaded.tool_group_enabled("computer"));
        assert!(loaded.tool_group_enabled("market"));
        assert_eq!(loaded.tool_groups.len(), 2);
    }

    #[test]
    fn computer_use_follows_coding_where_commands_were() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"toolGroups": {"coding": false, "files": true}}"#).unwrap();
        let loaded = SettingsStore::load(path.clone()).get();
        assert!(!loaded.tool_group_enabled("computer"));

        std::fs::write(&path, r#"{"toolGroups": {"coding": true, "apps": false}}"#).unwrap();
        let loaded = SettingsStore::load(path.clone()).get();
        assert!(loaded.tool_group_enabled("computer"));
        assert!(!loaded.tool_groups.contains_key("apps"));

        // Switched off on its own, it stays off.
        std::fs::write(&path, r#"{"toolGroups": {"coding": true, "computer": false}}"#).unwrap();
        assert!(!SettingsStore::load(path).get().tool_group_enabled("computer"));
    }

    #[test]
    fn files_from_before_a_setting_get_its_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"defaultModel": "local:x", "chatListOpen": false}"#).unwrap();
        let loaded = SettingsStore::load(path).get();
        assert_eq!(loaded.download_approval_seconds, 60);
        assert_eq!(loaded.permission_mode, PermissionMode::Balanced);
        assert_eq!(loaded.default_model.as_deref(), Some("local:x"));
        assert!(!loaded.chat_list_open);
    }

    #[test]
    fn files_from_before_the_updater_update_automatically_from_releases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"defaultModel": "local:x", "downloadApprovalSeconds": 30}"#).unwrap();
        let loaded = SettingsStore::load(path.clone()).get();
        assert_eq!(loaded.update_channel, UpdateChannel::Release);
        assert!(loaded.auto_update);
        assert_eq!(loaded.download_approval_seconds, 30);

        std::fs::write(&path, r#"{"updateChannel": "prerelease", "autoUpdate": false}"#).unwrap();
        let loaded = SettingsStore::load(path).get();
        assert_eq!(loaded.update_channel, UpdateChannel::Prerelease);
        assert!(!loaded.auto_update);
    }

    #[test]
    fn compaction_is_on_by_default_and_its_threshold_can_be_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::load(dir.path().join("settings.json"));
        assert!(store.get().auto_compact);
        assert_eq!(store.get().auto_compact_tokens, None);
        store.patch(serde_json::json!({"autoCompactTokens": 12_300})).unwrap();
        assert_eq!(store.get().auto_compact_tokens, Some(12_300));
        store.patch(serde_json::json!({"autoCompactTokens": null})).unwrap();
        assert_eq!(store.get().auto_compact_tokens, None);
    }
}
