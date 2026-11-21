//! What this computer has, and which model runtime suits it.
//!
//! Read fresh every time it is asked for: memory in use changes, and a card can be plugged in.
//! NVIDIA cards are asked through the driver (NVML), which also says which CUDA it supports; if that is
//! not there, through `nvidia-smi`. Every other card is found through the system's own list.
//!
//! The advice at the end is a pure function of what was found, so it is tested without any hardware.

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SystemInfo {
    pub os: Os,
    pub cpu: Cpu,
    pub memory: Memory,
    pub gpus: Vec<Gpu>,
    pub runtime: Advice,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Os {
    pub name: String,
    pub version: String,
    pub kernel: String,
    /// For example `x86_64` or `aarch64`.
    pub arch: String,
    /// `linux`, `windows` or `macos`.
    pub family: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Cpu {
    pub model: String,
    pub threads: usize,
    pub cores: Option<usize>,
    /// The instruction sets that matter for running models: `avx2`, `avx512f`, `neon`.
    pub features: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Memory {
    pub total_gb: f64,
    pub available_gb: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Gpu {
    /// `nvidia`, `amd`, `intel` or `apple`.
    pub vendor: String,
    pub name: String,
    /// Memory of its own. Empty for graphics built into the processor, which share the computer's memory.
    pub vram_gb: Option<f64>,
    pub vram_used_gb: Option<f64>,
    /// True for graphics built into the processor: no memory of its own.
    pub shares_memory: bool,
    pub driver: Option<String>,
    /// The newest CUDA this driver supports, for example `13.0`. NVIDIA only.
    pub cuda: Option<String>,
    /// How it was found: `nvml`, `nvidia-smi` or `system`.
    pub found_by: String,
}

/// One downloadable build of the model runtime.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Build {
    /// For example `cuda-12.8`.
    pub id: String,
    pub name: String,
    /// The file to fetch from the runtime's own releases. `{build}` stands for the build tag.
    pub file: String,
    /// Why it is, or is not, the one to take.
    pub why: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Advice {
    /// The runtime these builds belong to.
    pub runtime: String,
    /// Where its files come from. Demido downloads them from there; it does not ship them.
    pub source: String,
    pub recommended: Build,
    /// Builds that would also run here.
    pub others: Vec<Build>,
}

const GB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB_PER_GB: f64 = 1024.0;
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn detect() -> SystemInfo {
    let os = Os {
        name: sysinfo::System::name().unwrap_or_else(|_| std::env::consts::OS.to_string()),
        version: sysinfo::System::os_version().unwrap_or_default(),
        kernel: sysinfo::System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
        family: std::env::consts::OS.to_string(),
    };
    // If the system will not tell us, say "unknown" rather than fail: the rest is still worth showing.
    let sys = sysinfo::System::new().ok().map(|mut s| {
        s.refresh_memory();
        s.refresh_cpu_all();
        s
    });
    let cpu = Cpu {
        model: sys.as_ref().and_then(|s| s.cpus().first().map(|c| c.brand().trim().to_string())).unwrap_or_default(),
        threads: sys.as_ref().map(|s| s.cpus().len()).filter(|n| *n > 0).unwrap_or_else(|| std::thread::available_parallelism().map(usize::from).unwrap_or(1)),
        cores: sysinfo::System::physical_core_count().ok(),
        features: cpu_features(),
    };
    let memory = Memory { total_gb: round1(sys.as_ref().map_or(0, |s| s.total_memory()) as f64 / GB), available_gb: round1(sys.as_ref().map_or(0, |s| s.available_memory()) as f64 / GB) };
    let gpus = gpus();
    let runtime = advise(&os.family, &os.arch, &gpus);
    SystemInfo { os, cpu, memory, gpus, runtime }
}

fn cpu_features() -> Vec<String> {
    #[allow(unused_mut)]
    let mut found: Vec<&str> = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx") {
            found.push("avx");
        }
        if std::is_x86_feature_detected!("avx2") {
            found.push("avx2");
        }
        if std::is_x86_feature_detected!("fma") {
            found.push("fma");
        }
        if std::is_x86_feature_detected!("f16c") {
            found.push("f16c");
        }
        if std::is_x86_feature_detected!("avx512f") {
            found.push("avx512f");
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        found.push("neon");
    }
    found.into_iter().map(String::from).collect()
}

fn gpus() -> Vec<Gpu> {
    let mut found = nvidia_by_driver().or_else(nvidia_by_tool).unwrap_or_default();
    for g in others_by_system() {
        // An NVIDIA card already found through its driver is not listed twice.
        if !(g.vendor == "nvidia" && found.iter().any(|f| f.vendor == "nvidia")) {
            found.push(g);
        }
    }
    found
}

/// NVIDIA cards, asked through the driver's own library. `None` when the library is not there.
fn nvidia_by_driver() -> Option<Vec<Gpu>> {
    let nvml = nvml_wrapper::Nvml::init().ok()?;
    let driver = nvml.sys_driver_version().ok();
    // The driver reports CUDA as one number: 13000 is 13.0, 12080 is 12.8.
    let cuda = nvml.sys_cuda_driver_version().ok().map(|v| format!("{}.{}", v / 1000, (v % 1000) / 10));
    let count = nvml.device_count().ok()?;
    let cards = (0..count)
        .filter_map(|i| {
            let d = nvml.device_by_index(i).ok()?;
            let mem = d.memory_info().ok();
            Some(Gpu {
                vendor: "nvidia".into(),
                name: d.name().unwrap_or_else(|_| "NVIDIA graphics card".into()),
                vram_gb: mem.as_ref().map(|m| round1(m.total as f64 / GB)),
                vram_used_gb: mem.as_ref().map(|m| round1(m.used as f64 / GB)),
                shares_memory: false,
                driver: driver.clone(),
                cuda: cuda.clone(),
                found_by: "nvml".into(),
            })
        })
        .collect::<Vec<_>>();
    Some(cards)
}

/// NVIDIA cards, asked through the `nvidia-smi` program. It does not say which CUDA the driver supports.
fn nvidia_by_tool() -> Option<Vec<Gpu>> {
    let out = std::process::Command::new("nvidia-smi").args(["--query-gpu=name,driver_version,memory.total,memory.used", "--format=csv,noheader,nounits"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_nvidia_smi(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 4 || f[0].is_empty() {
                return None;
            }
            let gb = |s: &str| s.parse::<f64>().ok().map(|mib| round1(mib / MIB_PER_GB));
            Some(Gpu {
                vendor: "nvidia".into(),
                name: f[0].to_string(),
                vram_gb: gb(f[2]),
                vram_used_gb: gb(f[3]),
                shares_memory: false,
                driver: Some(f[1].to_string()),
                cuda: None,
                found_by: "nvidia-smi".into(),
            })
        })
        .collect()
}

/// Every graphics card the system lists, with its maker.
fn others_by_system() -> Vec<Gpu> {
    let Ok(list) = sysinfo::Gpus::new_with_refreshed_list() else { return Vec::new() };
    list.iter()
        .filter_map(|g| {
            let maker = g.vendor().unwrap_or("").to_ascii_lowercase();
            let model = g.model().unwrap_or("").trim().to_string();
            let both = format!("{maker} {}", model.to_ascii_lowercase());
            let (vendor, label) = if both.contains("nvidia") {
                ("nvidia", "NVIDIA graphics card")
            } else if both.contains("amd") || both.contains("advanced micro") || both.contains("radeon") {
                ("amd", "AMD graphics card")
            } else if both.contains("intel") {
                ("intel", "Intel graphics")
            } else if both.contains("apple") {
                ("apple", "Apple graphics")
            } else {
                return None;
            };
            // Intel's and Apple's graphics sit in the processor. What the system reports as "their" memory
            // is the computer's own, so it is not shown as graphics memory.
            let shares = vendor == "intel" || vendor == "apple";
            Some(Gpu {
                vendor: vendor.into(),
                name: if model.is_empty() { label.into() } else { model },
                vram_gb: if shares { None } else { g.total_memory().map(|b| round1(b as f64 / GB)) },
                vram_used_gb: if shares { None } else { g.used_memory().map(|b| round1(b as f64 / GB)) },
                shares_memory: shares,
                driver: None,
                cuda: None,
                found_by: "system".into(),
            })
        })
        .collect()
}

fn build(id: &str, name: &str, file: &str, why: &str) -> Build {
    Build { id: id.into(), name: name.into(), file: file.into(), why: why.into() }
}

/// "12.8" is at least (12, 8).
fn at_least(cuda: &str, major: u32, minor: u32) -> bool {
    let mut parts = cuda.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let have = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    have >= (major, minor)
}

/// Which llama.cpp build to download for this computer. See docs/research/03-inference-runtime.md.
pub fn advise(family: &str, arch: &str, gpus: &[Gpu]) -> Advice {
    let x64 = arch == "x86_64";
    let (os, ext) = match family {
        "windows" => ("win", "zip"),
        "macos" => ("macos", "tar.gz"),
        _ => ("ubuntu", "tar.gz"),
    };
    let cpu_arch = if x64 { "x64" } else { "arm64" };
    let file = |kind: &str| if kind.is_empty() { format!("llama-{{build}}-bin-{os}-{cpu_arch}.{ext}") } else { format!("llama-{{build}}-bin-{os}-{kind}-{cpu_arch}.{ext}") };
    let cpu_file = if family == "windows" { file("cpu") } else { file("") };
    let cuda12 = if family == "windows" { "12.4" } else { "12.8" };
    let nvidia = gpus.iter().find(|g| g.vendor == "nvidia");
    let amd = gpus.iter().any(|g| g.vendor == "amd");
    let intel = gpus.iter().any(|g| g.vendor == "intel");

    let cpu = |why: &str| build("cpu", "llama.cpp · CPU", &cpu_file, why);
    let vulkan = |why: &str| build("vulkan", "llama.cpp · Vulkan", &file("vulkan"), why);
    let cuda = |v: &str, why: &str| build(&format!("cuda-{v}"), &format!("llama.cpp · CUDA {v}"), &file(&format!("cuda-{v}")), why);

    let mut options: Vec<Build> = Vec::new();
    if family == "macos" {
        options.push(build("metal", "llama.cpp · Metal", &file(""), "The macOS build uses the graphics chip through Metal. Nothing else is needed."));
    } else {
        if let Some(card) = nvidia {
            match card.cuda.as_deref() {
                Some(v) if at_least(v, 13, 5) => {
                    options.push(cuda("13.5", &format!("You have an NVIDIA card and its driver supports CUDA {v}.")));
                    options.push(cuda(cuda12, "Also runs on this driver. Take it if the newer one gives trouble."));
                }
                Some(v) if at_least(v, 12, if family == "windows" { 4 } else { 8 }) => {
                    options.push(cuda(cuda12, &format!("You have an NVIDIA card and its driver supports CUDA {v}.")));
                }
                Some(v) => options.push(vulkan(&format!("Your NVIDIA driver supports CUDA {v}, which is older than the CUDA builds need. Vulkan works with it; a newer driver would be faster."))),
                None => {
                    options.push(cuda(cuda12, "You have an NVIDIA card. Its driver could not be asked which CUDA it supports, so this is the safer of the two CUDA builds."));
                }
            }
            if x64 && !options.iter().any(|b| b.id == "vulkan") {
                options.push(vulkan("Runs on any modern card. Slower than CUDA on NVIDIA."));
            }
        } else if amd {
            options.push(vulkan("You have an AMD card. Vulkan runs on it without anything else installed."));
            if x64 {
                options.push(build("rocm-10.0", "llama.cpp · ROCm 10.0", &file("rocm-10.0"), "AMD's own route. Can be faster, but needs AMD's ROCm installed first. Not checked by Demido yet."));
            }
        } else if intel {
            options.push(vulkan("You have Intel graphics. Vulkan runs on it without anything else installed."));
        }
        options.push(cpu(if options.is_empty() { "No graphics card that can run models was found, so models run on the processor." } else { "Runs on the processor only. Works everywhere, slowly." }));
    }
    let recommended = options.remove(0);
    Advice { runtime: "llama.cpp".into(), source: "https://github.com/ggml-org/llama.cpp/releases".into(), recommended, others: options }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvidia(cuda: Option<&str>) -> Gpu {
        Gpu {
            vendor: "nvidia".into(),
            name: "NVIDIA GeForce RTX 3060".into(),
            vram_gb: Some(12.0),
            vram_used_gb: Some(0.9),
            shares_memory: false,
            driver: Some("620.70".into()),
            cuda: cuda.map(Into::into),
            found_by: "nvml".into(),
        }
    }
    fn other(vendor: &str) -> Gpu {
        Gpu { vendor: vendor.into(), name: "card".into(), vram_gb: None, vram_used_gb: None, shares_memory: false, driver: None, cuda: None, found_by: "system".into() }
    }

    #[test]
    fn a_new_nvidia_driver_gets_the_newest_cuda_build() {
        let a = advise("linux", "x86_64", &[nvidia(Some("13.5"))]);
        assert_eq!(a.recommended.id, "cuda-13.5");
        assert_eq!(a.recommended.file, "llama-{build}-bin-ubuntu-cuda-13.5-x64.tar.gz");
        assert!(a.others.iter().any(|b| b.id == "cuda-12.8"));
        assert!(a.others.iter().any(|b| b.id == "cpu"));
    }

    #[test]
    fn an_older_nvidia_driver_gets_the_cuda_12_build() {
        assert_eq!(advise("linux", "x86_64", &[nvidia(Some("12.9"))]).recommended.id, "cuda-12.8");
        assert_eq!(advise("windows", "x86_64", &[nvidia(Some("12.6"))]).recommended.file, "llama-{build}-bin-win-cuda-12.4-x64.zip");
    }

    #[test]
    fn a_driver_too_old_for_cuda_falls_back_to_vulkan() {
        let a = advise("linux", "x86_64", &[nvidia(Some("11.8"))]);
        assert_eq!(a.recommended.id, "vulkan");
        assert!(a.recommended.why.contains("11.8"));
    }

    #[test]
    fn amd_and_intel_get_vulkan_and_no_card_gets_the_processor() {
        assert_eq!(advise("linux", "x86_64", &[other("amd")]).recommended.id, "vulkan");
        assert!(advise("linux", "x86_64", &[other("amd")]).others.iter().any(|b| b.id == "rocm-10.0"));
        assert_eq!(advise("linux", "x86_64", &[other("intel")]).recommended.id, "vulkan");
        let none = advise("linux", "x86_64", &[]);
        assert_eq!(none.recommended.id, "cpu");
        assert_eq!(none.recommended.file, "llama-{build}-bin-ubuntu-x64.tar.gz");
        assert_eq!(advise("windows", "x86_64", &[]).recommended.file, "llama-{build}-bin-win-cpu-x64.zip");
    }

    #[test]
    fn macos_gets_the_one_build_there_is() {
        let a = advise("macos", "aarch64", &[other("apple")]);
        assert_eq!(a.recommended.file, "llama-{build}-bin-macos-arm64.tar.gz");
        assert!(a.others.is_empty());
    }

    #[test]
    fn nvidia_smi_lines_are_read() {
        let cards = parse_nvidia_smi("NVIDIA GeForce RTX 3060, 620.70.06, 12288, 921\n");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].vram_gb, Some(12.0));
        assert_eq!(cards[0].vram_used_gb, Some(0.9));
        assert_eq!(cards[0].driver.as_deref(), Some("620.70.06"));
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn this_computer_can_be_read_without_failing() {
        let s = detect();
        assert!(s.cpu.threads > 0);
        assert!(s.memory.total_gb > 0.0);
        assert!(!s.runtime.recommended.file.is_empty());
    }
}
