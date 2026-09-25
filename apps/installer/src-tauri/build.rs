//! Embeds the app payload (a zip of the built app and its resources) when `DEMIDO_PAYLOAD`
//! points at one. Without it the installer still builds: it then provisions runtimes only,
//! which is how it runs in development.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=DEMIDO_PAYLOAD");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let payload = match std::env::var("DEMIDO_PAYLOAD") {
        Ok(path) if PathBuf::from(&path).is_file() => {
            println!("cargo:rerun-if-changed={path}");
            PathBuf::from(path)
        }
        _ => {
            let empty = out.join("empty-payload.zip");
            std::fs::write(&empty, b"").expect("write placeholder payload");
            empty
        }
    };
    println!("cargo:rustc-env=DEMIDO_PAYLOAD_PATH={}", payload.display());
    // tauri-build embeds icons/icon.ico but only reruns when tauri.conf.json changes.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build();
}
