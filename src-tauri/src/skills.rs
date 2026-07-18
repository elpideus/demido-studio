use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{command, AppHandle, Manager};

/// One declared parameter of a command, positional in schema order. The frontend binds invocation
/// tokens to these names and substitutes `$name` in the prompt body.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillCommandParam {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    /// Swallows every remaining token, so a trailing free-text param can contain spaces.
    #[serde(default)]
    pub rest: bool,
}

/// A slash command a skill exposes in the chat input. The prompt body comes from `file` (a path
/// inside the skill folder) or, for one-liners, from an inline `prompt`. `file` wins if both are set.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillCommand {
    pub name: String,
    pub description: String,
    pub file: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub params: Vec<SkillCommandParam>,
}

/// A tool a skill exposes to the *model*, as opposed to a `SkillCommand`, which the *user* types.
///
/// Lives in the skill folder's `tools.json`, not `skill.json`: tools are a capability surface with
/// several kinds (see `SkillToolEntry`), and keeping them out of the metadata that ships in every
/// prompt means adding one costs the model nothing until it is offered.
///
/// `prompt` kind. Body from `file` (a path inside the skill folder) or inline `prompt`, `file`
/// wins — same shape as a command on purpose. Calling it returns the expanded body as the tool
/// result; nothing executes. A prompt tool that needs a shell says so in its body and lets the
/// model call `run_command`, which is gated properly.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillPromptTool {
    pub name: String,
    pub description: String,
    pub file: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub params: Vec<SkillCommandParam>,
}

/// A backend-implemented tool a skill surfaces — a `{"type": "builtin"}` entry.
///
/// Some capabilities cannot be text: `install_skill` writes files, `delete_skill` is
/// `remove_dir_all`. They are implemented in Rust and dispatched by `executor::execute_tool` under
/// their real names, unprefixed. A skill does not *implement* them — it declares which of them it
/// puts in front of the model, so they are offered only while that skill is enabled and the Tools
/// popup can list them under the skill that brings them.
///
/// Only names in `agent::exposable_builtin_defs` may be claimed (`install_skill`, `delete_skill`),
/// so this cannot reach `run_command` or the filesystem tools — those stay behind `agent_mode`,
/// where a skill has no say. Each claimed tool keeps its own permission rule from
/// `permissions::is_permitted`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillBuiltinTool {
    pub name: String,
    /// Overrides the backend's own description in the Tools popup. The model always sees the real
    /// one — a skill may not reword what a tool claims to do.
    #[serde(default)]
    pub description: Option<String>,
}

/// One entry in a skill's `tools.json`, discriminated by `type`.
///
/// An `mcp` entry declares a *server*, so it contributes however many tools that server reports —
/// unlike `prompt` and `builtin`, which are exactly one tool each. All are listed here because
/// from the user's side they are the same thing: capabilities this skill brings, on while the
/// skill is on.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkillToolEntry {
    Prompt(SkillPromptTool),
    Mcp(SkillMcpServer),
    Builtin(SkillBuiltinTool),
}

impl SkillToolEntry {
    pub fn name(&self) -> &str {
        match self {
            SkillToolEntry::Prompt(t) => &t.name,
            SkillToolEntry::Mcp(s) => &s.name,
            SkillToolEntry::Builtin(b) => &b.name,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SkillToolsConfig {
    #[serde(default)]
    pub tools: Vec<SkillToolEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub commands: Vec<SkillCommand>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub commands: Vec<SkillCommand>,
    /// This skill's `tools.json` entries, prompt and MCP alike. Offered only while the skill is
    /// enabled — see `skill_tool_defs` and `skill_mcp_servers`.
    pub tools: Vec<SkillToolEntry>,
    /// Raw `skill.json` text. This — not SKILL.md — is what an enabled skill puts in the prompt:
    /// the metadata is a few hundred tokens, the body is thousands, and the model can read the
    /// body itself off `files` when the skill actually applies.
    pub meta_json: String,
    /// Absolute paths of every file in the skill folder except `skill.json` (its content is
    /// already inlined as `meta_json`). Lets the model open SKILL.md and bundled references.
    pub files: Vec<String>,
    /// Absolute path of the skill folder. Commands surface this to the model, which otherwise has
    /// no way to resolve a relative path mentioned in a prompt body.
    pub path: String,
}

/// Absolute paths of every file under a skill folder, `skill.json` excluded — the prompt inlines
/// its content, so listing it too would just invite a redundant read_file.
fn collect_paths(dir: &PathBuf, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect_paths(&path, out);
        } else if !e.file_name().eq_ignore_ascii_case("skill.json") {
            out.push(path.to_string_lossy().to_string());
        }
    }
    out.sort();
}

pub fn skills_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("skills")
}

/// Every installed skill's parsed `skill.json`, with its folder and the raw text.
///
/// One reader for `list_skills` and the tool builder both: a skill that fails to parse must be
/// invisible to each of them identically, or a tool would be offered for a skill the Tools panel
/// never showed. Parse failures are still dropped silently here — pre-existing behaviour.
fn all_skill_metas(app: &AppHandle) -> Vec<(SkillMeta, PathBuf, String)> {
    let dir = skills_dir(app);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let path = entry.path();
            let json_raw = std::fs::read_to_string(path.join("skill.json")).ok()?;
            let meta: SkillMeta = serde_json::from_str(&json_raw).ok()?;
            Some((meta, path, json_raw))
        })
        .collect()
}

/// A skill folder's `tools.json`, or an empty config when it has none.
///
/// A skill without tools is the normal case, so a missing file is not an error. An *unparseable*
/// one is: it means the author declared tools that will silently not exist, so it is logged rather
/// than swallowed. `install_skill` rejects that case up front — this guards hand-edited folders.
pub fn read_tools_config(skill_path: &std::path::Path) -> SkillToolsConfig {
    let Ok(raw) = std::fs::read_to_string(skill_path.join("tools.json")) else {
        return SkillToolsConfig::default();
    };
    match serde_json::from_str(&raw) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "[skills] {} has an unparseable tools.json — its tools will not be offered: {e}",
                skill_path.display()
            );
            SkillToolsConfig::default()
        }
    }
}

#[command]
pub fn list_skills(app: AppHandle) -> Vec<Skill> {
    all_skill_metas(&app)
        .into_iter()
        .map(|(meta, path, json_raw)| {
            let mut files = Vec::new();
            collect_paths(&path, &mut files);
            let tools = read_tools_config(&path).tools;
            Skill {
                id: meta.id,
                name: meta.name,
                description: meta.description,
                version: meta.version,
                commands: meta.commands,
                tools,
                meta_json: json_raw,
                files,
                path: path.to_string_lossy().to_string(),
            }
        })
        .collect()
}

#[command]
pub fn delete_skill(app: AppHandle, id: String) -> Result<(), String> {
    let path = skill_dir(&app, &id)?;
    if !path.exists() {
        return Ok(());
    }
    let base = skills_dir(&app).canonicalize().map_err(|e| e.to_string())?;
    let target = path.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&base) {
        return Err("invalid path".into());
    }
    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())
}

/// One editable file inside a skill folder. `name` is relative to the skill dir, `/`-separated.
#[derive(Debug, Serialize)]
pub struct SkillFile {
    pub name: String,
    pub content: String,
}

/// A skill folder path, rejecting ids that could escape the skills dir.
fn skill_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("invalid id".into());
    }
    Ok(skills_dir(app).join(id))
}

fn collect_files(dir: &PathBuf, prefix: &str, out: &mut Vec<SkillFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{}/{}", prefix, name)
        };
        if path.is_dir() {
            collect_files(&path, &rel, out);
        } else if let Ok(content) = std::fs::read_to_string(&path) {
            // read_to_string failing is how binaries (icons, archives) get skipped — there is
            // nothing to edit in them.
            out.push(SkillFile { name: rel, content });
        }
    }
}

/// Every text file in a skill folder, for the artifact editor's tabs.
#[command]
pub fn read_skill_files(app: AppHandle, id: String) -> Result<Vec<SkillFile>, String> {
    let dir = skill_dir(&app, &id)?;
    if !dir.is_dir() {
        return Err("skill not found".into());
    }
    let mut out = Vec::new();
    collect_files(&dir, "", &mut out);
    // SKILL.md is the file the user opens the editor for; keep it first.
    out.sort_by_key(|f| (f.name != "SKILL.md", f.name.to_lowercase()));
    Ok(out)
}

/// One file of a skill being installed by the agent.
#[derive(Debug, Deserialize)]
pub struct IncomingFile {
    pub path: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, content: &str) -> IncomingFile {
        IncomingFile {
            path: path.into(),
            content: content.into(),
        }
    }
    const META: &str = r#"{"id":"x","name":"X","description":"d","version":"1.0.0","commands":[]}"#;

    #[test]
    fn parses_a_command_that_omits_file_in_favour_of_an_inline_prompt() {
        // Guards the shape the app silently drops a skill for: any parse failure here means the
        // whole skill vanishes from Tools → Skills with no error.
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","prompt":"body"}]}"#;
        let parsed: Result<SkillMeta, _> = serde_json::from_str(meta);
        assert!(parsed.is_ok(), "{:?}", parsed.err());
    }

    #[test]
    fn accepts_a_minimal_valid_skill() {
        let files = vec![f("skill.json", META), f("SKILL.md", "body")];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    /// A skill with no tools.json is the normal case and must stay valid.
    #[test]
    fn a_skill_without_a_tools_json_is_valid() {
        let parsed: SkillMeta = serde_json::from_str(META).unwrap();
        assert_eq!(parsed.id, "x");
        let files = vec![f("skill.json", META), f("SKILL.md", "b")];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    /// `tools.json` is a list of typed entries — this is the contract the whole feature hangs on.
    fn tools_json(body: &str) -> String {
        format!(r#"{{"tools":[{body}]}}"#)
    }

    #[test]
    fn accepts_a_skill_declaring_a_prompt_tool() {
        let cfg = tools_json(
            r#"{"type":"prompt","name":"review","description":"r","file":"review.md","params":[{"name":"path","required":true}]}"#,
        );
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
            f("review.md", "b"),
        ];
        assert!(
            validate_skill_files("x", &files).is_ok(),
            "{:?}",
            validate_skill_files("x", &files)
        );
    }

    /// An unknown `type` must fail loudly: silently ignoring it would install a skill whose tools
    /// do not exist.
    #[test]
    fn rejects_an_unknown_tool_type() {
        let cfg = tools_json(r#"{"type":"exec","name":"go","description":"g","command":"rm"}"#);
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("tools.json"));
    }

    #[test]
    fn rejects_a_tool_whose_file_is_missing() {
        let cfg =
            tools_json(r#"{"type":"prompt","name":"review","description":"r","file":"nope.md"}"#);
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("nope.md"));
    }

    #[test]
    fn rejects_a_tool_with_no_body() {
        let cfg = tools_json(r#"{"type":"prompt","name":"review","description":"r"}"#);
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("no prompt body"));
    }

    /// The wire name is `skill_<id>_<tool>`, and providers reject anything outside
    /// `^[a-zA-Z0-9_-]{1,64}$` — so a name that installs must also be offerable.
    #[test]
    fn rejects_a_tool_name_a_provider_would_reject() {
        let cfg =
            tools_json(r#"{"type":"prompt","name":"re:view","description":"r","prompt":"b"}"#);
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("invalid name"));
    }

    #[test]
    fn rejects_an_id_a_provider_would_reject_only_when_the_skill_declares_tools() {
        let meta = r#"{"id":"my.skill","name":"X","description":"d","version":"1.0.0"}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        // No tools: the id never reaches a provider, so it stays legal.
        assert!(validate_skill_files("my.skill", &files).is_ok());

        let cfg = tools_json(r#"{"type":"prompt","name":"go","description":"g","prompt":"b"}"#);
        let files = vec![
            f("skill.json", meta),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("my.skill", &files)
            .unwrap_err()
            .contains("declares tools"));
    }

    #[test]
    fn rejects_a_wire_name_over_the_provider_limit() {
        let long = "t".repeat(60);
        let cfg = tools_json(&format!(
            r#"{{"type":"prompt","name":"{long}","description":"r","prompt":"b"}}"#
        ));
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("64"));
    }

    #[test]
    fn rejects_rest_on_a_tool_param_because_a_tool_has_no_tokens_to_swallow() {
        let cfg = tools_json(
            r#"{"type":"prompt","name":"go","description":"g","prompt":"b","params":[{"name":"a","rest":true}]}"#,
        );
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("rest"));
    }

    /// Names must be unique *across kinds* — a prompt tool and an MCP server sharing a name would
    /// make the popup ambiguous and the skill's own docs wrong.
    #[test]
    fn rejects_two_tools_with_the_same_name() {
        let cfg = tools_json(
            r#"{"type":"prompt","name":"go","description":"g","prompt":"b"},{"type":"mcp","name":"go","command":"npx"}"#,
        );
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("declared twice"));
    }

    fn param(name: &str) -> SkillCommandParam {
        SkillCommandParam {
            name: name.into(),
            description: None,
            required: false,
            rest: false,
        }
    }

    #[test]
    fn expands_named_params_positionals_and_arguments() {
        let params = vec![param("path"), param("mode")];
        let args = json!({ "path": "src/a.ts", "mode": "strict" });
        assert_eq!(
            expand_skill_tool_body("review $path in $mode", &params, &args),
            "review src/a.ts in strict"
        );
        assert_eq!(
            expand_skill_tool_body("$1/$2", &params, &args),
            "src/a.ts/strict"
        );
        assert_eq!(
            expand_skill_tool_body("all: $ARGUMENTS", &params, &args),
            "all: src/a.ts strict"
        );
    }

    /// Same rule as `expandCommand`: an unknown `$word` is prose, not a placeholder.
    #[test]
    fn leaves_undeclared_placeholders_alone_and_unescapes_escaped_ones() {
        let params = vec![param("path")];
        let args = json!({ "path": "a.ts" });
        // `$other` is prose and survives. `$5` does not: $1..$9 are always placeholders, so an
        // out-of-range one substitutes empty — same as `expandCommand`'s `tokens[n] ?? ''`.
        assert_eq!(
            expand_skill_tool_body("$path and $other", &params, &args),
            "a.ts and $other"
        );
        assert_eq!(expand_skill_tool_body("$5", &params, &args), "");
        // An escaped placeholder unescapes but does not count as a substitution, so the arguments
        // are still appended — `expandCommand` treats an escape-only body the same way.
        assert_eq!(
            expand_skill_tool_body(r"talk about \$ARGUMENTS", &params, &args),
            "talk about $ARGUMENTS\n\na.ts"
        );
        assert_eq!(
            expand_skill_tool_body(r"$path, not \$ARGUMENTS", &params, &args),
            "a.ts, not $ARGUMENTS"
        );
    }

    /// Mirrors `expandCommand`: a body that never mentions its arguments still receives them.
    #[test]
    fn appends_arguments_when_the_body_substitutes_nothing() {
        let params = vec![param("path")];
        let args = json!({ "path": "a.ts" });
        assert_eq!(
            expand_skill_tool_body("Review the file.", &params, &args),
            "Review the file.\n\na.ts"
        );
        // Nothing to append: no trailing blank lines.
        assert_eq!(
            expand_skill_tool_body("Review.", &params, &json!({})),
            "Review."
        );
    }

    const MCP_TOOLS_JSON: &str = r#"{"tools":[{"type":"mcp","name":"wf","command":"npx","args":["-y","wf-mcp"],"bypassAgentMode":true}]}"#;

    #[test]
    fn accepts_a_skill_bundling_an_mcp_server() {
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", MCP_TOOLS_JSON),
        ];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    #[test]
    fn rejects_an_mcp_server_with_no_command() {
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f(
                "tools.json",
                r#"{"tools":[{"type":"mcp","name":"x","command":""}]}"#,
            ),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("no command"));
    }

    #[test]
    fn rejects_an_unparseable_tools_json_rather_than_installing_a_dead_server() {
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", "{nope"),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("tools.json is not valid JSON"));
    }

    #[test]
    fn detects_a_bundled_mcp_payload() {
        let with = vec![f("skill.json", META), f("tools.json", MCP_TOOLS_JSON)];
        let prompt_only = vec![
            f("skill.json", META),
            f(
                "tools.json",
                r#"{"tools":[{"type":"prompt","name":"go","description":"g","prompt":"b"}]}"#,
            ),
        ];
        let none = vec![f("skill.json", META), f("SKILL.md", "b")];
        assert!(payload_bundles_mcp(&with));
        // A prompt tool executes nothing, so it must not drag the install prompt in with it.
        assert!(!payload_bundles_mcp(&prompt_only));
        assert!(!payload_bundles_mcp(&none));
    }

    /// The prompt has to carry the command line and the bypass request: those are the decision.
    #[test]
    fn describes_what_the_bundled_server_will_run() {
        let files = vec![f("skill.json", META), f("tools.json", MCP_TOOLS_JSON)];
        let d = describe_payload_mcp(&files).unwrap();
        assert!(d.contains("npx -y wf-mcp"), "{d}");
        assert!(d.contains("bypass"), "{d}");
    }

    /// `bypassAgentMode` absent means gated — a skill has to ask for the exception, it never gets
    /// it by omission.
    #[test]
    fn bypass_defaults_to_off() {
        let cfg: SkillToolsConfig =
            serde_json::from_str(r#"{"tools":[{"type":"mcp","name":"x","command":"npx"}]}"#)
                .unwrap();
        match &cfg.tools[0] {
            SkillToolEntry::Mcp(s) => assert!(!s.bypass_agent_mode),
            _ => panic!("expected an mcp entry"),
        }
    }

    /// The field is `bypassAgentMode` on the wire. Without the camelCase rename it parsed as
    /// absent — i.e. every skill asking for a bypass silently got a gated server instead.
    #[test]
    fn bypass_is_read_from_the_camel_case_key() {
        let cfg: SkillToolsConfig = serde_json::from_str(MCP_TOOLS_JSON).unwrap();
        match &cfg.tools[0] {
            SkillToolEntry::Mcp(s) => assert!(s.bypass_agent_mode),
            _ => panic!("expected an mcp entry"),
        }
    }

    #[test]
    fn accepts_a_skill_claiming_a_builtin() {
        let cfg = tools_json(r#"{"type":"builtin","name":"install_skill"}"#);
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    /// The allowlist is the boundary. A skill claiming `run_command` would be handing itself a
    /// mode-gated tool — the model can author skills, so this must fail at install.
    #[test]
    fn rejects_a_builtin_outside_the_allowlist() {
        for name in ["run_command", "write_file", "read_file", "nonsense"] {
            let cfg = tools_json(&format!(r#"{{"type":"builtin","name":"{name}"}}"#));
            let files = vec![
                f("skill.json", META),
                f("SKILL.md", "b"),
                f("tools.json", &cfg),
            ];
            let err = validate_skill_files("x", &files).unwrap_err();
            assert!(
                err.contains("not a tool a skill may surface"),
                "{name}: {err}"
            );
            // The message names the ones that do exist — the set is short and not guessable.
            assert!(err.contains("install_skill"), "{name}: {err}");
        }
    }

    /// A claimed builtin is not offered under a `skill_` prefix — it dispatches to the real
    /// implementation in `executor::execute_tool`, which matches its own name.
    #[test]
    fn a_claimed_builtin_keeps_its_real_name() {
        assert!(!is_skill_tool("install_skill"));
        assert!(crate::agent::exposable_builtin("install_skill").is_some());
        assert!(crate::agent::exposable_builtin("run_command").is_none());
    }

    /// Both kinds in one file, which is the point of tools.json.
    #[test]
    fn accepts_prompt_and_mcp_entries_side_by_side() {
        let cfg = tools_json(
            r#"{"type":"prompt","name":"review","description":"r","prompt":"b"},{"type":"mcp","name":"wf","command":"npx"}"#,
        );
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("tools.json", &cfg),
        ];
        assert!(validate_skill_files("x", &files).is_ok());
        assert!(payload_bundles_mcp(&files));
    }

    /// The bundled skills ship in the repo and are installed by hand, so nothing else would ever
    /// run them past the validator. A `file` that isn't there, or a param a body doesn't use, would
    /// only surface as a broken skill on a user's machine.
    #[test]
    fn bundled_skills_pass_their_own_validator() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("skills");
        let Ok(entries) = std::fs::read_dir(&root) else {
            return; // skills/ is not shipped in every checkout
        };
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            let dir = entry.path();
            let id = dir.file_name().unwrap().to_string_lossy().to_string();
            let mut files = Vec::new();
            let mut stack = vec![dir.clone()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).unwrap().flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        let rel = p
                            .strip_prefix(&dir)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/");
                        files.push(IncomingFile {
                            path: rel,
                            content: std::fs::read_to_string(&p).unwrap_or_default(),
                        });
                    }
                }
            }
            assert!(
                validate_skill_files(&id, &files).is_ok(),
                "bundled skill '{id}' fails validation: {}",
                validate_skill_files(&id, &files).unwrap_err()
            );
        }
    }

    #[test]
    fn wire_names_are_namespaced_by_skill_id() {
        assert_eq!(
            skill_tool_name("my-skill", "review"),
            "skill_my-skill_review"
        );
        assert!(is_skill_tool("skill_my-skill_review"));
        assert!(!is_skill_tool("read_file"));
    }

    #[test]
    fn rejects_missing_skill_json_because_the_app_would_ignore_it() {
        let files = vec![f("SKILL.md", "body")];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("skill.json"));
    }

    #[test]
    fn rejects_missing_skill_md() {
        let files = vec![f("skill.json", META)];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("SKILL.md"));
    }

    #[test]
    fn rejects_id_mismatch_with_folder_name() {
        let files = vec![f("skill.json", META), f("SKILL.md", "b")];
        assert!(validate_skill_files("other", &files)
            .unwrap_err()
            .contains("id mismatch"));
    }

    #[test]
    fn rejects_a_command_whose_file_was_not_provided() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","file":"commands/go.md"}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        let err = validate_skill_files("x", &files).unwrap_err();
        assert!(err.contains("commands/go.md"), "{err}");
    }

    #[test]
    fn accepts_a_command_whose_file_is_provided() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","file":"commands/go.md"}]}"#;
        let files = vec![
            f("skill.json", meta),
            f("SKILL.md", "b"),
            f("commands/go.md", "go"),
        ];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    #[test]
    fn rejects_a_command_with_neither_a_file_nor_a_prompt() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g"}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        let err = validate_skill_files("x", &files).unwrap_err();
        assert!(err.contains("no prompt body"), "{err}");
    }

    #[test]
    fn rejects_a_command_whose_inline_prompt_is_blank() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","prompt":"   "}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        let err = validate_skill_files("x", &files).unwrap_err();
        assert!(err.contains("no prompt body"), "{err}");
    }

    #[test]
    fn rejects_a_param_name_that_could_never_substitute() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","prompt":"p",
                "params":[{"name":"my param"}]}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("invalid param name"));
    }

    #[test]
    fn rejects_a_rest_param_that_is_not_last_because_it_would_eat_the_others() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","prompt":"p",
                "params":[{"name":"a","rest":true},{"name":"b"}]}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("not last"));
    }

    #[test]
    fn accepts_a_command_with_a_valid_param_schema() {
        let meta = r#"{"id":"x","name":"X","description":"d","version":"1.0.0",
            "commands":[{"name":"go","description":"g","prompt":"p",
                "params":[{"name":"file","required":true},{"name":"notes","rest":true}]}]}"#;
        let files = vec![f("skill.json", meta), f("SKILL.md", "b")];
        assert!(validate_skill_files("x", &files).is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        let files = vec![
            f("skill.json", META),
            f("SKILL.md", "b"),
            f("../evil.md", "x"),
        ];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("invalid file path"));
    }

    #[test]
    fn rejects_invalid_json_with_a_usable_message() {
        let files = vec![f("skill.json", "{not json"), f("SKILL.md", "b")];
        assert!(validate_skill_files("x", &files)
            .unwrap_err()
            .contains("not valid JSON"));
    }
}

/// Write a whole skill into the skills dir, creating or replacing its folder.
///
/// Confined to the skills dir by construction, which is why the agent may call this without a
/// working folder — the reason mutating tools are otherwise gated (unbounded writes anywhere on
/// disk) does not apply. Enforces the invariants that make a skill actually load, rather than
/// trusting the model to have checked them.
/// Reject a skill that would install but not work. These are exactly the checks the conversion
/// guide asks a model to perform by hand — enforced here so a wrong answer fails loudly at install
/// time instead of silently at send time.
pub fn validate_skill_files(id: &str, files: &[IncomingFile]) -> Result<(), String> {
    let has = |name: &str| {
        files
            .iter()
            .any(|f| f.path.replace('\\', "/").eq_ignore_ascii_case(name))
    };
    if !has("skill.json") {
        return Err("missing skill.json — without it the app silently ignores the skill".into());
    }
    if !has("SKILL.md") {
        return Err(
            "missing SKILL.md — it is where a skill's knowledge lives, and the model is told to read it by path".into(),
        );
    }

    for f in files {
        let rel = f.path.replace('\\', "/");
        if rel.is_empty() || rel.contains("..") || rel.starts_with('/') {
            return Err(format!("invalid file path: {}", f.path));
        }
    }

    // The id is the folder name, so a mismatch would install a skill the user cannot find by the
    // name it reports.
    let meta_raw = &files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("skill.json"))
        .unwrap()
        .content;
    let meta: SkillMeta =
        serde_json::from_str(meta_raw).map_err(|e| format!("skill.json is not valid JSON: {e}"))?;
    if meta.id != id {
        return Err(format!(
            "id mismatch: called with '{id}' but skill.json says '{}'",
            meta.id
        ));
    }

    // A command whose file is absent fails at send time, long after the model has moved on.
    for c in &meta.commands {
        // A param name that isn't a valid placeholder identifier would never substitute — the body
        // would ship to the model with a literal `$foo bar` in it.
        for p in &c.params {
            let valid = !p.name.is_empty()
                && p.name
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_alphabetic() || ch == '_')
                    .unwrap_or(false)
                && p.name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if !valid || p.name == "ARGUMENTS" {
                return Err(format!(
                    "command '{}' has invalid param name '{}' — use letters, digits and underscores, starting with a letter, and not 'ARGUMENTS'",
                    c.name, p.name
                ));
            }
        }
        if let Some(rest_at) = c.params.iter().position(|p| p.rest) {
            if rest_at != c.params.len() - 1 {
                return Err(format!(
                    "command '{}': param '{}' is 'rest' but is not last — it would swallow the params after it",
                    c.name, c.params[rest_at].name
                ));
            }
        }
        match (&c.file, &c.prompt) {
            (Some(f), _) => {
                if !has(&f.replace('\\', "/")) {
                    return Err(format!(
                        "command '{}' declares file '{f}', which is not among the files provided",
                        c.name
                    ));
                }
            }
            // Neither source of a body: the command installs fine and then errors the first time
            // anyone types it. Catch it here, while the author is still listening.
            (None, prompt) if prompt.as_ref().is_none_or(|p| p.trim().is_empty()) => {
                return Err(format!(
                    "command '{}' has no prompt body — give it a 'file' (a path among the files provided) or a non-empty inline 'prompt'",
                    c.name
                ));
            }
            (None, _) => {}
        }
    }

    validate_tools_json(id, files, &has)?;
    Ok(())
}

/// Validate a payload's `tools.json`, if it has one.
///
/// A tool is offered to the model *by name*, so a broken one is worse than a broken command: the
/// model sees it, calls it, and gets back an error it cannot act on. An unparseable file is worse
/// still — the tools silently do not exist, and for an MCP entry the user would have been prompted
/// for a server that never starts.
fn validate_tools_json(
    id: &str,
    files: &[IncomingFile],
    has: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    let Some(raw) = files
        .iter()
        .find(|f| f.path.replace('\\', "/").eq_ignore_ascii_case("tools.json"))
    else {
        return Ok(());
    };
    let cfg: SkillToolsConfig = serde_json::from_str(&raw.content).map_err(|e| {
        format!(
            "tools.json is not valid JSON, or an entry has an unknown 'type' (use \"prompt\" or \"mcp\"): {e}"
        )
    })?;
    if cfg.tools.is_empty() {
        return Err("tools.json declares no tools — omit the file or add one".into());
    }
    // The skill id is part of every prompt tool's wire name, so it has to survive a provider's
    // name rule. Ids are otherwise free-form.
    if !is_tool_name_safe(id) {
        return Err(format!(
            "skill id '{id}' declares tools, so it must use only letters, digits, '_' and '-' — \
             providers reject any other character in a tool name"
        ));
    }

    for entry in &cfg.tools {
        match entry {
            SkillToolEntry::Prompt(t) => {
                if !is_tool_name_safe(&t.name) {
                    return Err(format!(
                        "tool '{}' has an invalid name — use only letters, digits, '_' and '-'",
                        t.name
                    ));
                }
                // Providers cap tool names at 64 chars, and the id is part of the wire name.
                let wire = skill_tool_name(id, &t.name);
                if wire.len() > 64 {
                    return Err(format!(
                        "tool '{}' is offered as '{wire}' ({} chars) — providers reject names over 64; \
                         shorten the tool name or the skill id",
                        t.name,
                        wire.len()
                    ));
                }
                for p in &t.params {
                    let valid = !p.name.is_empty()
                        && p.name
                            .chars()
                            .next()
                            .map(|ch| ch.is_ascii_alphabetic() || ch == '_')
                            .unwrap_or(false)
                        && p.name
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                    if !valid || p.name == "ARGUMENTS" {
                        return Err(format!(
                            "tool '{}' has invalid param name '{}' — use letters, digits and underscores, starting with a letter, and not 'ARGUMENTS'",
                            t.name, p.name
                        ));
                    }
                    // `rest` swallows trailing tokens of a typed invocation. A tool call has no
                    // tokens to swallow — its arguments arrive named — so it would do nothing.
                    if p.rest {
                        return Err(format!(
                            "tool '{}': param '{}' sets 'rest', which only applies to typed commands — a tool's arguments arrive named, so drop it",
                            t.name, p.name
                        ));
                    }
                }
                match (&t.file, &t.prompt) {
                    (Some(f), _) => {
                        if !has(&f.replace('\\', "/")) {
                            return Err(format!(
                                "tool '{}' declares file '{f}', which is not among the files provided",
                                t.name
                            ));
                        }
                    }
                    (None, prompt) if prompt.as_ref().is_none_or(|p| p.trim().is_empty()) => {
                        return Err(format!(
                            "tool '{}' has no prompt body — give it a 'file' (a path among the files provided) or a non-empty inline 'prompt'",
                            t.name
                        ));
                    }
                    (None, _) => {}
                }
            }
            SkillToolEntry::Mcp(s) => {
                if s.name.trim().is_empty() {
                    return Err("tools.json has an mcp entry with no name".into());
                }
                if s.command.trim().is_empty() {
                    return Err(format!(
                        "mcp entry '{}' has no command — there would be nothing to spawn",
                        s.name
                    ));
                }
            }
            // A name outside the allowlist would install fine and then simply never appear. Say so
            // here, and say which names exist — the set is short and not guessable.
            SkillToolEntry::Builtin(b) => {
                if crate::agent::exposable_builtin(&b.name).is_none() {
                    let known: Vec<String> = crate::agent::exposable_builtin_defs()
                        .into_iter()
                        .map(|t| t.name)
                        .collect();
                    return Err(format!(
                        "builtin entry '{}' is not a tool a skill may surface — the ones that exist are: {}",
                        b.name,
                        known.join(", ")
                    ));
                }
            }
        }
    }

    if let Some(dup) = cfg.tools.iter().enumerate().find_map(|(i, t)| {
        cfg.tools[i + 1..]
            .iter()
            .find(|o| o.name() == t.name())
            .map(|_| t.name().to_string())
    }) {
        return Err(format!(
            "tool '{dup}' is declared twice — names must be unique within a skill's tools.json"
        ));
    }
    Ok(())
}

/// One MCP server a skill bundles — a `{"type": "mcp"}` entry in its `tools.json`.
///
/// A manifest, not a grant: it describes a server and asks for a permission. `install_skill`
/// prompts when a payload's `tools.json` declares one (see `payload_bundles_mcp`), which is what
/// makes the *user* the one who let it in — `tools.json` is data the model can write.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMcpServer {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
    /// Shown in the Tools popup next to the server's own tools. Optional: the server's tools carry
    /// their own descriptions from `tools/list`.
    #[serde(default)]
    pub description: Option<String>,
    /// Ask for this server's tools to skip the `agent_mode` gate. Defaults to false — the skill
    /// must say so explicitly, and the user consented to the whole file at install.
    #[serde(default)]
    pub bypass_agent_mode: bool,
}

/// The MCP servers declared in an `install_skill` payload's `tools.json`.
fn payload_mcp_servers(files: &[IncomingFile]) -> Vec<SkillMcpServer> {
    let Some(raw) = files
        .iter()
        .find(|f| f.path.replace('\\', "/").eq_ignore_ascii_case("tools.json"))
    else {
        return vec![];
    };
    match serde_json::from_str::<SkillToolsConfig>(&raw.content) {
        Ok(cfg) => cfg
            .tools
            .into_iter()
            .filter_map(|t| match t {
                SkillToolEntry::Mcp(s) => Some(s),
                // Neither spawns anything, so neither triggers the install prompt.
                SkillToolEntry::Prompt(_) | SkillToolEntry::Builtin(_) => None,
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// Whether an `install_skill` payload declares an MCP server.
///
/// The trigger for a permission prompt: a prompt-only skill is inert and installs freely, while
/// one bundling an MCP server can spawn a process.
pub fn payload_bundles_mcp(files: &[IncomingFile]) -> bool {
    !payload_mcp_servers(files).is_empty()
}

/// Human-readable summary of what a payload's servers will run — for the install prompt.
///
/// The command line and the bypass request are the whole decision, so they go in the prompt
/// verbatim. "This skill wants to install" is not a decision; "this skill runs `npx foo`, ungated"
/// is.
pub fn describe_payload_mcp(files: &[IncomingFile]) -> Option<String> {
    let servers = payload_mcp_servers(files);
    if servers.is_empty() {
        return None;
    }
    Some(
        servers
            .iter()
            .map(|s| {
                format!(
                    "{} — runs: {} {}{}",
                    s.name,
                    s.command,
                    s.args.join(" "),
                    if s.bypass_agent_mode {
                        " [requests bypass of the agent-mode permission gate]"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The MCP servers declared by the given enabled skills, as manager-ready `McpServer`s.
///
/// Ids are namespaced `skill:<skill id>:<server name>` so the frontend can tell a skill's server
/// from a hand-configured one and nest its tools under the skill.
pub fn skill_mcp_servers(app: &AppHandle, enabled: &[String]) -> Vec<crate::mcp::types::McpServer> {
    let mut out = Vec::new();
    for (meta, path, _raw) in all_skill_metas(app) {
        if !enabled.contains(&meta.id) {
            continue;
        }
        for entry in read_tools_config(&path).tools {
            let SkillToolEntry::Mcp(s) = entry else {
                continue;
            };
            out.push(crate::mcp::types::McpServer {
                id: skill_mcp_server_id(&meta.id, &s.name),
                name: format!("{} ({})", s.name, meta.name),
                transport: "stdio".into(),
                command: Some(s.command),
                args: Some(s.args),
                env: s.env,
                url: None,
                enabled: true,
                skill_id: Some(meta.id.clone()),
                bypass_agent_mode: s.bypass_agent_mode,
            });
        }
    }
    out
}

pub fn skill_mcp_server_id(skill_id: &str, server: &str) -> String {
    format!("skill:{skill_id}:{server}")
}

/// Wire-name prefix for every skill-declared tool.
///
/// Underscore, not `:`, because Anthropic and OpenAI both constrain tool names to
/// `^[a-zA-Z0-9_-]{1,64}$` — a colon is rejected by the provider, not by us.
pub const SKILL_TOOL_PREFIX: &str = "skill_";

/// The name a skill's tool is offered under: `skill_<skill id>_<tool name>`.
///
/// Namespaced because two skills may each declare a `review`, and a provider sees one flat list.
/// The mapping is not parsed back apart — `run_skill_tool` re-derives every name and matches whole,
/// so an id containing `_` cannot be mis-split.
pub fn skill_tool_name(skill_id: &str, tool: &str) -> String {
    format!("{SKILL_TOOL_PREFIX}{skill_id}_{tool}")
}

pub fn is_skill_tool(name: &str) -> bool {
    name.starts_with(SKILL_TOOL_PREFIX)
}

/// Chars a provider accepts in a tool name. Ids are otherwise free-form (they only have to be a
/// folder name), so a skill that declares tools is held to the stricter rule.
fn is_tool_name_safe(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Substitute a tool body's placeholders from the call's arguments.
///
/// Mirrors `expandCommand` in `src/stores/skills.ts` deliberately — a skill author writes one kind
/// of body and it behaves the same whether the user types it or the model calls it. The one honest
/// difference: a tool's arguments arrive named, so `$1`..`$9` and `$ARGUMENTS` resolve against the
/// params in *declaration order* rather than against typed tokens.
fn expand_skill_tool_body(body: &str, params: &[SkillCommandParam], args: &Value) -> String {
    let value_of = |name: &str| -> String {
        match &args[name] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    };
    let ordered: Vec<String> = params.iter().map(|p| value_of(&p.name)).collect();
    let all = ordered
        .iter()
        .filter(|v| !v.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let re = regex::Regex::new(r"\\?\$(ARGUMENTS|[1-9]|[A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let mut substituted = false;
    let out = re.replace_all(body, |caps: &regex::Captures| {
        let whole = &caps[0];
        // A backslash escapes a placeholder: a body may legitimately talk *about* $ARGUMENTS.
        if let Some(unescaped) = whole.strip_prefix('\\') {
            return unescaped.to_string();
        }
        let key = &caps[1];
        if key == "ARGUMENTS" {
            substituted = true;
            return all.clone();
        }
        if let Ok(i) = key.parse::<usize>() {
            substituted = true;
            return ordered.get(i - 1).cloned().unwrap_or_default();
        }
        if params.iter().any(|p| p.name == key) {
            substituted = true;
            return value_of(key);
        }
        // An unknown $word is left alone — prompt prose is full of dollar-prefixed words that are
        // not placeholders, and only declared params may claim one.
        whole.to_string()
    });

    if substituted || all.is_empty() {
        out.into_owned()
    } else {
        format!("{}\n\n{}", out.trim_end(), all)
    }
}

/// The tool defs for the given enabled skills.
///
/// `enabled` comes from the frontend because that is where the toggle lives (`skill_enabled` in
/// `prefs.json`); the backend owns the schema. A disabled skill contributes nothing — that is the
/// whole point of the feature, and it is what makes the Tools popup honest when it nests these
/// under their skill.
pub fn skill_tool_defs(app: &AppHandle, enabled: &[String]) -> Vec<crate::providers::ToolDef> {
    let mut defs = Vec::new();
    for (meta, path, _raw) in all_skill_metas(app) {
        if !enabled.contains(&meta.id) || !is_tool_name_safe(&meta.id) {
            continue;
        }
        for entry in read_tools_config(&path).tools {
            // MCP entries reach the model through the MCP manager, not here.
            let t = match entry {
                SkillToolEntry::Mcp(_) => continue,
                // A claimed builtin is offered under its own name, with the backend's own
                // description: the skill decides *whether* the model sees it, never what it says
                // it does.
                SkillToolEntry::Builtin(b) => {
                    if let Some(def) = crate::agent::exposable_builtin(&b.name) {
                        defs.push(def);
                    }
                    continue;
                }
                SkillToolEntry::Prompt(t) => t,
            };
            if !is_tool_name_safe(&t.name) {
                continue;
            }
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            for p in &t.params {
                properties.insert(
                    p.name.clone(),
                    json!({
                        "type": "string",
                        "description": p.description.clone().unwrap_or_default()
                    }),
                );
                if p.required {
                    required.push(Value::String(p.name.clone()));
                }
            }
            defs.push(crate::providers::ToolDef {
                name: skill_tool_name(&meta.id, &t.name),
                // The skill is named in the description because the model sees a flat list: two
                // skills with a same-named tool are otherwise told apart only by a mangled id.
                description: format!("{} (from the \"{}\" skill)", t.description, meta.name),
                input_schema: json!({
                    "type": "object",
                    "properties": Value::Object(properties),
                    "required": required
                }),
            });
        }
    }
    defs
}

/// Run a skill tool: read its body, substitute the call's arguments, return the text.
///
/// Nothing executes — the result is a prompt. This is also the delivery path for a skill's
/// knowledge when `read_file` is unavailable: in Off mode an enabled skill's SKILL.md is otherwise
/// unreachable, since the prompt carries only `skill.json` and the model has no filesystem tool.
///
/// Enabled state is not re-checked here. It lives in the frontend, and a tool the offered list
/// withheld can still be called from a stale history replay; expanding a body the model already
/// had access to is not worth threading the enabled set through `execute_tool`'s signature.
pub fn run_skill_tool(app: &AppHandle, name: &str, args: &Value) -> String {
    for (meta, path, _raw) in all_skill_metas(app) {
        for entry in read_tools_config(&path).tools {
            let SkillToolEntry::Prompt(t) = entry else {
                continue;
            };
            if skill_tool_name(&meta.id, &t.name) != name {
                continue;
            }
            for p in t.params.iter().filter(|p| p.required) {
                if args[&p.name]
                    .as_str()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    return format!("Error: '{}' is required by {}.", p.name, name);
                }
            }
            let body = match (&t.file, &t.prompt) {
                (Some(f), _) => {
                    let target = path.join(f.replace('\\', "/"));
                    match std::fs::read_to_string(&target) {
                        Ok(b) => b,
                        Err(e) => {
                            return format!(
                                "Error reading body of {name} at {}: {e}",
                                target.display()
                            )
                        }
                    }
                }
                (None, Some(p)) => p.clone(),
                (None, None) => return format!("Error: {name} has no prompt body."),
            };
            // Same reason commands get this: a body routinely says "read the reference at X", and
            // the model has no idea where the skill lives on disk.
            return format!(
                "[Skill \"{}\" is installed at {} — resolve any relative path below against that \
                 folder, using absolute paths when reading files.]\n\n{}",
                meta.name,
                path.display(),
                expand_skill_tool_body(&body, &t.params, args)
            );
        }
    }
    format!("Error: no enabled skill declares a tool named '{name}'.")
}

pub fn install_skill(app: &AppHandle, id: &str, files: &[IncomingFile]) -> Result<String, String> {
    let dir = skill_dir(app, id)?;
    let replacing = dir.is_dir();
    validate_skill_files(id, files)?;

    for f in files {
        let rel = f.path.replace('\\', "/");
        let target = dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, &f.content).map_err(|e| e.to_string())?;
    }

    // Deliberately not wiping the folder first: a replace must not destroy files the user
    // hand-edited. Undeclared leftovers are inert — only SKILL.md and declared command files load.
    Ok(format!(
        "{} skill '{}' ({} file(s)) at {}. It is live — the user can enable it in Tools → Skills; no restart or import step is needed.",
        if replacing { "Replaced" } else { "Installed" },
        id,
        files.len(),
        dir.display()
    ))
}

/// The prompt body behind one slash command: the contents of `file`, relative to the skill folder.
/// Separate from `read_skill_files` on purpose — invoking a command should not pull every bundled
/// file into the frontend just to use one of them.
#[command]
pub fn read_skill_command(app: AppHandle, id: String, file: String) -> Result<String, String> {
    let dir = skill_dir(&app, &id)?;
    if file.is_empty() || file.contains("..") || file.starts_with('/') || file.starts_with('\\') {
        return Err("invalid file".into());
    }
    let target = dir
        .join(&file)
        .canonicalize()
        .map_err(|_| format!("'{file}' is missing from skill '{id}'"))?;
    if !target.starts_with(&dir.canonicalize().map_err(|e| e.to_string())?) {
        return Err("invalid path".into());
    }
    std::fs::read_to_string(&target).map_err(|e| e.to_string())
}

/// Overwrite one existing file inside a skill folder.
#[command]
pub fn write_skill_file(
    app: AppHandle,
    id: String,
    file: String,
    content: String,
) -> Result<(), String> {
    let dir = skill_dir(&app, &id)?;
    if file.is_empty() || file.contains("..") || file.starts_with('/') {
        return Err("invalid file".into());
    }
    let target = dir.join(&file);
    // Only files the skill already has are writable — this is an editor, not an installer.
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&dir.canonicalize().map_err(|e| e.to_string())?) {
        return Err("invalid path".into());
    }
    std::fs::write(&target, content).map_err(|e| e.to_string())
}
