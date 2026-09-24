use tauri_plugin_opener::OpenerExt;

use super::St;
use crate::error::{AppError, CmdResult};
use crate::skills::Skill;

#[tauri::command]
pub fn list_skills(state: St<'_>) -> Vec<Skill> {
    state.skills.list()
}

#[tauri::command]
pub fn set_skill_enabled(state: St<'_>, id: String, enabled: bool) -> CmdResult<Vec<Skill>> {
    state.skills.set_enabled(&id, enabled)?;
    Ok(state.skills.list())
}

#[tauri::command]
pub fn delete_skill(state: St<'_>, id: String) -> CmdResult<Vec<Skill>> {
    state.skills.delete(&id)?;
    Ok(state.skills.list())
}

#[tauri::command]
pub fn read_skill_file(state: St<'_>, id: String, path: String) -> CmdResult<String> {
    state.skills.read_file(&id, &path)
}

#[tauri::command]
pub fn write_skill_file(state: St<'_>, id: String, path: String, content: String) -> CmdResult<Skill> {
    state.skills.write_file(&id, &path, &content)?;
    state
        .skills
        .get(&id)
        .ok_or_else(|| AppError::msg("That skill is gone."))
}

#[tauri::command]
pub fn create_skill(
    state: St<'_>,
    name: String,
    description: String,
    instructions: String,
) -> CmdResult<Skill> {
    state
        .skills
        .create(&name, &description, &instructions, &[], None, false)
}

#[tauri::command]
pub fn open_skills_folder(app: tauri::AppHandle, state: St<'_>, id: Option<String>) -> CmdResult<()> {
    let dir = match id {
        Some(id) => state.skills.resolve_file(&id, "")?,
        None => state.skills.dir().to_path_buf(),
    };
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::msg(e.to_string()))
}
