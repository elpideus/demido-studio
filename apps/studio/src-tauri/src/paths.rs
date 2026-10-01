//! Where Demido Studio reads and writes.
//!
//! Program files come from the install folder the installer created (`install.json` beside the
//! executable). Everything a person makes lives in the per-user data folder. In development the
//! install folder is `.dev/install`, provisioned by `pnpm dev:runtime`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use demido_core::InstallManifest;
use demido_core::manifest::resolve;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    /// Folder holding `install.json`, if one was found.
    pub install_dir: Option<PathBuf>,
    #[serde(skip)]
    pub manifest: Option<InstallManifest>,
    /// Read-only files shipped with the app (default skills, the market service).
    pub resources_dir: PathBuf,
    pub data_dir: PathBuf,
    pub db_file: PathBuf,
    pub settings_file: PathBuf,
    pub models_file: PathBuf,
    pub providers_file: PathBuf,
    pub skills_state_file: PathBuf,
    pub skills_dir: PathBuf,
    /// Where models downloaded from the app are saved.
    pub models_dir: PathBuf,
    pub avatars_dir: PathBuf,
    pub workspaces_dir: PathBuf,
    /// Files attached in the composer, before their message is sent (emptied at every start).
    pub staging_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// Isolated browser profile for the TradingView sign-in window.
    pub tradingview_profile_dir: PathBuf,
    /// Downloaded updates waiting to be installed (see `updater::staging`). A development build
    /// has its own, see [`updates_dir`].
    pub updates_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let install_dir = find_install_dir();
        let manifest = install_dir.as_deref().and_then(|d| InstallManifest::load(d).ok());
        let data_dir = demido_core::paths::user_data_dir();
        let paths = AppPaths {
            resources_dir: find_resources_dir(),
            db_file: data_dir.join("demido.db"),
            settings_file: data_dir.join("settings.json"),
            models_file: data_dir.join("models.json"),
            providers_file: data_dir.join("providers.json"),
            skills_state_file: data_dir.join("skills.json"),
            skills_dir: data_dir.join("skills"),
            models_dir: data_dir.join("models"),
            avatars_dir: data_dir.join("avatars"),
            workspaces_dir: data_dir.join("workspaces"),
            staging_dir: data_dir.join("staging"),
            logs_dir: data_dir.join("logs"),
            cache_dir: data_dir.join("cache"),
            tradingview_profile_dir: data_dir.join("webview-tradingview"),
            updates_dir: updates_dir(cfg!(debug_assertions), &data_dir, &repo_root()),
            data_dir,
            install_dir,
            manifest,
        };
        for dir in [
            &paths.data_dir,
            &paths.skills_dir,
            &paths.models_dir,
            &paths.avatars_dir,
            &paths.workspaces_dir,
            &paths.staging_dir,
            &paths.logs_dir,
            &paths.cache_dir,
        ] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(paths)
    }

    fn install_path(&self, rel: &Path) -> Option<PathBuf> {
        self.install_dir.as_deref().map(|d| resolve(d, rel))
    }

    /// `llama-server` from the installed runtime.
    pub fn llama_server(&self) -> Option<PathBuf> {
        let rt = self.manifest.as_ref()?.runtime.as_ref()?;
        self.install_path(&rt.server).filter(|p| p.is_file())
    }

    pub fn node(&self) -> Option<PathBuf> {
        let tool = self.manifest.as_ref()?.node.as_ref()?;
        self.install_path(&tool.exe).filter(|p| p.is_file())
    }

    pub fn python(&self) -> Option<PathBuf> {
        let tool = self.manifest.as_ref()?.python.as_ref()?;
        self.install_path(&tool.exe).filter(|p| p.is_file())
    }

    /// Every folder scanned for GGUF models: the app's own, the installer's shared one.
    pub fn builtin_model_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.models_dir.clone()];
        if let Some(shared) = self.manifest.as_ref().and_then(|m| m.models_dir.clone())
            && !dirs.contains(&shared)
        {
            dirs.push(shared);
        }
        dirs
    }

    /// Bundled default skills.
    pub fn default_skills_dir(&self) -> PathBuf {
        self.resources_dir.join("skills")
    }

    /// The market data service script.
    pub fn market_service(&self) -> PathBuf {
        self.resources_dir.join("sidecars").join("market.mjs")
    }

    /// A chat's working folder, created on demand.
    pub fn workspace(&self, chat_id: &str) -> PathBuf {
        self.workspaces_dir.join(chat_id)
    }
}

fn find_install_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(demido_core::paths::INSTALL_DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    if exe_dir.join(InstallManifest::FILE_NAME).is_file() {
        return Some(exe_dir);
    }
    if cfg!(debug_assertions) {
        let dev = repo_root().join(".dev").join("install");
        if dev.join(InstallManifest::FILE_NAME).is_file() {
            return Some(dev);
        }
    }
    None
}

fn find_resources_dir() -> PathBuf {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("resources")))
        .filter(|d| d.is_dir())
    {
        return dir;
    }
    // Development: resources are assembled under `.dev/resources` by the dev scripts, falling
    // back to the repository layout.
    let dev = repo_root().join(".dev").join("resources");
    if dev.is_dir() { dev } else { repo_root() }
}

/// Where updates are downloaded: `<data>/updates`, except in a development (`debug`) build,
/// which uses `<repo>/.dev/updates`. A development build shares the data folder with the
/// installed app but still checks, downloads and verifies updates to test the Updates tab; in its
/// own folder its launch tidy-up never deletes what the installed app has staged, and its
/// downloads never replace it.
fn updates_dir(debug: bool, data_dir: &Path, repo: &Path) -> PathBuf {
    if debug {
        repo.join(".dev").join("updates")
    } else {
        data_dir.join("updates")
    }
}

/// Repository root at compile time (development builds only).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_development_build_keeps_its_updates_out_of_the_installed_apps_folder() {
        let data = Path::new("C:\\Users\\me\\AppData\\Local\\Demido Studio");
        let repo = Path::new("S:\\demido-studio");
        assert_eq!(updates_dir(false, data, repo), data.join("updates"));
        assert_eq!(updates_dir(true, data, repo), repo.join(".dev").join("updates"));
    }

    #[test]
    fn the_repository_root_holds_the_workspace() {
        assert!(repo_root().join("Cargo.toml").is_file());
        assert!(repo_root().join("apps").join("studio").is_dir());
    }
}
