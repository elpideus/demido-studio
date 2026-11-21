//! What Demido has learned about each model on this computer, kept.
//!
//! Some things about a model are only known once the runtime has looked at it or loaded it: what it needs
//! on the graphics card, how much it can hold in mind, how long it takes to load. They cost time to find out
//! and do not change while the file stays the same, so they are written down the moment they are known
//! (the caching rule, DECISIONS P) and shown even when the runtime is not running.
//!
//! These are also the first lines of the roster that models will be given about each other, so that an
//! orchestrating model can pick a model for each agent (DECISIONS O1). Not there yet: how fast each model
//! answers. That comes with the chat loop, from real answers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Which file the facts are about. Another file under the same name starts from nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Identity {
    /// Bytes on disk.
    pub size: u64,
    /// When it last changed, in milliseconds since 1970.
    pub changed_ms: u64,
}

impl Identity {
    /// Of a model file, or of a folder that holds a model of several files.
    pub fn of(path: &Path, size: u64) -> Self {
        let changed_ms = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
        Self { size, changed_ms }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Facts {
    file: Identity,
    /// The most the model itself can hold in mind, in tokens. A property of the file.
    pub context_length: Option<u64>,
    /// How long the last load took, in milliseconds.
    pub load_ms: Option<u64>,
    /// What it needs on the graphics card, in MiB. True only for the context and the runtime it was measured with.
    needs_mb: Option<u64>,
    measured_with: Option<(u64, String)>,
}

impl Facts {
    /// The memory need, if it was measured with this context and this runtime.
    pub fn needs_mb(&self, context: u64, runtime: &str) -> Option<u64> {
        self.measured_with.as_ref().filter(|(c, r)| *c == context && r == runtime).and(self.needs_mb)
    }
}

pub(super) struct Known {
    path: PathBuf,
    by_id: Mutex<HashMap<String, Facts>>,
}

impl Known {
    /// Reads what was kept. A missing or unreadable file means nothing is known yet.
    pub fn open(path: PathBuf) -> Self {
        let by_id = std::fs::read(&path).ok().and_then(|text| serde_json::from_slice(&text).ok()).unwrap_or_default();
        Self { path, by_id: Mutex::new(by_id) }
    }

    /// What is known about this model, if the file is still the one the facts are about.
    pub fn about(&self, id: &str, file: Identity) -> Option<Facts> {
        self.by_id.lock().unwrap_or_else(|e| e.into_inner()).get(id).filter(|f| f.file == file).cloned()
    }

    /// Adds to what is known, and writes it down if anything is new.
    pub fn learn(&self, id: &str, file: Identity, context_length: Option<u64>, needs: Option<(u64, u64, &str)>, load_ms: Option<u64>) {
        let mut all = self.by_id.lock().unwrap_or_else(|e| e.into_inner());
        let before = all.get(id).cloned();
        // A changed file starts from nothing.
        let mut facts = before.clone().filter(|f| f.file == file).unwrap_or(Facts { file, ..Facts::default() });
        if context_length.is_some() {
            facts.context_length = context_length;
        }
        if let Some((needs_mb, context, runtime)) = needs {
            facts.needs_mb = Some(needs_mb);
            facts.measured_with = Some((context, runtime.to_string()));
        }
        if load_ms.is_some() {
            facts.load_ms = load_ms;
        }
        if before.as_ref() == Some(&facts) {
            return;
        }
        all.insert(id.to_string(), facts);
        // Written beside its place and moved there, so a crash never leaves half a file.
        let saved = (|| -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let beside = self.path.with_extension("json.new");
            std::fs::write(&beside, serde_json::to_vec_pretty(&*all).unwrap_or_default())?;
            std::fs::rename(beside, &self.path)
        })();
        if let Err(e) = saved {
            tracing::warn!("What was learned about {id} could not be kept on disk: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_are_kept_until_what_they_are_about_changes() {
        let dir = std::env::temp_dir().join(format!("demido-known-{}", uuid::Uuid::new_v4()));
        let path = dir.join("engine").join("known.json");
        let file = Identity { size: 100, changed_ms: 5 };
        let known = Known::open(path.clone());
        assert_eq!(known.about("m", file), None);

        known.learn("m", file, Some(40_960), Some((1_814, 8_192, "llama.cpp-b1-cuda")), None);
        known.learn("m", file, None, None, Some(820));
        let facts = known.about("m", file).unwrap();
        assert_eq!((facts.context_length, facts.load_ms), (Some(40_960), Some(820)));
        assert_eq!(facts.needs_mb(8_192, "llama.cpp-b1-cuda"), Some(1_814));
        // The memory need was measured at one context with one runtime. It says nothing about another.
        assert_eq!(facts.needs_mb(4_096, "llama.cpp-b1-cuda"), None);
        assert_eq!(facts.needs_mb(8_192, "llama.cpp-b2-cuda"), None);

        // It is on disk: a new start knows it too.
        let again = Known::open(path.clone());
        assert_eq!(again.about("m", file), Some(facts));

        // Another file under the same name: nothing is known about it, and learning starts over.
        let other = Identity { size: 200, changed_ms: 9 };
        assert_eq!(again.about("m", other), None);
        again.learn("m", other, None, None, Some(1_500));
        let fresh = again.about("m", other).unwrap();
        assert_eq!((fresh.context_length, fresh.load_ms, fresh.needs_mb(8_192, "llama.cpp-b1-cuda")), (None, Some(1_500), None));
        std::fs::remove_dir_all(dir).ok();
    }
}
