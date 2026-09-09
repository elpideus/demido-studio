//! The manifest against the figures it claims to carry.
//!
//! The manifest is data, so this needs no seam and mocks nothing: it reads
//! `docs/rules/setup.md` section 4, which is where the measurements live, and
//! asserts every row of the manifest against the row that measured it. Both
//! sides are checked, so an archive nobody measured fails and a measured
//! archive nobody pinned fails too.
//!
//! The figures are `#27`'s and this ticket's, taken from the server and from
//! each archive's own index. None of them is an estimate, and this test is what
//! keeps that true: a pin that moves without a measurement moving with it is a
//! wizard that states a size it does not spend.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;

use demido_catalog::{Kind, MANIFEST};

/// One row of section 4's table, as written.
struct Measured {
    what: String,
    pin: String,
    download_mib: f64,
    on_disk_mib: f64,
    license: String,
}

fn setup_md() -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "..",
        "docs",
        "rules",
        "setup.md",
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("the rules file this test measures against: {path:?}: {err}"))
}

/// Section 4's table, one entry per row that names an archive.
fn measured() -> Vec<Measured> {
    let source = setup_md();
    let mut rows = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().replace(['`', '*'], ""))
            .collect();
        if cells.len() != 6 {
            continue;
        }
        if cells[0] != "Required" && cells[0] != "Capability" {
            continue;
        }
        let (Ok(download_mib), Ok(on_disk_mib)) =
            (cells[3].parse::<f64>(), cells[4].parse::<f64>())
        else {
            // The model row is the user's choice and its size varies, which is
            // the honest cell rather than a missing one.
            continue;
        };
        rows.push(Measured {
            what: cells[1].clone(),
            pin: cells[2].clone(),
            download_mib,
            on_disk_mib,
            license: cells[5].clone(),
        });
    }
    assert!(
        rows.len() > 3,
        "section 4's table did not parse: {} rows found",
        rows.len()
    );
    rows
}

#[test]
fn every_pin_size_and_license_in_the_manifest_is_the_measured_one() {
    let measured = measured();
    for archive in MANIFEST {
        let row = measured
            .iter()
            .find(|row| row.what.contains(archive.name))
            .unwrap_or_else(|| {
                panic!(
                    "{} is pinned but section 4 measured no such row",
                    archive.name
                )
            });

        // "same release" is how a companion names the pin of the build it
        // travels with, which is the release id itself.
        let pin = if row.pin == "same release" {
            demido_catalog::RELEASE
        } else {
            row.pin.as_str()
        };
        assert_eq!(archive.pin, pin, "{}: pin", archive.name);
        assert_eq!(
            archive.download_mib, row.download_mib,
            "{}: download size",
            archive.name
        );
        assert_eq!(
            archive.on_disk_mib, row.on_disk_mib,
            "{}: size on disk",
            archive.name
        );
        assert_eq!(
            archive.license.label(),
            row.license,
            "{}: license",
            archive.name
        );
    }
}

#[test]
fn every_llama_cpp_archive_section_four_measured_is_pinned() {
    for row in measured() {
        // The capability group is #46's second group and this slice fetches
        // none of it, so a manifest with no row for uv or Node is correct. What
        // cannot be missing is an archive of the backend itself.
        let backend = row.what.starts_with("llama-") || row.what.starts_with("cudart-");
        if !backend {
            continue;
        }
        assert!(
            MANIFEST
                .iter()
                .any(|archive| row.what.contains(archive.name)),
            "{} is a measured backend archive that nothing pins",
            row.what
        );
    }
}

#[test]
fn a_url_is_the_permanent_upstream_one_for_its_pin() {
    for archive in MANIFEST {
        let url = archive.url();
        assert!(
            url.starts_with("https://github.com/ggml-org/llama.cpp/releases/download/"),
            "{url}"
        );
        assert!(url.ends_with(archive.name), "{url}");
        assert!(url.contains(archive.pin), "{url}");
    }
}

#[test]
fn every_build_in_the_manifest_is_for_the_platform_this_release_supports() {
    for archive in MANIFEST.iter().filter(|a| a.kind == Kind::Build) {
        assert_eq!(archive.os, demido_catalog::Os::Windows, "{}", archive.name);
        assert_eq!(archive.arch, demido_catalog::Arch::X64, "{}", archive.name);
    }
}
