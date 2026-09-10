//! The live suite: a real pin, off the real network, onto a real disk, and
//! verified by really starting it.
//!
//! [#46](https://github.com/elpideus/demido-studio/issues/46) writes the cost
//! of the fetcher having no seam down rather than glossing it: "a fetch
//! failure partway through a large archive has no automated test in S1, and
//! the retry path is exercised by hand during the window gate with the result
//! recorded in the closing comment." This file is how it is exercised, so the
//! hand doing it presses one command rather than improvising.
//!
//! **It fails rather than skips when the rig is absent**, matching
//! `demido-inference/tests/rig.rs`: a live suite that quietly passes on a
//! machine with no models proves the least exactly where it would be trusted
//! most.
//!
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml -p demido-runtimes -- --ignored --nocapture
//! ```

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use demido_catalog::{Archive, Kind, Selector, MANIFEST};
use demido_hardware::Machine;
use demido_runtimes::{Cancel, Files, Installed, Runtimes, Verification, LLAMA_CPP};

/// The CPU build: 17.6 MiB, no card needed to unpack it, and the same fetch
/// path the 372.9 MiB `cudart` takes. The point of the live run is the
/// network and the disk, not the size of the file.
fn cpu_build() -> &'static Archive {
    MANIFEST
        .iter()
        .find(|archive| archive.kind == Kind::Build && archive.toolkit.is_none())
        .expect("the manifest pins a CPU build")
}

/// A GGUF to verify against. `DEMIDO_MODELS` overrides the root, as in
/// `demido-inference`'s rig.
fn a_model() -> PathBuf {
    let root = match std::env::var("DEMIDO_MODELS") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_default();
            PathBuf::from(home).join(".lmstudio/models")
        }
    };
    let model = root.join("unsloth/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q8_0.gguf");
    assert!(
        model.exists(),
        "no development model at {}. Point DEMIDO_MODELS at the library root.",
        model.display()
    );
    model
}

/// A profile whose verification is the declared command, run for real.
fn profile(dir: &PathBuf) -> Runtimes<Files> {
    let model = a_model();
    Runtimes::new(
        Files::in_profile(dir),
        dir.join("runtimes"),
        move |installed: Installed| {
            let model = model.clone();
            Box::pin(async move {
                Verification::LoadsAModelAndGeneratesOneToken
                    .run(&installed, &model)
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("demido-runtimes-live")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The whole ticket in one run: fetch the pin, unpack it, and let the
/// declared verification command decide whether the row may claim it works.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads a real archive and needs a model; the window gate runs it"]
async fn the_pinned_build_arrives_verifies_and_is_owned() {
    let dir = scratch("required");
    let runtimes = profile(&dir);
    let build = cpu_build();

    let seen = AtomicU64::new(0);
    let outcome = runtimes
        .fetch_row(
            LLAMA_CPP,
            build.pin,
            std::slice::from_ref(build),
            |archive, progress| {
                if progress.bytes > seen.swap(progress.bytes, Ordering::Relaxed) {
                    // Printed rather than asserted on: what this proves is
                    // that a person watching a 373 MiB fetch sees it move.
                    print!(
                        "\r{archive}: {} of {} bytes",
                        progress.bytes, progress.total
                    );
                }
            },
            &Cancel::new(),
        )
        .await
        .expect("the fetch itself");

    assert_eq!(
        outcome,
        demido_runtimes::Outcome::Verified,
        "the pinned build did not load a model and generate a token"
    );
    assert!(
        seen.load(Ordering::Relaxed) > 0,
        "a fetch that reported no progress is a progress bar that never moved"
    );

    let ledger = runtimes.read().expect("read");
    assert!(
        ledger.on_disk_mib() > 0.0,
        "the ledger totals what was actually unpacked"
    );
    assert!(
        runtimes.unused().expect("diffed").is_empty(),
        "a clean fetch leaves nothing for the Unused row: the archive is gone \
         and no half of it is left behind"
    );
}

/// The retry path the ticket says is exercised by hand: a fetch cancelled
/// partway leaves its bytes, and the next one carries on from them rather
/// than starting the download again.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads a real archive; the window gate runs it"]
async fn a_cancelled_fetch_resumes_from_upstream_rather_than_restarting() {
    let dir = scratch("resume");
    std::fs::create_dir_all(&dir).expect("made the scratch");
    let build = cpu_build();
    let client = reqwest::Client::new();

    let cancel = Cancel::new();
    let interrupted = {
        let cancel_at_a_megabyte = cancel.clone();
        demido_runtimes::fetch::fetch(
            build,
            &dir,
            &client,
            move |progress| {
                if progress.bytes > 1024 * 1024 {
                    cancel_at_a_megabyte.cancel();
                }
            },
            &cancel,
        )
        .await
    };
    assert!(
        matches!(
            interrupted,
            Err(demido_runtimes::FetchError::Cancelled { .. })
        ),
        "the fetch was meant to be cancelled partway"
    );

    let part = dir.join(format!("{}.part", build.name));
    let stopped_at = std::fs::metadata(&part).expect("the partial file").len();
    assert!(
        stopped_at > 0,
        "cancelling threw away everything it had downloaded"
    );

    let resumed_from = AtomicU64::new(u64::MAX);
    let finished = demido_runtimes::fetch::fetch(
        build,
        &dir,
        &client,
        |progress| {
            let _ = resumed_from.fetch_min(progress.bytes, Ordering::Relaxed);
        },
        &Cancel::new(),
    )
    .await
    .expect("the second attempt finishes");

    assert!(
        resumed_from.load(Ordering::Relaxed) >= stopped_at,
        "upstream honoured the range request: the second attempt started at \
         {} rather than at zero",
        resumed_from.load(Ordering::Relaxed)
    );
    let whole = std::fs::metadata(&finished)
        .expect("the finished archive")
        .len();
    let expected = (build.download_mib * 1024.0 * 1024.0) as u64;
    let drift = whole.abs_diff(expected);
    assert!(
        drift < 512 * 1024,
        "the resumed file is {whole} bytes against a measured {expected}"
    );
}

/// The gate the rig exists for: what this machine is actually asked to run.
///
/// Detection, selection and the fetch in one line each, which is the whole
/// chain #45 and #46 split between them. It is the CUDA pair rather than the
/// CPU build on purpose, because the two things only this can prove are that
/// the companion lands beside the build (`llama-server` resolves
/// `cublasLt64_13.dll` next to itself) and that verification touches the card
/// at all. `docs/rules/runtimes.md` section 2: "a CUDA build that cannot
/// resolve `cublasLt64_13.dll` does not load a model", and a check that never
/// asks the card cannot tell.
///
/// Offload stays `Auto`, which is `LlamaCppConfig`'s default: verification
/// asks whether the runtime runs, not whether one particular model fits, and a
/// row called broken because a 8 GiB model did not fit a 12 GiB card would be
/// the wrong answer to the right question.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads 515.5 MiB and needs the card; the window gate runs it"]
async fn the_pair_this_machine_selects_arrives_and_verifies_on_the_card() {
    let dir = scratch("cuda");
    let runtimes = profile(&dir);

    let machine = Machine::detect();
    let selector = Selector::for_machine(&machine);
    let selection = selector.selection().expect("this machine gets a build");
    let archives: Vec<&Archive> = selection.archives().collect();
    println!(
        "detected {:?}, chose {} and {} companion(s), {:.1} MiB down and {:.1} on disk",
        machine.preselection(),
        selection.build.name,
        selection.companions.len(),
        selection.download_mib(),
        selection.on_disk_mib()
    );
    assert!(
        archives
            .iter()
            .any(|archive| archive.kind == Kind::CudaRuntime),
        "this gate is the CUDA pair; the selector offered {:?}",
        archives.iter().map(|a| a.name).collect::<Vec<_>>()
    );

    let outcome = runtimes
        .fetch_row(
            LLAMA_CPP,
            selection.build.pin,
            &archives,
            |archive, progress| {
                if progress.total > 0 && progress.bytes == progress.total {
                    println!("{archive}: {} bytes", progress.bytes);
                }
            },
            &Cancel::new(),
        )
        .await
        .expect("the fetch itself");
    assert_eq!(outcome, demido_runtimes::Outcome::Verified);

    let row = dir.join("runtimes").join(demido_runtimes::directory_name(
        LLAMA_CPP,
        selection.build.pin,
    ));
    assert!(row.join("llama-server.exe").exists(), "the build");
    let dlls = std::fs::read_dir(&row)
        .expect("read the row")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("cudart64_"))
        .count();
    assert!(
        dlls > 0,
        "the companion unpacked beside the build rather than into a folder of its own"
    );

    let ledger = runtimes.read().expect("read");
    println!("ledger: {:.1} MiB", ledger.on_disk_mib());
    assert!(
        (ledger.on_disk_mib() - selection.on_disk_mib()).abs() < 40.0,
        "measured {:.1} MiB against a manifest claiming {:.1}",
        ledger.on_disk_mib(),
        selection.on_disk_mib()
    );
    assert!(runtimes.unused().expect("diffed").is_empty());
}
