//! AMD and Intel GPUs on Linux through `/sys/class/drm`, named from the PCI ID database, with
//! AMD's architecture from the kernel's compute driver (KFD), and the libraries the Linux runtimes
//! load from the system.

use std::path::Path;

use crate::SystemLibraries;
use crate::gpu::{GpuInfo, GpuVendor};

/// Where distributions keep the PCI ID database (`hwdata`, `pciutils`).
const PCI_IDS: [&str; 3] = [
    "/usr/share/hwdata/pci.ids",
    "/usr/share/misc/pci.ids",
    "/usr/share/pci.ids",
];

pub(crate) fn adapters() -> Vec<GpuInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let pci_ids = PCI_IDS.iter().find_map(|p| std::fs::read_to_string(p).ok());
    let gfx_targets = gfx_targets();
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
        let device_id = read_hex(&device.join("device"));
        let vram = read_u64(&device.join("mem_info_vram_total")).unwrap_or(0);
        // The card's own name is rare outside server cards; the PCI ID database has the rest.
        let product = std::fs::read_to_string(device.join("product_name"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let ids = pci_ids.as_deref()?;
                let sub = read_hex(&device.join("subsystem_vendor")).zip(read_hex(&device.join("subsystem_device")));
                pci_name(ids, vendor_id, device_id?, sub)
            })
            .unwrap_or_else(|| match vendor {
                GpuVendor::Amd => "AMD Radeon GPU".into(),
                GpuVendor::Intel => "Intel GPU".into(),
                _ => "GPU".into(),
            });
        let gfx_target = (vendor == GpuVendor::Amd)
            .then(|| {
                let id = device_id?;
                gfx_targets.iter().find(|(d, _)| *d == id).map(|(_, t)| t.clone())
            })
            .flatten();
        out.push(GpuInfo {
            vendor,
            name: product,
            vram,
            integrated: vram < 2 * crate::GIB,
            driver_version: None,
            compute_capability: None,
            gfx_target,
            source: "sysfs".into(),
        });
    }
    out
}

/// A GPU's name from the PCI ID database `ids`: the subsystem's when listed ("Radeon RX 7900
/// XTX"), else the device's marketing name in brackets ("Navi 31 [Radeon RX 7900 XT/7900 XTX]"
/// gives "Radeon RX 7900 XT/7900 XTX"), with the vendor in front.
fn pci_name(ids: &str, vendor: u32, device: u32, sub: Option<(u32, u32)>) -> Option<String> {
    let vendor_line = format!("{vendor:04x}  ");
    let device_line = format!("\t{device:04x}  ");
    let sub_line = sub.map(|(v, d)| format!("\t\t{v:04x} {d:04x}  "));
    let mut lines = ids.lines().skip_while(|l| !l.starts_with(&vendor_line)).skip(1);
    let device_name = lines
        .by_ref()
        .take_while(|l| l.starts_with('\t') || l.starts_with('#') || l.is_empty())
        .find_map(|l| l.strip_prefix(&device_line))?;
    let sub_name = sub_line.and_then(|prefix| {
        lines
            .take_while(|l| l.starts_with("\t\t") || l.starts_with('#'))
            .find_map(|l| l.strip_prefix(&prefix))
    });
    let name = sub_name.unwrap_or_else(|| match (device_name.find('['), device_name.rfind(']')) {
        (Some(open), Some(close)) if open < close => &device_name[open + 1..close],
        _ => device_name,
    });
    let brand = match GpuVendor::from_pci_id(vendor) {
        GpuVendor::Amd => "AMD",
        GpuVendor::Intel => "Intel",
        _ => "",
    };
    let name = name.trim();
    Some(if brand.is_empty() || name.starts_with(brand) {
        name.to_string()
    } else {
        format!("{brand} {name}")
    })
}

/// Every AMD GPU the kernel's compute driver knows, by PCI device id, with its architecture.
fn gfx_targets() -> Vec<(u32, String)> {
    let Ok(nodes) = std::fs::read_dir("/sys/class/kfd/kfd/topology/nodes") else {
        return Vec::new();
    };
    nodes
        .flatten()
        .filter_map(|n| kfd_node(&std::fs::read_to_string(n.path().join("properties")).ok()?))
        .collect()
}

/// A KFD topology node's `properties`: the GPU's PCI device id and architecture, or `None` for a
/// CPU node. `gfx_target_version` is major·10000 + minor·100 + stepping, and ROCm writes the
/// stepping in hexadecimal: 110000 is gfx1100, 90010 is gfx90a.
fn kfd_node(properties: &str) -> Option<(u32, String)> {
    let value = |key: &str| {
        properties.lines().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            (k == key).then(|| v.trim().parse::<u32>().ok()).flatten()
        })
    };
    let version = value("gfx_target_version").filter(|v| *v != 0)?;
    let device = value("device_id")?;
    let (major, minor, stepping) = (version / 10000, version / 100 % 100, version % 100);
    Some((device, format!("gfx{major}{minor}{stepping:x}")))
}

/// Looks for the libraries the Linux runtimes load from the system where the dynamic loader
/// would: its cache (`ldconfig -p`) and `LD_LIBRARY_PATH`. Without `ldconfig`, the usual
/// folders.
pub(crate) fn libraries() -> SystemLibraries {
    let cache = ["/usr/bin/ldconfig", "/sbin/ldconfig", "/usr/sbin/ldconfig"]
        .into_iter()
        .find_map(|ldconfig| {
            let mut cmd = std::process::Command::new(ldconfig);
            demido_core::appimage::use_host_env(&mut cmd);
            let out = cmd.arg("-p").output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        });
    // Not the AppImage's own folders, which an AppImage's launcher puts in front: a runtime
    // started from it does not get them (demido_core::appimage).
    let appdir = demido_core::appimage::appdir();
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("LD_LIBRARY_PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| appdir.as_ref().is_none_or(|a| !d.starts_with(a)))
                .collect()
        })
        .unwrap_or_default();
    if cache.is_none() {
        dirs.extend(
            [
                "/usr/lib",
                "/usr/lib64",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib/aarch64-linux-gnu",
                "/opt/rocm/lib",
            ]
            .map(std::path::PathBuf::from),
        );
    }
    let files: Vec<String> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    libraries_in(cache.as_deref().unwrap_or(""), &files)
}

/// Which of the libraries are in the loader's cache listing `ldconfig` or among `files`.
fn libraries_in(ldconfig: &str, files: &[String]) -> SystemLibraries {
    let cached: Vec<&str> = ldconfig.lines().filter_map(|l| l.split_whitespace().next()).collect();
    let has = |found: &dyn Fn(&str) -> bool| cached.iter().any(|n| found(n)) || files.iter().any(|n| found(n));
    SystemLibraries {
        vulkan: Some(has(&|n| n == "libvulkan.so.1")),
        rocm: Some(has(&|n| n.starts_with("libamdhip64.so"))),
    }
}

fn read_hex(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: &str = "\
# comment
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t744c  Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]
\t\t1002 0e3b  Radeon RX 7900 XTX
\t\t1da2 471e  Nitro+ Radeon RX 7900 XTX Vapor-X
\t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]
\t1586  Strix Halo [Radeon 8050S / 8060S Graphics]
10de  NVIDIA Corporation
\t2504  GA106 [GeForce RTX 3060 Lite Hash Rate]
8086  Intel Corporation
\t4680  Alder Lake-S GT1 [UHD Graphics 770]
\t56a0  DG2 [Arc A770]
";

    #[test]
    fn gpus_are_named_from_the_pci_id_database() {
        // The subsystem's name when the card's maker is listed, else the marketing name.
        assert_eq!(
            pci_name(IDS, 0x1002, 0x744c, Some((0x1002, 0x0e3b))).as_deref(),
            Some("AMD Radeon RX 7900 XTX")
        );
        assert_eq!(
            pci_name(IDS, 0x1002, 0x744c, Some((0x1da2, 0x471e))).as_deref(),
            Some("AMD Nitro+ Radeon RX 7900 XTX Vapor-X")
        );
        assert_eq!(
            pci_name(IDS, 0x1002, 0x744c, Some((0x1043, 0x0001))).as_deref(),
            Some("AMD Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M")
        );
        assert_eq!(
            pci_name(IDS, 0x1002, 0x1586, None).as_deref(),
            Some("AMD Radeon 8050S / 8060S Graphics")
        );
        assert_eq!(
            pci_name(IDS, 0x8086, 0x4680, None).as_deref(),
            Some("Intel UHD Graphics 770")
        );
        // A device of another vendor with the same id is not taken.
        assert_eq!(pci_name(IDS, 0x8086, 0x744c, None), None);
        assert_eq!(pci_name(IDS, 0x1234, 0x0001, None), None);
    }

    #[test]
    fn the_architecture_comes_from_the_compute_driver() {
        let gpu = "cpu_cores_count 0\nsimd_count 192\ngfx_target_version 110000\nvendor_id 4098\ndevice_id 29772\n";
        assert_eq!(kfd_node(gpu), Some((29772, "gfx1100".into())));
        assert_eq!(
            kfd_node("gfx_target_version 100300\ndevice_id 29631\n"),
            Some((29631, "gfx1030".into()))
        );
        assert_eq!(
            kfd_node("gfx_target_version 110501\ndevice_id 5510\n"),
            Some((5510, "gfx1151".into()))
        );
        assert_eq!(
            kfd_node("gfx_target_version 90010\ndevice_id 29812\n"),
            Some((29812, "gfx90a".into()))
        );
        // The CPU's node.
        assert_eq!(
            kfd_node("cpu_cores_count 16\ngfx_target_version 0\ndevice_id 0\n"),
            None
        );
    }

    #[test]
    fn libraries_are_found_where_the_loader_looks() {
        let cache = "\
1234 libs found in cache `/etc/ld.so.cache'
\tlibvulkan.so.1 (libc6,x86-64) => /usr/lib/libvulkan.so.1
\tlibz.so.1 (libc6,x86-64) => /usr/lib/libz.so.1
";
        assert_eq!(
            libraries_in(cache, &[]),
            SystemLibraries {
                vulkan: Some(true),
                rocm: Some(false)
            }
        );
        let rocm = libraries_in("", &["libamdhip64.so.7".into(), "libvulkan.so".into()]);
        assert_eq!(rocm.rocm, Some(true));
        // The development link alone is not what the runtime loads.
        assert_eq!(rocm.vulkan, Some(false));
    }

    #[test]
    fn this_machine_reports_its_libraries() {
        let found = libraries();
        assert!(found.vulkan.is_some() && found.rocm.is_some());
    }
}
