//! AMD and Intel GPUs on Linux through `/sys/class/drm`.

use std::path::Path;

use crate::gpu::{GpuInfo, GpuVendor};

pub(crate) fn adapters() -> Vec<GpuInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // `card0`, not `card0-HDMI-A-1`.
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let Some(vendor_id) = read_hex(&device.join("vendor")) else {
            continue;
        };
        let vendor = GpuVendor::from_pci_id(vendor_id);
        if vendor == GpuVendor::Nvidia {
            continue; // nvidia-smi reports these with more detail.
        }
        let vram = read_u64(&device.join("mem_info_vram_total")).unwrap_or(0);
        let product = std::fs::read_to_string(device.join("product_name"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| match vendor {
                GpuVendor::Amd => "AMD Radeon GPU".into(),
                GpuVendor::Intel => "Intel GPU".into(),
                _ => "GPU".into(),
            });
        out.push(GpuInfo {
            vendor,
            name: product,
            vram,
            integrated: vram < 2 * crate::GIB,
            driver_version: None,
            compute_capability: None,
            source: "sysfs".into(),
        });
    }
    out
}

fn read_hex(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
