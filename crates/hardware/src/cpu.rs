use crate::CpuInfo;

pub(crate) fn detect(sys: &sysinfo::System) -> CpuInfo {
    let name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Unknown processor".into());

    CpuInfo {
        name,
        physical_cores: sysinfo::System::physical_core_count().unwrap_or(1),
        logical_cores: sys.cpus().len().max(1),
        avx2: has_avx2(),
        avx512: has_avx512(),
    }
}

#[cfg(target_arch = "x86_64")]
fn has_avx2() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn has_avx2() -> bool {
    false
}

#[cfg(target_arch = "x86_64")]
fn has_avx512() -> bool {
    std::arch::is_x86_feature_detected!("avx512f")
}

#[cfg(not(target_arch = "x86_64"))]
fn has_avx512() -> bool {
    false
}
