//! Runs one command in a folder through the sandbox and prints what came of it. For trying by hand:
//! `cargo run --example sandbox_try -- <folder> <command>`
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (folder, script) = (args.next().expect("a folder"), args.next().expect("a command"));
    let sandbox = demido_core::sandbox::Sandbox::find();
    eprintln!("sandbox: {} {}", sandbox.kind, sandbox.why_not.clone().unwrap_or_default());
    let ran = sandbox.run(std::path::Path::new(&folder), &script, Duration::from_secs(120), &AtomicBool::new(false)).await.expect("ran");
    println!("{}", ran.told());
    eprintln!("({} ms)", ran.took_ms);
}
