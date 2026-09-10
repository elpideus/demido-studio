//! What a machine gets offered, and what it gets given.
//!
//! Two seams, both pure: `select` picks archives out of a list for a target,
//! and `Selector` turns a detected machine into the rows the wizard draws. The
//! list is a parameter rather than the manifest, so the release's other CUDA
//! builds can be offered to the selector here without anything ever being
//! fetched. Nothing in this file touches the network or the disk.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use demido_catalog::{
    Arch, Archive, Availability, Group, Kind, License, Os, Selector, Target, MANIFEST,
};
use demido_hardware::{
    CudaDriver, CudaVersion, Ecosystem, Gpu, Machine, Preselection, Reason, Vendor,
};

/// The Windows x64 archives release `b10816` actually published, which is more
/// than the manifest pins: the point of the selector is that it is handed a
/// choice.
///
/// Download sizes are the release's own byte counts. The size on disk is zero
/// for the archives the manifest does not pin, and honestly so: nothing has
/// measured them because nothing fetches them.
fn published() -> Vec<Archive> {
    let mut archives: Vec<Archive> = MANIFEST.to_vec();
    archives.push(Archive {
        name: "llama-b10816-bin-win-cuda-12.4-x64.zip",
        group: Group::Required,
        pin: "b10816",
        kind: Kind::Build,
        ecosystem: Ecosystem::Cuda,
        toolkit: Some(CudaVersion::new(12, 4)),
        os: Os::Windows,
        arch: Arch::X64,
        download_mib: 242.2,
        on_disk_mib: 0.0,
        license: License::Mit { owner: "ggml-org" },
    });
    archives.push(Archive {
        name: "cudart-llama-bin-win-cuda-12.4-x64.zip",
        group: Group::Required,
        pin: "b10816",
        kind: Kind::CudaRuntime,
        ecosystem: Ecosystem::Cuda,
        toolkit: Some(CudaVersion::new(12, 4)),
        os: Os::Windows,
        arch: Arch::X64,
        download_mib: 373.3,
        on_disk_mib: 0.0,
        license: License::NvidiaCudaEula,
    });
    archives
}

fn rig() -> Machine {
    machine(
        vec![gpu("NVIDIA GeForce RTX 3060", Vendor::Nvidia)],
        CudaDriver::Supports(CudaVersion::new(13, 2)),
    )
}

fn machine(gpus: Vec<Gpu>, cuda: CudaDriver) -> Machine {
    Machine {
        gpus,
        cuda,
        notes: vec![],
        os: "windows".into(),
        arch: "x86_64".into(),
    }
}

fn gpu(name: &str, vendor: Vendor) -> Gpu {
    Gpu {
        index: 0,
        name: name.into(),
        vendor,
        dedicated_memory: 12 * 1024 * 1024 * 1024,
        shared_memory: 0,
    }
}

fn row(selector: &Selector<'_>, ecosystem: Ecosystem) -> Availability {
    selector
        .rows
        .iter()
        .find(|row| row.ecosystem == ecosystem)
        .unwrap_or_else(|| panic!("no {ecosystem:?} row"))
        .availability
        .clone()
}

#[test]
fn a_thirteen_two_driver_takes_the_thirteen_three_archive_not_the_twelve_four_one() {
    // This is #19's defect, and the rig is the machine it was found on: driver
    // 596.49 reports CUDA 13.2, and the 13.3 build initialises CUDA and
    // offloads every layer on it. v2 compared the whole version and handed this
    // card 242.2 MiB instead of 142.6 for no gain. #19 writes that pair as
    // `254 MB` and `143 MB`: the same two files, the first in decimal MB and
    // the second in MiB. Everything here is MiB.
    let published = published();
    let selection = demido_catalog::select(
        &published,
        Target {
            os: Os::Windows,
            arch: Arch::X64,
            ecosystem: Ecosystem::Cuda,
            cuda: CudaDriver::Supports(CudaVersion::new(13, 2)),
        },
    )
    .expect("the release publishes a CUDA build this driver runs");

    assert_eq!(
        selection.build.name,
        "llama-b10816-bin-win-cuda-13.3-x64.zip"
    );
    assert_eq!(
        selection
            .companions
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>(),
        vec!["cudart-llama-bin-win-cuda-13.3-x64.zip"],
        "the CUDA build links against cudart and ships without it"
    );
    assert_eq!(selection.download_mib(), 142.6 + 372.9);
    assert_eq!(selection.on_disk_mib(), 182.6 + 489.0);
}

#[test]
fn a_twelve_four_driver_takes_the_twelve_four_archive_because_a_newer_major_will_not_load() {
    let published = published();
    let selection = demido_catalog::select(
        &published,
        Target {
            os: Os::Windows,
            arch: Arch::X64,
            ecosystem: Ecosystem::Cuda,
            cuda: CudaDriver::Supports(CudaVersion::new(12, 4)),
        },
    )
    .expect("the release publishes a CUDA 12 build");

    assert_eq!(
        selection.build.name,
        "llama-b10816-bin-win-cuda-12.4-x64.zip"
    );
    assert_eq!(
        selection
            .companions
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>(),
        vec!["cudart-llama-bin-win-cuda-12.4-x64.zip"],
        "a companion is the runtime of the same toolkit, never of another"
    );
}

#[test]
fn a_machine_with_no_cuda_driver_is_offered_no_cuda_build_at_all() {
    let published = published();
    let refused = demido_catalog::select(
        &published,
        Target {
            os: Os::Windows,
            arch: Arch::X64,
            ecosystem: Ecosystem::Cuda,
            cuda: CudaDriver::Absent,
        },
    );
    assert!(
        refused.is_err(),
        "a CUDA build on a machine with no CUDA driver is a download that cannot load"
    );
}

#[test]
fn the_cpu_build_needs_no_driver_and_no_companion() {
    let published = published();
    let selection = demido_catalog::select(
        &published,
        Target {
            os: Os::Windows,
            arch: Arch::X64,
            ecosystem: Ecosystem::Cpu,
            cuda: CudaDriver::Absent,
        },
    )
    .expect("the CPU build is what makes an empty machine a working one");

    assert_eq!(selection.build.name, "llama-b10816-bin-win-cpu-x64.zip");
    assert!(selection.companions.is_empty());
}

#[test]
fn a_platform_this_release_has_no_build_for_is_a_refusal_rather_than_a_guess() {
    let published = published();
    let refused = demido_catalog::select(
        &published,
        Target {
            os: Os::Linux,
            arch: Arch::Arm64,
            ecosystem: Ecosystem::Cpu,
            cuda: CudaDriver::Unknown,
        },
    );
    assert!(refused.is_err());
}

#[test]
fn the_rig_opens_on_a_cuda_row_that_is_chosen_and_carries_its_reason() {
    let machine = rig();
    let selector = Selector::for_machine(&machine);

    assert_eq!(selector.chosen, Ecosystem::Cuda);
    assert_eq!(
        selector.preselection,
        Preselection {
            ecosystem: Ecosystem::Cuda,
            reason: Reason::CudaDriver {
                adapter: "NVIDIA GeForce RTX 3060".into(),
                driver: CudaVersion::new(13, 2),
            },
        },
        "the row states what it saw, not only what it picked"
    );
    assert_eq!(
        row(&selector, Ecosystem::Cuda),
        Availability::Offered {
            download_mib: 142.6 + 372.9,
            on_disk_mib: 182.6 + 489.0,
        },
        "every byte is stated before anything is fetched"
    );
}

#[test]
fn the_answered_row_is_still_a_row_and_every_accelerator_has_one() {
    let selector = Selector::for_machine(&rig());
    assert_eq!(
        selector
            .rows
            .iter()
            .map(|row| row.ecosystem)
            .collect::<Vec<_>>(),
        vec![
            Ecosystem::Cuda,
            Ecosystem::Rocm,
            Ecosystem::Vulkan,
            Ecosystem::Cpu
        ],
        "a step that answers itself is still rendered, and the brief's selector is present"
    );
}

#[test]
fn rocm_and_vulkan_are_rows_that_say_no_build_is_fetched_for_them_yet() {
    let selector = Selector::for_machine(&rig());
    assert_eq!(row(&selector, Ecosystem::Rocm), Availability::NoBuildYet);
    assert_eq!(row(&selector, Ecosystem::Vulkan), Availability::NoBuildYet);
}

#[test]
fn the_preselected_row_can_be_overridden_for_any_row_that_carries_a_build() {
    let mut selector = Selector::for_machine(&rig());

    assert!(selector.choose(Ecosystem::Cpu));
    assert_eq!(selector.chosen, Ecosystem::Cpu);
    assert_eq!(
        selector.selection().map(|s| s.build.name),
        Some("llama-b10816-bin-win-cpu-x64.zip")
    );

    assert!(
        !selector.choose(Ecosystem::Rocm),
        "a row with no build is honest about it rather than selectable"
    );
    assert_eq!(
        selector.chosen,
        Ecosystem::Cpu,
        "a refused override changes nothing"
    );
}

#[test]
fn a_machine_with_no_discrete_gpu_lands_on_a_cpu_row_that_works() {
    let selector = Selector::for_machine(&machine(vec![], CudaDriver::Absent));

    assert_eq!(selector.chosen, Ecosystem::Cpu);
    assert_eq!(selector.preselection.reason, Reason::NoAdapter);
    assert_eq!(
        row(&selector, Ecosystem::Cpu),
        Availability::Offered {
            download_mib: 17.6,
            on_disk_mib: 44.6,
        }
    );
    assert_eq!(
        selector.selection().map(|s| s.build.name),
        Some("llama-b10816-bin-win-cpu-x64.zip"),
        "the row a machine with nothing on it lands on has to be one that runs"
    );
}

#[test]
fn an_amd_card_reads_the_rocm_row_and_is_answered_with_the_cpu_one() {
    let machine = machine(
        vec![gpu("AMD Radeon RX 7800 XT", Vendor::Amd)],
        CudaDriver::Absent,
    );
    let selector = Selector::for_machine(&machine);

    assert_eq!(
        selector.preselection.ecosystem,
        Ecosystem::Rocm,
        "detection reports what the card indicates, whether or not a build of it is fetched"
    );
    assert_eq!(
        selector.chosen,
        Ecosystem::Cpu,
        "and the machine is left on a row that runs rather than on one that does not exist"
    );
    assert_eq!(row(&selector, Ecosystem::Rocm), Availability::NoBuildYet);
}

#[test]
fn an_nvidia_card_with_no_driver_reads_a_cuda_row_that_names_the_driver_as_missing() {
    let machine = machine(
        vec![gpu("NVIDIA GeForce RTX 3060", Vendor::Nvidia)],
        CudaDriver::Absent,
    );
    let selector = Selector::for_machine(&machine);

    assert_eq!(
        row(&selector, Ecosystem::Cuda),
        Availability::NeedsCudaDriver { runs: None },
        "a CUDA row that vanished would leave a user wondering; it is a driver install away"
    );
    assert_eq!(selector.chosen, Ecosystem::Cpu);
}

#[test]
fn a_driver_too_old_for_every_build_reads_as_a_driver_to_update() {
    let machine = machine(
        vec![gpu("NVIDIA GeForce GTX 1060", Vendor::Nvidia)],
        CudaDriver::Supports(CudaVersion::new(11, 8)),
    );
    let selector = Selector::for_machine(&machine);

    assert_eq!(
        row(&selector, Ecosystem::Cuda),
        Availability::NeedsCudaDriver {
            runs: Some(CudaVersion::new(11, 8)),
        },
        "what the driver runs is what turns install into update"
    );
    assert_eq!(selector.chosen, Ecosystem::Cpu);
}
