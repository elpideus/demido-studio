//! Hardware detection.
//!
//! [`detect`] builds a [`HardwareReport`]: every GPU with its dedicated memory, the CPU and the
//! system memory. It never fails: a probe that errors is logged and skipped, because a missing
//! GPU entry only means we fall back to a safer runtime, never that setup cannot continue.

mod cpu;
#[cfg(windows)]
mod dxgi;
mod gpu;
mod nvidia;
#[cfg(target_os = "linux")]
mod sysfs;

use demido_core::{Arch, Os};
use serde::{Deserialize, Serialize};

pub use gpu::{GpuInfo, GpuVendor};

pub const GIB: u64 = 1 << 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub name: String,
    pub physical_cores: usize,
    pub logical_cores: usize,
    /// AVX2 is what the prebuilt x86 CPU runtimes are tuned for.
    pub avx2: bool,
    pub avx512: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareReport {
    pub os: Os,
    pub arch: Arch,
    pub os_version: String,
    pub cpu: CpuInfo,
    pub total_memory: u64,
    pub available_memory: u64,
    /// Every GPU found, best first (see [`HardwareReport::primary_gpu`]).
    pub gpus: Vec<GpuInfo>,
}

impl HardwareReport {
    /// The GPU a model should run on: the discrete card with the most memory, or failing that
    /// the integrated one with the most memory.
    pub fn primary_gpu(&self) -> Option<&GpuInfo> {
        self.gpus.first()
    }

    pub fn total_memory_gb(&self) -> f64 {
        self.total_memory as f64 / GIB as f64
    }

    pub fn is_apple_silicon(&self) -> bool {
        self.os == Os::Macos && self.arch == Arch::Aarch64
    }
}

/// Probes the machine. Blocking: spawns `nvidia-smi` and queries the OS, which can take a
/// second on a cold start, so call it off the UI thread.
pub fn detect() -> HardwareReport {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let cpu = cpu::detect(&sys);
    let mut gpus = gpu::detect(&sys);
    gpu::rank(&mut gpus);

    HardwareReport {
        os: Os::current(),
        arch: Arch::current(),
        os_version: sysinfo::System::long_os_version().unwrap_or_else(|| "unknown".into()),
        cpu,
        total_memory: sys.total_memory(),
        available_memory: sys.available_memory(),
        gpus,
    }
}
