//! What this machine can run: the adapters it has, what CUDA its driver runs,
//! and the accelerator those two facts indicate.
//!
//! **Detection reports, it does not decide.** This crate says what is present
//! and which accelerator that points at. Which archive that turns into is
//! `demido-catalog`'s, the choice is still the user's, and it is verified by a
//! model actually loading rather than by anything asserted here. A detector
//! that guesses right nine times and lies confidently the tenth is worse than
//! one that reports what it saw.
//!
//! **And it does not speak.** Every reason and every note is a variant, never a
//! sentence: the window owns the words. That keeps one wording in one place
//! (`docs/rules/setup.md` section 3 fixes what the accelerator row says) and
//! keeps prose out of a crate that has no business holding any.
//!
//! Nothing here fails startup. An empty adapter list is a CPU pre-selection,
//! not an error.

mod cuda;
mod probe;

pub use cuda::{detect as cuda_driver, CudaDriver, CudaVersion};

use serde::{Deserialize, Serialize};

/// Who made the adapter, resolved from its PCI vendor id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Vendor {
    /// PCI-SIG vendor ids, as reported by DXGI.
    pub fn from_pci_id(id: u32) -> Self {
        match id {
            0x10de => Vendor::Nvidia,
            0x1002 | 0x1022 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            _ => Vendor::Other,
        }
    }

    /// The accelerator this vendor's cards are driven through. What is
    /// indicated, never what is available: whether a build of it is fetched at
    /// all is the manifest's answer, and the two are kept apart so that a row
    /// nobody can pick yet is still an honest row.
    pub fn indicates(self) -> Ecosystem {
        match self {
            Vendor::Nvidia => Ecosystem::Cuda,
            Vendor::Amd => Ecosystem::Rocm,
            Vendor::Intel | Vendor::Other => Ecosystem::Vulkan,
        }
    }
}

/// A way of getting work onto an accelerator. Which llama.cpp build to fetch
/// follows from this.
///
/// Four, because Windows is the platform of the pre-release versions
/// (Brief B03: "windows-only at first, during the pre-release versions") and
/// Metal is a variant nobody could pick. It is one line the day macOS is a
/// target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Cuda,
    Rocm,
    Vulkan,
    /// Always available, always last.
    Cpu,
}

/// One adapter, as the platform describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    /// Adapter order, stable within a boot, so it matches what a backend will
    /// be told to use.
    pub index: u32,
    pub name: String,
    pub vendor: Vendor,
    /// Memory soldered to the card, in bytes. The number a VRAM figure is drawn
    /// from.
    pub dedicated_memory: u64,
    /// Host memory the driver may lend the adapter. Real, and far slower, so it
    /// is reported separately rather than added in.
    pub shared_memory: u64,
}

/// Something detection could not do, in the shape the window renders.
///
/// A variant rather than a sentence, for the reason at the top of this file. A
/// note never fails startup: it explains a thin answer, and the CPU row still
/// works underneath every one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "kebab-case")]
pub enum Note {
    /// Windows would not open a DXGI factory, so no adapter could be asked.
    AdaptersUnreadable { detail: String },
    /// One adapter would not describe itself. The others still did.
    AdapterUnreadable { index: u32, detail: String },
    /// Windows answered, and reported no hardware adapter at all.
    NoAdapters,
    /// A platform whose adapters this build does not know how to enumerate.
    NotProbed,
}

/// Everything detection found, plus what it could not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub gpus: Vec<Gpu>,
    /// What CUDA the installed driver runs, which is what decides *which* CUDA
    /// build can load. Reported rather than resolved into a choice, for the
    /// same reason as everything else here.
    pub cuda: CudaDriver,
    /// Why detection came back thin, when it did.
    pub notes: Vec<Note>,
    /// What this build of Demido was compiled for: `windows`, `x86_64`.
    ///
    /// Half of why an archive is offered or is not, so it is reported beside
    /// the cards. From `std::env::consts` rather than probed, because the
    /// question is what this binary can load rather than what the machine could
    /// in principle run.
    pub os: String,
    pub arch: String,
}

impl Machine {
    /// Look at the machine. Never fails: a failed probe becomes a note.
    pub fn detect() -> Self {
        let (gpus, notes) = probe::gpus();
        Self {
            gpus,
            cuda: cuda::detect(),
            notes,
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        }
    }

    /// The card the answer is about: the one with the most memory.
    ///
    /// The largest, not the first. A machine with an integrated adapter at
    /// index 0 and a discrete card at index 1 is the common case rather than
    /// the exception.
    pub fn primary(&self) -> Option<&Gpu> {
        self.gpus.iter().max_by_key(|gpu| gpu.dedicated_memory)
    }

    /// The accelerator this machine indicates, and why.
    ///
    /// **Pre-selects and never asks.** A person with an NVIDIA card is not
    /// asked a question the machine can see the answer to
    /// (`docs/rules/setup.md` section 3), and the reason travels with the
    /// answer so the row can say what it saw rather than only what it picked.
    ///
    /// This is still reporting: what an AMD card indicates is ROCm whether or
    /// not a ROCm build is fetched, and `demido-catalog` is where that meets
    /// the manifest and becomes a row that can be chosen.
    pub fn preselection(&self) -> Preselection {
        let Some(gpu) = self.primary() else {
            return Preselection {
                ecosystem: Ecosystem::Cpu,
                reason: Reason::NoAdapter,
            };
        };
        let adapter = gpu.name.clone();
        match (gpu.vendor, self.cuda.ceiling()) {
            (Vendor::Nvidia, Some(driver)) => Preselection {
                ecosystem: Ecosystem::Cuda,
                reason: Reason::CudaDriver { adapter, driver },
            },
            // An NVIDIA card whose driver runs no CUDA cannot load a CUDA
            // build, so CUDA is not what this machine indicates. Reporting
            // rather than deciding: what is reported is that the driver said
            // no, and a user whose CUDA row is a driver install away is owed
            // that sentence.
            (Vendor::Nvidia, None) => Preselection {
                ecosystem: Ecosystem::Cpu,
                reason: Reason::NoCudaDriver { adapter },
            },
            (vendor, _) => Preselection {
                ecosystem: vendor.indicates(),
                reason: Reason::Adapter { adapter, vendor },
            },
        }
    }
}

/// The accelerator a machine indicates, with the reason attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preselection {
    pub ecosystem: Ecosystem,
    pub reason: Reason,
}

/// Why an accelerator was pre-selected, as facts rather than as a sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum Reason {
    /// An NVIDIA card, and the driver that says which CUDA it runs.
    CudaDriver {
        adapter: String,
        driver: CudaVersion,
    },
    /// An NVIDIA card whose driver answered nothing about CUDA.
    NoCudaDriver { adapter: String },
    /// A card from a vendor whose accelerator follows from the vendor alone.
    Adapter { adapter: String, vendor: Vendor },
    /// No hardware adapter at all, so there is nothing to accelerate onto.
    NoAdapter,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    use super::*;

    fn machine(gpus: Vec<Gpu>, cuda: CudaDriver) -> Machine {
        Machine {
            gpus,
            cuda,
            notes: vec![],
            os: "windows".into(),
            arch: "x86_64".into(),
        }
    }

    fn gpu(index: u32, name: &str, vendor: Vendor, gib: u64) -> Gpu {
        Gpu {
            index,
            name: name.into(),
            vendor,
            dedicated_memory: gib * 1024 * 1024 * 1024,
            shared_memory: 0,
        }
    }

    #[test]
    fn vendors_come_from_pci_ids() {
        assert_eq!(Vendor::from_pci_id(0x10de), Vendor::Nvidia);
        assert_eq!(Vendor::from_pci_id(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from_pci_id(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from_pci_id(0xbeef), Vendor::Other);
    }

    #[test]
    fn the_primary_card_is_the_largest_not_the_first() {
        let machine = machine(
            vec![
                gpu(0, "Intel UHD Graphics 770", Vendor::Intel, 2),
                gpu(1, "NVIDIA GeForce RTX 3060", Vendor::Nvidia, 12),
            ],
            CudaDriver::Supports(CudaVersion::new(13, 2)),
        );
        assert_eq!(machine.primary().map(|gpu| gpu.index), Some(1));
    }

    #[test]
    fn an_nvidia_card_preselects_cuda_with_the_driver_it_reported() {
        let machine = machine(
            vec![gpu(0, "NVIDIA GeForce RTX 3060", Vendor::Nvidia, 12)],
            CudaDriver::Supports(CudaVersion::new(13, 2)),
        );
        assert_eq!(
            machine.preselection(),
            Preselection {
                ecosystem: Ecosystem::Cuda,
                reason: Reason::CudaDriver {
                    adapter: "NVIDIA GeForce RTX 3060".into(),
                    driver: CudaVersion::new(13, 2),
                },
            }
        );
    }

    #[test]
    fn an_nvidia_card_with_no_cuda_driver_preselects_cpu_and_says_which() {
        let machine = machine(
            vec![gpu(0, "NVIDIA GeForce RTX 3060", Vendor::Nvidia, 12)],
            CudaDriver::Absent,
        );
        assert_eq!(
            machine.preselection(),
            Preselection {
                ecosystem: Ecosystem::Cpu,
                reason: Reason::NoCudaDriver {
                    adapter: "NVIDIA GeForce RTX 3060".into(),
                },
            }
        );
    }

    #[test]
    fn an_amd_card_indicates_rocm_whether_or_not_a_build_of_it_exists() {
        let machine = machine(
            vec![gpu(0, "AMD Radeon RX 7800 XT", Vendor::Amd, 16)],
            CudaDriver::Absent,
        );
        assert_eq!(machine.preselection().ecosystem, Ecosystem::Rocm);
    }

    #[test]
    fn a_machine_with_no_adapter_preselects_cpu() {
        let machine = machine(vec![], CudaDriver::Absent);
        assert_eq!(
            machine.preselection(),
            Preselection {
                ecosystem: Ecosystem::Cpu,
                reason: Reason::NoAdapter,
            }
        );
    }

    #[test]
    fn detecting_this_machine_never_panics_and_always_answers() {
        let machine = Machine::detect();
        let preselection = machine.preselection();
        println!("this machine: {machine:?} -> {preselection:?}");
    }
}
