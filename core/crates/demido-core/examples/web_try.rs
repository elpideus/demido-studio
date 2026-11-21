//! Tries reading a page or searching, by hand, exactly as a model's tool would.
//!
//!     cargo run --example web_try -- read https://example.com "what is this page for"
//!     cargo run --example web_try -- search "grey heron"
//!
//! It asks real sites, under Demido's own name. Nothing is kept between runs.

use std::sync::atomic::AtomicBool;

use demido_core::web::{Web, Which};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (web, stop) = (Web::new(None), AtomicBool::new(false));
    let started = std::time::Instant::now();
    let gave = match args.first().map(String::as_str) {
        Some("read") if args.len() >= 2 => web.read(&args[1], args.get(2).map_or("", String::as_str), false, &stop).await.map(|read| read.gave),
        Some("search") if args.len() >= 2 => web.search(&args[1..].join(" "), "", &Which::default(), &stop).await.map(|found| found.told()),
        _ => Err("usage: web_try read <address> [what it is about] | web_try search <words>".into()),
    };
    match gave {
        Ok(gave) => println!("{gave}\n\n[{} characters, {} ms]", gave.chars().count(), started.elapsed().as_millis()),
        Err(why) => println!("Not read: {why}. [{} ms]", started.elapsed().as_millis()),
    }
}
