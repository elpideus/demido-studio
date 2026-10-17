//! Claude Code, the official `claude` CLI, as the way to use a Claude subscription (Pro or Max).
//!
//! Anthropic lets a subscription be used only through its own apps, so Demido runs the CLI the
//! person installed rather than signing in itself. The CLI runs as a user of its own: its config
//! lives in `~/.claude/demido_studio` (`CLAUDE_CONFIG_DIR`), apart from the person's own Claude
//! Code, with no settings, skills, hooks, plugins or MCP servers, and none of its built-in tools
//! unless a provider turns one on (see [`BUILTIN_TOOLS`]).
//!
//! Each Claude subscription added is a provider with a profile of its own, one config folder
//! each, so several accounts can be signed in at once. A profile is named after its account's
//! email, the part before `@` (`demido_studio_jane_doe` for `jane.doe@example.com`); as the email
//! is known only once signed in, signing in happens in a new profile (`demido_studio__new`),
//! renamed when the subscription is added.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use crate::bail_msg;
use crate::error::{AppError, CmdResult};

/// Built-in Claude Code tools a provider may turn on, one by one, as Demido learns to show what
/// they do. All are off by default. WebFetch is left out on purpose: Demido fetches pages itself.
pub const BUILTIN_TOOLS: [&str; 8] = [
    "WebSearch",
    "Read",
    "Glob",
    "Grep",
    "Edit",
    "Write",
    "NotebookEdit",
    "Bash",
];

pub const INSTALL_DOCS: &str = "https://code.claude.com/docs/en/setup";

/// What every profile's name starts with, and the profile of a provider added before there
/// could be several (renamed after its account at the next start, see [`rename_profile`]).
pub const DEFAULT_PROFILE: &str = "demido_studio";

/// Longest part of a profile name taken from an email.
const MAX_NAME_CHARS: usize = 40;

/// The `n`th profile for signing in a new account: `demido_studio__new`, `demido_studio__new_2`...
/// The double `_` keeps them apart from the names of [`profile_for_email`], which never have one.
pub fn new_profile(n: usize) -> String {
    if n <= 1 {
        format!("{DEFAULT_PROFILE}__new")
    } else {
        format!("{DEFAULT_PROFILE}__new_{n}")
    }
}

/// The profile named after an email: the part before `@`, in lowercase, with any character but
/// letters, digits and `-` made `_` (once, however many in a row) and none at either end.
pub fn profile_for_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or_default().to_lowercase();
    let mut name = String::new();
    for c in local.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' };
        if !(c == '_' && (name.is_empty() || name.ends_with('_'))) {
            name.push(c);
        }
    }
    let name: String = name.chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end_matches('_');
    format!("{DEFAULT_PROFILE}_{}", if name.is_empty() { "account" } else { name })
}

/// Whether `profile` is named after its account (not the first profile or a new one).
pub fn named_after_account(profile: &str) -> bool {
    valid_profile(profile) && profile != DEFAULT_PROFILE && !profile.contains("__")
}

/// Whether `profile` is one Demido makes; anything else (a path, say) is refused.
pub fn valid_profile(profile: &str) -> bool {
    profile == DEFAULT_PROFILE
        || profile
            .strip_prefix(DEFAULT_PROFILE)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|rest| {
                (1..=MAX_NAME_CHARS + 8).contains(&rest.len())
                    && rest
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            })
}

/// Moves a signed-in profile's folder to `to`, and back should the sign-in not survive the move
/// (a credential store that remembers the folder). Says whether it moved.
pub async fn rename_profile(from: &str, to: &str) -> bool {
    if !valid_profile(from) || !valid_profile(to) || config_dir(to).exists() {
        return false;
    }
    if let Err(e) = std::fs::rename(config_dir(from), config_dir(to)) {
        tracing::warn!("could not rename Claude Code profile {from} to {to}: {e}");
        return false;
    }
    if status(to).await.account.logged_in {
        return true;
    }
    tracing::warn!("Claude Code profile {from} lost its sign-in when renamed to {to}; moving it back");
    let _ = std::fs::rename(config_dir(to), config_dir(from));
    false
}

/// Signs a profile out and deletes its folder, once its subscription is removed.
pub async fn forget(profile: &str) {
    if logout(profile).await.is_ok() || find_binary().is_none() {
        let _ = std::fs::remove_dir_all(config_dir(profile));
    }
}

/// `profile`, checked, for a command that names one.
pub fn checked_profile(profile: &str) -> CmdResult<&str> {
    if !valid_profile(profile) {
        bail_msg!("Unknown Claude Code profile.");
    }
    Ok(profile)
}

/// The folder Claude Code keeps a profile's sign-in and state in.
pub fn config_dir(profile: &str) -> PathBuf {
    home().unwrap_or_else(std::env::temp_dir).join(".claude").join(profile)
}

/// Where the CLI runs: an empty folder, so it finds no project files or CLAUDE.md to read.
pub fn work_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("demido-claude-code");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The `claude` executable: on the PATH, or where the official installer puts it.
pub fn find_binary() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    let home = home()?;
    [home.join(".local").join("bin"), home.join(".claude").join("local")]
        .into_iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// A `claude` command with Demido's own config and none of the person's: variables a parent
/// Claude Code session or an API key would pass down are removed, so the CLI uses the
/// subscription it signed in with.
pub fn command(binary: &Path, profile: &str) -> Command {
    let mut cmd = Command::new(binary);
    for (key, _) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        let inherited = key.starts_with("CLAUDE_CODE_")
            || key.starts_with("CLAUDE_AGENT_SDK_")
            || key.starts_with("ANTHROPIC_")
            || matches!(key, "CLAUDECODE" | "CLAUDE_PID" | "CLAUDE_EFFORT" | "CLAUDE_CONFIG_DIR");
        if inherited {
            cmd.env_remove(key);
        }
    }
    cmd.env("CLAUDE_CONFIG_DIR", config_dir(profile))
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false")
        .env("DISABLE_AUTOUPDATER", "1")
        .current_dir(work_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// How to install Claude Code on this system.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallHelp {
    pub shell: &'static str,
    pub command: &'static str,
    pub docs: &'static str,
}

pub fn install_help() -> InstallHelp {
    if cfg!(windows) {
        InstallHelp {
            shell: "PowerShell",
            command: "irm https://claude.ai/install.ps1 | iex",
            docs: INSTALL_DOCS,
        }
    } else {
        InstallHelp {
            shell: "Terminal",
            command: "curl -fsSL https://claude.ai/install.sh | bash",
            docs: INSTALL_DOCS,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub logged_in: bool,
    /// `claude.ai` for a subscription, `console` for an API key from the Console.
    pub auth_method: Option<String>,
    /// `pro`, `max`, `team`, `enterprise`...
    pub subscription_type: Option<String>,
    pub email: Option<String>,
    pub org_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub account: Account,
    /// The profile asked about, or for a new subscription the first one no provider uses.
    pub profile: String,
    pub config_dir: String,
    pub install: InstallHelp,
    pub builtin_tools: Vec<&'static str>,
}

async fn run(binary: &Path, profile: &str, args: &[&str]) -> Option<(bool, String)> {
    let mut cmd = command(binary, profile);
    cmd.args(args);
    let output = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .ok()?
        .ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

pub async fn status(profile: &str) -> Status {
    let binary = find_binary();
    let mut status = Status {
        installed: binary.is_some(),
        path: binary.as_ref().map(|p| p.display().to_string()),
        version: None,
        account: Account::default(),
        profile: profile.to_string(),
        config_dir: config_dir(profile).display().to_string(),
        install: install_help(),
        builtin_tools: BUILTIN_TOOLS.to_vec(),
    };
    let Some(binary) = binary else {
        return status;
    };
    let (version, auth) = tokio::join!(
        run(&binary, profile, &["--version"]),
        run(&binary, profile, &["auth", "status"])
    );
    status.version = version
        .filter(|(ok, _)| *ok)
        .and_then(|(_, out)| out.split_whitespace().next().map(str::to_string));
    // Signed out, the CLI exits 1 and still says so in JSON.
    if let Some(v) = auth.and_then(|(_, out)| serde_json::from_str::<Value>(out.trim()).ok()) {
        let text = |k: &str| v[k].as_str().map(str::to_string).filter(|s| !s.is_empty());
        status.account = Account {
            logged_in: v["loggedIn"].as_bool().unwrap_or(false),
            auth_method: text("authMethod"),
            subscription_type: text("subscriptionType"),
            email: text("email"),
            org_name: text("orgName"),
        };
    }
    status
}

/// Checks Claude Code is there and the profile signed in, for adding a provider.
pub async fn require_ready(profile: &str) -> CmdResult<Status> {
    let status = status(profile).await;
    if !status.installed {
        bail_msg!(
            "Claude Code is not installed. Install it with `{}`, then try again.",
            status.install.command
        );
    }
    if !status.account.logged_in {
        bail_msg!("Sign in with your Claude account first.");
    }
    Ok(status)
}

// ---------------------------------------------------------------- signing in

/// A sign-in in progress: `claude auth login`, waiting for the person to sign in in the browser
/// and, when the browser cannot hand the code back, to paste it.
struct Login {
    profile: String,
    child: Child,
    stdin: Option<ChildStdin>,
    url: Option<String>,
    output: Arc<Mutex<String>>,
}

static LOGIN: Mutex<Option<Login>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LoginState {
    Idle,
    /// Waiting for the person; `url` opens the sign-in page.
    Waiting {
        url: Option<String>,
    },
    Done {
        account: Account,
    },
    Failed {
        message: String,
    },
}

/// Starts signing in and returns the page to open. `console` signs in with a Console account
/// (API billing) instead of a subscription.
pub async fn login_start(profile: &str, console: bool) -> CmdResult<LoginState> {
    login_cancel();
    let profile = checked_profile(profile)?;
    let Some(binary) = find_binary() else {
        bail_msg!("Claude Code is not installed.");
    };
    let _ = std::fs::create_dir_all(config_dir(profile));
    let mut cmd = command(&binary, profile);
    cmd.args(["auth", "login", if console { "--console" } else { "--claudeai" }])
        .stdin(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::msg(format!("Could not start Claude Code: {e}")))?;
    let stdin = child.stdin.take();
    let output = Arc::new(Mutex::new(String::new()));
    let (url_tx, mut url_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for pipe in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let output = output.clone();
        let url_tx = url_tx.clone();
        tokio::spawn(async move {
            // The prompt for the code ends without a newline, so read by chunk, not by line.
            let mut pipe = pipe;
            let mut buf = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let mut all = output.lock();
                all.push_str(&text);
                if let Some(url) = find_url(&all) {
                    let _ = url_tx.send(url);
                }
            }
        });
    }
    drop(url_tx);
    let url = tokio::time::timeout(Duration::from_secs(20), url_rx.recv())
        .await
        .ok()
        .flatten();
    *LOGIN.lock() = Some(Login {
        profile: profile.to_string(),
        child,
        stdin,
        url: url.clone(),
        output,
    });
    Ok(login_state().await)
}

fn find_url(text: &str) -> Option<String> {
    let start = text.find("https://")?;
    let url: String = text[start..].chars().take_while(|c| !c.is_whitespace()).collect();
    // Only a whole URL: the rest of the line may still be on its way.
    text[start + url.len()..]
        .starts_with(char::is_whitespace)
        .then_some(url)
}

/// Hands the code the sign-in page showed to the waiting CLI.
pub async fn login_code(code: &str) -> CmdResult<LoginState> {
    let code = code.trim().to_string();
    if code.is_empty() {
        bail_msg!("Paste the code the sign-in page showed.");
    }
    let stdin = LOGIN.lock().as_mut().and_then(|l| l.stdin.take());
    let Some(mut stdin) = stdin else {
        bail_msg!("No sign-in is waiting for a code. Start again.");
    };
    stdin
        .write_all(format!("{code}\n").as_bytes())
        .await
        .map_err(|e| AppError::msg(format!("Could not pass the code to Claude Code: {e}")))?;
    let _ = stdin.flush().await;
    drop(stdin);
    // Give the CLI a moment to trade the code for a sign-in.
    for _ in 0..30 {
        let state = login_state().await;
        if !matches!(state, LoginState::Waiting { .. }) {
            return Ok(state);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(login_state().await)
}

/// Where the sign-in is: still waiting, done, or failed (with what the CLI said).
pub async fn login_state() -> LoginState {
    let finished = {
        let mut guard = LOGIN.lock();
        let Some(login) = guard.as_mut() else {
            return LoginState::Idle;
        };
        match login.child.try_wait() {
            Ok(None) => return LoginState::Waiting { url: login.url.clone() },
            Ok(Some(exit)) => {
                let output = login.output.lock().clone();
                let profile = login.profile.clone();
                *guard = None;
                Some((exit.success(), output, profile))
            }
            Err(e) => {
                let profile = login.profile.clone();
                *guard = None;
                Some((false, e.to_string(), profile))
            }
        }
    };
    let Some((ok, output, profile)) = finished else {
        return LoginState::Idle;
    };
    let account = status(&profile).await.account;
    if ok && account.logged_in {
        LoginState::Done { account }
    } else {
        let said = output
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.contains("https://") && !l.starts_with("Paste code"))
            .unwrap_or("Signing in did not finish.")
            .to_string();
        LoginState::Failed { message: said }
    }
}

pub fn login_cancel() {
    if let Some(mut login) = LOGIN.lock().take() {
        let _ = login.child.start_kill();
    }
}

pub async fn logout(profile: &str) -> CmdResult<Account> {
    let profile = checked_profile(profile)?;
    login_cancel();
    let Some(binary) = find_binary() else {
        bail_msg!("Claude Code is not installed.");
    };
    run(&binary, profile, &["auth", "logout"]).await;
    Ok(status(profile).await.account)
}

/// Reads a child's lines as they come, for the chat client.
pub fn lines<R: tokio::io::AsyncRead + Unpin>(reader: R) -> tokio::io::Lines<BufReader<R>> {
    BufReader::new(reader).lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_whole_urls() {
        assert_eq!(
            find_url(
                "Opening browser…\nIf the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true"
            ),
            None
        );
        assert_eq!(
            find_url("visit: https://claude.com/cai/oauth/authorize?code=true&x=1\nPaste code here if prompted > ")
                .as_deref(),
            Some("https://claude.com/cai/oauth/authorize?code=true&x=1")
        );
    }

    #[test]
    fn profiles_are_named_after_emails() {
        assert_eq!(profile_for_email("elpideus@gmail.com"), "demido_studio_elpideus");
        assert_eq!(
            profile_for_email("Jane.Doe+claude@example.com"),
            "demido_studio_jane_doe_claude"
        );
        assert_eq!(profile_for_email("..a--b..@x"), "demido_studio_a--b");
        assert_eq!(profile_for_email("ünï@x"), "demido_studio_n");
        assert_eq!(profile_for_email("@x"), "demido_studio_account");
        assert_eq!(profile_for_email(&"a".repeat(90)).len(), "demido_studio_".len() + 40);
        assert_eq!(new_profile(1), "demido_studio__new");
        assert_eq!(new_profile(2), "demido_studio__new_2");
        for name in ["x.y@z", "Jane.Doe+claude@example.com", "@x"] {
            let profile = profile_for_email(name);
            assert!(named_after_account(&profile), "{profile}");
        }
        assert!(!named_after_account(&new_profile(1)));
        assert!(!named_after_account(DEFAULT_PROFILE));
    }

    #[test]
    fn profiles_are_demido_folders_only() {
        for ok in ["demido_studio", "demido_studio__new_2", "demido_studio_jane-doe_2"] {
            assert!(valid_profile(ok), "{ok}");
        }
        for bad in [
            "",
            "demido_studio_",
            "demido_studio_X",
            "demido_studio_a.b",
            "demido_studio/../x",
            "demido_studio_a/../../x",
            "../demido_studio",
            "other",
        ] {
            assert!(!valid_profile(bad), "{bad}");
        }
    }

    #[test]
    fn web_fetch_is_never_offered() {
        assert!(BUILTIN_TOOLS.contains(&"WebSearch"));
        assert!(!BUILTIN_TOOLS.contains(&"WebFetch"));
    }
}
