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
    /// Tools the person chose to always allow without asking.
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
    /// Which releases the updater offers.
    pub update_channel: UpdateChannel,
    /// Download new versions in the background and install them the next time the app starts.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let tool_groups = ["market", "coding", "files", "skills"]
            .into_iter()
            .map(|g| (g.to_string(), true))
            .collect();
        Settings {
            disclaimer_accepted: None,
            default_model: None,
            preload_default_model: true,
            tool_groups,
            always_allowed_tools: BTreeSet::new(),
            extra_model_dirs: Vec::new(),
            chat_list_open: true,
            window_layout: serde_json::Value::Null,
            last_chat_id: None,
            download_approval_seconds: 60,
            update_channel: UpdateChannel::Release,
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn tool_group_enabled(&self, group: &str) -> bool {
        self.tool_groups.get(group).copied().unwrap_or(true)
    }

    /// Brings switches saved by an older version up to date. Python and the terminal became one
    /// group, Coding, which stays off if either was switched off; Pine scripts joined Market data.
    fn upgrade(&mut self) {
        if !self.tool_groups.contains_key("coding") {
            let coding = ["python", "terminal"].iter().all(|g| self.tool_group_enabled(g));
            self.tool_groups.insert("coding".into(), coding);
        }
        for old in ["python", "terminal", "pine"] {
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
        assert!(!loaded.tool_groups.contains_key("python"));

        std::fs::write(
            &path,
            r#"{"toolGroups": {"python": true, "terminal": true, "pine": false}}"#,
        )
        .unwrap();
        let loaded = SettingsStore::load(path).get();
        assert!(loaded.tool_group_enabled("coding"));
        assert!(loaded.tool_group_enabled("market"));
        assert_eq!(loaded.tool_groups.len(), 1);
    }

    #[test]
    fn files_from_before_a_setting_get_its_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"defaultModel": "local:x", "chatListOpen": false}"#).unwrap();
        let loaded = SettingsStore::load(path).get();
        assert_eq!(loaded.download_approval_seconds, 60);
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
}
