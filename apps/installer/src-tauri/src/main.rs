// Keeps a console window from opening next to the installer in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    demido_setup_lib::run();
}
