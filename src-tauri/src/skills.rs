use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{command, AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillCommand {
    pub name: String,
    pub description: String,
    pub file: Option<String>,
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
    pub content: String,
}

pub fn skills_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("skills")
}

#[command]
pub fn list_skills(app: AppHandle) -> Vec<Skill> {
    let dir = skills_dir(&app);
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
            let content = std::fs::read_to_string(path.join("SKILL.md")).unwrap_or_default();
            Some(Skill {
                id: meta.id,
                name: meta.name,
                description: meta.description,
                version: meta.version,
                commands: meta.commands,
                content,
            })
        })
        .collect()
}

#[command]
pub fn delete_skill(app: AppHandle, id: String) -> Result<(), String> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("invalid id".into());
    }
    let base = skills_dir(&app);
    let path = base.join(&id);
    if !path.exists() {
        return Ok(());
    }
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    let target = path.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&base) {
        return Err("invalid path".into());
    }
    std::fs::remove_dir_all(&target).map_err(|e| e.to_string())
}
