fn main() {
    // tauri-build embeds icons/icon.ico but only reruns when tauri.conf.json changes.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build();
}
