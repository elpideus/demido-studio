use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other,
}

impl GpuVendor {
    pub fn from_pci_id(id: u32) -> GpuVendor {
        match id {
            0x10DE => GpuVendor::Nvidia,
            0x1002 | 0x1022 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            0x106B => GpuVendor::Apple,
            _ => GpuVendor::Other,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
    /// Dedicated memory in bytes. For Apple Silicon this is the share of unified memory the GPU
    /// can use; for integrated GPUs it is whatever the firmware reserved (often tiny).
    pub vram: u64,
    pub integrated: bool,
    pub driver_version: Option<String>,
    /// CUDA compute capability, NVIDIA only (e.g. `8.6` for an RTX 3060).
    pub compute_capability: Option<f64>,
    /// Which probe produced this entry, for diagnostics.
    pub source: String,
}

impl GpuInfo {
    pub fn vram_gb(&self) -> f64 {
        self.vram as f64 / crate::GIB as f64
    }

    /// Whether AMD's ROCm stack officially targets this card (RDNA2 high end, RDNA3, RDNA4,
    /// Strix Halo). Other Radeons are better served by Vulkan.
    pub fn rocm_supported(&self) -> bool {
        if self.vendor != GpuVendor::Amd {
            return false;
        }
        let name = self.name.to_ascii_uppercase();
        let rx_series = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| {
                w[0] == "RX"
                    && w[1].len() == 4
                    && w[1].chars().all(|c| c.is_ascii_digit())
                    && matches!(&w[1][..2], "68" | "69" | "76" | "77" | "78" | "79" | "90")
            });
        rx_series
            || name.contains("PRO W7")
            || name.contains("PRO W6800")
            || name.contains("PRO W9")
            || name.contains("8060S")
            || name.contains("8050S")
    }
}

/// Collects GPUs from every probe available on this platform, merging duplicates.
pub(crate) fn detect(sys: &sysinfo::System) -> Vec<GpuInfo> {
    let _ = sys;
    let mut gpus: Vec<GpuInfo> = Vec::new();

    #[cfg(windows)]
    gpus.extend(crate::dxgi::adapters());

    #[cfg(target_os = "linux")]
    gpus.extend(crate::sysfs::adapters());

    #[cfg(target_os = "macos")]
    gpus.extend(apple_gpu(sys));

    // nvidia-smi knows the driver version and compute capability that DXGI and sysfs do not, so
    // its entries replace the generic ones for the same card.
    let nvidia = crate::nvidia::query();
    if !nvidia.is_empty() {
        gpus.retain(|g| g.vendor != GpuVendor::Nvidia);
        gpus.extend(nvidia);
    }

    gpus
}

/// Best GPU first: discrete before integrated, then by memory.
pub(crate) fn rank(gpus: &mut [GpuInfo]) {
    gpus.sort_by(|a, b| {
        a.integrated
            .cmp(&b.integrated)
            .then(b.vram.cmp(&a.vram))
            .then(a.name.cmp(&b.name))
    });
}

#[cfg(target_os = "macos")]
fn apple_gpu(sys: &sysinfo::System) -> Option<GpuInfo> {
    if !cfg!(target_arch = "aarch64") {
        return None;
    }
    let chip = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Apple Silicon".into());
    // macOS lets the GPU wire roughly two thirds of unified memory by default; keep a margin.
    let budget = (sys.total_memory() as f64 * 0.6) as u64;
    Some(GpuInfo {
        vendor: GpuVendor::Apple,
        name: format!("{chip} GPU"),
        vram: budget,
        integrated: false,
        driver_version: None,
        compute_capability: None,
        source: "sysctl".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amd(name: &str) -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Amd,
            name: name.into(),
            vram: 16 * crate::GIB,
            integrated: false,
            driver_version: None,
            compute_capability: None,
            source: "test".into(),
        }
    }

    #[test]
    fn rocm_support_follows_the_rdna_generation() {
        assert!(amd("AMD Radeon RX 7900 XTX").rocm_supported());
        assert!(amd("AMD Radeon RX 9070 XT").rocm_supported());
        assert!(amd("AMD Radeon RX 6800 XT").rocm_supported());
        assert!(amd("AMD Radeon PRO W7800").rocm_supported());
        assert!(!amd("AMD Radeon RX 6600").rocm_supported());
        assert!(!amd("AMD Radeon RX 580 Series").rocm_supported());
        assert!(!amd("AMD Radeon(TM) Graphics").rocm_supported());
    }

    #[test]
    fn discrete_cards_rank_before_integrated_ones() {
        let mut gpus = vec![
            GpuInfo {
                integrated: true,
                vram: 32 * crate::GIB,
                vendor: GpuVendor::Intel,
                name: "Intel Iris Xe".into(),
                ..amd("x")
            },
            GpuInfo {
                vram: 8 * crate::GIB,
                ..amd("AMD Radeon RX 7600")
            },
        ];
        rank(&mut gpus);
        assert_eq!(gpus[0].name, "AMD Radeon RX 7600");
    }
}
