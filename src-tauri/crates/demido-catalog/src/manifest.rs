//! The pins, and what each one costs.
//!
//! **Data, and nothing else.** There is no code here that could be wrong in an
//! interesting way, which is why the whole file is covered by one table-driven
//! test against `docs/rules/setup.md` section 4 and needs no seam at all.
//!
//! **Pins ship inside the build.** Demido never asks upstream what the newest
//! release is: not on launch, not on a schedule, not behind a button. A pin
//! moves when somebody moves it here, measures it, and writes the measurement
//! into section 4, and `tests/the_manifest.rs` fails until both have happened.

use demido_hardware::{CudaVersion, Ecosystem};
use serde::Serialize;

/// The llama.cpp release every archive below comes from.
///
/// `b10816`, commit `427291b5b34cd914a31b3fd3b61a68f6184f4b9f`, dated
/// 2026-09-05. It is the build every closing comment cites
/// (`docs/rules/done.md`).
pub const RELEASE: &str = "b10816";

/// Where a release's assets live. Permanent for a pin, which is why a rollback
/// is a re-download rather than a retained predecessor
/// (`docs/rules/runtimes.md`).
const DOWNLOAD_ROOT: &str = "https://github.com/ggml-org/llama.cpp/releases/download";

/// What an archive is, which decides whether it can be the thing that runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Carries `llama-server`.
    Build,
    /// NVIDIA's redistributable runtime. A CUDA build links against
    /// `cudart64_*.dll` and ships without it, so this travels with one and is
    /// never chosen on its own.
    CudaRuntime,
}

/// Which of section 4's two groups an archive belongs to.
///
/// **The group is a field, not a screen.** `docs/rules/setup.md` section 4
/// offers both groups with their sizes, and the wizard's manifest step draws
/// whatever is here: adding uv, Python, SearXNG, Node, `agent-browser` or
/// Chrome is adding rows with `Group::Capability` on them, never a second step
/// and never a second code path
/// ([#48](https://github.com/elpideus/demido-studio/issues/48)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    /// Without it nothing answers.
    Required,
    /// A feature of the brief that silently does not exist without it.
    Capability,
}

/// Whose terms an archive arrives under.
///
/// Two, because the required group is two owners: 373 of the required 516 MiB
/// is NVIDIA's rather than ggml-org's, which is a separate row in
/// `THIRD_PARTY_NOTICES.md` and a separate folder under `licenses/` on the
/// commit that first fetches it (`docs/rules/setup.md` section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "license", rename_all = "kebab-case")]
pub enum License {
    Mit { owner: &'static str },
    NvidiaCudaEula,
}

impl License {
    /// How section 4 writes it, which is also what the credits surface shows.
    pub fn label(self) -> String {
        match self {
            License::Mit { owner } => format!("MIT, {owner}"),
            License::NvidiaCudaEula => "NVIDIA CUDA EULA".to_owned(),
        }
    }
}

/// The platform an archive was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Linux,
    MacOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X64,
    Arm64,
}

impl Os {
    /// From what `Machine` reported about itself, which is
    /// `std::env::consts::OS`. `None` is a platform no archive here targets,
    /// and it is treated as "nothing is pinned for you" rather than guessed at.
    pub fn named(os: &str) -> Option<Self> {
        match os {
            "windows" => Some(Os::Windows),
            "linux" => Some(Os::Linux),
            "macos" => Some(Os::MacOs),
            _ => None,
        }
    }
}

impl Arch {
    /// From `std::env::consts::ARCH`, as above.
    pub fn named(arch: &str) -> Option<Self> {
        match arch {
            "x86_64" => Some(Arch::X64),
            "aarch64" => Some(Arch::Arm64),
            _ => None,
        }
    }
}

impl std::fmt::Display for Os {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::MacOs => "macos",
        })
    }
}

impl std::fmt::Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Arch::X64 => "x64",
            Arch::Arm64 => "arm64",
        })
    }
}

/// One pinned archive, with what it costs to take it.
///
/// **Both sizes, always.** Download is the byte count the server sends, from
/// the response header; on disk is what the archive expands to, read out of its
/// own index. A wizard that states one and spends the other is a wizard nobody
/// can plan a disk around. Both are MiB, 1024 by 1024.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Archive {
    pub name: &'static str,
    /// Which of section 4's two groups this is in.
    pub group: Group,
    /// The release this file belongs to. Part of its URL, so it is what makes
    /// the URL permanent.
    pub pin: &'static str,
    pub kind: Kind,
    pub ecosystem: Ecosystem,
    /// The CUDA toolkit this archive was compiled against, and the only thing
    /// that decides whether a driver can load it. `None` for anything that asks
    /// nothing of a driver.
    pub toolkit: Option<CudaVersion>,
    pub os: Os,
    pub arch: Arch,
    pub download_mib: f64,
    pub on_disk_mib: f64,
    pub license: License,
}

impl Archive {
    /// The permanent upstream URL. Built from the pin rather than stored, so
    /// there is one place a pin can be wrong.
    pub fn url(&self) -> String {
        format!("{DOWNLOAD_ROOT}/{}/{}", self.pin, self.name)
    }
}

/// What S1 pins: the required group of `docs/rules/setup.md` section 4.
///
/// Three rows for two answers. CUDA is the build the rig runs and CPU is what
/// a machine with no discrete card runs, which is the whole of "CUDA and CPU
/// only in the first cut". ROCm and Vulkan are published upstream and are
/// deliberately not here: there is no AMD card on the rig, and shipping an
/// untested path is the built-but-not-working failure with a fresh coat on.
/// They are rows in the selector regardless, saying exactly this.
///
/// The capability group (uv, Python, SearXNG, Node, `agent-browser`, Chrome) is
/// out of S1 and is more rows here when it lands, not a second screen.
pub const MANIFEST: &[Archive] = &[
    Archive {
        name: "llama-b10816-bin-win-cuda-13.3-x64.zip",
        group: Group::Required,
        pin: RELEASE,
        kind: Kind::Build,
        ecosystem: Ecosystem::Cuda,
        toolkit: Some(CudaVersion::new(13, 3)),
        os: Os::Windows,
        arch: Arch::X64,
        download_mib: 142.6,
        on_disk_mib: 182.6,
        license: License::Mit { owner: "ggml-org" },
    },
    Archive {
        name: "cudart-llama-bin-win-cuda-13.3-x64.zip",
        group: Group::Required,
        pin: RELEASE,
        kind: Kind::CudaRuntime,
        ecosystem: Ecosystem::Cuda,
        toolkit: Some(CudaVersion::new(13, 3)),
        os: Os::Windows,
        arch: Arch::X64,
        download_mib: 372.9,
        on_disk_mib: 489.0,
        license: License::NvidiaCudaEula,
    },
    Archive {
        name: "llama-b10816-bin-win-cpu-x64.zip",
        group: Group::Required,
        pin: RELEASE,
        kind: Kind::Build,
        ecosystem: Ecosystem::Cpu,
        toolkit: None,
        os: Os::Windows,
        arch: Arch::X64,
        download_mib: 17.6,
        on_disk_mib: 44.6,
        license: License::Mit { owner: "ggml-org" },
    },
];

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_cuda_archive_declares_the_toolkit_that_decides_whether_it_loads() {
        for archive in MANIFEST.iter().filter(|a| a.ecosystem == Ecosystem::Cuda) {
            assert!(
                archive.toolkit.is_some(),
                "{} would be offered to a driver that cannot load it",
                archive.name
            );
        }
    }

    #[test]
    fn a_url_is_built_from_the_pin() {
        let cpu = MANIFEST
            .iter()
            .find(|a| a.ecosystem == Ecosystem::Cpu)
            .map(Archive::url);
        assert_eq!(
            cpu.as_deref(),
            Some(
                "https://github.com/ggml-org/llama.cpp/releases/download/b10816/llama-b10816-bin-win-cpu-x64.zip"
            )
        );
    }
}
