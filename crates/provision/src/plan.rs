use std::path::PathBuf;
use std::sync::Arc;

use demido_catalog::{Catalog, ModelPick};
use demido_core::{Backend, InstallScope};
use serde::{Deserialize, Serialize};

/// The app itself, as a zip archive embedded in the installer.
#[derive(Clone)]
pub struct AppPayload {
    pub zip: Arc<[u8]>,
    /// The installer executable, copied into the install folder as the uninstaller.
    pub uninstaller_source: Option<PathBuf>,
}

impl std::fmt::Debug for AppPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppPayload")
            .field("zip_bytes", &self.zip.len())
            .field("uninstaller_source", &self.uninstaller_source)
            .finish()
    }
}

/// Everything the person chose, resolved against the catalog.
#[derive(Clone, Debug)]
pub struct InstallPlan {
    pub scope: InstallScope,
    pub install_dir: PathBuf,
    pub backend: Backend,
    /// Catalog runtime variant id, e.g. `windows-cuda13`.
    pub variant: String,
    /// Starter model; `None` skips the download.
    pub model: Option<ModelPick>,
    pub model_context: u32,
    pub models_dir: PathBuf,
    pub python: bool,
    pub node: bool,
    pub shortcuts: bool,
    /// Registers the uninstaller with the OS. Off for development installs.
    pub register: bool,
    pub payload: Option<AppPayload>,
    /// Hardware snapshot stored in the manifest.
    pub hardware: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepId {
    App,
    Runtime,
    Node,
    Uv,
    Python,
    PythonPackages,
    Model,
    Shortcuts,
    Finalize,
}

impl StepId {
    /// A failed critical step aborts the installation.
    pub fn critical(self) -> bool {
        matches!(self, StepId::App | StepId::Finalize)
    }
}

/// A step as the installer lists it before it starts.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    pub id: StepId,
    pub label: String,
    pub detail: String,
    /// Bytes to download, or an estimate; used to weight overall progress.
    pub size: u64,
}

impl InstallPlan {
    /// The ordered steps this plan runs.
    pub fn steps(&self, catalog: &Catalog) -> Vec<StepInfo> {
        let rt = &catalog.runtimes;
        let mut steps = Vec::new();
        if let Some(payload) = &self.payload {
            steps.push(StepInfo {
                id: StepId::App,
                label: "Demido Studio".into(),
                detail: format!("Version {}", demido_core::brand::VERSION),
                size: payload.zip.len() as u64,
            });
        }
        if let Some(variant) = rt.llama_cpp.variant(&self.variant) {
            steps.push(StepInfo {
                id: StepId::Runtime,
                label: format!("AI runtime ({})", self.backend.label()),
                detail: format!("{} · {}", rt.llama_cpp.label, variant.label),
                size: variant.download_size(),
            });
        }
        let (os, arch) = (demido_core::Os::current(), demido_core::Arch::current());
        if self.node
            && let Some(asset) = rt.node.for_platform(os, arch)
        {
            steps.push(StepInfo {
                id: StepId::Node,
                label: "Node.js".into(),
                detail: format!("Version {} · runs the market data service", rt.node.version),
                size: asset.size,
            });
        }
        if self.python
            && let Some(asset) = rt.uv.for_platform(os, arch)
        {
            steps.push(StepInfo {
                id: StepId::Uv,
                label: "uv".into(),
                detail: format!("Version {} · manages Python", rt.uv.version),
                size: asset.size,
            });
            steps.push(StepInfo {
                id: StepId::Python,
                label: format!("Python {}", rt.python.version),
                detail: "Lets the assistant run analysis code".into(),
                size: rt.python.estimated_size,
            });
            steps.push(StepInfo {
                id: StepId::PythonPackages,
                label: "Data packages".into(),
                detail: rt.python.packages.join(", "),
                size: rt.python.estimated_packages_size,
            });
        }
        if let Some(model) = &self.model {
            steps.push(StepInfo {
                id: StepId::Model,
                label: model.name.clone(),
                detail: format!("{} · {}", model.quant, model.repo),
                size: model.size,
            });
        }
        if self.shortcuts || self.register {
            steps.push(StepInfo {
                id: StepId::Shortcuts,
                label: "Shortcuts".into(),
                detail: "Start menu and desktop".into(),
                size: 0,
            });
        }
        steps.push(StepInfo {
            id: StepId::Finalize,
            label: "Finishing up".into(),
            detail: "Writing the install manifest".into(),
            size: 0,
        });
        steps
    }

    /// Total bytes the plan downloads or unpacks.
    pub fn total_size(&self, catalog: &Catalog) -> u64 {
        self.steps(catalog).iter().map(|s| s.size).sum()
    }
}
