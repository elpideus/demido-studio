//! Running helper programs (uv, llama-server, node) with their output streamed line by line.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub(crate) struct Run<'a> {
    pub program: &'a Path,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
    pub cwd: Option<&'a Path>,
    pub timeout: Duration,
}

/// Runs a program to completion, calling `on_line` for every line it prints (stdout and stderr).
/// Returns everything it printed. Fails on a non-zero exit, a timeout or cancellation.
pub(crate) async fn run(
    spec: Run<'_>,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(&str),
) -> anyhow::Result<String> {
    let mut cmd = Command::new(spec.program);
    cmd.args(&spec.args)
        .envs(spec.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = spec.cwd {
        cmd.current_dir(cwd);
    }
    hide_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("starting {}", spec.program.display()))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for reader in [
        Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box::new(stderr),
    ] {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let mut output = String::new();
    let deadline = tokio::time::sleep(spec.timeout);
    tokio::pin!(deadline);
    let status = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                bail!("cancelled");
            }
            _ = &mut deadline => {
                let _ = child.kill().await;
                bail!("{} did not finish within {:?}", spec.program.display(), spec.timeout);
            }
            line = rx.recv() => match line {
                Some(line) => {
                    let clean = strip_ansi(&line);
                    if !clean.trim().is_empty() {
                        on_line(&clean);
                        output.push_str(&clean);
                        output.push('\n');
                    }
                }
                None => break child.wait().await?,
            },
        }
    };
    if !status.success() {
        let tail: Vec<&str> = output.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        bail!(
            "{} exited with {status}:\n{}",
            spec.program.display(),
            tail.join("\n")
        );
    }
    Ok(output)
}

/// Keeps console programs from flashing a window when started by a GUI process.
pub(crate) fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Removes ANSI color and cursor sequences (uv prints them even when piped on some terminals).
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_color_codes() {
        assert_eq!(super::strip_ansi("\u{1b}[32mok\u{1b}[0m\r"), "ok");
    }
}
