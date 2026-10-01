//! Prints what a model gets from a file: its kind, pages, note, passages and the start of its
//! text.
//!
//!     cargo run -p demido-extract --release --example read -- <file> [characters to show]

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: read <file> [characters to show]");
        std::process::exit(2);
    };
    let show: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(1500);
    let started = Instant::now();
    let read = match demido_extract::extract(std::path::Path::new(&path)) {
        Ok(read) => read,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let took = started.elapsed();
    println!("kind: {:?}, mime: {}", read.kind, read.mime);
    println!("pages: {:?}, page markers: {}", read.pages, read.page_starts.len());
    println!("dimensions: {:?}", read.dimensions);
    println!("note: {:?}", read.note);
    println!("read in {took:.2?}");
    if let Some(text) = &read.text {
        let passages = demido_extract::chunks(text, &read.page_starts, 2000, 200);
        println!("characters: {}, passages: {}", text.chars().count(), passages.len());
        println!("---");
        println!("{}", text.chars().take(show).collect::<String>());
    }
}
