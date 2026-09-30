//! What each model can do: read images, read sound, use tools, think before answering. The
//! app shows them as icons wherever a model can be picked.
//!
//! Every answer comes from a source that knows, never from a guess:
//!
//! - **Local models**: llama.cpp itself ([`super::probe`]), once the model is on disk. The
//!   answer is kept in `cache/capabilities.json` for that file, projector and llama.cpp build,
//!   and asked again when any of them changes.
//! - **Gemini**: Google's model list says which models think; [models.dev](https://models.dev),
//!   an open database of model specifications, says which use tools and read images or sound.
//!   Its Google entries are kept in `cache/models-dev.json` and refreshed weekly.
//!
//! A model neither source knows about has unknown capabilities, shown as such.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::now_ms;

/// What a model can do. `None` while nobody has said.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Reads images.
    pub vision: Option<bool>,
    /// Reads sound.
    pub audio: Option<bool>,
    /// Calls tools.
    pub tools: Option<bool>,
    /// Thinks before answering.
    pub thinking: Option<bool>,
}

// ---------------------------------------------------------------- local models

/// What makes a local model's answer current: the file, the projector it is paired with and
/// the llama.cpp build that answered.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalKey {
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
    pub projector: Option<PathBuf>,
    pub runtime: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalRecord {
    key: LocalKey,
    capabilities: Capabilities,
    /// Why llama.cpp could not say (the model does not load, for example). Kept only while the
    /// app runs, so the next start asks again.
    error: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct LocalFile {
    models: BTreeMap<PathBuf, LocalRecord>,
}

/// llama.cpp's answers per model file.
pub struct LocalChecks {
    file: PathBuf,
    records: BTreeMap<PathBuf, LocalRecord>,
}

impl LocalChecks {
    pub fn load(file: PathBuf) -> Self {
        let records = demido_core::fsx::read_json::<LocalFile>(&file)
            .map(|f| f.models)
            .unwrap_or_default();
        Self { file, records }
    }

    /// The answer for `model`, if llama.cpp gave one for exactly this key.
    pub fn get(&self, model: &Path, key: &LocalKey) -> Option<Capabilities> {
        self.records
            .get(model)
            .filter(|r| &r.key == key)
            .map(|r| r.capabilities)
    }

    pub fn is_current(&self, model: &Path, key: &LocalKey) -> bool {
        self.records.get(model).is_some_and(|r| &r.key == key)
    }

    /// Records an answer (or the reason there is none) and forgets models that are gone.
    pub fn record(&mut self, model: &Path, key: LocalKey, result: Result<Capabilities, String>, present: &[PathBuf]) {
        let (capabilities, error) = match result {
            Ok(c) => (c, None),
            Err(e) => (Capabilities::default(), Some(e)),
        };
        self.records.insert(
            model.to_path_buf(),
            LocalRecord {
                key,
                capabilities,
                error,
            },
        );
        self.records.retain(|path, _| present.contains(path));
        self.save();
    }

    /// Forgets models that are no longer on disk.
    pub fn forget_missing(&mut self, present: &[PathBuf]) {
        let before = self.records.len();
        self.records.retain(|path, _| present.contains(path));
        if self.records.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        let answered = self
            .records
            .iter()
            .filter(|(_, r)| r.error.is_none())
            .map(|(path, r)| (path.clone(), r.clone()))
            .collect();
        let saved = demido_core::fsx::write_json(&self.file, &LocalFile { models: answered });
        if let Err(e) = saved {
            tracing::warn!("could not save model capabilities: {e}");
        }
    }
}

// ---------------------------------------------------------------- cloud models

const MODELS_DEV: &str = "https://models.dev/api.json";
/// models.dev's key for the Gemini API.
const GOOGLE: &str = "google";
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudEntry {
    pub vision: bool,
    pub audio: bool,
    pub tools: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CloudFile {
    fetched_at: Option<i64>,
    etag: Option<String>,
    google: BTreeMap<String, CloudEntry>,
}

/// models.dev's entries for Google's models.
pub struct CloudCatalog {
    file: PathBuf,
    data: CloudFile,
}

impl CloudCatalog {
    pub fn load(file: PathBuf) -> Self {
        let data = demido_core::fsx::read_json::<CloudFile>(&file).unwrap_or_default();
        Self { file, data }
    }

    /// Capabilities of a Gemini model: thinking from Google, the rest from models.dev.
    pub fn gemini(&self, model: &str, thinking: bool) -> Capabilities {
        let entry = self.data.google.get(model);
        Capabilities {
            vision: entry.map(|e| e.vision),
            audio: entry.map(|e| e.audio),
            tools: entry.map(|e| e.tools),
            thinking: Some(thinking),
        }
    }

    pub fn is_stale(&self) -> bool {
        self.data
            .fetched_at
            .is_none_or(|at| now_ms() - at > REFRESH_AFTER.as_millis() as i64)
    }

    /// ETag of the copy kept, so an unchanged catalog is not downloaded again.
    pub fn etag(&self) -> Option<String> {
        self.data.etag.clone()
    }

    /// Keeps a fresh download (`Some`), or notes that the copy kept is still current (`None`).
    pub fn update(&mut self, fetched: Option<(BTreeMap<String, CloudEntry>, Option<String>)>) {
        if let Some((google, etag)) = fetched {
            self.data.google = google;
            self.data.etag = etag;
        }
        self.data.fetched_at = Some(now_ms());
        if let Err(e) = demido_core::fsx::write_json(&self.file, &self.data) {
            tracing::warn!("could not save the models.dev catalog: {e}");
        }
    }
}

/// Downloads models.dev's catalog unless it still has `etag`. `Ok(None)` means unchanged.
pub async fn fetch_catalog(
    http: &reqwest::Client,
    etag: Option<&str>,
) -> anyhow::Result<Option<(BTreeMap<String, CloudEntry>, Option<String>)>> {
    let mut req = http.get(MODELS_DEV).timeout(Duration::from_secs(60));
    if let Some(tag) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = req.send().await?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    let resp = resp.error_for_status()?;
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let catalog: Value = resp.json().await?;
    Ok(Some((google_entries(&catalog), etag)))
}

/// `{"google": {"models": {"gemini-2.5-flash": {"tool_call": true, "modalities": {"input":
/// ["text", "image", ...]}}}}}` → one entry per model.
fn google_entries(catalog: &Value) -> BTreeMap<String, CloudEntry> {
    catalog[GOOGLE]["models"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, m)| {
            let input: Vec<&str> = m["modalities"]["input"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let entry = CloudEntry {
                vision: input.contains(&"image"),
                audio: input.contains(&"audio"),
                tools: m["tool_call"].as_bool().unwrap_or(false),
            };
            (id.clone(), entry)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn google_entries_read_tools_and_inputs() {
        let catalog = json!({
            "google": {"models": {
                "gemini-2.5-flash": {
                    "tool_call": true,
                    "modalities": {"input": ["text", "image", "audio", "video", "pdf"], "output": ["text"]},
                },
                "gemini-2.5-flash-image": {
                    "tool_call": false,
                    "modalities": {"input": ["text", "image"], "output": ["text", "image"]},
                },
            }},
            "openai": {"models": {"gpt-5": {"tool_call": true}}},
        });
        let entries = google_entries(&catalog);
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries["gemini-2.5-flash"],
            CloudEntry {
                vision: true,
                audio: true,
                tools: true,
            }
        );
        assert_eq!(
            entries["gemini-2.5-flash-image"],
            CloudEntry {
                vision: true,
                audio: false,
                tools: false,
            }
        );
    }

    #[test]
    fn a_gemini_model_models_dev_does_not_know_only_has_thinking() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = CloudCatalog::load(dir.path().join("models-dev.json"));
        assert_eq!(
            catalog.gemini("gemini-9-ultra", true),
            Capabilities {
                thinking: Some(true),
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_local_answer_holds_only_for_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut checks = LocalChecks::load(dir.path().join("capabilities.json"));
        let model = PathBuf::from("m.gguf");
        let key = LocalKey {
            size: 10,
            modified: Some(1),
            projector: None,
            runtime: "b11146".into(),
        };
        let caps = Capabilities {
            tools: Some(true),
            ..Default::default()
        };
        checks.record(&model, key.clone(), Ok(caps), std::slice::from_ref(&model));
        assert_eq!(checks.get(&model, &key), Some(caps));
        let newer_runtime = LocalKey {
            runtime: "b12000".into(),
            ..key.clone()
        };
        assert_eq!(checks.get(&model, &newer_runtime), None);

        // Kept across restarts, and forgotten once the file is gone.
        let mut checks = LocalChecks::load(dir.path().join("capabilities.json"));
        assert!(checks.is_current(&model, &key));
        let other = PathBuf::from("other.gguf");
        checks.record(&other, key.clone(), Err("no".into()), std::slice::from_ref(&other));
        assert!(!checks.is_current(&model, &key));

        // A failure stands for this run only: unknown now, asked again at the next start.
        assert!(checks.is_current(&other, &key));
        assert_eq!(checks.get(&other, &key), Some(Capabilities::default()));
        let reloaded = LocalChecks::load(dir.path().join("capabilities.json"));
        assert!(!reloaded.is_current(&other, &key));

        checks.forget_missing(&[]);
        assert!(!checks.is_current(&other, &key));
    }
}
