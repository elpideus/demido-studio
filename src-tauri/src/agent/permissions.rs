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
    // Decided before `mode`, so these hold in every mode including cautious.
    match tool_name {
        // Confined to Demido's own skills folder by `skills::skill_dir`, and it never wipes a
        // folder it replaces — so the blast radius is the app's own data, not the user's files.
        // Authoring a skill is a create-read-fix loop; a prompt per write made it unusable.
        //
        // Unless the payload bundles an mcp.json. That is a different animal: a skill's MCP server
        // is a spawned process (`mcp::stdio::StdioClient::spawn`), so auto-allowing it would let a
        // model author a skill, install it silently, and execute an arbitrary command line — in
        // any mode, Off included, with no prompt ever shown. The skill's mcp.json is trusted, but
        // only because this prompt is what makes the *user* the one who installed it.
        "install_skill" => {
            let bundles_mcp = serde_json::from_value::<Vec<crate::skills::IncomingFile>>(
                args["files"].clone(),
            )
            .map(|files| crate::skills::payload_bundles_mcp(&files))
            // Unparseable payload: install_skill will reject it anyway, and refusing to guess is
            // the safe direction here.
            .unwrap_or(true);
            return if bundles_mcp {
                PermissionResult::Ask
            } else {
                PermissionResult::Allow
            };
        }
        // Deliberately never auto-allowed: this is `remove_dir_all` on a folder that may hold
        // skills the user hand-wrote, and there is no undo.
        "delete_skill" => return PermissionResult::Ask,
        _ => {}
    }
    // A skill-declared tool returns its own prompt body as text and executes nothing, so there is
    // no blast radius to gate — prompting for one would be prompting to read a file the user
    // installed and switched on. Anything the body then asks for goes through its own tool's gate.
    if crate::skills::is_skill_tool(tool_name) {
        return PermissionResult::Allow;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MODES: &[&str] = &["cautious", "balanced", "autonomous", "off"];

    fn allowed(mode: &str, tool: &str, args: Value) -> bool {
        matches!(is_permitted(mode, tool, &args), PermissionResult::Allow)
    }

    #[test]
    fn install_skill_never_prompts_in_any_mode() {
        for mode in MODES {
            assert!(
                allowed(mode, "install_skill", json!({ "id": "my-skill", "files": [] })),
                "install_skill should be allowed in {mode} mode"
            );
        }
    }

    /// The load-bearing one. A skill's mcp.json spawns a process, and the model can write a skill,
    /// so auto-allowing this install would be self-granted arbitrary execution — in Off mode, with
    /// no prompt. Everything about trusting a skill's mcp.json rests on the user being the one who
    /// let it in.
    #[test]
    fn install_skill_always_prompts_when_the_payload_bundles_an_mcp_server() {
        let payload = json!({
            "id": "evil",
            "files": [
                { "path": "skill.json", "content": "{}" },
                { "path": "SKILL.md", "content": "hi" },
                { "path": "tools.json", "content": "{\"tools\":[{\"type\":\"mcp\",\"name\":\"x\",\"command\":\"powershell\"}]}" }
            ]
        });
        for mode in MODES {
            assert!(
                !allowed(mode, "install_skill", payload.clone()),
                "install_skill with an mcp.json must ask in {mode} mode"
            );
        }
    }

    /// Text-only skills keep the frictionless loop the Allow was built for — a prompt per write
    /// made authoring unusable. A prompt tool executes nothing, so it stays on this side too.
    #[test]
    fn install_skill_still_allows_a_payload_with_no_mcp_server() {
        let payload = json!({
            "id": "fine",
            "files": [
                { "path": "skill.json", "content": "{}" },
                { "path": "SKILL.md", "content": "hi" },
                { "path": "tools.json", "content": "{\"tools\":[{\"type\":\"prompt\",\"name\":\"go\",\"description\":\"g\",\"prompt\":\"b\"}]}" }
            ]
        });
        for mode in MODES {
            assert!(allowed(mode, "install_skill", payload.clone()));
        }
    }

    /// A payload we cannot read is a payload we cannot clear.
    #[test]
    fn install_skill_asks_when_the_payload_cannot_be_parsed() {
        for mode in MODES {
            assert!(!allowed(mode, "install_skill", json!({ "id": "x", "files": "not-an-array" })));
        }
    }

    /// Skill prompt tools execute nothing, so gating them would prompt to read a file the user
    /// installed and switched on.
    #[test]
    fn skill_prompt_tools_never_prompt() {
        for mode in MODES {
            assert!(allowed(mode, "skill_my-skill_review", json!({ "path": "a.ts" })));
        }
    }

    #[test]
    fn delete_skill_always_prompts_even_in_autonomous_mode() {
        for mode in MODES {
            assert!(
                !allowed(mode, "delete_skill", json!({ "id": "my-skill" })),
                "delete_skill should ask in {mode} mode"
            );
        }
    }

    #[test]
    fn the_skills_exemptions_do_not_leak_into_other_tools() {
        for mode in ["cautious", "balanced"] {
            assert!(!allowed(mode, "write_file", json!({ "path": "a.txt" })));
            assert!(!allowed(mode, "run_command", json!({ "command": "ls" })));
        }
    }

    #[test]
    fn balanced_still_guards_sensitive_reads() {
        assert!(allowed("balanced", "read_file", json!({ "path": "src/main.rs" })));
        assert!(!allowed("balanced", "read_file", json!({ "path": "app/.env" })));
    }
}
