// Keeps a console window from opening next to the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started by the app itself to read one attached file in a process of its own (see
    // `demido_extract::isolate`): read it, print what was found, exit. Nothing of the app starts.
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == demido_extract::CHILD_ARG)
        && let Some(path) = args.next()
    {
        std::process::exit(demido_extract::child_main(std::path::Path::new(&path)));
    }
    demido_studio_lib::run();
}
