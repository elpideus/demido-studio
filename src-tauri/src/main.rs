// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `skill` / `mcp` subcommands run headless and exit; anything else launches the app.
    if demido_studio_lib::cli::try_run() {
        return;
    }
    demido_studio_lib::run()
}
