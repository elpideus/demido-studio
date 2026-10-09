//! Turning a hardware report into choices: which runtime backends can run here, which one is
//! best, and which starter model fits.

use demido_core::Backend;
use demido_hardware::{GpuInfo, GpuVendor, HardwareReport};
use serde::{Deserialize, Serialize};

use crate::data::{Catalog, ModelPick, RuntimeVariant};

/// One runtime option as the installer shows it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackendChoice {
    pub backend: Backend,
    /// The machine can run this backend.
    pub available: bool,
    /// The best option for this machine. At most one choice is recommended.
    pub recommended: bool,
    /// Why the option is (not) available, in one short sentence.
    pub note: String,
    /// The device the backend would run on, e.g. `NVIDIA GeForce RTX 3060 · 12 GB`.
    pub device: Option<String>,
    /// Catalog variant that would be installed.
    pub variant: Option<String>,
    pub download_size: u64,
    /// Memory the model may use on this backend, in GiB. Models are matched against it.
    pub memory_budget_gb: f64,
    /// True when the budget is system RAM rather than dedicated VRAM.
    pub uses_system_memory: bool,
}

/// All five backends, in a fixed order, with availability worked out for this machine.
pub fn backend_choices(report: &HardwareReport, catalog: &Catalog) -> Vec<BackendChoice> {
    let variants_here: Vec<&RuntimeVariant> = catalog
        .runtimes
        .llama_cpp
        .variants
        .iter()
        .filter(|v| v.os == report.os && v.arch == report.arch)
        .collect();

    let discrete = |vendor: GpuVendor| {
        report
            .gpus
            .iter()
            .filter(|g| g.vendor == vendor && !g.integrated)
            .max_by_key(|g| g.vram)
    };
    let ram_gb = report.total_memory_gb();

    let mut choices: Vec<BackendChoice> = Backend::ALL
        .iter()
        .map(|&backend| {
            let candidates: Vec<&RuntimeVariant> =
                variants_here.iter().copied().filter(|v| v.backend == backend).collect();
            let mut choice = BackendChoice {
                backend,
                available: false,
                recommended: false,
                note: String::new(),
                device: None,
                variant: None,
                download_size: 0,
                memory_budget_gb: 0.0,
                uses_system_memory: false,
            };
            if candidates.is_empty() {
                choice.note = "Not available on this operating system.".into();
                return choice;
            }
            match backend {
                Backend::Cuda => match discrete(GpuVendor::Nvidia) {
                    None => choice.note = "No NVIDIA graphics card found.".into(),
                    Some(gpu) => match candidates.iter().find(|v| satisfies(v, gpu)) {
                        Some(v) => {
                            use_gpu(&mut choice, gpu, v);
                            choice.note = format!("Runs entirely on your {}.", short_name(gpu));
                        }
                        None => {
                            choice.device = Some(describe(gpu));
                            choice.note = format!(
                                "Your NVIDIA driver{} is too old. Update it to use CUDA.",
                                gpu.driver_version
                                    .as_deref()
                                    .map(|d| format!(" ({d})"))
                                    .unwrap_or_default()
                            );
                        }
                    },
                },
                Backend::Rocm => match discrete(GpuVendor::Amd) {
                    None => choice.note = "No AMD Radeon graphics card found.".into(),
                    Some(gpu) => {
                        use_gpu(&mut choice, gpu, candidates[0]);
                        choice.note = if gpu.rocm_supported() {
                            format!("Runs on your {} through AMD ROCm.", short_name(gpu))
                        } else {
                            "Your card is not officially supported by ROCm; Vulkan is safer.".into()
                        };
                    }
                },
                Backend::Metal => {
                    if report.is_apple_silicon() {
                        if let Some(gpu) = report.gpus.iter().find(|g| g.vendor == GpuVendor::Apple) {
                            use_gpu(&mut choice, gpu, candidates[0]);
                        } else {
                            choice.available = true;
                            choice.variant = Some(candidates[0].id.clone());
                            choice.download_size = candidates[0].download_size();
                            choice.memory_budget_gb = ram_gb * 0.6;
                        }
                        choice.note = "Uses the GPU cores and unified memory of your Mac.".into();
                    } else {
                        choice.note = "Needs a Mac with an Apple Silicon chip.".into();
                    }
                }
                Backend::Vulkan => {
                    let best = report.gpus.iter().find(|g| g.vendor != GpuVendor::Apple);
                    match best {
                        None => choice.note = "No graphics card found.".into(),
                        Some(gpu) => {
                            use_gpu(&mut choice, gpu, candidates[0]);
                            if gpu.integrated {
                                // Integrated GPUs share system memory; size models like the CPU.
                                choice.memory_budget_gb = ram_gb;
                                choice.uses_system_memory = true;
                            }
                            choice.note = format!("Works on your {}.", short_name(gpu));
                        }
                    }
                }
                Backend::Cpu => {
                    let v = candidates[0];
                    choice.available = true;
                    choice.variant = Some(v.id.clone());
                    choice.download_size = v.download_size();
                    choice.memory_budget_gb = ram_gb;
                    choice.uses_system_memory = true;
                    choice.device = Some(report.cpu.name.clone());
                    choice.note = format!(
                        "Runs on your {} with {:.0} GB of memory.",
                        short_cpu_name(&report.cpu.name),
                        ram_gb
                    );
                }
            }
            choice
        })
        .collect();

    // Exactly one recommendation, in order of preference. The CPU is never recommended: it is
    // only preselected when nothing else can run (see `default_backend`).
    let preferred = [
        (Backend::Cuda, true),
        (Backend::Metal, true),
        (
            Backend::Rocm,
            discrete(GpuVendor::Amd).is_some_and(GpuInfo::rocm_supported),
        ),
        (Backend::Vulkan, true),
    ];
    if let Some(&(winner, _)) = preferred
        .iter()
        .find(|(b, extra)| *extra && choices.iter().any(|c| c.backend == *b && c.available))
    {
        for c in &mut choices {
            c.recommended = c.backend == winner;
        }
    }
    choices
}

/// The backend to preselect: the recommended one, or the CPU when nothing else runs here.
pub fn default_backend(choices: &[BackendChoice]) -> Backend {
    choices
        .iter()
        .find(|c| c.recommended)
        .map(|c| c.backend)
        .unwrap_or(Backend::Cpu)
}

/// The starter model for a backend choice, one pick per family.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecommendation {
    pub tier: String,
    pub context_length: u32,
    /// `(family id, pick)`, in the catalog's family order.
    pub picks: Vec<(String, ModelPick)>,
    /// Family preselected in the installer.
    pub default_family: String,
}

/// The search model for a runtime choice: the larger one when the GPU it runs on has the memory
/// for it, the small one otherwise and on the CPU.
pub fn search_model<'a>(choice: &BackendChoice, catalog: &'a Catalog) -> &'a crate::SearchModel {
    let gpu_memory = (choice.backend.is_gpu() && !choice.uses_system_memory).then_some(choice.memory_budget_gb);
    catalog.models.search.for_memory(gpu_memory)
}

/// The speech model for a runtime choice, picked like the search model.
pub fn speech_model<'a>(choice: &BackendChoice, catalog: &'a Catalog) -> &'a crate::SpeechModel {
    let gpu_memory = (choice.backend.is_gpu() && !choice.uses_system_memory).then_some(choice.memory_budget_gb);
    catalog.models.speech.for_memory(gpu_memory)
}

pub fn recommend_models(choice: &BackendChoice, catalog: &Catalog) -> ModelRecommendation {
    let models = &catalog.models;
    let (tier, context_length, table) = if choice.uses_system_memory || !choice.backend.is_gpu() {
        let t = models
            .cpu_tiers
            .iter()
            .find(|t| choice.memory_budget_gb >= t.min_ram_gb)
            .or_else(|| models.cpu_tiers.last())
            .expect("catalog has CPU tiers");
        (t.id.clone(), t.context_length, &t.models)
    } else {
        match models.tiers.iter().find(|t| choice.memory_budget_gb >= t.min_vram_gb) {
            Some(t) => (t.id.clone(), t.context_length, &t.models),
            None => {
                let t = models.cpu_tiers.last().expect("catalog has CPU tiers");
                (t.id.clone(), t.context_length, &t.models)
            }
        }
    };
    let picks: Vec<(String, ModelPick)> = models
        .families
        .iter()
        .filter_map(|f| table.get(&f.id).map(|p| (f.id.clone(), p.clone())))
        .collect();
    let default_family = models
        .families
        .iter()
        .find(|f| f.recommended)
        .or(models.families.first())
        .map(|f| f.id.clone())
        .unwrap_or_default();
    ModelRecommendation {
        tier,
        context_length,
        picks,
        default_family,
    }
}

fn satisfies(variant: &RuntimeVariant, gpu: &GpuInfo) -> bool {
    let req = &variant.requires;
    if let Some(min) = &req.nvidia_driver_min {
        match gpu.driver_version.as_deref() {
            Some(have) if version_at_least(have, min) => {}
            // Unknown driver: only the most compatible build is a safe bet.
            None => {}
            _ => return false,
        }
    }
    if let (Some(min), Some(have)) = (req.compute_capability_min, gpu.compute_capability)
        && have + 1e-6 < min
    {
        return false;
    }
    true
}

/// Compares dotted driver versions such as `596.49` and `580.88`.
fn version_at_least(have: &str, min: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let (a, b) = (parse(have), parse(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

fn use_gpu(choice: &mut BackendChoice, gpu: &GpuInfo, variant: &RuntimeVariant) {
    choice.available = true;
    choice.device = Some(describe(gpu));
    choice.variant = Some(variant.id.clone());
    choice.download_size = variant.download_size();
    choice.memory_budget_gb = gpu.vram_gb();
}

fn describe(gpu: &GpuInfo) -> String {
    if gpu.integrated {
        format!("{} · shared memory", gpu.name)
    } else {
        format!("{} · {:.0} GB", gpu.name, gpu.vram_gb())
    }
}

fn short_name(gpu: &GpuInfo) -> String {
    gpu.name
        .trim_start_matches("NVIDIA ")
        .trim_start_matches("AMD ")
        .to_string()
}

/// `12th Gen Intel(R) Core(TM) i7-12700K` → `12th Gen Intel Core i7-12700K`.
fn short_cpu_name(name: &str) -> String {
    name.replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use demido_core::{Arch, Os};
    use demido_hardware::{CpuInfo, GIB};

    fn report(gpus: Vec<GpuInfo>, os: Os, arch: Arch, ram_gb: u64) -> HardwareReport {
        HardwareReport {
            os,
            arch,
            os_version: "test".into(),
            cpu: CpuInfo {
                name: "Test CPU".into(),
                physical_cores: 8,
                logical_cores: 16,
                avx2: true,
                avx512: false,
            },
            total_memory: ram_gb * GIB,
            available_memory: ram_gb * GIB / 2,
            gpus,
        }
    }

    fn nvidia(name: &str, gb: u64, driver: &str, cc: f64) -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Nvidia,
            name: name.into(),
            vram: gb * GIB,
            integrated: false,
            driver_version: Some(driver.into()),
            compute_capability: Some(cc),
            source: "test".into(),
        }
    }

    fn find(choices: &[BackendChoice], b: Backend) -> &BackendChoice {
        choices.iter().find(|c| c.backend == b).unwrap()
    }

    #[test]
    fn modern_nvidia_gets_cuda_13_and_the_mainstream_tier() {
        let r = report(
            vec![nvidia("NVIDIA GeForce RTX 3060", 12, "596.49", 8.6)],
            Os::Windows,
            Arch::X86_64,
            32,
        );
        let choices = backend_choices(&r, crate::catalog());
        let cuda = find(&choices, Backend::Cuda);
        assert!(cuda.available && cuda.recommended);
        assert_eq!(cuda.variant.as_deref(), Some("windows-cuda13"));
        assert_eq!(default_backend(&choices), Backend::Cuda);
        assert!(!find(&choices, Backend::Cpu).recommended);
        let rec = recommend_models(cuda, crate::catalog());
        assert_eq!(rec.tier, "mainstream");
        assert_eq!(rec.default_family, "qwen");
        assert_eq!(rec.picks[0].1.file, "Qwen3.5-9B-UD-Q6_K_XL.gguf");
    }

    #[test]
    fn pascal_cards_and_old_drivers_fall_back_to_cuda_12() {
        let r = report(
            vec![nvidia("NVIDIA GeForce GTX 1080", 8, "566.03", 6.1)],
            Os::Windows,
            Arch::X86_64,
            16,
        );
        let choices = backend_choices(&r, crate::catalog());
        assert_eq!(find(&choices, Backend::Cuda).variant.as_deref(), Some("windows-cuda12"));
    }

    #[test]
    fn ancient_driver_disables_cuda_and_recommends_vulkan() {
        let r = report(
            vec![nvidia("NVIDIA GeForce GTX 970", 4, "472.12", 5.2)],
            Os::Windows,
            Arch::X86_64,
            16,
        );
        let choices = backend_choices(&r, crate::catalog());
        assert!(!find(&choices, Backend::Cuda).available);
        assert!(find(&choices, Backend::Vulkan).recommended);
    }

    #[test]
    fn supported_radeon_gets_rocm_and_older_radeon_gets_vulkan() {
        let radeon = |name: &str| GpuInfo {
            vendor: GpuVendor::Amd,
            name: name.into(),
            vram: 16 * GIB,
            integrated: false,
            driver_version: None,
            compute_capability: None,
            source: "test".into(),
        };
        let r = report(vec![radeon("AMD Radeon RX 7800 XT")], Os::Windows, Arch::X86_64, 32);
        assert_eq!(default_backend(&backend_choices(&r, crate::catalog())), Backend::Rocm);
        let r = report(vec![radeon("AMD Radeon RX 6600")], Os::Windows, Arch::X86_64, 32);
        assert_eq!(default_backend(&backend_choices(&r, crate::catalog())), Backend::Vulkan);
    }

    #[test]
    fn no_gpu_preselects_cpu_without_recommending_it() {
        let r = report(vec![], Os::Windows, Arch::X86_64, 8);
        let choices = backend_choices(&r, crate::catalog());
        assert!(choices.iter().all(|c| !c.recommended));
        assert_eq!(default_backend(&choices), Backend::Cpu);
        let rec = recommend_models(find(&choices, Backend::Cpu), crate::catalog());
        assert_eq!(rec.tier, "cpu-tight");
    }

    #[test]
    fn cpu_names_lose_their_trademark_signs() {
        assert_eq!(
            short_cpu_name("12th Gen Intel(R) Core(TM) i7-12700K"),
            "12th Gen Intel Core i7-12700K"
        );
        assert_eq!(
            short_cpu_name("AMD Ryzen 9 7950X 16-Core Processor"),
            "AMD Ryzen 9 7950X 16-Core Processor"
        );
    }

    #[test]
    fn apple_silicon_uses_metal_with_a_unified_memory_budget() {
        let apple = GpuInfo {
            vendor: GpuVendor::Apple,
            name: "Apple M3 Pro GPU".into(),
            vram: (36.0 * 0.6 * GIB as f64) as u64,
            integrated: false,
            driver_version: None,
            compute_capability: None,
            source: "test".into(),
        };
        let r = report(vec![apple], Os::Macos, Arch::Aarch64, 36);
        let choices = backend_choices(&r, crate::catalog());
        let metal = find(&choices, Backend::Metal);
        assert!(metal.recommended);
        assert_eq!(recommend_models(metal, crate::catalog()).tier, "enthusiast");
        assert!(!find(&choices, Backend::Cuda).available);
    }

    #[test]
    fn driver_versions_compare_numerically() {
        assert!(version_at_least("596.49", "580.88"));
        assert!(version_at_least("580.88", "580.88"));
        assert!(!version_at_least("566.03", "580.88"));
        assert!(version_at_least("1000.1", "999.99"));
    }
}
