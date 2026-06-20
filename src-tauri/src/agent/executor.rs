use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn resolve_path(path_str: &str, working_dir: Option<&str>) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir
            .map(|d| Path::new(d).join(path_str))
            .unwrap_or_else(|| p.to_path_buf())
    }
}

pub fn execute_tool(name: &str, args: &Value, working_dir: Option<&str>) -> String {
    match name {
        "read_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => format!("Error reading {}: {}", path.display(), e),
            }
        }
        "write_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            let content = args["content"].as_str().unwrap_or("");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, content) {
                Ok(_) => format!("Written {} bytes to {}", content.len(), path.display()),
                Err(e) => format!("Error writing {}: {}", path.display(), e),
            }
        }
        "edit_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            let old_str = args["old_str"].as_str().unwrap_or("");
            let new_str = args["new_str"].as_str().unwrap_or("");
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if !content.contains(old_str) {
                        return format!("Error: old_str not found in {}", path.display());
                    }
                    let new_content = content.replacen(old_str, new_str, 1);
                    match std::fs::write(&path, &new_content) {
                        Ok(_) => "Edit applied successfully".into(),
                        Err(e) => format!("Error writing {}: {}", path.display(), e),
                    }
                }
                Err(e) => format!("Error reading {}: {}", path.display(), e),
            }
        }
        "list_dir" => {
            let path_str = args["path"].as_str().unwrap_or(".");
            let path = resolve_path(path_str, working_dir);
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut lines: Vec<String> = entries
                        .flatten()
                        .map(|e| {
                            let meta = e.metadata().ok();
                            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                            let name = e.file_name().to_string_lossy().into_owned();
                            if is_dir {
                                format!("{}/", name)
                            } else {
                                format!("{}  ({} bytes)", name, size)
                            }
                        })
                        .collect();
                    lines.sort();
                    if lines.is_empty() {
                        "(empty directory)".into()
                    } else {
                        lines.join("\n")
                    }
                }
                Err(e) => format!("Error listing {}: {}", path.display(), e),
            }
        }
        "run_command" => {
            let command = args["command"].as_str().unwrap_or("");
            let extra_args: Vec<String> = args["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let cwd = args["cwd"]
                .as_str()
                .map(|p| resolve_path(p, working_dir))
                .or_else(|| working_dir.map(PathBuf::from));

            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args(["-NonInteractive", "-Command", command]);
            for a in &extra_args {
                cmd.arg(a);
            }
            if let Some(dir) = &cwd {
                cmd.current_dir(dir);
            }

            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                    let mut result = stdout;
                    if !stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("STDERR:\n");
                        result.push_str(&stderr);
                    }
                    const MAX: usize = 10240;
                    if result.len() > MAX {
                        result.truncate(MAX);
                        result.push_str("\n[output truncated]");
                    }
                    if result.is_empty() {
                        result = format!("[exit code: {}]", output.status.code().unwrap_or(-1));
                    }
                    result
                }
                Err(e) => format!("Error running command: {}", e),
            }
        }
        "search_files" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let search_root = args["path"]
                .as_str()
                .map(|p| resolve_path(p, working_dir))
                .or_else(|| working_dir.map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            let glob = args["glob"].as_str().unwrap_or("*");

            let re = match Regex::new(pattern) {
                Ok(r) => r,
                Err(e) => return format!("Invalid regex pattern: {}", e),
            };

            let mut matches: Vec<String> = Vec::new();
            const MAX_MATCHES: usize = 200;

            'outer: for entry in WalkDir::new(&search_root)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy();
                if !glob_match(glob, &file_name) {
                    continue;
                }
                // Skip files larger than 1 MB to avoid OOM on large repos
                if entry.metadata().map(|m| m.len()).unwrap_or(0) > 1_048_576 {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                for (line_no, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        let rel = entry
                            .path()
                            .strip_prefix(&search_root)
                            .unwrap_or(entry.path())
                            .display()
                            .to_string();
                        matches.push(format!("{}:{}: {}", rel, line_no + 1, line.trim()));
                        if matches.len() >= MAX_MATCHES {
                            matches.push("[truncated — more matches exist]".into());
                            break 'outer;
                        }
                    }
                }
            }

            if matches.is_empty() {
                "No matches found".into()
            } else {
                matches.join("\n")
            }
        }
        _ => format!("Unknown built-in tool: {}", name),
    }
}

/// Simple glob matcher supporting `*` (match any substring).
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut pat = pattern.split('*').peekable();
    let mut pos = 0usize;
    let mut first = true;
    while let Some(seg) = pat.next() {
        if first {
            first = false;
            if !name.starts_with(seg) && !seg.is_empty() {
                return false;
            }
            pos = seg.len();
        } else if pat.peek().is_none() {
            return name.ends_with(seg);
        } else {
            if let Some(idx) = name[pos..].find(seg) {
                pos += idx + seg.len();
            } else {
                return false;
            }
        }
    }
    true
}
