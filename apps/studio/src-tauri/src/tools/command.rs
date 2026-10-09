//! `run_command`: a command line in the user's own shell (see [`crate::shell`]), so the assistant
//! can use the programs installed on the computer: yt-dlp, ffmpeg, git, ping, winget and so on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::changes::{changed_files, snapshot};
use super::{ToolContext, ToolOutput, arg_str, clip, require_str};
use crate::shell::{self, Ending, Request, Shown};

const DEFAULT_TIMEOUT: u64 = 120;
const MAX_TIMEOUT: u64 = 3_600;
const MAX_FOR_MODEL: usize = 10_000;
const MAX_DISPLAY: usize = 40_000;

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "In the shell's own syntax"},
            "directory": {"type": "string", "description": "Absolute path, ~ for the home folder, or a workspace folder"},
            "timeout": {"type": "integer", "description": "Seconds before it is stopped (default 120, max 3600): longer for downloads and other long jobs; 3 to 5 for full-screen programs such as btop, whose screen is then returned"}
        },
        "required": ["command"]
    })
}

pub async fn run(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let command = require_str(args, "command")?;
    let length = command.chars().count();
    if length > shell::MAX_COMMAND_CHARS {
        return Err(format!(
            "The command is {length} characters long; commands can be at most {}. Save it as a script with write_file and run the script.",
            shell::MAX_COMMAND_CHARS
        ));
    }
    let shell = tokio::task::spawn_blocking(shell::detect)
        .await
        .ok()
        .flatten()
        .ok_or("No shell was found on this computer, so commands cannot run.")?;
    let dir = directory(&ctx.workspace, arg_str(args, "directory"), home().as_deref())?;
    let timeout = args
        .get("timeout")
        .and_then(Value::as_f64)
        .map(|s| s.round().max(1.0) as u64)
        .unwrap_or(DEFAULT_TIMEOUT)
        .min(MAX_TIMEOUT);
    // Only the workspace is watched for new files: walking a home folder would take long.
    let before = (dir == ctx.workspace).then(|| snapshot(&ctx.workspace));

    let base = json!({
        "kind": "command",
        "command": command,
        "shell": shell.name,
        "powershell": shell.is_powershell(),
        "directory": dir.to_string_lossy(),
    });
    let live = |shown: &Shown| {
        let mut display = base.clone();
        display["output"] = json!(clip(&shown.text, MAX_DISPLAY));
        display["fullScreen"] = json!(shown.full_screen);
        display["running"] = json!(true);
        display
    };
    ctx.set_display(live(&Shown::default()));
    let request = Request {
        shell,
        command,
        dir: &dir,
        env: vec![("DEMIDO_WORKSPACE", ctx.workspace.clone().into_os_string())],
        timeout: Duration::from_secs(timeout),
    };
    let Some(outcome) = shell::run(request, &ctx.stop, &ctx.cancel, |shown| ctx.set_display(live(shown))).await? else {
        return Err("Cancelled.".into());
    };

    let files = before
        .map(|before| changed_files(&ctx.workspace, &before))
        .unwrap_or_default();
    let (exit_code, ending, note) = match outcome.ending {
        Ending::Exited(code) => (Some(code), "exited", None),
        Ending::TimedOut => (
            None,
            "timedOut",
            Some(format!(
                "The command was still running after {timeout} s and was stopped. The output is what it showed until then."
            )),
        ),
        Ending::Stopped => (
            None,
            "stopped",
            Some(format!(
                "The user stopped the command after {} s. The output is what it showed until then.",
                outcome.elapsed.as_secs()
            )),
        ),
    };
    let mut for_model = json!({
        "exitCode": exit_code,
        "output": clip(&outcome.shown.text, MAX_FOR_MODEL),
    });
    if let Some(note) = note {
        for_model["note"] = json!(note);
    }
    if !files.is_empty() {
        for_model["filesCreatedOrChanged"] = files.iter().map(|f| f["path"].clone()).collect();
    }
    let mut display = base;
    display["output"] = json!(clip(&outcome.shown.text, MAX_DISPLAY));
    display["fullScreen"] = json!(outcome.shown.full_screen);
    display["running"] = json!(false);
    display["ending"] = json!(ending);
    display["exitCode"] = json!(exit_code);
    display["timeout"] = json!(timeout);
    display["files"] = json!(files);
    display["durationMs"] = json!(outcome.elapsed.as_millis() as u64);
    Ok(ToolOutput {
        // A command stopped on purpose (a full-screen program's timeout, the Stop button) did
        // what was asked; only a non-zero exit code is a failure.
        ok: !matches!(outcome.ending, Ending::Exited(code) if code != 0),
        content: for_model.to_string(),
        display,
    })
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The folder a command runs in: the workspace (made if needed), an absolute folder, the home
/// folder (`~`, `~/Downloads`) or a folder in the workspace.
fn directory(workspace: &Path, dir: Option<&str>, home: Option<&Path>) -> Result<PathBuf, String> {
    let Some(dir) = dir else {
        std::fs::create_dir_all(workspace).map_err(|e| e.to_string())?;
        return Ok(workspace.to_path_buf());
    };
    let path = if let Some(rest) = dir
        .strip_prefix('~')
        .filter(|r| r.is_empty() || r.starts_with(['/', '\\']))
    {
        let home = home.ok_or("The home folder is not known; use an absolute path.")?;
        home.join(rest.trim_start_matches(['/', '\\']))
    } else if Path::new(dir).is_absolute() {
        PathBuf::from(dir)
    } else {
        super::workspace_path(workspace, dir)?
    };
    if !path.is_dir() {
        return Err(format!("The folder {} does not exist.", path.display()));
    }
    Ok(path)
}

/// The command on one line, cut to `max` characters, for the card's title.
pub fn one_line(command: &str, max: usize) -> String {
    let flat = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_run_in_the_workspace_unless_told_otherwise() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws");
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("Downloads")).unwrap();

        assert_eq!(directory(&workspace, None, None).unwrap(), workspace);
        assert!(workspace.is_dir(), "the workspace is made on first use");
        std::fs::create_dir(workspace.join("videos")).unwrap();
        assert_eq!(
            directory(&workspace, Some("videos"), None).unwrap(),
            workspace.join("videos")
        );
        assert_eq!(directory(&workspace, Some("~"), Some(&home)).unwrap(), home);
        assert_eq!(
            directory(&workspace, Some("~/Downloads"), Some(&home)).unwrap(),
            home.join("Downloads")
        );
        let absolute = home.join("Downloads");
        assert_eq!(
            directory(&workspace, Some(absolute.to_str().unwrap()), None).unwrap(),
            absolute
        );
        assert!(directory(&workspace, Some("missing"), None).is_err());
        assert!(directory(&workspace, Some("../home"), None).is_err());
        assert!(directory(&workspace, Some("~"), None).is_err());
        // "~name" is a folder called that, not someone's home.
        assert!(directory(&workspace, Some("~other"), Some(&home)).is_err());
    }

    #[test]
    fn titles_show_the_command_on_one_line() {
        assert_eq!(one_line("ping -n 4\n  google.com", 40), "ping -n 4 google.com");
        assert_eq!(
            one_line("yt-dlp https://example.com/a/very/long/url", 20),
            "yt-dlp https://exam…"
        );
    }
}
