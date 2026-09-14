//! Which archives one target gets, out of the ones it is offered.
//!
//! Pure, and handed its candidates rather than reading the manifest, so a
//! release's other CUDA builds can be put in front of it in a test without
//! anything being fetched. `Selector` is what points it at the manifest.

use demido_hardware::{CudaDriver, CudaVersion, Ecosystem};

use crate::{Arch, Archive, Kind, Os};

/// The machine a build is being chosen for.
///
/// A target rather than a `Machine`, because it is also something chosen by
/// hand: a person preparing an install for a second machine picks its
/// accelerator, and `cuda` is then `Unknown` rather than this machine's answer.
/// A type that could only describe the current host would quietly make that
/// wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
    pub ecosystem: Ecosystem,
    /// What the driver of the machine this build is for can run. Carried on the
    /// target rather than asked during selection, because asking this machine
    /// about another one produces a confidently wrong answer.
    pub cuda: CudaDriver,
}

/// What to fetch to get one working build onto a machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection<'a> {
    /// The archive holding `llama-server`.
    pub build: &'a Archive,
    /// Archives that must be unpacked alongside it. One case exists and it is
    /// not optional: a Windows CUDA build links against `cudart64_*.dll` and
    /// ships without it, and a build missing it starts, prints nothing useful
    /// and dies.
    pub companions: Vec<&'a Archive>,
}

impl<'a> Selection<'a> {
    /// Everything to fetch, the build first.
    pub fn archives(&self) -> impl Iterator<Item = &'a Archive> + '_ {
        std::iter::once(self.build).chain(self.companions.iter().copied())
    }

    /// What the network spends, in MiB. Stated before a byte is fetched.
    pub fn download_mib(&self) -> f64 {
        self.archives().map(|archive| archive.download_mib).sum()
    }

    /// What the disk holds afterwards, in MiB. The other half of the sentence,
    /// because an archive that downloads small can still fill a disk.
    pub fn on_disk_mib(&self) -> f64 {
        self.archives().map(|archive| archive.on_disk_mib).sum()
    }
}

/// Why a target gets nothing.
///
/// Two, because a person can act on both and the actions are different:
/// nothing was pinned for that combination, or something was pinned and this
/// driver cannot load it. One error covering both would be actionable for
/// neither, and these are exactly the two states a row renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoBuild {
    /// No archive in the manifest is for this platform and accelerator.
    NotPinned {
        os: Os,
        arch: Arch,
        ecosystem: Ecosystem,
    },
    /// A build is pinned and this driver runs no CUDA new enough for it.
    /// `runs` is what the driver reported, and `None` is no CUDA driver at all,
    /// which is the difference between updating one and installing one.
    NeedsCudaDriver { runs: Option<CudaVersion> },
}

impl std::fmt::Display for NoBuild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoBuild::NotPinned {
                os,
                arch,
                ecosystem,
            } => write!(f, "no {ecosystem:?} build is pinned for {os} {arch}"),
            NoBuild::NeedsCudaDriver { runs: Some(runs) } => {
                write!(f, "every pinned CUDA build is newer than CUDA {runs}")
            }
            NoBuild::NeedsCudaDriver { runs: None } => f.write_str("no CUDA driver answered"),
        }
    }
}

impl std::error::Error for NoBuild {}

/// Pick the archives for `target` out of `archives`.
///
/// **No silent substitution.** A target whose accelerator has no build it can
/// load is told so, rather than quietly handed the CPU build: the row it came
/// from is what says a build cannot be had, and swapping one out underneath a
/// user is how "why is inference slow" becomes unanswerable. Falling back is
/// the caller's, and `Selector` is where it happens, in the open.
///
/// The build is the newest toolkit the driver can load, which is
/// [#19](https://github.com/elpideus/demido-studio/issues/19)'s fix: the major
/// version is the gate and the minor version is not, so a 13.2 driver takes the
/// 13.3 archive rather than the 12.4 one. When nobody could be asked
/// (`CudaDriver::Unknown`, a target describing another machine) the least
/// demanding build wins instead: an assumption that cannot fail beats a
/// preference that can.
pub fn select<'a>(archives: &'a [Archive], target: Target) -> Result<Selection<'a>, NoBuild> {
    let not_pinned = NoBuild::NotPinned {
        os: target.os,
        arch: target.arch,
        ecosystem: target.ecosystem,
    };

    let candidates: Vec<&Archive> = archives
        .iter()
        .filter(|archive| {
            archive.kind == Kind::Build
                && archive.os == target.os
                && archive.arch == target.arch
                && archive.ecosystem == target.ecosystem
        })
        .collect();
    if candidates.is_empty() {
        return Err(not_pinned);
    }

    let loadable: Vec<&Archive> = candidates
        .into_iter()
        .filter(|archive| {
            archive
                .toolkit
                .is_none_or(|toolkit| target.cuda.runs(toolkit))
        })
        .collect();
    if loadable.is_empty() {
        return Err(NoBuild::NeedsCudaDriver {
            runs: target.cuda.ceiling(),
        });
    }

    // Both arms are a `max_by` over the same tie break, and only the toolkit
    // comparison turns around: written as a `min_by` instead, the shorter name
    // would quietly stop winning on the arm nobody was looking at.
    let shortest_name_first =
        |a: &&Archive, b: &&Archive| b.name.len().cmp(&a.name.len()).then(b.name.cmp(a.name));
    let newest_toolkit = |a: &&Archive, b: &&Archive| a.toolkit.cmp(&b.toolkit);
    let build = match target.cuda {
        CudaDriver::Unknown => loadable
            .into_iter()
            .max_by(|a, b| newest_toolkit(b, a).then_with(|| shortest_name_first(a, b))),
        _ => loadable
            .into_iter()
            .max_by(|a, b| newest_toolkit(a, b).then_with(|| shortest_name_first(a, b))),
    };
    let Some(build) = build else {
        return Err(not_pinned);
    };

    Ok(Selection {
        companions: companions(archives, build),
        build,
    })
}

/// The runtime archives that have to be unpacked beside `build`.
///
/// The companion of the same toolkit, never of another: upstream publishes one
/// per CUDA version and they are not interchangeable.
fn companions<'a>(archives: &'a [Archive], build: &Archive) -> Vec<&'a Archive> {
    if build.ecosystem != Ecosystem::Cuda {
        return Vec::new();
    }
    archives
        .iter()
        .filter(|archive| {
            archive.kind == Kind::CudaRuntime
                && archive.os == build.os
                && archive.arch == build.arch
                && archive.toolkit == build.toolkit
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    use super::*;
    use crate::MANIFEST;

    fn windows(ecosystem: Ecosystem, cuda: CudaDriver) -> Target {
        Target {
            os: Os::Windows,
            arch: Arch::X64,
            ecosystem,
            cuda,
        }
    }

    #[test]
    fn the_manifest_answers_the_rig() {
        let selection = select(
            MANIFEST,
            windows(
                Ecosystem::Cuda,
                CudaDriver::Supports(CudaVersion::new(13, 2)),
            ),
        );
        assert_eq!(
            selection.map(|s| s.build.name),
            Ok("llama-b10816-bin-win-cuda-13.3-x64.zip")
        );
    }

    #[test]
    fn nothing_is_pinned_for_an_accelerator_this_cut_does_not_fetch() {
        assert_eq!(
            select(MANIFEST, windows(Ecosystem::Rocm, CudaDriver::Absent)),
            Err(NoBuild::NotPinned {
                os: Os::Windows,
                arch: Arch::X64,
                ecosystem: Ecosystem::Rocm,
            })
        );
    }

    #[test]
    fn a_machine_nobody_could_ask_is_given_the_build_that_asks_least() {
        // A target describing somebody else's machine. The newest build might
        // load there and the oldest certainly does.
        let selection = select(MANIFEST, windows(Ecosystem::Cuda, CudaDriver::Unknown));
        assert_eq!(
            selection.map(|s| s.build.name),
            Ok("llama-b10816-bin-win-cuda-13.3-x64.zip"),
            "one CUDA build is pinned, so least demanding and newest are the same file"
        );
    }
}
