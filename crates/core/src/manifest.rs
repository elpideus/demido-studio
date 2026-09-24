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
        }
    }

    /// Reads `install.json` from `install_dir`.
    pub fn load(install_dir: &Path) -> anyhow::Result<Self> {
        crate::fsx::read_json(&install_dir.join(Self::FILE_NAME))
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
