//! Headless CLI for the app binary: `demido-studio skill add <path>`, `demido-studio mcp add ...`.
//!
//! Runs *before* the Tauri builder in `main`, so no window and no `AppHandle` exist here — paths
//! are recomputed with `dirs` to match what `app.path().app_data_dir()` would return.

use crate::mcp::types::McpServer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const IDENTIFIER: &str = "studio.demido.app";

const HELP: &str = "\
Demido Studio CLI

Usage:
  demido-studio                                  launch the app
  demido-studio skill add [--convert] <path>     install a skill folder
  demido-studio skill list                       list installed skills
  demido-studio skill remove <id>                uninstall a skill
  demido-studio mcp add [opts] <name> <target>   add an MCP server
  demido-studio mcp list                         list MCP servers
  demido-studio mcp remove <name>                remove an MCP server

Skill add options:
  --convert   port a Claude Code / OpenCode / other-harness skill: strip the YAML
              frontmatter, inline referenced files into SKILL.md, and report what
              needs a human or model pass afterwards

MCP add options:
  --transport <stdio|sse|http>   default: stdio ('http' is an alias for 'sse')
  --env KEY=VALUE                repeatable
  --disabled                     add without enabling

Examples:
  demido-studio skill add C:\\path\\to\\skill
  demido-studio mcp add --transport http docs https://example.com/mcp
  demido-studio mcp add ctx7 npx -y @upstash/context7-mcp
";

/// Tauri's `app_data_dir()` for this identifier, without an `AppHandle`.
fn app_data_dir() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|d| d.join(IDENTIFIER))
        .ok_or_else(|| "cannot resolve the app data directory".into())
}

fn open_db() -> Result<rusqlite::Connection, String> {
    let dir = app_data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::db::init(&dir.join("demido.db")).map_err(|e| e.to_string())
}

/// Attach to the parent terminal so `println!` is visible: the release binary is built with
/// `windows_subsystem = "windows"` and therefore starts with no console of its own.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
#[cfg(not(windows))]
fn attach_console() {}

/// Handle a CLI invocation. `true` means the process should exit without launching the app.
pub fn try_run() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first().map(String::as_str) else {
        return false;
    };
    // Anything else (Tauri/webview switches, file associations) belongs to the GUI.
    if !matches!(first, "skill" | "mcp" | "help" | "--help" | "-h") {
        return false;
    }

    attach_console();

    let code = match dispatch(&args) {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    match args[0].as_str() {
        "help" | "--help" | "-h" => Ok(HELP.to_string()),
        "skill" => match rest.first().copied() {
            Some("add") => skill_add(&rest[1..]),
            Some("list") | None => skill_list(),
            Some("remove") | Some("rm") => skill_remove(rest.get(1).copied()),
            Some(other) => Err(format!("unknown `skill` subcommand: {other}")),
        },
        "mcp" => match rest.first().copied() {
            Some("add") => mcp_add(&rest[1..]),
            Some("list") | None => mcp_list(),
            Some("remove") | Some("rm") => mcp_remove(rest.get(1).copied()),
            Some(other) => Err(format!("unknown `mcp` subcommand: {other}")),
        },
        other => Err(format!("unknown command: {other}")),
    }
}

// ---------------------------------------------------------------- skills

fn skills_dir() -> Result<PathBuf, String> {
    let dir = app_data_dir()?.join("skills");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Split a `SKILL.md` into its YAML frontmatter keys and the body below it. Ecosystem skills
/// (Claude Code, OpenCode, …) carry frontmatter instead of the `skill.json` that
/// `skills::list_skills` reads, so an install has to translate.
fn split_frontmatter(md: &str) -> (HashMap<String, String>, &str) {
    let mut out = HashMap::new();
    let Some(after) = md.strip_prefix("---") else {
        return (out, md);
    };
    let Some(end) = after.find("\n---") else {
        return (out, md);
    };
    for line in after[..end].lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
        if !v.is_empty() {
            out.insert(k.trim().to_lowercase(), v.to_string());
        }
    }
    // Skip past the closing `---` line.
    let body = after[end + 4..].trim_start_matches('\n');
    (out, body)
}

fn parse_frontmatter(md: &str) -> HashMap<String, String> {
    split_frontmatter(md).0
}

/// Text files in the skill folder that the body actually points at. Demido loads *only* SKILL.md
/// (`skills.rs:54`), so a multi-file skill loses these unless `--convert` inlines them.
///
/// `exempt` holds files backing declared slash commands. Those are loaded on demand at invocation,
/// so inlining them into the always-on SKILL.md would defeat the point of declaring them.
fn referenced_files(body: &str, dir: &Path, exempt: &[String]) -> Vec<String> {
    let re = regex::Regex::new(r"[A-Za-z0-9_./\-]+\.(?:md|txt|json|ya?ml|csv)").unwrap();
    let mut seen = Vec::new();
    for m in re.find_iter(body) {
        let rel = m.as_str().trim_start_matches("./");
        if rel.contains("..") || rel.starts_with('/') {
            continue;
        }
        if rel.eq_ignore_ascii_case("SKILL.md") || rel.eq_ignore_ascii_case("skill.json") {
            continue;
        }
        if exempt.iter().any(|e| e.eq_ignore_ascii_case(rel)) {
            continue;
        }
        if seen.iter().any(|s: &String| s == rel) {
            continue;
        }
        if dir.join(rel).is_file() {
            seen.push(rel.to_string());
        }
    }
    seen
}

const SCRIPT_EXTS: [&str; 7] = ["py", "sh", "ps1", "js", "mjs", "ts", "rb"];

fn has_scripts(dir: &Path) -> bool {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| SCRIPT_EXTS.contains(&x))
        })
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        // `metadata` follows symlinks — the `skills` CLI symlinks its installs.
        let meta = std::fs::metadata(&from).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn skill_add(args: &[&str]) -> Result<String, String> {
    let convert = args.contains(&"--convert");
    let path = args.iter().find(|a| !a.starts_with("--"));
    if let Some(bad) = args
        .iter()
        .find(|a| a.starts_with("--") && **a != "--convert")
    {
        return Err(format!("unknown option: {bad}"));
    }
    let Some(path) = path else {
        return Err("usage: demido-studio skill add [--convert] <path>".into());
    };
    let src = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    if !src.is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    let md_path = src.join("SKILL.md");
    if !md_path.is_file() {
        return Err(format!("{path} has no SKILL.md — not a skill folder"));
    }

    let folder = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or("cannot read the folder name")?;

    // An existing skill.json wins; otherwise synthesize one from the frontmatter so the installed
    // skill is visible to `list_skills`, which requires that file.
    let existing: Option<crate::skills::SkillMeta> =
        std::fs::read_to_string(src.join("skill.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());

    let id = existing.as_ref().map(|m| m.id.clone()).unwrap_or(folder);
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!("invalid skill id: {id}"));
    }

    let dest = skills_dir()?.join(&id);
    let replaced = dest.exists();
    if replaced {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    copy_dir(&src, &dest)?;

    let mut notes: Vec<String> = Vec::new();

    if convert {
        let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;
        let (_, body) = split_frontmatter(&md);
        // The frontmatter is harness metadata, not instructions — Demido pastes SKILL.md straight
        // into the system prompt, so leaving the YAML in would just be noise the model reads.
        let mut converted = body.to_string();

        let command_files: Vec<String> = existing
            .as_ref()
            .map(|m| m.commands.iter().filter_map(|c| c.file.clone()).collect())
            .unwrap_or_default();
        let refs = referenced_files(body, &src, &command_files);
        for rel in &refs {
            let content = std::fs::read_to_string(src.join(rel)).unwrap_or_default();
            converted.push_str(&format!(
                "\n\n---\n\n## Bundled file: {rel}\n\n{}\n",
                content.trim_end()
            ));
        }
        if !refs.is_empty() {
            notes.push(format!(
                "inlined {} referenced file(s): {}",
                refs.len(),
                refs.join(", ")
            ));
        }
        std::fs::write(dest.join("SKILL.md"), &converted).map_err(|e| e.to_string())?;

        // Every enabled skill's full body ships on *every* message (`skills.ts:59`), so size is a
        // standing cost, not a per-use one.
        let kb = converted.len() / 1024;
        if kb >= 16 {
            notes.push(format!(
                "SKILL.md is ~{kb}KB and is sent with every message — consider trimming it"
            ));
        }
        if has_scripts(&src) {
            notes.push(
                "bundled scripts were copied but Demido never runs them automatically".into(),
            );
        }
    }

    // An existing skill.json wins; otherwise synthesize one, since list_skills requires it.
    if existing.is_none() {
        let md = std::fs::read_to_string(&md_path).map_err(|e| e.to_string())?;
        let fm = parse_frontmatter(&md);
        let meta = crate::skills::SkillMeta {
            id: id.clone(),
            name: fm.get("name").cloned().unwrap_or_else(|| id.clone()),
            description: fm.get("description").cloned().unwrap_or_default(),
            version: fm.get("version").cloned().unwrap_or_else(|| "0.0.0".into()),
            commands: vec![],
        };
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        std::fs::write(dest.join("skill.json"), json).map_err(|e| e.to_string())?;
    }

    let verb = if replaced { "replaced" } else { "installed" };
    let mut out = format!("{verb} skill '{id}' -> {}\n", dest.display());
    for n in notes {
        out.push_str(&format!("  note: {n}\n"));
    }
    if convert {
        out.push_str(
            "  mechanical conversion only — enable the 'skill-convert' skill and ask the model\n\
             \x20 to review the result for trigger-style wording that assumes on-demand loading\n",
        );
    }
    Ok(out)
}

fn skill_list() -> Result<String, String> {
    let dir = skills_dir()?;
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(path.join("skill.json")) else {
            rows.push(format!(
                "  {}  (no skill.json — not loaded by the app)",
                entry.file_name().to_string_lossy()
            ));
            continue;
        };
        match serde_json::from_str::<crate::skills::SkillMeta>(&raw) {
            Ok(m) => rows.push(format!("  {}  {} v{}", m.id, m.name, m.version)),
            Err(e) => rows.push(format!(
                "  {}  (invalid skill.json: {e})",
                entry.file_name().to_string_lossy()
            )),
        }
    }
    if rows.is_empty() {
        return Ok(format!("no skills installed in {}\n", dir.display()));
    }
    Ok(format!("{}\n", rows.join("\n")))
}

fn skill_remove(id: Option<&str>) -> Result<String, String> {
    let Some(id) = id else {
        return Err("usage: demido-studio skill remove <id>".into());
    };
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("invalid id".into());
    }
    let dest = skills_dir()?.join(id);
    if !dest.is_dir() {
        return Err(format!("skill '{id}' is not installed"));
    }
    std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    Ok(format!("removed skill '{id}'\n"))
}

// ---------------------------------------------------------------- mcp

struct AddOpts {
    transport: String,
    env: HashMap<String, String>,
    enabled: bool,
    positional: Vec<String>,
}

fn parse_add_opts(args: &[&str]) -> Result<AddOpts, String> {
    let mut o = AddOpts {
        transport: "stdio".into(),
        env: HashMap::new(),
        enabled: true,
        positional: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--transport" | "-t" => {
                let v = args.get(i + 1).ok_or("--transport needs a value")?;
                o.transport = match *v {
                    // The stored vocabulary is stdio | sse (see src/types.ts).
                    "http" | "sse" => "sse".into(),
                    "stdio" => "stdio".into(),
                    other => return Err(format!("unknown transport: {other}")),
                };
                i += 2;
            }
            "--env" | "-e" => {
                let v = args.get(i + 1).ok_or("--env needs KEY=VALUE")?;
                let (k, val) = v.split_once('=').ok_or("--env expects KEY=VALUE")?;
                o.env.insert(k.to_string(), val.to_string());
                i += 2;
            }
            "--disabled" => {
                o.enabled = false;
                i += 1;
            }
            // Everything after the name is the server's own command line.
            other if other.starts_with('-') && o.positional.is_empty() => {
                return Err(format!("unknown option: {other}"))
            }
            other => {
                o.positional.push(other.to_string());
                i += 1;
            }
        }
    }
    Ok(o)
}

fn mcp_add(args: &[&str]) -> Result<String, String> {
    let o = parse_add_opts(args)?;
    let mut pos = o.positional.into_iter();
    let name = pos
        .next()
        .ok_or("usage: demido-studio mcp add [opts] <name> <url|command [args...]>")?;
    let rest: Vec<String> = pos.collect();
    if rest.is_empty() {
        return Err(if o.transport == "sse" {
            "an sse/http server needs a url".into()
        } else {
            "a stdio server needs a command".into()
        });
    }

    let (command, cmd_args, url) = if o.transport == "sse" {
        (None, None, Some(rest[0].clone()))
    } else {
        let args = if rest.len() > 1 {
            Some(rest[1..].to_vec())
        } else {
            None
        };
        (Some(rest[0].clone()), args, None)
    };

    let conn = open_db()?;
    let mut servers = crate::db::mcp_servers::list(&conn).map_err(|e| e.to_string())?;
    if servers.iter().any(|s| s.name == name) {
        return Err(format!("an MCP server named '{name}' already exists"));
    }
    servers.push(McpServer {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
        transport: o.transport.clone(),
        command,
        args: cmd_args,
        env: if o.env.is_empty() { None } else { Some(o.env) },
        url,
        enabled: o.enabled,
        // Hand-configured, like anything else added through settings or this CLI.
        skill_id: None,
        bypass_agent_mode: false,
    });
    crate::db::mcp_servers::save_all(&conn, &servers).map_err(|e| e.to_string())?;

    let mut out = format!("added MCP server '{name}' ({})\n", o.transport);
    if o.transport == "sse" {
        // mcp/mod.rs only spawns stdio servers; an sse row is stored but never connected.
        out.push_str("warning: sse/http servers are stored but not yet connected by the app\n");
    }
    out.push_str("restart Demido Studio (or reopen it) to connect the server\n");
    Ok(out)
}

fn mcp_list() -> Result<String, String> {
    let conn = open_db()?;
    let servers = crate::db::mcp_servers::list(&conn).map_err(|e| e.to_string())?;
    if servers.is_empty() {
        return Ok("no MCP servers configured\n".to_string());
    }
    let rows: Vec<String> = servers
        .iter()
        .map(|s| {
            let target = match s.transport.as_str() {
                "stdio" => {
                    let args = s.args.as_ref().map(|a| a.join(" ")).unwrap_or_default();
                    format!("{} {}", s.command.clone().unwrap_or_default(), args)
                }
                _ => s.url.clone().unwrap_or_default(),
            };
            let state = if s.enabled { "enabled" } else { "disabled" };
            format!(
                "  {}  [{}] {}  ({state})",
                s.name,
                s.transport,
                target.trim()
            )
        })
        .collect();
    Ok(format!("{}\n", rows.join("\n")))
}

fn mcp_remove(name: Option<&str>) -> Result<String, String> {
    let Some(name) = name else {
        return Err("usage: demido-studio mcp remove <name>".into());
    };
    let conn = open_db()?;
    let servers = crate::db::mcp_servers::list(&conn).map_err(|e| e.to_string())?;
    let before = servers.len();
    let kept: Vec<McpServer> = servers.into_iter().filter(|s| s.name != name).collect();
    if kept.len() == before {
        return Err(format!("no MCP server named '{name}'"));
    }
    crate::db::mcp_servers::save_all(&conn, &kept).map_err(|e| e.to_string())?;
    Ok(format!("removed MCP server '{name}'\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_parses_name_and_description() {
        let md = "---\nname: skill-creator\ndescription: \"Create new skills\"\n---\n\n# Body\n";
        let fm = parse_frontmatter(md);
        assert_eq!(fm.get("name").unwrap(), "skill-creator");
        assert_eq!(fm.get("description").unwrap(), "Create new skills");
    }

    #[test]
    fn frontmatter_absent_is_empty() {
        assert!(parse_frontmatter("# Just a heading\n").is_empty());
    }

    #[test]
    fn split_frontmatter_returns_body_without_yaml() {
        let md = "---\nname: x\nallowed-tools: Read, Bash\n---\n\n# Body\n\ntext\n";
        let (fm, body) = split_frontmatter(md);
        assert_eq!(fm.get("name").unwrap(), "x");
        assert_eq!(body, "# Body\n\ntext\n");
        assert!(!body.contains("allowed-tools"));
    }

    #[test]
    fn split_frontmatter_passes_through_when_absent() {
        let md = "# Body only\n";
        let (fm, body) = split_frontmatter(md);
        assert!(fm.is_empty());
        assert_eq!(body, md);
    }

    #[test]
    fn referenced_files_finds_only_existing_and_ignores_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("references")).unwrap();
        std::fs::write(dir.path().join("references/notes.md"), "n").unwrap();
        let body = "See references/notes.md and missing/gone.md and ../../etc/passwd.txt";
        let refs = referenced_files(body, dir.path(), &[]);
        assert_eq!(refs, vec!["references/notes.md"]);
    }

    #[test]
    fn command_files_are_never_inlined() {
        // They load on demand at invocation; inlining would put them in every message.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("commands")).unwrap();
        std::fs::write(dir.path().join("commands/go.md"), "go").unwrap();
        std::fs::write(dir.path().join("notes.md"), "n").unwrap();
        let body = "commands/go.md and notes.md";
        let refs = referenced_files(body, dir.path(), &["commands/go.md".to_string()]);
        assert_eq!(refs, vec!["notes.md"]);
    }

    #[test]
    fn referenced_files_skips_self_and_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "a").unwrap();
        std::fs::write(dir.path().join("SKILL.md"), "s").unwrap();
        let refs = referenced_files("a.md then a.md again, plus SKILL.md", dir.path(), &[]);
        assert_eq!(refs, vec!["a.md"]);
    }

    #[test]
    fn http_is_an_alias_for_sse() {
        let o = parse_add_opts(&["--transport", "http", "docs", "https://x/mcp"]).unwrap();
        assert_eq!(o.transport, "sse");
        assert_eq!(o.positional, vec!["docs", "https://x/mcp"]);
    }

    #[test]
    fn stdio_args_survive_leading_dashes() {
        let o = parse_add_opts(&["ctx7", "npx", "-y", "@upstash/context7-mcp"]).unwrap();
        assert_eq!(o.transport, "stdio");
        assert_eq!(
            o.positional,
            vec!["ctx7", "npx", "-y", "@upstash/context7-mcp"]
        );
    }

    #[test]
    fn env_pairs_parse() {
        let o = parse_add_opts(&["--env", "TOKEN=abc", "s", "cmd"]).unwrap();
        assert_eq!(o.env.get("TOKEN").unwrap(), "abc");
    }
}
