use std::path::{Path, PathBuf};
use std::sync::Arc;

use demido_catalog::{BackendChoice, Catalog, ModelPick, SearchModel};
use demido_core::{Backend, InstallManifest, InstallScope};
use demido_hardware::HardwareReport;
use serde::{Deserialize, Serialize};

/// The app itself, as a zip archive embedded in the installer.
#[derive(Clone)]
pub struct AppPayload {
    pub zip: Arc<[u8]>,
    /// The installer executable, copied into the install folder as the uninstaller.
    pub uninstaller_source: Option<PathBuf>,
    /// The copied uninstaller must match the release signature next to `uninstaller_source`
    /// (`<source>.sig`, which the app writes next to every installer it stages). Set when setup
    /// runs elevated for an update the app started: it then runs from the app's updates folder,
    /// where the file can be replaced while setup runs, and copies itself where only
    /// administrators can write. Otherwise a signature there is checked when there is one, and
    /// none is fine: an installer someone downloads and opens has none next to it.
    pub signature_required: bool,
}

impl std::fmt::Debug for AppPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppPayload")
            .field("zip_bytes", &self.zip.len())
            .field("uninstaller_source", &self.uninstaller_source)
            .field("signature_required", &self.signature_required)
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
    /// Embedding model for searching attached files by meaning; `None` skips it.
    pub search_model: Option<SearchModel>,
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
    SearchModel,
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
    /// The plan that updates the installation in `install_dir`, whose `install.json` is
    /// `manifest`, to the app in `payload`, keeping what was chosen when it was installed: its
    /// scope, its runtime and its models folder. When the catalog no longer has the installed
    /// runtime build, the same backend's build for this machine takes its place, or failing that
    /// the one setup would pick today. The runtime and tool steps skip what is already installed
    /// at the version the catalog pins, so an update downloads only what changed.
    ///
    /// No starter model is downloaded, and no shortcut is made again: they point at the same
    /// executable, and one the person deleted stays deleted. The entry in Installed apps is
    /// written again, so it shows the new version and size. The search model came after the
    /// first releases: an installation without one gets the one setup would pick today, and one
    /// that has one keeps it.
    pub fn for_update(
        install_dir: &Path,
        manifest: &InstallManifest,
        hardware: &HardwareReport,
        catalog: &Catalog,
        payload: Option<AppPayload>,
    ) -> Self {
        let choices = demido_catalog::backend_choices(hardware, catalog);
        let usable = |backend: Backend| {
            choices
                .iter()
                .find(|c| c.backend == backend && c.available)
                .and_then(|c| Some((c.backend, c.variant.clone()?)))
        };
        let default = demido_catalog::default_backend(&choices);
        let fallback = || usable(default).unwrap_or((default, String::new()));
        let (backend, variant) = match &manifest.runtime {
            Some(r) if catalog.runtimes.llama_cpp.variant(&r.variant).is_some() => (r.backend, r.variant.clone()),
            Some(r) => usable(r.backend).unwrap_or_else(fallback),
            None => fallback(),
        };
        // Only a starter model uses it, and an update downloads none; kept sensible all the same.
        let recommended = |c: &BackendChoice| demido_catalog::recommend_models(c, catalog).context_length;
        let models_dir = manifest
            .models_dir
            .clone()
            .unwrap_or_else(|| demido_core::paths::starter_models_dir(manifest.scope));
        let installed_search = catalog
            .models
            .search
            .models
            .iter()
            .find(|m| {
                models_dir
                    .join(m.repo.replace('/', std::path::MAIN_SEPARATOR_STR))
                    .join(&m.file)
                    .is_file()
            })
            .cloned();
        let search_model = installed_search.or_else(|| {
            choices
                .iter()
                .find(|c| c.backend == backend && c.available)
                .map(|c| demido_catalog::search_model(c, catalog).clone())
        });
        let model_context = choices
            .iter()
            .find(|c| c.backend == backend && c.available)
            .map(recommended)
            .or_else(|| manifest.starter_model.as_ref().map(|m| m.context_length))
            .or_else(|| choices.iter().find(|c| c.backend == default).map(recommended))
            .unwrap_or(4096);
        InstallPlan {
            scope: manifest.scope,
            install_dir: install_dir.to_path_buf(),
            backend,
            variant,
            model: None,
            search_model,
            model_context,
            models_dir,
            python: true,
            node: true,
            shortcuts: false,
            register: payload.is_some(),
            payload,
            hardware: serde_json::to_value(hardware).unwrap_or_default(),
        }
    }

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
        if let Some(search) = &self.search_model {
            steps.push(StepInfo {
                id: StepId::SearchModel,
                label: "Search model".into(),
                detail: format!("{} · finds what you ask about in attached files", search.name),
                size: search.size,
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

#[cfg(test)]
mod tests {
    use demido_core::manifest::RuntimeInfo;
    use demido_core::{Arch, Os};
    use demido_hardware::{CpuInfo, GIB, GpuInfo, GpuVendor};

    use super::*;

    /// A Windows PC with `gpus`, where the catalog's Windows builds apply.
    fn pc(gpus: Vec<GpuInfo>) -> HardwareReport {
        HardwareReport {
            os: Os::Windows,
            arch: Arch::X86_64,
            os_version: "test".into(),
            cpu: CpuInfo {
                name: "Test CPU".into(),
                physical_cores: 8,
                logical_cores: 16,
                avx2: true,
                avx512: false,
            },
            total_memory: 32 * GIB,
            available_memory: 16 * GIB,
            gpus,
        }
    }

    /// A card CUDA 13 runs on.
    fn rtx_3060() -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 3060".into(),
            vram: 12 * GIB,
            integrated: false,
            driver_version: Some("596.49".into()),
            compute_capability: Some(8.6),
            source: "test".into(),
        }
    }

    fn installed(scope: InstallScope, runtime: Option<(Backend, &str)>) -> InstallManifest {
        let mut m = InstallManifest::new(scope);
        m.runtime = runtime.map(|(backend, variant)| RuntimeInfo {
            backend,
            variant: variant.into(),
            release: "b1".into(),
            dir: "runtime/llama".into(),
            server: "runtime/llama/llama-server.exe".into(),
        });
        m
    }

    fn update(manifest: &InstallManifest, hardware: &HardwareReport) -> InstallPlan {
        InstallPlan::for_update(
            Path::new(r"C:\Apps\Demido Studio"),
            manifest,
            hardware,
            demido_catalog::catalog(),
            None,
        )
    }

    #[test]
    fn an_update_keeps_the_installed_runtime_build() {
        // CUDA 13 would be picked today; the CUDA 12 build is still in the catalog.
        let plan = update(
            &installed(InstallScope::User, Some((Backend::Cuda, "windows-cuda12"))),
            &pc(vec![rtx_3060()]),
        );
        assert_eq!((plan.backend, plan.variant.as_str()), (Backend::Cuda, "windows-cuda12"));
    }

    #[test]
    fn a_build_the_catalog_dropped_gives_way_to_the_same_backends() {
        let plan = update(
            &installed(InstallScope::User, Some((Backend::Cuda, "windows-cuda11"))),
            &pc(vec![rtx_3060()]),
        );
        assert_eq!((plan.backend, plan.variant.as_str()), (Backend::Cuda, "windows-cuda13"));
    }

    #[test]
    fn a_dropped_build_whose_backend_no_longer_runs_gives_way_to_the_default() {
        let plan = update(
            &installed(InstallScope::User, Some((Backend::Rocm, "windows-rocm5"))),
            &pc(vec![]),
        );
        assert_eq!((plan.backend, plan.variant.as_str()), (Backend::Cpu, "windows-cpu"));
    }

    #[test]
    fn without_a_runtime_an_update_installs_the_default_one() {
        let plan = update(&installed(InstallScope::User, None), &pc(vec![rtx_3060()]));
        assert_eq!((plan.backend, plan.variant.as_str()), (Backend::Cuda, "windows-cuda13"));
        let plan = update(&installed(InstallScope::User, None), &pc(vec![]));
        assert_eq!((plan.backend, plan.variant.as_str()), (Backend::Cpu, "windows-cpu"));
    }

    #[test]
    fn an_update_keeps_the_scope_and_models_folder_and_adds_nothing_new() {
        let hardware = pc(vec![rtx_3060()]);
        let mut manifest = installed(InstallScope::Machine, Some((Backend::Cuda, "windows-cuda13")));
        manifest.models_dir = Some(PathBuf::from(r"D:\Models"));
        let payload = AppPayload {
            zip: Arc::from(Vec::new()),
            uninstaller_source: None,
            signature_required: false,
        };
        let plan = InstallPlan::for_update(
            Path::new(r"C:\Program Files\Demido Studio"),
            &manifest,
            &hardware,
            demido_catalog::catalog(),
            Some(payload),
        );
        assert_eq!(plan.scope, InstallScope::Machine);
        assert_eq!(plan.install_dir, Path::new(r"C:\Program Files\Demido Studio"));
        assert_eq!(plan.models_dir, Path::new(r"D:\Models"));
        assert!(plan.model.is_none(), "an update never downloads a starter model");
        assert!(!plan.shortcuts, "an update never makes shortcuts again");
        assert!(plan.register, "the Installed apps entry shows the new version");
        assert!(plan.python && plan.node);
        assert!(plan.model_context > 0);
        assert_eq!(plan.hardware["gpus"][0]["name"], "NVIDIA GeForce RTX 3060");
        let steps: Vec<StepId> = plan.steps(demido_catalog::catalog()).iter().map(|s| s.id).collect();
        assert!(!steps.contains(&StepId::Model), "{steps:?}");
        assert!(steps.contains(&StepId::SearchModel), "{steps:?}");
        assert_eq!(
            plan.search_model.map(|m| m.id).as_deref(),
            Some("qwen3-embedding-0.6b"),
            "a 12 GB GPU has room for the larger search model"
        );

        // Without a recorded models folder, the scope's; without an app, nothing to register.
        manifest.models_dir = None;
        let plan = update(&manifest, &hardware);
        assert_eq!(
            plan.models_dir,
            demido_core::paths::starter_models_dir(InstallScope::Machine)
        );
        assert!(!plan.register);
    }
}
