//! Skill authoring tools: the assistant saving a procedure for next time.

use std::path::Path;

use serde_json::{Value, json};

use super::{ToolContext, ToolOutput, arg_str, clip, require_str};

pub fn create_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Short title, e.g. \"EURUSD weekly volatility\""},
            "description": {"type": "string", "description": "One sentence saying when to use the skill"},
            "instructions": {"type": "string", "description": "Markdown instructions: numbered steps, the tools and parameters to use, how to present the result"},
            "files": {
                "type": "array",
                "description": "Extra files stored with the skill, e.g. a Python script or reference notes",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File name, e.g. analysis.py or notes.md"},
                        "content": {"type": "string"}
                    },
                    "required": ["path", "content"]
                }
            },
            "include_files": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Workspace files to copy into the skill, e.g. a Python script you wrote and ran (analysis.py)"
            },
            "overwrite": {"type": "boolean", "description": "Replace an existing skill with the same name"}
        },
        "required": ["name", "description", "instructions"]
    })
}

pub fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "skill": {"type": "string", "description": "Skill id or name"},
            "path": {"type": "string", "description": "File inside the skill (default SKILL.md)"}
        },
        "required": ["skill"]
    })
}

pub fn create(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let name = require_str(args, "name")?;
    let description = arg_str(args, "description").unwrap_or(name);
    let instructions = require_str(args, "instructions")?;
    let files: Vec<(String, String)> = args["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    Some((
                        f["path"].as_str()?.to_string(),
                        f["content"].as_str().unwrap_or_default().to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut files = files;
    let overwrite = args["overwrite"].as_bool().unwrap_or(false);

    // Scripts and notes the assistant wrote in the workspace belong with the skill, or the skill
    // breaks in any other chat. Copy the ones asked for and the ones the instructions name.
    let mut wanted: Vec<String> = args["include_files"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    wanted.extend(referenced_workspace_files(&ctx.workspace, instructions));
    let mut copied = Vec::new();
    for rel in wanted {
        if files.iter().any(|(p, _)| p == &rel || p.ends_with(&format!("/{rel}"))) {
            continue;
        }
        let Ok(path) = super::workspace_path(&ctx.workspace, &rel) else { continue };
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(rel.clone());
        if files.iter().any(|(p, _)| p == &name) {
            continue;
        }
        files.push((name.clone(), content));
        copied.push(name);
    }
    let mut instructions = instructions.to_string();
    if !copied.is_empty() {
        let id = crate::skills::slug(name);
        instructions.push_str("\n\n## Files\n");
        for f in &copied {
            if f.ends_with(".py") {
                instructions.push_str(&format!(
                    "- `{f}`: run it with run_python and file \"skill:{id}/{f}\" (pass inputs such as the CSV path in args).\n"
                ));
            } else {
                instructions.push_str(&format!("- `{f}`: read it with read_skill_file.\n"));
            }
        }
    }
    let mut skill = ctx
        .state
        .skills
        .create(name, description, &instructions, &files, Some("assistant"), overwrite)
        .map_err(|e| e.to_string())?;

    // Models write `skill:<id>/<file>` before they know the id; point those at the real one.
    let file_names: Vec<String> = skill
        .files
        .iter()
        .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path).to_string())
        .collect();
    if let Ok(text) = ctx.state.skills.read_file(&skill.id, crate::skills::ENTRY_FILE) {
        let fixed = fix_skill_references(&text, &skill.id, &file_names);
        if fixed != text {
            ctx.state
                .skills
                .write_file(&skill.id, crate::skills::ENTRY_FILE, &fixed)
                .map_err(|e| e.to_string())?;
            if let Some(updated) = ctx.state.skills.get(&skill.id) {
                skill = updated;
            }
        }
    }
    let file_list: Vec<String> = skill.files.iter().map(|f| f.path.clone()).collect();
    Ok(ToolOutput::ok(
        json!({
            "created": skill.id,
            "name": skill.name,
            "files": file_list,
            "enabled": true,
            "note": "The skill is saved and enabled; it is part of future conversations. Tell the user its name.",
        })
        .to_string(),
        json!({"kind": "skill", "id": skill.id, "name": skill.name, "description": skill.description, "files": file_list, "folder": skill.folder}),
    ))
}

pub fn read(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let key = require_str(args, "skill")?;
    let skill = ctx
        .state
        .skills
        .find(key)
        .ok_or_else(|| format!("There is no skill called {key}."))?;
    let path = arg_str(args, "path").unwrap_or(crate::skills::ENTRY_FILE);
    let text = ctx
        .state
        .skills
        .read_file(&skill.id, path)
        .map_err(|e| e.to_string())?;
    Ok(ToolOutput::ok(
        clip(&text, 40_000),
        json!({"kind": "skillFile", "id": skill.id, "name": skill.name, "path": path}),
    ))
}

/// Rewrites `skill:<anything>/<file>` to `skill:<id>/<file>` for files this skill has.
fn fix_skill_references(text: &str, id: &str, files: &[String]) -> String {
    let re = regex::Regex::new(r"skill:([A-Za-z0-9_.\-]+)/([A-Za-z0-9_.\-/]+)").expect("valid regex");
    re.replace_all(text, |caps: &regex::Captures<'_>| {
        let file = &caps[2];
        let name = file.rsplit('/').next().unwrap_or(file);
        if &caps[1] != id && files.iter().any(|f| f == name) {
            format!("skill:{id}/{name}")
        } else {
            caps[0].to_string()
        }
    })
    .into_owned()
}

/// Workspace scripts and notes whose file names appear in the instructions.
fn referenced_workspace_files(workspace: &Path, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(workspace)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let Some(name) = entry.file_name().to_str() else { continue };
        let lower = name.to_ascii_lowercase();
        let reusable = [".py", ".md", ".txt", ".json", ".yaml", ".yml"].iter().any(|ext| lower.ends_with(ext));
        if !reusable || !text.contains(name) {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(workspace) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !rel.starts_with(".demido") {
                found.push(rel);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_references_are_pointed_at_the_real_id() {
        let text = "Run `skill:sma_cross/sma_cross.py` then skill:other/notes.md and skill:sma/unknown.py";
        let fixed = fix_skill_references(text, "sma-crossover", &["sma_cross.py".into(), "SKILL.md".into()]);
        assert_eq!(
            fixed,
            "Run `skill:sma-crossover/sma_cross.py` then skill:other/notes.md and skill:sma/unknown.py"
        );
    }

    #[test]
    fn workspace_files_named_in_instructions_are_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sma_cross.py"), "print(1)").unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::write(dir.path().join("data/prices.csv"), "a,b").unwrap();
        let found = referenced_workspace_files(dir.path(), "run sma_cross.py on data/prices.csv");
        assert_eq!(found, vec!["sma_cross.py".to_string()]);
    }
}
