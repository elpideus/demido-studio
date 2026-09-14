//! Running a program in the workspace.
//!
//! The most useful tool there is and the one with the least containment, and
//! both have to be said plainly.
//!
//! **A shell is confined where it starts and nowhere else.** `cwd` goes through
//! the workspace like every other path, and a running program can then reach
//! whatever the user can. That is why this is `Ability::Shell`, and why the
//! cautious modes ask about every call.
//!
//! **A command that ran failed by its exit status, and never by stderr.** One
//! that exits non-zero, or is killed at its deadline, is `Err`; one that exits
//! zero is `Ok` however much it wrote to stderr, because `ffmpeg` and `curl`
//! both write there on success
//! ([`docs/rules/lessons.md`](../../../../docs/rules/lessons.md)). A call
//! refused before anything ran (no command, a `cwd` outside the workspace) is
//! `Err` too, as a failed call to any tool is.
//!
//! **On Windows, the call owns its tree.** See [`crate::tree`]: nothing a
//! command started outlives the call that started it, whether the call
//! returned, ran out of time, or was dropped because a generation was stopped.
//! Elsewhere only the child is killed.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use crate::tool::{Ability, Context, Failure, Intent, Outcome, Tool};
use crate::tree::Tree;

/// How long a command may run before it is killed, when nobody says.
const DEFAULT_SECONDS: u64 = 60;

/// The longest anybody may ask for. A model that wants an hour has
/// misunderstood what it is doing, and the person is waiting either way.
const MOST_SECONDS: u64 = 600;

/// Most bytes kept from each of stdout and stderr.
///
/// Half of what `read_file` returns in one call, per stream, so that both
/// together are a ceiling on the same order. What is past it is still read,
/// because a child writing into a full pipe stops and waits for it to drain.
const MOST_OUTPUT_BYTES: usize = crate::files::MOST_BYTES / 2;

pub struct RunCommand;

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "cwd": { "type": "string" },
                "timeout_seconds": { "type": "integer" },
            },
            "required": ["command"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, _: &Context<'_>) -> Intent {
        let command = arguments["command"].as_str().unwrap_or_default();
        let summary = match arguments["cwd"].as_str() {
            Some(cwd) => format!("Run {command} in {cwd}"),
            None => format!("Run {command}"),
        };
        Intent {
            ability: Ability::Shell,
            summary,
            destructive: looks_destructive(command),
            // Nothing, and not because a command changes nothing: there is no
            // way to know what it will change, which is why the modes that ask,
            // ask.
            touches: Vec::new(),
        }
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let command = arguments["command"].as_str().unwrap_or_default().trim();
        if command.is_empty() {
            // not-a-prompt: a tool result naming what was wrong with this call.
            return Err(Failure::retryable("there is no command to run."));
        }

        // Where it starts is the one part of a command the workspace can
        // confine, so it goes through the workspace like every other path.
        let at = context.resolve_dir(arguments["cwd"].as_str().unwrap_or("."))?;

        let mut child = shell_running(command)
            .current_dir(plain(at.path()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| Failure::final_(format!("{command} could not be started: {err}")))?;

        // On the line after the spawn, before any way out of here: from now on
        // every exit, a dropped future included, takes the whole tree with it.
        let tree = Tree::around(&child);

        let seconds = arguments["timeout_seconds"]
            .as_u64()
            .unwrap_or(DEFAULT_SECONDS)
            .clamp(1, MOST_SECONDS);

        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let finished = tokio::time::timeout(Duration::from_secs(seconds), async {
            let exited = async {
                let status = child.wait().await;
                // The call is over when the child is, whatever it left behind.
                // A grandchild still holding the pipes would otherwise hold the
                // call open until it finished too.
                tree.kill();
                status
            };
            tokio::join!(exited, drain(stdout, "stdout"), drain(stderr, "stderr"))
        })
        .await;

        let Ok((status, stdout, stderr)) = finished else {
            tree.kill();
            let _ = child.start_kill();
            // The runner's own line, worded as `docs/rules/lessons.md` has it: a
            // killed command says nothing, and a failure with no text in it can
            // never become a lesson.
            return Err(Failure::retryable(format!(
                "the command did not finish within {seconds} seconds and was killed. Run something \
                 quicker, or pass a larger timeout_seconds."
            )));
        };
        let status = status
            .map_err(|err| Failure::final_(format!("{command} could not be watched: {err}")))?;

        let said = describe(command, status.code(), &stdout, &stderr);
        match status.success() {
            true => Ok(said),
            false => Err(Failure::retryable(said)),
        }
    }
}

/// What a pipe carries, up to the ceiling, read until it closes.
///
/// Everything past [`MOST_OUTPUT_BYTES`] is read and counted rather than kept:
/// a child writing into a pipe nobody reads fills it and stops, and a call that
/// stopped reading would hold that child until its deadline.
async fn drain<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>, named: &str) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };

    let mut kept = Vec::new();
    let mut dropped = 0usize;
    let mut buffer = [0u8; 8192];
    loop {
        let read = match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let room = MOST_OUTPUT_BYTES.saturating_sub(kept.len()).min(read);
        kept.extend_from_slice(&buffer[..room]);
        dropped += read - room;
    }

    let mut text = String::from_utf8_lossy(&kept).into_owned();
    if dropped > 0 {
        text.push_str(&format!(
            "\n({dropped} more bytes of {named} were not kept. Narrow the command, or send \
             its output to a file and read part of it.)"
        ));
    }
    text
}

/// Both streams and the exit code, in the order a person reads them.
fn describe(command: &str, code: Option<i32>, stdout: &str, stderr: &str) -> String {
    let mut parts = match code {
        Some(0) => vec![format!("$ {command}")],
        Some(code) => vec![format!("$ {command}\n(exit code {code})")],
        None => vec![format!("$ {command}\n(stopped without an exit code)")],
    };
    if !stdout.trim().is_empty() {
        parts.push(stdout.trim_end().to_owned());
    }
    if !stderr.trim().is_empty() {
        parts.push(format!("stderr:\n{}", stderr.trim_end()));
    }
    parts.join("\n")
}

/// Whether a command is recognisably about destroying something.
///
/// A net, not a wall. It exists so that the mode which approves everything else
/// still stops for the obvious cases. Within what it recognises it errs towards
/// yes, because a false positive costs a click and a false negative costs the
/// thing; a command it does not recognise is declared not destructive, which is
/// where it departs from `docs/rules/tools.md` (see the crate's `AGENTS.md`).
///
/// A dangerous name is matched as the *program being run*, not anywhere in the
/// line. `format` as the first word is a disk being wiped; `npm run format` is a
/// code formatter, and a check that could not tell them apart would make the
/// permissive mode ask about everything, which is the same as not having it.
/// The cost is that `xargs rm -rf` slips through, which is what a net means.
fn looks_destructive(command: &str) -> bool {
    /// Programs whose whole purpose is to remove or overwrite something.
    const RUIN: &[&str] = &[
        "rm", "rmdir", "del", "erase", "rd", "shred", "unlink", "mkfs", "fdisk", "format", "dd",
        "shutdown", "reboot", "halt", "kill", "killall", "taskkill", "truncate",
    ];

    /// Whole phrases, because the danger is sometimes the flag rather than the
    /// program: `git reset` is ordinary and `git reset --hard` is not.
    const PHRASES: &[&str] = &[
        "reset --hard",
        "clean -f",
        "push --force",
        "push -f",
        "checkout --",
        "branch -d",
        "drop table",
        "drop database",
        "reg delete",
        "npm publish",
        "cargo publish",
    ];

    let lowered = command.to_lowercase();
    if PHRASES.iter().any(|phrase| lowered.contains(phrase)) {
        return true;
    }

    // Each segment on its own, so a command chained behind `&&`, `;` or a pipe
    // is looked at as the separate command it is.
    lowered
        .split(['&', '|', ';', '\n'])
        .any(|segment| program_of(segment).is_some_and(|program| RUIN.contains(&program)))
}

/// The program a command segment runs, past any wrapper words and flags.
fn program_of(segment: &str) -> Option<&str> {
    /// Words that stand in front of the real program without being it.
    const PREFIXES: &[&str] = &[
        "sudo",
        "doas",
        "time",
        "env",
        "nohup",
        "nice",
        "cmd",
        "powershell",
        "pwsh",
    ];

    for word in segment.split_whitespace() {
        // A flag belongs to whatever came before it, so it is not the program.
        if word.starts_with('-') || word.starts_with('/') {
            continue;
        }
        // A program can be named by its path, and on Windows with its
        // extension: `C:\Windows\System32\taskkill.exe` is `taskkill`.
        let file = word.rsplit(['/', '\\']).next().unwrap_or(word);
        let name = [".exe", ".com", ".cmd", ".bat"]
            .iter()
            .find_map(|extension| file.strip_suffix(extension))
            .unwrap_or(file);
        if PREFIXES.contains(&name) {
            continue;
        }
        return Some(name);
    }
    None
}

/// A workspace path as `cmd.exe` will accept it for a working directory.
///
/// The workspace root is canonical, and on Windows canonical means the
/// `\\?\` prefix, which `cmd.exe` refuses as a current directory and replaces
/// with the Windows directory without failing.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(share) => PathBuf::from(format!(r"\\{share}")),
        None => PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)),
    }
}

/// The platform's shell, with the command line on it.
///
/// Rust quotes an argument the way a C runtime reads it back, and `cmd.exe` is
/// not a C runtime: it has never read `\"` as an escaped quote. So the line goes
/// on verbatim through `raw_arg`, `/d` keeps a machine's AutoRun scripts out of
/// it, and `/s` with the whole line in one pair of quotes is the documented way
/// to make `cmd.exe` strip exactly that pair and take the rest as typed.
fn shell_running(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let mut shell = tokio::process::Command::new("cmd");
        shell.args(["/d", "/s", "/c"]);
        shell.as_std_mut().raw_arg(format!("\"{command}\""));
        shell
    }

    #[cfg(not(windows))]
    {
        let mut shell = tokio::process::Command::new("sh");
        shell.arg("-c").arg(command);
        shell
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::workspace::Workspace;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("here.txt"), "hello").unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    #[tokio::test]
    async fn a_command_runs_in_the_workspace_and_declares_the_shell() {
        // The whole of the containment a shell has, so it had better be true.
        let (_dir, workspace) = workspace();
        let context = Context::over(&workspace);
        let listing = json!({ "command": if cfg!(windows) { "dir /b" } else { "ls" } });

        assert_eq!(
            RunCommand.intent(&listing, &context).ability,
            Ability::Shell
        );
        let text = RunCommand.run(&listing, &context).await.unwrap();
        assert!(text.contains("here.txt"), "{text}");
    }

    #[tokio::test]
    async fn a_command_can_run_in_a_directory_of_the_workspace() {
        let (dir, workspace) = workspace();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        let listing = if cfg!(windows) { "dir /b" } else { "ls" };

        let text = RunCommand
            .run(
                &json!({ "command": listing, "cwd": "src" }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(text.contains("main.rs"), "{text}");
        assert!(!text.contains("here.txt"), "{text}");
    }

    #[tokio::test]
    async fn a_directory_outside_the_workspace_is_refused_before_anything_runs() {
        let (dir, workspace) = workspace();
        let marker = if cfg!(windows) {
            "echo ran> ran.txt"
        } else {
            "echo ran > ran.txt"
        };

        let failure = RunCommand
            .run(
                &json!({ "command": marker, "cwd": ".." }),
                &Context::over(&workspace),
            )
            .await
            .expect_err("outside");

        assert!(
            failure.message.contains("outside the workspace"),
            "{failure}"
        );
        assert!(!dir.path().parent().unwrap().join("ran.txt").exists());
    }

    #[test]
    fn the_recognisably_destructive_declares_itself_destructive() {
        let (_dir, workspace) = workspace();
        let context = Context::over(&workspace);

        for command in [
            "rm -rf build",
            "git reset --hard HEAD~3",
            "git push --force origin main",
            "cargo build && rm -rf target",
            "del /q *.log",
            r"C:\Windows\System32\taskkill.exe /im node.exe",
            "npm publish",
        ] {
            let intent = RunCommand.intent(&json!({ "command": command }), &context);
            assert!(intent.destructive, "missed: {command}");
        }
    }

    #[test]
    fn ordinary_work_is_not_mistaken_for_destruction() {
        // A net that caught everything would make Autonomous ask about every
        // command, which is the same as not having it.
        let (_dir, workspace) = workspace();
        let context = Context::over(&workspace);

        for command in [
            "cargo build",
            "cargo fmt --check",
            "git status",
            "git reset HEAD~1",
            "npm run format",
            "grep -rn formatter src",
            "pnpm typecheck",
            "python -m pytest",
        ] {
            let intent = RunCommand.intent(&json!({ "command": command }), &context);
            assert!(!intent.destructive, "false alarm: {command}");
            assert!(intent.touches.is_empty(), "{command}");
        }
    }

    #[tokio::test]
    async fn an_empty_command_is_refused_rather_than_run() {
        let failure = run("   ").await.expect_err("nothing to run");
        assert!(failure.retryable);
    }

    #[tokio::test]
    async fn output_past_the_ceiling_is_cut_and_says_how_much_was_not_kept() {
        // A model that runs `cat` on a lock file should lose a sentence to it,
        // not its whole context.
        let (dir, workspace) = workspace();
        std::fs::write(
            dir.path().join("big.txt"),
            "x".repeat(3 * MOST_OUTPUT_BYTES),
        )
        .unwrap();
        let show = if cfg!(windows) {
            "type big.txt"
        } else {
            "cat big.txt"
        };

        let text = RunCommand
            .run(&json!({ "command": show }), &Context::over(&workspace))
            .await
            .unwrap();

        assert!(
            text.len() < 2 * MOST_OUTPUT_BYTES,
            "{} bytes came back",
            text.len()
        );
        assert!(
            text.contains(&format!("{} more bytes", 2 * MOST_OUTPUT_BYTES)),
            "{}",
            &text[text.len() - 200..]
        );
    }

    async fn run(command: &str) -> Outcome {
        let (_dir, workspace) = workspace();
        RunCommand
            .run(&json!({ "command": command }), &Context::over(&workspace))
            .await
    }

    #[tokio::test]
    async fn a_command_exiting_non_zero_failed_and_keeps_what_it_printed() {
        let failing = if cfg!(windows) {
            "echo went wrong 1>&2 & exit 3"
        } else {
            "echo went wrong 1>&2; exit 3"
        };
        let failure = run(failing).await.expect_err("it exited 3");

        assert!(failure.retryable, "another command could work");
        assert!(failure.message.contains("exit code 3"), "{failure}");
        assert!(failure.message.contains("went wrong"), "{failure}");
    }

    #[tokio::test]
    async fn a_command_exiting_zero_did_not_fail_however_much_it_said_on_stderr() {
        // `ffmpeg` writes its banner to stderr and `curl` its progress meter,
        // both exiting zero (`docs/rules/lessons.md`).
        let noisy = if cfg!(windows) {
            "echo a banner 1>&2 & echo done"
        } else {
            "echo a banner 1>&2; echo done"
        };
        let text = run(noisy).await.expect("it exited 0");

        assert!(text.contains("a banner"), "{text}");
        assert!(text.contains("done"), "{text}");
    }

    #[tokio::test]
    async fn a_command_past_its_deadline_is_killed_and_says_so_in_its_own_words() {
        // The line `docs/rules/lessons.md` has the runner write, because a
        // killed command says nothing and a failure with no text never teaches.
        let (_dir, workspace) = workspace();
        let forever = if cfg!(windows) {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let started = std::time::Instant::now();

        let failure = RunCommand
            .run(
                &json!({ "command": forever, "timeout_seconds": 1 }),
                &Context::over(&workspace),
            )
            .await
            .expect_err("still running at its deadline");

        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(failure.retryable);
        assert!(
            failure
                .message
                .contains("the command did not finish within 1 seconds and was killed"),
            "{failure}"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_quoted_argument_reaches_the_program_the_way_it_was_typed() {
        // v2's live run found this: Rust quotes an argument for the C runtime,
        // `cmd.exe` is not one, and `grep -r "brass astrolabe" depot/` reached
        // grep with backslashes in it and found nothing in a folder holding
        // exactly that sentence.
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("argv.cmd"), "@echo [%~1] [%~2]\r\n").unwrap();

        let text = RunCommand
            .run(
                &json!({ "command": r#".\argv.cmd "one two" "three""# }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(text.contains("[one two] [three]"), "{text}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_program_whose_name_has_a_space_in_it_runs_with_quoted_arguments() {
        // The case `/s` exists for. Without it `cmd.exe` strips the first and
        // last quote on the line, which here are two different pairs.
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("my tool.cmd"), "@echo ran with [%~1]\r\n").unwrap();

        let text = RunCommand
            .run(
                &json!({ "command": r#"".\my tool.cmd" "one two""# }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(text.contains("ran with [one two]"), "{text}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_bare_name_finds_its_extension_the_way_a_terminal_would() {
        // `npm` is `npm.cmd`. `CreateProcess` does not read PATHEXT, which is
        // why v2 resolved it by hand; going through `cmd.exe` means the shell
        // reads it, in its own order, which is this machine's PATHEXT.
        //
        // Named as `.\greet` rather than `greet`. Whether `cmd.exe` looks in
        // the current directory for a bare name at all is the user's
        // `NoDefaultCurrentDirectoryInExePath`, which the session that wrote
        // this had set, and PATHEXT applies to either spelling.
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("greet.cmd"), "@echo greeted by cmd\r\n").unwrap();
        std::fs::write(dir.path().join("twin.bat"), "@echo the bat\r\n").unwrap();
        std::fs::write(dir.path().join("twin.cmd"), "@echo the cmd\r\n").unwrap();
        let context = Context::over(&workspace);

        let greeted = RunCommand
            .run(&json!({ "command": r".\greet" }), &context)
            .await
            .unwrap();
        assert!(greeted.contains("greeted by cmd"), "{greeted}");

        let pathext = std::env::var("PATHEXT").unwrap_or_default().to_uppercase();
        let first = match (pathext.find(".BAT"), pathext.find(".CMD")) {
            (Some(bat), Some(cmd)) if cmd < bat => "the cmd",
            _ => "the bat",
        };
        let twin = RunCommand
            .run(&json!({ "command": r".\twin" }), &context)
            .await
            .unwrap();
        assert!(twin.contains(first), "PATHEXT is {pathext}: {twin}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_bare_name_on_path_is_found_with_its_extension() {
        // The case v2's `program.rs` existed for: a program on PATH named
        // without its extension. `where` is `where.exe`, on every Windows.
        let text = run("where cmd").await.expect("where found cmd");

        assert!(text.to_lowercase().contains("cmd.exe"), "{text}");
    }

    /// A workspace holding `later.cmd`, which says it started, waits about three
    /// seconds and then leaves `escaped.txt` behind. Started with `start /b`, it
    /// is a grandchild of the call: `cmd.exe` is the child, and killing the
    /// child alone leaves it running.
    #[cfg(windows)]
    fn a_grandchild_in_waiting() -> (tempfile::TempDir, Workspace) {
        let (dir, workspace) = workspace();
        std::fs::write(
            dir.path().join("later.cmd"),
            "@echo started> started.txt\r\n@ping -n 4 127.0.0.1 >nul\r\n@echo escaped> escaped.txt\r\n",
        )
        .unwrap();
        (dir, workspace)
    }

    /// Long enough for the grandchild to have written its marker, had it lived.
    #[cfg(windows)]
    async fn outlast_the_grandchild() {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_a_call_kills_the_grandchild_it_started() {
        // What stopping a generation mid call does to a tool: the future is
        // dropped. Nothing the call started may be running afterwards.
        let (dir, workspace) = a_grandchild_in_waiting();
        let call = tokio::spawn(async move {
            let arguments =
                json!({ "command": r#"start "" /b later.cmd & ping -n 30 127.0.0.1 >nul"# });
            RunCommand.run(&arguments, &Context::over(&workspace)).await
        });

        for _ in 0..50 {
            if dir.path().join("started.txt").exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            dir.path().join("started.txt").exists(),
            "nothing was started, so nothing is being proved"
        );

        call.abort();
        let _ = call.await;
        outlast_the_grandchild().await;

        assert!(
            !dir.path().join("escaped.txt").exists(),
            "the grandchild outlived the call that started it"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_call_past_its_deadline_kills_the_grandchild_it_started() {
        let (dir, workspace) = a_grandchild_in_waiting();

        let failure = RunCommand
            .run(
                &json!({
                    "command": r#"start "" /b later.cmd & ping -n 30 127.0.0.1 >nul"#,
                    "timeout_seconds": 1,
                }),
                &Context::over(&workspace),
            )
            .await
            .expect_err("still running at its deadline");
        assert!(
            dir.path().join("started.txt").exists(),
            "nothing was started, so nothing is being proved: {failure}"
        );
        outlast_the_grandchild().await;

        assert!(
            !dir.path().join("escaped.txt").exists(),
            "the grandchild outlived the call that started it"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_call_that_returns_leaves_nothing_it_started_running() {
        // `cmd.exe` exits at once and the grandchild still holds both pipes,
        // so a call that waited for the pipes to close would wait for it.
        let (dir, workspace) = a_grandchild_in_waiting();

        let text = RunCommand
            .run(
                &json!({ "command": r#"start "" /b later.cmd"# }),
                &Context::over(&workspace),
            )
            .await
            .expect("start exits zero");
        outlast_the_grandchild().await;

        assert!(
            !dir.path().join("escaped.txt").exists(),
            "the grandchild outlived the call that started it: {text}"
        );
    }
}
