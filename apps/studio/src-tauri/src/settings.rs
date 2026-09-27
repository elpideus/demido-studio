//! `settings.json`: small preferences that are not chats, models, providers or skills.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error::CmdResult;

/// Version of the safety notice. Bumping it shows the notice again to everyone.
pub const DISCLAIMER_VERSION: &str = "2026-09";

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
}

impl Default for Settings {
    fn default() -> Self {
        let tool_groups = ["market", "python", "files", "skills"]
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
        }
    }
}

impl Settings {
    pub fn tool_group_enabled(&self, group: &str) -> bool {
        self.tool_groups.get(group).copied().unwrap_or(true)
    }
}

pub struct SettingsStore {
    path: PathBuf,
    value: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let value = demido_core::fsx::read_json::<Settings>(&path).unwrap_or_else(|err| {
            if path.exists() {
                tracing::warn!("settings.json could not be read, using defaults: {err:#}");
            }
            Settings::default()
        });
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
            .patch(serde_json::json!({"toolGroups": {"python": false}, "defaultModel": "local:x"}))
            .unwrap();
        let reloaded = SettingsStore::load(dir.path().join("settings.json")).get();
        assert_eq!(reloaded.default_model.as_deref(), Some("local:x"));
        assert!(!reloaded.tool_group_enabled("python"));
        assert!(reloaded.tool_group_enabled("market"));
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
}
