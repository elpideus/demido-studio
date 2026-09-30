//! Commands run the way a person runs them in a terminal: in their own shell (PowerShell 7 when it
//! is installed on Windows), with the environment a new terminal window gets, so every program
//! they installed is there, in a pseudo-terminal whose screen is what the model reads.

mod detect;
mod pty;
mod screen;

pub use detect::{MAX_COMMAND_CHARS, Shell, current, detect, missing};
pub use pty::{Ending, Request, run};
pub use screen::Shown;
