//! Workspace file tools.

use serde_json::{Value, json};

use super::changes::file_kind;
use super::{ToolContext, ToolOutput, arg_str, clip, require_str, workspace_path};

const MAX_READ: usize = 60_000;

pub fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Folder inside the workspace (default: the workspace itself)"}
        }
    })
}

pub fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path inside the workspace, e.g. data/FX_EURUSD_1h.csv"},
            "max_lines": {"type": "integer", "description": "Read at most this many lines (default 200)"}
        },
        "required": ["path"]
    })
}

pub fn write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path inside the workspace"},
            "content": {"type": "string", "description": "Full text content of the file"}
        },
        "required": ["path", "content"]
    })
}

pub fn list(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let dir = workspace_path(&ctx.workspace, arg_str(args, "path").unwrap_or(""))?;
    if !dir.is_dir() {
        return Ok(ToolOutput::ok(
            json!({"files": [], "note": "The workspace is empty."}).to_string(),
            json!({"kind": "files", "files": []}),
        ));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&dir)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let Ok(rel) = entry.path().strip_prefix(&ctx.workspace) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.starts_with(".demido") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        files.push(json!({"path": rel, "size": size, "kind": file_kind(entry.path())}));
        if files.len() >= 200 {
            break;
        }
    }
    Ok(ToolOutput::ok(
        json!({"files": files}).to_string(),
        json!({"kind": "files", "files": files}),
    ))
}

pub fn read(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let rel = require_str(args, "path")?;
    let path = workspace_path(&ctx.workspace, rel)?;
    let bytes = std::fs::read(&path).map_err(|_| format!("{rel} does not exist in the workspace."))?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        return Err(format!("{rel} is a binary file and cannot be read as text."));
    }
    let text = String::from_utf8_lossy(&bytes);
    let max_lines = args["max_lines"].as_u64().unwrap_or(200).clamp(1, 5000) as usize;
    let total_lines = text.lines().count();
    let shown: String = text.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    let mut content = clip(&shown, MAX_READ);
    if total_lines > max_lines {
        content.push_str(&format!(
            "\n… {} more lines (file has {total_lines} lines).",
            total_lines - max_lines
        ));
    }
    Ok(ToolOutput::ok(
        content,
        json!({"kind": "file", "path": rel, "lines": total_lines, "size": bytes.len()}),
    ))
}

pub fn write(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let rel = require_str(args, "path")?;
    let content = args["content"]
        .as_str()
        .ok_or("The \"content\" argument is required.")?;
    let path = workspace_path(&ctx.workspace, rel)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| format!("could not write {rel}: {e}"))?;
    Ok(ToolOutput::ok(
        json!({"written": rel, "bytes": content.len()}).to_string(),
        json!({"kind": "file", "path": rel, "absolute": path.to_string_lossy(), "size": content.len(), "written": true}),
    ))
}
