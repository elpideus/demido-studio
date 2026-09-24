//! NVIDIA cards through `nvidia-smi`, which ships with every NVIDIA driver.

use std::path::PathBuf;
use std::process::Command;

use crate::gpu::{GpuInfo, GpuVendor};

/// Queries every NVIDIA GPU. Empty when there is no NVIDIA driver.
pub(crate) fn query() -> Vec<GpuInfo> {
    for exe in candidates() {
        // Older drivers do not know `compute_cap`; ask again without it.
        for fields in [
            "name,memory.total,driver_version,compute_cap",
            "name,memory.total,driver_version",
        ] {
            let Some(out) = run(&exe, fields) else {
                continue;
            };
            let gpus = parse(&out);
            if !gpus.is_empty() {
                return gpus;
            }
        }
    }
    Vec::new()
}

fn candidates() -> Vec<PathBuf> {
    let mut list = vec![PathBuf::from("nvidia-smi")];
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            list.push(PathBuf::from(root).join("System32").join("nvidia-smi.exe"));
        }
        list.push(PathBuf::from(
            r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe",
        ));
    }
    list
}

fn run(exe: &PathBuf, fields: &str) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.arg(format!("--query-gpu={fields}"))
        .arg("--format=csv,noheader,nounits");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Parses `name, memory MiB, driver[, compute_cap]` lines.
pub(crate) fn parse(csv: &str) -> Vec<GpuInfo> {
    csv.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split(',').map(str::trim).collect();
            if cols.len() < 3 {
                return None;
            }
            let mib: u64 = cols[1].parse().ok()?;
            Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                name: cols[0].to_string(),
                vram: mib * 1024 * 1024,
                integrated: false,
                driver_version: Some(cols[2].to_string()).filter(|d| !d.is_empty()),
                compute_capability: cols.get(3).and_then(|c| c.parse().ok()),
                source: "nvidia-smi".into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_output() {
        let gpus = parse("NVIDIA GeForce RTX 3060, 12288, 596.49, 8.6\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 3060");
        assert_eq!(gpus[0].vram, 12288 * 1024 * 1024);
        assert_eq!(gpus[0].driver_version.as_deref(), Some("596.49"));
        assert_eq!(gpus[0].compute_capability, Some(8.6));
    }

    #[test]
    fn parses_legacy_output_and_skips_garbage() {
        let gpus = parse("GeForce GTX 1080, 8192, 472.12\nnot a gpu\n");
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].compute_capability, None);
    }
}
