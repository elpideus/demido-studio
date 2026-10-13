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
    // Started as `<AppImage> --setup …` (see `setup`): what follows is for the setup that travels
    // inside, which takes this process's place, so the AppImage stays mounted while it runs.
    #[cfg(unix)]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == demido_core::setup_args::SETUP)
    {
        use std::os::unix::process::CommandExt;
        let setup = std::env::current_exe().ok().and_then(|exe| {
            Some(
                exe.parent()?
                    .join(demido_core::platform::exe(demido_core::brand::SETUP_BIN)),
            )
        });
        let error = match setup {
            Some(setup) => std::process::Command::new(setup)
                .args(std::env::args_os().skip(2))
                .exec(),
            None => std::io::Error::other("this executable's folder is unknown"),
        };
        eprintln!("could not start setup: {error}");
        std::process::exit(1);
    }
    demido_core::platform::prepare_webview();
    demido_studio_lib::run();
}
