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

/// Tools that manage Demido's own skills folder.
///
/// Deliberately **not** in `builtin_tool_defs`: those are gated on `agent_mode`, and these must
/// not be. They can only touch the app's own data (`skills::skill_dir`), never the user's files,
/// so "Off" — which means no filesystem access — has nothing to protect here. Gating them was
/// worse than useless: a model asked to author a skill in Off mode had the one tool for the job
/// withheld while web and Google stayed on, so it improvised with whatever was left.
///
/// **Not offered on their own.** These reach the model only when a skill claims them with a
/// `{"type": "builtin"}` entry in its `tools.json` — see `exposable_builtin_defs`. `skill-manager`
/// is the skill that does, which is why they appear under it in the Tools popup.
pub fn skills_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "install_skill".into(),
            description: "Install a skill into Demido's own skills folder, where the app loads it from. Writes every file in one call and requires no working folder. Use this to create or replace a skill — do not hand-write skill files with write_file. 'skill.json' and 'SKILL.md' are both required, and every command 'file' declared in skill.json must be included. skill.json shape: {id, name, description, version, commands: [{name, description, file|prompt, params?}]}, where a param is {name, description?, required?, rest?} — bound positionally from the invocation and substituted into the command body as $name (also $1..$9, or $ARGUMENTS for the whole string). A skill may also define tools the *model* calls (a command is typed by the *user*) in a separate optional 'tools.json' file: {tools: [entry, ...]} where each entry has a 'type'. type 'builtin' = {type, name, description?}: surfaces a tool the app already implements, offered under its real name while the skill is enabled — only 'install_skill' and 'delete_skill' may be claimed, and skill-manager already claims both, so do not add these to another skill. type 'prompt' = {type, name, description, file|prompt, params?}: offered as 'skill_<skill id>_<tool name>' while the skill is enabled, and calling it returns its expanded body as text — nothing is executed, so a prompt tool needing a shell or a file must say so in its body and let the model use run_command or read_file. Its params take no 'rest' (a tool's arguments arrive named, not as tokens). type 'mcp' = {type, name, command, args?, env?, description?, bypassAgentMode?}: an MCP server spawned while the skill is enabled, whose own tools are offered under the skill; they obey the agent-mode permission gate unless bypassAgentMode is true. An unknown type is rejected. Names must be unique within tools.json, and a tool name and its skill's id must use only letters, digits, '_' and '-'. Declaring an mcp entry makes install_skill ask the user first, showing the command line — so only add one when the skill genuinely needs a server, and never as a way to run a command.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Skill id; becomes the folder name. Must match the id inside skill.json. No slashes."
                    },
                    "files": {
                        "type": "array",
                        "description": "Every file in the skill, including skill.json and SKILL.md.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string", "description": "Path relative to the skill folder, e.g. 'SKILL.md' or 'commands/go.md'" },
                                "content": { "type": "string", "description": "Full file content" }
                            },
                            "required": ["path", "content"]
                        }
                    }
                },
                "required": ["id", "files"]
            }),
        },
        ToolDef {
            name: "delete_skill".into(),
            description: "Delete a skill from Demido's own skills folder, removing the whole folder and every file in it. Requires no working folder. This cannot be undone and always asks the user first, so only call it when removal is the actual request — to change a skill, install_skill over it instead.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Id of the skill to delete; the folder name. No slashes."
                    }
                },
                "required": ["id"]
            }),
        },
    ]
}

/// The builtins a skill may claim in its `tools.json`, by name.
///
/// The allowlist *is* the safety boundary for `{"type": "builtin"}`: a skill can surface these and
/// nothing else. `run_command`, `write_file` and the rest of `builtin_tool_defs` are absent on
/// purpose — they are gated on `agent_mode`, and a skill (which a model can author) must not be
/// able to hand itself one. Everything here is already offered in every mode, so claiming it grants
/// no permission that did not exist; it only decides *which skill's toggle* controls it.
pub fn exposable_builtin_defs() -> Vec<ToolDef> {
    skills_tool_defs()
}

/// The `ToolDef` a skill's `{"type": "builtin", "name": …}` entry resolves to.
pub fn exposable_builtin(name: &str) -> Option<ToolDef> {
    exposable_builtin_defs().into_iter().find(|t| t.name == name)
}

pub fn web_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search".into(),
            description: "Search the web. Uses hosted Exa/Parallel search providers (optionally boosted by an API key set in Tools > Web Browsing), falling back to a DuckDuckGo scrape if both are unavailable. Returns up to 15 results with title, URL, and snippet.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "page": { "type": "integer", "description": "Page number, DuckDuckGo fallback only (0 = first page, default 0)", "default": 0 }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "web_fetch".into(),
            description: "Fetch content from a web page at the given URL, converted to the requested format. Output is truncated at 20k characters.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Full URL to fetch (must be http or https)" },
                    "format": { "type": "string", "enum": ["markdown", "text", "html"], "description": "Output format for HTML pages (default markdown)", "default": "markdown" }
                },
                "required": ["url"]
            }),
        },
    ]
}

pub fn google_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_emails".into(),
            description: "Search or list emails from the user's connected Gmail account. Returns subject, from, date, and snippet.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Gmail search query (e.g. 'from:boss@company.com is:unread'). Empty for recent emails." },
                    "max_results": { "type": "integer", "description": "Maximum emails to return (1-20, default 10)" }
                }
            }),
        },
        ToolDef {
            name: "read_email".into(),
            description: "Read the full body of an email by its ID (obtained from list_emails).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The email message ID" }
                },
                "required": ["id"]
            }),
        },
        ToolDef {
            name: "list_calendar_events".into(),
            description: "List upcoming calendar events from the user's connected Google Calendar.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "days_ahead": { "type": "integer", "description": "How many days ahead to look (default 7)" },
                    "max_results": { "type": "integer", "description": "Maximum events to return (1-50, default 20)" }
                }
            }),
        },
        ToolDef {
            name: "list_contacts".into(),
            description: "Search or list contacts from the user's connected Google account.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query. Empty to list all contacts." },
                    "max_results": { "type": "integer", "description": "Maximum contacts to return (1-50, default 20)" }
                }
            }),
        },
        ToolDef {
            name: "read_contact".into(),
            description: "Read full details of a contact by their resource ID (obtained from list_contacts).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The contact resource name (e.g. people/c12345678)" }
                },
                "required": ["id"]
            }),
        },
    ]
}

/// The connected-account service a Google tool needs, or `None` for anything that isn't one.
/// Single source of truth: `executor::run_google_tool` resolves the account with it, and the
/// tool-list builder in `commands.rs` uses it to decide whether the tool is offerable at all.
pub fn google_service_for(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "list_emails" | "read_email" => Some("email"),
        "list_calendar_events" => Some("calendar"),
        "list_contacts" | "read_contact" => Some("contacts"),
        _ => None,
    }
}

/// Whether `execute_tool` handles this name — i.e. it is ours, not an MCP server's.
///
/// Skill-declared tools count: they dispatch through the same `match` (its fallback arm), take the
/// same permission path, and must not be mistaken for an MCP tool by the router in `commands.rs`.
pub fn is_builtin(tool_name: &str) -> bool {
    if crate::skills::is_skill_tool(tool_name) {
        return true;
    }
    matches!(
        tool_name,
        "read_file"
            | "write_file"
            | "install_skill"
            | "delete_skill"
            | "edit_file"
            | "list_dir"
            | "run_command"
            | "search_files"
            | "web_search"
            | "web_fetch"
            | "list_emails"
            | "read_email"
            | "list_calendar_events"
            | "list_contacts"
            | "read_contact"
    )
}
