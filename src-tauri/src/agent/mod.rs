pub mod executor;
pub mod permissions;

use crate::providers::ToolDef;
use serde_json::json;

pub fn builtin_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file".into(),
            description: "Read the full contents of a file at the given path.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file (absolute or relative to working directory)" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "write_file".into(),
            description: "Create or overwrite a file with the given content. Creates parent directories if needed.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "edit_file".into(),
            description: "Find the first occurrence of old_str in a file and replace it with new_str. Returns an error if old_str is not found.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "old_str": { "type": "string", "description": "Exact string to find (must be unique enough to identify the location)" },
                    "new_str": { "type": "string", "description": "Replacement string" }
                },
                "required": ["path", "old_str", "new_str"]
            }),
        },
        ToolDef {
            name: "list_dir".into(),
            description: "List the contents of a directory, showing names, types (file/dir), and file sizes.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path (absolute or relative to working directory)" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "run_command".into(),
            description: "Run a PowerShell command and return its stdout and stderr. Output is capped at 10KB.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "PowerShell command string to execute" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional extra arguments appended to the command"
                    },
                    "cwd": { "type": "string", "description": "Working directory for the command (defaults to conversation working directory)" }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "search_files".into(),
            description: "Search for a regex pattern across files in a directory tree. Returns matching lines with file path and line number.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Root directory to search (defaults to working directory)" },
                    "glob": { "type": "string", "description": "Glob pattern to filter files, e.g. *.rs or *.ts (default: *)" }
                },
                "required": ["pattern"]
            }),
        },
    ]
}

pub fn is_builtin(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file" | "write_file" | "edit_file" | "list_dir" | "run_command" | "search_files"
    )
}
