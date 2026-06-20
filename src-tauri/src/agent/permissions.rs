use serde_json::Value;

pub enum PermissionResult {
    Allow,
    Ask,
}

/// Sensitive file patterns for Balanced mode (matched against lowercased path)
const SENSITIVE_PATTERNS: &[&str] = &[
    ".env",
    "secret",
    "credential",
    "password",
    ".key",
    ".pem",
    ".p12",
    ".pfx",
];

fn is_sensitive_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let filename = std::path::Path::new(&lower)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&lower);
    SENSITIVE_PATTERNS.iter().any(|pat| {
        let matches_filename = if pat.starts_with('.') {
            filename == *pat || filename.starts_with(*pat)
        } else {
            filename.contains(pat)
        };
        // Also check full path so "secrets/database.toml" or "config/password.yml" is caught
        let matches_path = lower.contains(pat);
        matches_filename || matches_path
    })
}

pub fn is_permitted(mode: &str, tool_name: &str, args: &Value) -> PermissionResult {
    match mode {
        "cautious" => PermissionResult::Ask,
        "autonomous" => PermissionResult::Allow,
        "balanced" => match tool_name {
            "read_file" => {
                let path = args["path"].as_str().unwrap_or("");
                if is_sensitive_path(path) {
                    PermissionResult::Ask
                } else {
                    PermissionResult::Allow
                }
            }
            "list_dir" | "search_files" => PermissionResult::Allow,
            _ => PermissionResult::Ask, // write_file, edit_file, run_command
        },
        _ => PermissionResult::Ask,
    }
}
