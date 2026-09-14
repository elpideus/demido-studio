// The console window is a Windows console attached to a GUI process, which the
// user never asked for. Debug builds keep it, because that is where the tracing
// output goes.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Exit code 1 rather than a panic: a failure at startup is a sentence on
/// stderr, not a backtrace.
fn main() -> std::process::ExitCode {
    match demido_studio_lib::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("demido: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
