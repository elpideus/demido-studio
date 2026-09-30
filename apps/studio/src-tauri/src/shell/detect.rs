//! Which shell commands run in: on Windows PowerShell 7 when it is installed (from its installer,
//! winget or the Microsoft Store), otherwise Windows PowerShell 5.1, which every Windows has; on
//! macOS and Linux the user's login shell. Found once per run of the app.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine;

#[derive(Clone, Debug)]
pub struct Shell {
    pub program: PathBuf,
    /// What it is called in the chat and the system prompt: "PowerShell 7.6.6",
    /// "Windows PowerShell 5.1", "zsh".
    pub name: String,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell,
    #[cfg_attr(windows, allow(dead_code))]
    Posix,
}

/// Longest command a PowerShell command line holds once encoded (UTF-16, then base64).
pub const MAX_COMMAND_CHARS: usize = 8_000;

/// Runs before the command: UTF-8 both ways between PowerShell and the programs it starts, and no
/// progress bars (they slow Windows PowerShell's downloads down and are gone by the end anyway).
/// One line, so an error in the command says "line 2" at most.
const PREAMBLE: &str = "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [Console]::OutputEncoding; $global:LASTEXITCODE = 0";

/// Runs after it: a failed program exits with its own code, as it would in a terminal, instead of
/// the 1 PowerShell reports for any failure.
const EPILOGUE: &str = "if (-not $?) { if ($LASTEXITCODE) { exit $LASTEXITCODE }; exit 1 }";

impl Shell {
    pub fn is_powershell(&self) -> bool {
        self.kind == Kind::PowerShell
    }

    /// The shell's arguments that run `command` and exit with its exit code.
    pub fn args(&self, command: &str) -> Vec<String> {
        match self.kind {
            // Encoded, the command reaches PowerShell exactly as written, whatever its quotes.
            Kind::PowerShell => [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
            ]
            .into_iter()
            .map(String::from)
            .chain([encode_powershell(&format!("{PREAMBLE}\n{command}\n{EPILOGUE}"))])
            .collect(),
            // A login shell reads the profile that puts Homebrew and the like on the PATH.
            Kind::Posix => vec!["-l".into(), "-c".into(), command.into()],
        }
    }
}

/// Windows PowerShell 5.1, the fallback, whatever else is installed.
#[cfg(all(test, windows))]
pub(crate) fn windows_powershell() -> Shell {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Shell {
        program: PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
        name: "Windows PowerShell 5.1".into(),
        kind: Kind::PowerShell,
    }
}

fn encode_powershell(script: &str) -> String {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

static SHELL: OnceLock<Option<Shell>> = OnceLock::new();

/// The shell, looked for on first use (on Windows that takes a second or two: PowerShell 7 is
/// started once to ask its version). Later calls return at once.
pub fn detect() -> Option<&'static Shell> {
    SHELL
        .get_or_init(|| {
            let shell = find();
            match &shell {
                Some(s) => tracing::info!("commands run in {} ({})", s.name, s.program.display()),
                None => tracing::warn!("no shell was found; the assistant cannot run commands"),
            }
            shell
        })
        .as_ref()
}

/// The shell if it was looked for already; never waits.
pub fn current() -> Option<&'static Shell> {
    SHELL.get().and_then(Option::as_ref)
}

/// Whether the shell was looked for and none was found.
pub fn missing() -> bool {
    matches!(SHELL.get(), Some(None))
}

#[cfg(windows)]
fn find() -> Option<Shell> {
    // The PATH a new terminal window gets (read from the registry), so PowerShell 7 installed
    // since the app started is found too.
    let path = portable_pty::CommandBuilder::new("pwsh")
        .get_env("PATH")
        .map(std::ffi::OsStr::to_os_string);
    for program in pwsh_candidates(path.as_deref()) {
        match powershell_version(&program) {
            Some(v) if major(&v) >= 7 => {
                return Some(Shell {
                    program,
                    name: format!("PowerShell {v}"),
                    kind: Kind::PowerShell,
                });
            }
            Some(v) => tracing::info!(
                "{} is PowerShell {v}; PowerShell 7 or later is preferred",
                program.display()
            ),
            None => tracing::warn!("{} did not tell its version", program.display()),
        }
    }
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let program = PathBuf::from(root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    if !program.is_file() {
        return None;
    }
    let name = match powershell_version(&program) {
        Some(v) => format!(
            "Windows PowerShell {}",
            v.split('.').take(2).collect::<Vec<_>>().join(".")
        ),
        None => "Windows PowerShell".into(),
    };
    Some(Shell {
        program,
        name,
        kind: Kind::PowerShell,
    })
}

#[cfg(unix)]
fn find() -> Option<Shell> {
    let program = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file())
        .or_else(|| {
            ["/bin/bash", "/bin/zsh", "/bin/sh"]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })?;
    let name = program.file_name()?.to_string_lossy().into_owned();
    Some(Shell {
        program,
        name,
        kind: Kind::Posix,
    })
}

/// Every `pwsh.exe` on `path`, then in the usual install folder, each once, in that order.
#[cfg_attr(not(windows), allow(dead_code))]
fn pwsh_candidates(path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = path
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("pwsh.exe"))
        .collect();
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(PathBuf::from(dir).join(r"PowerShell\7\pwsh.exe"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|p| {
        // The Store's PowerShell is an app execution alias: a link `is_file` cannot follow, but
        // one that starts all the same.
        seen.insert(p.to_string_lossy().to_lowercase()) && std::fs::symlink_metadata(p).is_ok()
    });
    candidates
}

/// Asks PowerShell its version ("7.6.6", "5.1.26100.4202").
#[cfg_attr(not(windows), allow(dead_code))]
fn powershell_version(program: &Path) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new(program);
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$PSVersionTable.PSVersion.ToString()",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    // A first start after boot can be slow; a PowerShell that hangs is not one to use.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let version = out.trim();
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn major(version: &str) -> u32 {
    version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> String {
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn powershell_gets_the_command_exactly_as_written() {
        let shell = Shell {
            program: "pwsh.exe".into(),
            name: "PowerShell 7.6.6".into(),
            kind: Kind::PowerShell,
        };
        let command = "yt-dlp -o \"%(title)s.%(ext)s\" 'https://x/?a=1&b=2'; echo \"done ✓\"";
        let args = shell.args(command);
        assert_eq!(args[args.len() - 2], "-EncodedCommand");
        let script = decode(args.last().unwrap());
        let lines: Vec<&str> = script.lines().collect();
        assert_eq!(lines, [PREAMBLE, command, EPILOGUE]);
    }

    #[test]
    fn posix_shells_run_the_command_as_a_login_shell() {
        let shell = Shell {
            program: "/bin/zsh".into(),
            name: "zsh".into(),
            kind: Kind::Posix,
        };
        assert_eq!(shell.args("ls -la"), ["-l", "-c", "ls -la"]);
        assert!(!shell.is_powershell());
    }

    #[test]
    fn versions_are_read_by_their_major_number() {
        assert_eq!(major("7.6.6"), 7);
        assert_eq!(major("7.6.0-preview.4"), 7);
        assert_eq!(major("5.1.26100.4202"), 5);
        assert_eq!(major("garbage"), 0);
    }

    #[test]
    fn pwsh_candidates_are_real_files_listed_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pwsh.exe"), b"").unwrap();
        let empty = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([dir.path(), empty.path(), dir.path(), Path::new("relative")]).unwrap();
        let found = pwsh_candidates(Some(&path));
        let ours: Vec<_> = found.iter().filter(|p| p.starts_with(dir.path())).collect();
        assert_eq!(ours, [&dir.path().join("pwsh.exe")]);
        assert!(
            !found
                .iter()
                .any(|p| p.starts_with(empty.path()) || p.starts_with("relative"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_always_has_a_powershell() {
        let shell = detect().expect("a PowerShell");
        assert!(shell.is_powershell());
        assert!(shell.name.contains("PowerShell"), "{}", shell.name);
        assert_eq!(current().map(|s| &s.name), Some(&shell.name));
    }
}
