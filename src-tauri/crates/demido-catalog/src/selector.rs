//! The accelerator selector: one row per accelerator, one of them already
//! answered.
//!
//! **Detection pre-selects and never asks, and the row is still drawn.** A
//! person with an NVIDIA card is not asked a question the machine can see the
//! answer to, and the brief's word is `selector`, so the answer is a row that
//! can be overridden rather than a decision taken off screen
//! (`docs/rules/setup.md` section 3).
//!
//! Brief B11: "Guided set-up on first launch (GPU & GPU Ecosystem (CUDA, ROCm, etc.) selector, Runtime & Dependencies installation, etc.)"
//!
//! **Every row is real, including the ones with nothing behind them.** ROCm and
//! Vulkan say no build is fetched for them yet. Hiding them would leave a
//! person with an AMD card looking for an option that is not there; claiming
//! them would ship a path no card on the rig has ever run.

use demido_hardware::{CudaDriver, Ecosystem, Machine, Preselection};
use serde::Serialize;

use crate::{select, Arch, Archive, NoBuild, Os, Selection, Target, MANIFEST};

/// The accelerators the wizard draws, in the order it draws them.
///
/// Fixed rather than derived from the machine, because a row's absence is a
/// question a user cannot ask about. What varies is a row's availability, never
/// whether it is there.
pub const ACCELERATORS: [Ecosystem; 4] = [
    Ecosystem::Cuda,
    Ecosystem::Rocm,
    Ecosystem::Vulkan,
    Ecosystem::Cpu,
];

/// One row of the selector.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub ecosystem: Ecosystem,
    pub availability: Availability,
}

/// What a row can offer, and what it says when it cannot.
///
/// Facts rather than sentences, for the same reason as `demido_hardware::Note`:
/// the window owns the words, and `docs/rules/setup.md` section 3 owns what
/// they say.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "availability", rename_all = "kebab-case")]
pub enum Availability {
    /// A build is pinned and this machine can load it. Both figures are MiB,
    /// stated before anything is fetched.
    Offered { download_mib: f64, on_disk_mib: f64 },
    /// A build is pinned and the driver cannot load it. `runs` is what the
    /// driver reported: `None` is a driver to install, `Some` is one to update.
    NeedsCudaDriver {
        runs: Option<demido_hardware::CudaVersion>,
    },
    /// No build is fetched for this accelerator yet.
    NoBuildYet,
}

/// The rows, the answer detection pre-selected, and the row that answer landed
/// on.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector<'a> {
    pub rows: Vec<Row>,
    /// What the machine indicated and why, unchanged by anything the manifest
    /// does or does not carry. The row shows the reason beside the answer.
    pub preselection: Preselection,
    /// The row currently selected. The pre-selection when a build can be had
    /// for it, and the CPU row otherwise, which is the only substitution that
    /// happens anywhere and it happens in the open.
    pub chosen: Ecosystem,
    archives: &'a [Archive],
    target: Option<(Os, Arch)>,
    cuda: CudaDriver,
}

impl Selector<'static> {
    /// The selector for this machine, over what S1 pins.
    pub fn for_machine(machine: &Machine) -> Self {
        Selector::from_archives(MANIFEST, machine)
    }
}

impl<'a> Selector<'a> {
    /// The same, over an arbitrary list of archives. The seam the tests use,
    /// and what a second manifest would enter through.
    pub fn from_archives(archives: &'a [Archive], machine: &Machine) -> Self {
        let target = Os::named(&machine.os).zip(Arch::named(&machine.arch));
        let mut selector = Self {
            rows: Vec::new(),
            preselection: machine.preselection(),
            chosen: Ecosystem::Cpu,
            archives,
            target,
            cuda: machine.cuda,
        };
        selector.rows = ACCELERATORS
            .iter()
            .map(|ecosystem| Row {
                ecosystem: *ecosystem,
                availability: selector.availability(*ecosystem),
            })
            .collect();
        if selector.offers(selector.preselection.ecosystem) {
            selector.chosen = selector.preselection.ecosystem;
        }
        selector
    }

    /// Override the answer. `false` when that row carries no build a person
    /// could take, and then nothing changes: a row that cannot be had says so
    /// rather than becoming a broken install.
    pub fn choose(&mut self, ecosystem: Ecosystem) -> bool {
        if !self.offers(ecosystem) {
            return false;
        }
        self.chosen = ecosystem;
        true
    }

    /// What the chosen row would fetch.
    ///
    /// `None` only when nothing is pinned for this machine at all, which is a
    /// platform this build has no archives for rather than a failure. Startup
    /// never blocks on it.
    pub fn selection(&self) -> Option<Selection<'a>> {
        self.target(self.chosen)
            .and_then(|target| select(self.archives, target).ok())
    }

    fn target(&self, ecosystem: Ecosystem) -> Option<Target> {
        self.target.map(|(os, arch)| Target {
            os,
            arch,
            ecosystem,
            cuda: self.cuda,
        })
    }

    fn availability(&self, ecosystem: Ecosystem) -> Availability {
        let Some(target) = self.target(ecosystem) else {
            return Availability::NoBuildYet;
        };
        match select(self.archives, target) {
            Ok(selection) => Availability::Offered {
                download_mib: selection.download_mib(),
                on_disk_mib: selection.on_disk_mib(),
            },
            Err(NoBuild::NeedsCudaDriver { runs }) => Availability::NeedsCudaDriver { runs },
            Err(NoBuild::NotPinned { .. }) => Availability::NoBuildYet,
        }
    }

    fn offers(&self, ecosystem: Ecosystem) -> bool {
        self.rows.iter().any(|row| {
            row.ecosystem == ecosystem && matches!(row.availability, Availability::Offered { .. })
        })
    }
}
