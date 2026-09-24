//! `run_python`: the installed Python, run in the chat's workspace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::{ToolContext, ToolOutput, arg_str, clip};

const TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CAPTURE: usize = 200_000;
const MAX_FOR_MODEL: usize = 8_000;

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "code": {"type": "string", "description": "Python source code to run"},
            "file": {"type": "string", "description": "Run this .py file instead of code: a workspace path, or skill:<skill-id>/<file> for a skill's script"},
            "args": {"type": "array", "items": {"type": "string"}, "description": "Command-line arguments for the script (sys.argv[1:])"}
        }
    })
}

pub async fn run(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let python = ctx
        .state
        .paths
        .python()
        .ok_or("Python is not installed. Run the Demido Studio installer again to add it.")?;
    std::fs::create_dir_all(&ctx.workspace).map_err(|e| e.to_string())?;
    let scratch = ctx.workspace.join(".demido");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;

    let script: PathBuf = match (arg_str(args, "code"), arg_str(args, "file")) {
        (Some(code), _) => {
            let path = scratch.join(format!("run-{}.py", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")));
            std::fs::write(&path, code).map_err(|e| e.to_string())?;
            path
        }
        (None, Some(file)) => resolve_script(ctx, file)?,
        (None, None) => return Err("Pass either code or file.".into()),
    };
    let script_args: Vec<String> = args["args"]
        .as_array()
        .map(|a| a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect())
        .unwrap_or_default();

    let before = snapshot(&ctx.workspace);
    let started = Instant::now();
    let mut cmd = Command::new(&python);
    cmd.arg("-X")
        .arg("utf8")
        .arg("-u")
        .arg(&script)
        .args(&script_args)
        .current_dir(&ctx.workspace)
        .env("MPLBACKEND", "Agg")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("DEMIDO_WORKSPACE", &ctx.workspace)
        .env("DEMIDO_SKILLS_DIR", ctx.state.skills.dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| format!("could not start Python: {e}"))?;
    crate::runtime::job::adopt(&child);
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out_task = tokio::spawn(async move { read_capped(&mut stdout).await });
    let err_task = tokio::spawn(async move { read_capped(&mut stderr).await });

    let (status, timed_out, cancelled) = tokio::select! {
        s = child.wait() => (s.ok(), false, false),
        _ = tokio::time::sleep(TIMEOUT) => { let _ = child.kill().await; (None, true, false) }
        _ = ctx.cancel.cancelled() => { let _ = child.kill().await; (None, false, true) }
    };
    let stdout = out_task.await.unwrap_or_default();
    let stderr = err_task.await.unwrap_or_default();
    let duration_ms = started.elapsed().as_millis() as u64;
    if cancelled {
        return Err("Cancelled.".into());
    }

    let files = changed_files(&ctx.workspace, &before);
    let exit_code = status.and_then(|s| s.code());
    let ok = !timed_out && exit_code == Some(0);
    let mut for_model = json!({
        "exitCode": exit_code,
        "stdout": clip(&stdout, MAX_FOR_MODEL),
        "stderr": clip(&stderr, MAX_FOR_MODEL / 2),
        "filesCreatedOrChanged": files.iter().map(|f| f["path"].clone()).collect::<Vec<_>>(),
    });
    if timed_out {
        for_model["error"] = json!(format!("The script was stopped after {} seconds.", TIMEOUT.as_secs()));
    }
    let display = json!({
        "kind": "python",
        "script": script.strip_prefix(&ctx.workspace).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| script.to_string_lossy().into_owned()),
        "code": arg_str(args, "code"),
        "exitCode": exit_code,
        "timedOut": timed_out,
        "stdout": clip(&stdout, 30_000),
        "stderr": clip(&stderr, 12_000),
        "files": files,
        "durationMs": duration_ms,
        "workspace": ctx.workspace.to_string_lossy(),
    });
    Ok(ToolOutput {
        ok,
        content: for_model.to_string(),
        display,
    })
}

fn resolve_script(ctx: &ToolContext, file: &str) -> Result<PathBuf, String> {
    let path = if let Some(rest) = file.strip_prefix("skill:") {
        let (skill, rel) = rest
            .trim_start_matches('/')
            .split_once('/')
            .ok_or("Skill scripts are written as skill:<skill-id>/<file>.")?;
        let skill = ctx
            .state
            .skills
            .find(skill)
            .ok_or_else(|| format!("There is no skill called {skill}."))?;
        ctx.state.skills.resolve_file(&skill.id, rel).map_err(|e| e.to_string())?
    } else {
        super::workspace_path(&ctx.workspace, file)?
    };
    if !path.is_file() {
        return Err(format!("{file} does not exist."));
    }
    Ok(path)
}

async fn read_capped(reader: &mut (impl AsyncReadExt + Unpin)) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if buf.len() < MAX_CAPTURE {
                    buf.extend_from_slice(&chunk[..n.min(MAX_CAPTURE - buf.len())]);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).replace("\r\n", "\n")
}

fn snapshot(dir: &Path) -> HashMap<PathBuf, (u64, SystemTime)> {
    walkdir::WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path().to_path_buf(), (m.len(), m.modified().ok()?)))
        })
        .collect()
}

fn changed_files(dir: &Path, before: &HashMap<PathBuf, (u64, SystemTime)>) -> Vec<Value> {
    let mut out: Vec<Value> = snapshot(dir)
        .into_iter()
        .filter(|(p, meta)| before.get(p) != Some(meta))
        .filter_map(|(p, (size, _))| {
            let rel = p.strip_prefix(dir).ok()?.to_string_lossy().replace('\\', "/");
            if rel.starts_with(".demido") {
                return None;
            }
            Some(json!({
                "path": rel,
                "absolute": p.to_string_lossy(),
                "size": size,
                "kind": file_kind(&p),
            }))
        })
        .collect();
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    out.truncate(40);
    out
}

pub fn file_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => "image",
        Some("csv" | "tsv") => "table",
        Some("json" | "md" | "txt" | "py" | "log" | "yaml" | "yml" | "html" | "xml") => "text",
        _ => "other",
    }
}
