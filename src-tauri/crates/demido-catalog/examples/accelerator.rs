//! What this machine is, what it was pre-selected for, and what that would
//! fetch.
//!
//! ```text
//! cargo run --manifest-path src-tauri/Cargo.toml -p demido-catalog --example accelerator
//! ```
//!
//! Run it when somebody reports the wizard offering the wrong accelerator. It
//! prints detection and the choice made from it separately, which is what
//! separates "the machine was read wrong" from "the row chosen from it is
//! wrong". Ask for its output first.

use demido_catalog::{Availability, Selector};
use demido_hardware::Machine;

fn main() {
    let machine = Machine::detect();

    println!("os        {} {}", machine.os, machine.arch);
    println!("cuda      {:?}", machine.cuda);
    for gpu in &machine.gpus {
        println!(
            "adapter   {} [{}] {:?}, {:.1} GiB dedicated, {:.1} GiB shared",
            gpu.index,
            gpu.name,
            gpu.vendor,
            gpu.dedicated_memory as f64 / 1073741824.0,
            gpu.shared_memory as f64 / 1073741824.0,
        );
    }
    for note in &machine.notes {
        println!("note      {note:?}");
    }

    let selector = Selector::for_machine(&machine);
    println!("\npreselected {:?}", selector.preselection.ecosystem);
    println!("because     {:?}", selector.preselection.reason);
    println!("chosen row  {:?}\n", selector.chosen);

    for row in &selector.rows {
        let mark = if row.ecosystem == selector.chosen {
            ">"
        } else {
            " "
        };
        match &row.availability {
            Availability::Offered {
                download_mib,
                on_disk_mib,
            } => println!(
                "{mark} {:?}: {download_mib:.1} MiB down, {on_disk_mib:.1} MiB on disk",
                row.ecosystem
            ),
            other => println!("{mark} {:?}: {other:?}", row.ecosystem),
        }
    }

    match selector.selection() {
        Some(selection) => {
            println!();
            for archive in selection.archives() {
                println!("fetches   {}", archive.url());
            }
        }
        // Nothing is pinned for this platform, which is the honest answer on a
        // machine this build has no archives for rather than a failure.
        None => println!("\nfetches   nothing: no archive is pinned for this platform"),
    }
}
