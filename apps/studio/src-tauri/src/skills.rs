//! Skills: folders of Markdown the assistant follows.
//!
//! Each skill is a folder under the skills directory holding a `SKILL.md` with a short
//! frontmatter (`name`, `description`) and instructions, plus any other files it references
//! (more Markdown, Python scripts). Enabled skills are written into the system prompt; their
//! other files are read on demand with the `read_skill_file` tool.
//!
//! A skill may also provide slash commands in a `commands.json` beside its `SKILL.md`: a list of
//! `{"name", "description", "args", "prompt"}` (see [`SkillCommand`] and `slash`). Commands are
//! not part of the system prompt; they cost context only when someone types one.
//!
//! The folder is watched: a skill created by the assistant, edited in an external editor or
//! dropped in by hand appears in the UI and in the next prompt without a restart.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::bail_msg;
use crate::error::CmdResult;

pub const CHANGED_EVENT: &str = "skills://changed";
pub const ENTRY_FILE: &str = "SKILL.md";
pub const COMMANDS_FILE: &str = "commands.json";
/// Commands one skill may provide.
const MAX_COMMANDS: usize = 20;
const MAX_PROMPT_CHARS: usize = 8_000;
const MAX_FILE_BYTES: u64 = 512 * 1024;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFile {
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Folder name.
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub folder: String,
    pub files: Vec<SkillFile>,
    /// `SKILL.md` without its frontmatter.
    pub body: String,
    pub updated_at: i64,
    /// Who wrote it: `assistant` for skills the model created.
    pub author: Option<String>,
    /// Why the skill cannot be used (for example a missing `SKILL.md`).
    pub problem: Option<String>,
    /// Slash commands from its `commands.json`.
    pub commands: Vec<SkillCommand>,
    /// What is wrong in its `commands.json`; the commands that are fine still work.
    pub commands_problem: Option<String>,
}

/// A slash command a skill provides: typing `/name args` sends `prompt` with the arguments
/// filled in (see `slash`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillCommand {
    /// What follows the slash: lowercase letters, digits, `-` and `_`.
    pub name: String,
    /// One line saying what it does, shown in the command list.
    #[serde(default)]
    pub description: String,
    /// What to type after the name, such as `<symbol> [timeframe]`.
    #[serde(default)]
    pub args: Option<String>,
    /// The message sent: `$ARGUMENTS` is everything typed after the name, `$1` to `$9` its
    /// words. Arguments the prompt does not place are added at its end.
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct SkillsState {
    enabled: BTreeMap<String, bool>,
    /// Bundled skills already copied in, so deleting one does not bring it back.
    seeded: Vec<String>,
}

pub struct SkillRegistry {
    app: AppHandle,
    dir: PathBuf,
    state_file: PathBuf,
    state: RwLock<SkillsState>,
    skills: RwLock<Vec<Skill>>,
    watcher: Mutex<Option<Debouncer<notify::RecommendedWatcher, RecommendedCache>>>,
}

impl SkillRegistry {
    pub fn new(app: AppHandle, dir: PathBuf, state_file: PathBuf) -> Arc<Self> {
        let state = demido_core::fsx::read_json(&state_file).unwrap_or_default();
        Arc::new(Self {
            app,
            dir,
            state_file,
            state: RwLock::new(state),
            skills: RwLock::new(Vec::new()),
            watcher: Mutex::new(None),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Copies bundled skills the person has not seen yet into their skills folder.
    pub fn seed_defaults(&self, bundled: &Path) {
        let Ok(entries) = std::fs::read_dir(bundled) else {
            return;
        };
        let mut state = self.state.write();
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            if !entry.path().join(ENTRY_FILE).is_file() || state.seeded.contains(&id) {
                continue;
            }
            let target = self.dir.join(&id);
            if !target.exists()
                && let Err(e) = copy_dir(&entry.path(), &target)
            {
                tracing::warn!("could not install bundled skill {id}: {e}");
                continue;
            }
            state.seeded.push(id);
        }
        let _ = demido_core::fsx::write_json(&self.state_file, &*state);
    }

    /// Watches the skills folder and rescans on every change.
    pub fn watch(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let debouncer = new_debouncer(Duration::from_millis(350), None, move |result: DebounceEventResult| {
            if result.is_ok()
                && let Some(me) = me.upgrade()
            {
                me.rescan();
            }
        });
        match debouncer {
            Ok(mut d) => {
                if let Err(e) = d.watch(&self.dir, RecursiveMode::Recursive) {
                    tracing::warn!("cannot watch the skills folder: {e}");
                }
                *self.watcher.lock() = Some(d);
            }
            Err(e) => tracing::warn!("cannot watch the skills folder: {e}"),
        }
    }

    /// Re-reads every skill; notifies the UI when anything changed.
    pub fn rescan(&self) {
        let state = self.state.read();
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().to_string();
                if id.starts_with('.') {
                    continue;
                }
                found.push(load_skill(
                    &entry.path(),
                    &id,
                    state.enabled.get(&id).copied().unwrap_or(true),
                ));
            }
        }
        drop(state);
        found.sort_by_key(|s| s.name.to_ascii_lowercase());
        let changed = {
            let mut current = self.skills.write();
            let changed = *current != found;
            *current = found.clone();
            changed
        };
        if changed {
            let _ = self.app.emit(CHANGED_EVENT, found);
        }
    }

    pub fn list(&self) -> Vec<Skill> {
        self.skills.read().clone()
    }

    pub fn get(&self, id: &str) -> Option<Skill> {
        self.skills.read().iter().find(|s| s.id == id).cloned()
    }

    /// Finds a skill by folder name or display name (case-insensitive).
    pub fn find(&self, key: &str) -> Option<Skill> {
        let k = key.trim().to_ascii_lowercase();
        self.skills
            .read()
            .iter()
            .find(|s| s.id.to_ascii_lowercase() == k || s.name.to_ascii_lowercase() == k)
            .cloned()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> CmdResult<()> {
        {
            let mut state = self.state.write();
            state.enabled.insert(id.to_string(), enabled);
            demido_core::fsx::write_json(&self.state_file, &*state)?;
        }
        self.rescan();
        Ok(())
    }

    pub fn delete(&self, id: &str) -> CmdResult<()> {
        let folder = self.folder(id)?;
        std::fs::remove_dir_all(&folder)?;
        {
            let mut state = self.state.write();
            state.enabled.remove(id);
            demido_core::fsx::write_json(&self.state_file, &*state)?;
        }
        self.rescan();
        Ok(())
    }

    fn folder(&self, id: &str) -> CmdResult<PathBuf> {
        if id.is_empty() || id.contains(['/', '\\']) || id == "." || id == ".." {
            bail_msg!("Invalid skill id.");
        }
        let folder = self.dir.join(id);
        if !folder.is_dir() {
            bail_msg!("There is no skill called {id}.");
        }
        Ok(folder)
    }

    /// Resolves a path inside the skills folder. Relative paths are taken from the skill's
    /// folder and may point into another skill (`../other/notes.md`), but never outside.
    pub fn resolve_file(&self, id: &str, rel: &str) -> CmdResult<PathBuf> {
        let base = self.folder(id)?;
        let joined = normalize(&base.join(rel.replace('\\', "/")));
        if !joined.starts_with(normalize(&self.dir)) {
            bail_msg!("That path is outside the skills folder.");
        }
        Ok(joined)
    }

    pub fn read_file(&self, id: &str, rel: &str) -> CmdResult<String> {
        let path = self.resolve_file(id, if rel.trim().is_empty() { ENTRY_FILE } else { rel })?;
        let meta = std::fs::metadata(&path)
            .map_err(|_| crate::error::AppError::msg(format!("{rel} does not exist in skill {id}.")))?;
        if meta.len() > MAX_FILE_BYTES {
            bail_msg!("That file is too large to read ({} KB).", meta.len() / 1024);
        }
        Ok(std::fs::read_to_string(&path)?)
    }

    pub fn write_file(&self, id: &str, rel: &str, content: &str) -> CmdResult<()> {
        let path = self.resolve_file(id, rel)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        demido_core::fsx::write_atomic(&path, content.as_bytes())?;
        self.rescan();
        Ok(())
    }

    /// Creates (or with `overwrite`, replaces) a skill. Returns its id.
    pub fn create(
        &self,
        name: &str,
        description: &str,
        instructions: &str,
        files: &[(String, String)],
        author: Option<&str>,
        overwrite: bool,
    ) -> CmdResult<Skill> {
        let name = name.trim();
        if name.is_empty() {
            bail_msg!("A skill needs a name.");
        }
        let mut id = slug(name);
        if id.is_empty() {
            id = "skill".into();
        }
        let existing = self.dir.join(&id);
        if existing.exists() && !overwrite {
            let mut n = 2;
            while self.dir.join(format!("{id}-{n}")).exists() {
                n += 1;
            }
            id = format!("{id}-{n}");
        }
        let folder = self.dir.join(&id);
        std::fs::create_dir_all(&folder)?;
        let mut md = String::from("---\n");
        md.push_str(&format!("name: {}\n", yaml_value(name)));
        md.push_str(&format!("description: {}\n", yaml_value(description.trim())));
        if let Some(a) = author {
            md.push_str(&format!("author: {a}\n"));
        }
        md.push_str("---\n\n");
        md.push_str(instructions.trim());
        md.push('\n');
        demido_core::fsx::write_atomic(&folder.join(ENTRY_FILE), md.as_bytes())?;
        for (rel, content) in files {
            let rel = rel.trim().trim_start_matches(['/', '\\']);
            if rel.is_empty() || rel.eq_ignore_ascii_case(ENTRY_FILE) {
                continue;
            }
            let path = normalize(&folder.join(rel));
            if !path.starts_with(normalize(&folder)) {
                bail_msg!("The file {rel} would be outside the skill's folder.");
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            demido_core::fsx::write_atomic(&path, content.as_bytes())?;
        }
        {
            let mut state = self.state.write();
            state.enabled.insert(id.clone(), true);
            demido_core::fsx::write_json(&self.state_file, &*state)?;
        }
        self.rescan();
        self.get(&id)
            .ok_or_else(|| crate::error::AppError::msg("The skill was written but could not be read back."))
    }

    /// The Skills section of the system prompt. Full instructions when they fit `budget_chars`,
    /// otherwise names and descriptions with a pointer to `read_skill_file`.
    pub fn prompt_section(&self, budget_chars: usize) -> Option<String> {
        let skills: Vec<Skill> = self
            .list()
            .into_iter()
            .filter(|s| s.enabled && s.problem.is_none())
            .collect();
        if skills.is_empty() {
            return None;
        }
        let full: usize = skills.iter().map(|s| s.body.len() + s.description.len() + 64).sum();
        let mut out = String::from(
            "## Skills\nSkills are procedures the user saved. When a request matches a skill, follow its instructions. Files listed with a skill can be read with read_skill_file.\n",
        );
        for s in &skills {
            out.push_str(&format!("\n### {} (skill id: {})\n{}\n", s.name, s.id, s.description));
            if full <= budget_chars {
                out.push('\n');
                out.push_str(s.body.trim());
                out.push('\n');
            }
            let others = s.other_files();
            if !others.is_empty() {
                out.push_str(&format!("Files: {}\n", others.join(", ")));
            }
        }
        if full > budget_chars {
            out.push_str("\nRead a skill's SKILL.md with read_skill_file before following it.\n");
        }
        Some(out)
    }

    /// Whether [`prompt_section`](Self::prompt_section) holds every skill's instructions at
    /// `budget_chars`.
    pub fn instructions_in_prompt(&self, budget_chars: usize) -> bool {
        let full: usize = self
            .list()
            .iter()
            .filter(|s| s.enabled && s.problem.is_none())
            .map(|s| s.body.len() + s.description.len() + 64)
            .sum();
        full <= budget_chars
    }
}

impl Skill {
    /// Its files the model may read, besides `SKILL.md` and the command list.
    pub fn other_files(&self) -> Vec<&str> {
        self.files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| *p != ENTRY_FILE && *p != COMMANDS_FILE)
            .collect()
    }
}

fn load_skill(folder: &Path, id: &str, enabled: bool) -> Skill {
    let entry = folder.join(ENTRY_FILE);
    let mut files: Vec<SkillFile> = walkdir::WalkDir::new(folder)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(folder).ok()?.to_string_lossy().replace('\\', "/");
            if rel.starts_with('.') || rel.contains("/.") {
                return None;
            }
            Some(SkillFile {
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
                path: rel,
            })
        })
        .collect();
    files.sort_by(|a, b| {
        (a.path != ENTRY_FILE)
            .cmp(&(b.path != ENTRY_FILE))
            .then(a.path.cmp(&b.path))
    });
    let updated_at = walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let raw = std::fs::read_to_string(&entry);
    let (meta, body, problem) = match raw {
        Ok(text) => {
            let (meta, body) = split_frontmatter(&text);
            (meta, body.to_string(), None)
        }
        Err(_) => (BTreeMap::new(), String::new(), Some(format!("{ENTRY_FILE} is missing"))),
    };
    let (commands, commands_problem) = match std::fs::read_to_string(folder.join(COMMANDS_FILE)) {
        Ok(text) => parse_commands(&text),
        Err(_) => (Vec::new(), None),
    };
    Skill {
        id: id.to_string(),
        name: meta
            .get("name")
            .cloned()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| id.replace(['-', '_'], " ")),
        description: meta.get("description").cloned().unwrap_or_else(|| first_line(&body)),
        enabled,
        folder: folder.to_string_lossy().into_owned(),
        files,
        body,
        updated_at,
        author: meta.get("author").cloned(),
        problem,
        commands,
        commands_problem,
    }
}

/// Reads a `commands.json`: a list of commands, or an object with one under `commands`. Bad
/// entries are left out and described in the second value.
pub fn parse_commands(text: &str) -> (Vec<SkillCommand>, Option<String>) {
    let value: serde_json::Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return (Vec::new(), Some(format!("{COMMANDS_FILE} is not valid JSON: {e}"))),
    };
    let list = match value {
        serde_json::Value::Array(list) => list,
        serde_json::Value::Object(mut o) => match o.remove("commands") {
            Some(serde_json::Value::Array(list)) => list,
            _ => return (Vec::new(), Some(format!("{COMMANDS_FILE} needs a \"commands\" list."))),
        },
        _ => {
            return (
                Vec::new(),
                Some(format!("{COMMANDS_FILE} should hold a list of commands.")),
            );
        }
    };
    let mut commands: Vec<SkillCommand> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for (i, entry) in list.into_iter().enumerate() {
        let mut c: SkillCommand = match serde_json::from_value(entry) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("command {}: {e}", i + 1));
                continue;
            }
        };
        c.name = c.name.trim().trim_start_matches('/').to_ascii_lowercase();
        c.description = c.description.trim().to_string();
        c.args = c.args.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        let valid = !c.name.is_empty()
            && c.name.len() <= 32
            && c.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
        if !valid {
            problems.push(format!(
                "command {}: \"{}\" is not a valid name (use letters, digits, - and _)",
                i + 1,
                c.name
            ));
        } else if c.prompt.trim().is_empty() {
            problems.push(format!("/{}: the prompt is empty", c.name));
        } else if c.prompt.chars().count() > MAX_PROMPT_CHARS {
            problems.push(format!(
                "/{}: the prompt is longer than {MAX_PROMPT_CHARS} characters",
                c.name
            ));
        } else if commands.iter().any(|o| o.name == c.name) {
            problems.push(format!("/{} is listed twice", c.name));
        } else if commands.len() == MAX_COMMANDS {
            problems.push(format!("only the first {MAX_COMMANDS} commands are used"));
            break;
        } else {
            commands.push(c);
        }
    }
    (commands, (!problems.is_empty()).then(|| problems.join("; ")))
}

/// Splits `---` YAML frontmatter (flat `key: value` pairs) from the body.
pub fn split_frontmatter(text: &str) -> (BTreeMap<String, String>, &str) {
    let mut meta = BTreeMap::new();
    let trimmed = text.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (meta, trimmed);
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let Some(end) = rest.find("\n---") else {
        return (meta, trimmed);
    };
    let header = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['-', '\r', '\n']);
    let mut current: Option<String> = None;
    for line in header.lines() {
        if let Some((k, v)) = line
            .split_once(':')
            .filter(|(k, _)| !k.starts_with(' ') && !k.trim().is_empty())
        {
            let key = k.trim().to_string();
            let value = v.trim();
            if value == ">" || value == "|" || value.is_empty() {
                meta.insert(key.clone(), String::new());
                current = Some(key);
            } else {
                meta.insert(key, unquote(value));
                current = None;
            }
        } else if let Some(key) = &current {
            let entry = meta.entry(key.clone()).or_default();
            if !entry.is_empty() {
                entry.push(' ');
            }
            entry.push_str(line.trim());
        }
    }
    (meta, body)
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
        v[1..v.len() - 1].replace("\\\"", "\"")
    } else {
        v.to_string()
    }
}

fn yaml_value(v: &str) -> String {
    let flat = v.replace(['\r', '\n'], " ");
    if flat.contains(':') || flat.contains('#') || flat.starts_with(['"', '\'', '-', '>', '|']) {
        format!("\"{}\"", flat.replace('"', "\\\""))
    } else {
        flat
    }
}

fn first_line(body: &str) -> String {
    body.lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Lexically resolves `.` and `..` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_is_parsed_and_stripped() {
        let (meta, body) = split_frontmatter(
            "---\nname: \"EUR: weekly\"\ndescription: >\n  Fetch data\n  and compute.\n---\n# Steps\nDo it.",
        );
        assert_eq!(meta["name"], "EUR: weekly");
        assert_eq!(meta["description"], "Fetch data and compute.");
        assert_eq!(body, "# Steps\nDo it.");
        let (meta, body) = split_frontmatter("no frontmatter here");
        assert!(meta.is_empty());
        assert_eq!(body, "no frontmatter here");
    }

    #[test]
    fn the_bundled_commands_are_valid() {
        let (commands, problem) = parse_commands(include_str!("../../../../skills/market-analysis/commands.json"));
        assert_eq!(problem, None);
        assert!(commands.iter().any(|c| c.name == "analyze"));
    }

    #[test]
    fn command_lists_are_read_leniently() {
        let (commands, problem) = parse_commands(
            r#"{"commands": [
                {"name": "/Analyze", "description": " Analyze a market ", "args": "<symbol>", "prompt": "Analyze $ARGUMENTS."},
                {"name": "bad name", "prompt": "x"},
                {"name": "empty", "prompt": "  "},
                {"description": "no name"},
                {"name": "analyze", "prompt": "again"}
            ]}"#,
        );
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "analyze");
        assert_eq!(commands[0].description, "Analyze a market");
        assert_eq!(commands[0].args.as_deref(), Some("<symbol>"));
        let problem = problem.unwrap();
        assert!(problem.contains("\"bad name\" is not a valid name"), "{problem}");
        assert!(problem.contains("/empty: the prompt is empty"), "{problem}");
        assert!(problem.contains("command 4: missing field `name`"), "{problem}");
        assert!(problem.contains("/analyze is listed twice"), "{problem}");

        let (commands, problem) = parse_commands(r#"[{"name": "go", "prompt": "Go."}]"#);
        assert_eq!((commands.len(), problem), (1, None));
        assert!(parse_commands("{nope").1.unwrap().contains("not valid JSON"));
        assert!(parse_commands(r#"{"other": 1}"#).1.unwrap().contains("needs a \"commands\" list"));
    }

    #[test]
    fn slugs_are_folder_safe() {
        assert_eq!(slug("EURUSD Weekly Volatility!"), "eurusd-weekly-volatility");
        assert_eq!(slug("  --  "), "");
        assert_eq!(yaml_value("a: b"), "\"a: b\"");
    }

    #[test]
    fn normalize_blocks_escapes() {
        let base = Path::new("/skills/a");
        assert_eq!(normalize(&base.join("../b/x.md")), PathBuf::from("/skills/b/x.md"));
        assert!(!normalize(&base.join("../../etc/passwd")).starts_with("/skills"));
    }
}
