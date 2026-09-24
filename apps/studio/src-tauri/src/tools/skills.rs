//! Skill authoring tools: the assistant saving a procedure for next time.

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
    let overwrite = args["overwrite"].as_bool().unwrap_or(false);
    let skill = ctx
        .state
        .skills
        .create(name, description, instructions, &files, Some("assistant"), overwrite)
        .map_err(|e| e.to_string())?;
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
