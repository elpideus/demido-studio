//! The installer's command line for an update. The app writes it and the installer reads it, so
//! both sides agree on it here.
//!
//! ```text
//! demido-setup --update --dir <install folder> [--wait-pid <pid>] [--relaunch]
//! ```
//!
//! `--update` installs the app carried by this setup over the installation in `--dir`, keeping
//! every choice made when it was installed, without the wizard. `--wait-pid` is the app that
//! started the update: setup waits for it to exit before it touches a file. `--relaunch` starts
//! the app again once the update is in place.

use std::path::PathBuf;

pub const UPDATE: &str = "--update";
pub const DIR: &str = "--dir";
pub const WAIT_PID: &str = "--wait-pid";
pub const RELAUNCH: &str = "--relaunch";

/// Passed to the *app*, not to setup: do not install a staged update at this launch. Setup opens
/// the app with it after an update that failed, so "Open Demido Studio" opens the app instead of
/// starting the same update again.
pub const SKIP_UPDATE: &str = "--skip-update";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateArgs {
    /// The installation to update. Without it, setup updates the installation it finds.
    pub dir: Option<PathBuf>,
    /// A process to wait for before touching any file: the app that started the update.
    pub wait_pid: Option<u32>,
    /// Start the app again once the update is installed.
    pub relaunch: bool,
}

impl UpdateArgs {
    /// The update arguments in `args` (the program name may be first), or `None` when `--update`
    /// is not among them. A flag without its value, or with a value that does not parse, is left
    /// unset rather than failing: setup then asks or looks for the installation itself.
    pub fn parse(args: &[String]) -> Option<Self> {
        if !args.iter().any(|a| a == UPDATE) {
            return None;
        }
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .filter(|v| !v.starts_with("--"))
        };
        Some(Self {
            dir: value(DIR)
                .map(|d| d.trim().trim_matches('"'))
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
            wait_pid: value(WAIT_PID).and_then(|p| p.parse().ok()),
            relaunch: args.iter().any(|a| a == RELAUNCH),
        })
    }

    /// The arguments, one per item, for `std::process::Command::args`.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![UPDATE.to_string()];
        if let Some(dir) = &self.dir {
            args.push(DIR.to_string());
            args.push(dir.to_string_lossy().into_owned());
        }
        if let Some(pid) = self.wait_pid {
            args.push(WAIT_PID.to_string());
            args.push(pid.to_string());
        }
        if self.relaunch {
            args.push(RELAUNCH.to_string());
        }
        args
    }

    /// The arguments as one command line, quoted the way Windows splits it again (for
    /// `ShellExecute`, which takes a single string, as when setup restarts itself elevated).
    pub fn to_command_line(&self) -> String {
        self.to_args()
            .iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Quotes `arg` for a Windows command line, so `CommandLineToArgvW` (and the Rust runtime) read
/// it back unchanged: quotes when it is empty or has spaces, tabs or quotes, with the backslashes
/// before a quote, and a trailing run of them, doubled.
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn round_trips_through_arguments() {
        let args = UpdateArgs {
            dir: Some(PathBuf::from(r"C:\Users\O'Neil\AppData\Local\Programs\Demido Studio")),
            wait_pid: Some(4242),
            relaunch: true,
        };
        let mut argv = vec!["demido-setup.exe".to_string()];
        argv.extend(args.to_args());
        assert_eq!(UpdateArgs::parse(&argv), Some(args));
    }

    #[test]
    fn without_update_there_is_no_update() {
        assert_eq!(UpdateArgs::parse(&strings(&["setup.exe", "--dir", "C:\\x"])), None);
        assert_eq!(UpdateArgs::parse(&strings(&["setup.exe", "--uninstall"])), None);
    }

    #[test]
    fn missing_or_broken_values_stay_unset() {
        let parsed = UpdateArgs::parse(&strings(&["setup.exe", "--update", "--dir", "--wait-pid", "abc"])).unwrap();
        assert_eq!(parsed, UpdateArgs::default());
        let parsed = UpdateArgs::parse(&strings(&["setup.exe", "--update", "--relaunch", "--dir"])).unwrap();
        assert_eq!(
            parsed,
            UpdateArgs {
                relaunch: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn quotes_like_windows_expects() {
        assert_eq!(quote_arg("--update"), "--update");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(
            quote_arg(r"C:\Program Files\Demido Studio"),
            r#""C:\Program Files\Demido Studio""#
        );
        // A trailing backslash would otherwise escape the closing quote.
        assert_eq!(quote_arg(r"C:\My Apps\"), r#""C:\My Apps\\""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r#"a\"b c"#), r#""a\\\"b c""#);
    }

    #[test]
    fn the_command_line_quotes_only_what_needs_it() {
        let args = UpdateArgs {
            dir: Some(PathBuf::from(r"C:\Program Files\Demido Studio")),
            wait_pid: Some(7),
            relaunch: true,
        };
        assert_eq!(
            args.to_command_line(),
            r#"--update --dir "C:\Program Files\Demido Studio" --wait-pid 7 --relaunch"#
        );
    }

    /// The command line splits back into the same arguments, by the rules Windows splits it with.
    #[test]
    fn the_command_line_splits_back_into_the_arguments() {
        for dir in [
            r"C:\Users\O'Neil\My Apps\Demido Studio\",
            r"D:\Demido",
            r#"C:\odd "quoted" \\ name"#,
        ] {
            let args = UpdateArgs {
                dir: Some(PathBuf::from(dir)),
                wait_pid: Some(12),
                relaunch: false,
            };
            assert_eq!(split_like_windows(&args.to_command_line()), args.to_args(), "{dir}");
        }
    }

    /// `CommandLineToArgvW`'s rules for everything after the program name.
    fn split_like_windows(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut has_arg = false;
        let mut backslashes = 0usize;
        for c in line.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        current.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                    backslashes = 0;
                    has_arg = true;
                }
                ' ' | '\t' if !in_quotes => {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                    if has_arg || !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                    has_arg = false;
                }
                _ => {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                    current.push(c);
                    has_arg = true;
                }
            }
        }
        current.extend(std::iter::repeat_n('\\', backslashes));
        if has_arg || !current.is_empty() {
            out.push(current);
        }
        out
    }
}
