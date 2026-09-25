//! `install.json`: what the installer put on this machine, read by the app at startup.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::backend::Backend;

/// Who the installation is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    /// Only the current user; no administrator rights needed.
    #[default]
    User,
    /// Everyone on this computer; needs administrator rights.
    Machine,
}

/// The llama.cpp build that was fetched.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub backend: Backend,
    /// Catalog variant id, e.g. `windows-cuda13`.
    pub variant: String,
    /// llama.cpp build tag, e.g. `b11146`.
    pub release: String,
    /// Folder holding the runtime, relative to the install folder.
    pub dir: PathBuf,
    /// `llama-server` executable, relative to the install folder.
    pub server: PathBuf,
}

/// A command line tool that was fetched (Node, uv, Python).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub version: String,
    /// Executable, relative to the install folder.
    pub exe: PathBuf,
}

/// The model downloaded at install time.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarterModel {
    pub name: String,
    pub repo: String,
    pub file: String,
    pub context_length: u32,
    /// Absolute path of the GGUF file.
    pub path: PathBuf,
}

/// Written by the installer next to the app binary. Paths are relative to the install folder
/// unless stated otherwise, so the manifest stays readable and the folder stays relocatable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallManifest {
    pub schema: u32,
    pub version: String,
    pub scope: InstallScope,
    pub installed_at: DateTime<Utc>,
    pub runtime: Option<RuntimeInfo>,
    pub node: Option<ToolInfo>,
    pub uv: Option<ToolInfo>,
    pub python: Option<ToolInfo>,
    /// Folder the starter model was saved to (absolute). Shared by all users on a machine install.
    pub models_dir: Option<PathBuf>,
    pub starter_model: Option<StarterModel>,
    /// Hardware as seen at install time, kept for diagnostics only.
    #[serde(default)]
    pub hardware: serde_json::Value,
    /// Top-level names setup created in the install folder. The uninstaller removes only these,
    /// so nothing else that shares the folder is touched. `None` only in manifests from builds
    /// that were never released; setup treats those like an `install.json` it cannot read.
    #[serde(default)]
    pub created: Option<Vec<String>>,
}

impl InstallManifest {
    pub const FILE_NAME: &'static str = "install.json";
    pub const SCHEMA: u32 = 1;

    pub fn new(scope: InstallScope) -> Self {
        Self {
            schema: Self::SCHEMA,
            version: crate::brand::VERSION.to_string(),
            scope,
            installed_at: Utc::now(),
            runtime: None,
            node: None,
            uv: None,
            python: None,
            models_dir: None,
            starter_model: None,
            hardware: serde_json::Value::Null,
            created: Some(Vec::new()),
        }
    }

    /// Reads `install.json` from `install_dir`. One a newer setup wrote for a later schema fails
    /// like a damaged one, even when it parses: its fields may not mean what this build takes
    /// them to, so neither an update nor the uninstaller may act on it.
    pub fn load(install_dir: &Path) -> anyhow::Result<Self> {
        let path = install_dir.join(Self::FILE_NAME);
        let manifest: Self = crate::fsx::read_json(&path)?;
        if manifest.schema > Self::SCHEMA {
            anyhow::bail!(
                "{} has schema {}, newer than the {} this build reads",
                path.display(),
                manifest.schema,
                Self::SCHEMA
            );
        }
        Ok(manifest)
    }

    /// Writes `install.json` into `install_dir`.
    pub fn save(&self, install_dir: &Path) -> anyhow::Result<()> {
        crate::fsx::write_json(&install_dir.join(Self::FILE_NAME), self)
    }
}

/// Resolves a manifest path against the install folder (absolute paths pass through).
pub fn resolve(install_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        install_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = InstallManifest::new(InstallScope::User);
        m.runtime = Some(RuntimeInfo {
            backend: Backend::Cuda,
            variant: "windows-cuda13".into(),
            release: "b11146".into(),
            dir: "runtime/llama".into(),
            server: "runtime/llama/llama-server.exe".into(),
        });
        m.save(dir.path()).unwrap();
        let back = InstallManifest::load(dir.path()).unwrap();
        assert_eq!(back, m);
        let raw = std::fs::read_to_string(dir.path().join("install.json")).unwrap();
        assert!(raw.contains("\"backend\": \"cuda\""));
    }

    #[test]
    fn records_what_setup_created() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = InstallManifest::new(InstallScope::User);
        m.created = Some(vec!["runtime".into(), "install.json".into()]);
        m.save(dir.path()).unwrap();
        let back = InstallManifest::load(dir.path()).unwrap();
        assert_eq!(back.created, m.created);
    }

    #[test]
    fn manifests_from_before_created_was_recorded_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = serde_json::to_value(InstallManifest::new(InstallScope::Machine)).unwrap();
        value.as_object_mut().unwrap().remove("created");
        std::fs::write(dir.path().join("install.json"), value.to_string()).unwrap();
        let back = InstallManifest::load(dir.path()).unwrap();
        assert_eq!(back.created, None);
        assert_eq!(back.scope, InstallScope::Machine);
    }

    #[test]
    fn a_newer_schema_does_not_load_and_older_ones_do() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = InstallManifest::new(InstallScope::User);
        m.schema = InstallManifest::SCHEMA + 1;
        m.save(dir.path()).unwrap();
        assert!(InstallManifest::load(dir.path()).is_err());
        for schema in [0, InstallManifest::SCHEMA] {
            m.schema = schema;
            m.save(dir.path()).unwrap();
            assert_eq!(InstallManifest::load(dir.path()).unwrap(), m);
        }
    }

    #[test]
    fn resolves_relative_and_absolute() {
        let base = Path::new("/opt/demido");
        assert_eq!(
            resolve(base, Path::new("runtime/x")),
            Path::new("/opt/demido/runtime/x")
        );
        let abs = std::env::temp_dir();
        assert_eq!(resolve(base, &abs), abs);
    }
}
