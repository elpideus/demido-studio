//! Characters (DECISIONS E): a name, a few words of what it is, a model, how warmly it picks its words, and the
//! person's own instructions for it. A session started with one, or set to one, is told its instructions after
//! Demido's own, word for word, as the person's (E2), and answers with its model and temperature.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::Db;
use crate::error::{Error, Result};
use crate::now_ms;

/// The longest name, in characters. A name is a label in a list and in a session's header.
pub const LONGEST_NAME: usize = 60;
/// The longest instructions, in characters: a page of text, which a small model still follows.
pub const LONGEST_INSTRUCTIONS: usize = 8_000;

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Character {
    pub id: String,
    pub name: String,
    /// What it is, in a few words: "Reads long documents and answers with sources."
    pub about: String,
    /// The model its sessions start with, by its name. `null`: the one a new session would have anyway.
    pub model: Option<String>,
    /// Whether its model thinks first: `off`, `on`, or one of the model's own levels. `null`: as the model does.
    pub thinking: Option<String>,
    /// How freely its model picks its words, 0 to 2. `null`: the model's own settings.
    pub temperature: Option<f64>,
    /// The person's own instructions, given to the model as they wrote them.
    pub instructions: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A character to make, or what a character becomes.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct NewCharacter {
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub instructions: String,
}

impl NewCharacter {
    /// What is wrong with it, in words the window shows.
    fn check(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("A character needs a name.".into()));
        }
        if name.chars().count() > LONGEST_NAME {
            return Err(Error::Invalid(format!("A name is {LONGEST_NAME} characters at most.")));
        }
        if self.instructions.chars().count() > LONGEST_INSTRUCTIONS {
            return Err(Error::Invalid(format!("Instructions are {LONGEST_INSTRUCTIONS} characters at most: a small model follows a short page better than a long one.")));
        }
        if self.temperature.is_some_and(|t| !t.is_finite() || !(0.0..=2.0).contains(&t)) {
            return Err(Error::Invalid("A temperature is from 0 to 2.".into()));
        }
        if self.thinking.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err(Error::Invalid("Thinking is off, on, or one of the model's levels.".into()));
        }
        Ok(())
    }
}

const COLUMNS: &str = "id, name, about, model, thinking, temperature, instructions, created_at, updated_at";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Character> {
    Ok(Character {
        id: r.get(0)?,
        name: r.get(1)?,
        about: r.get(2)?,
        model: r.get(3)?,
        thinking: r.get(4)?,
        temperature: r.get(5)?,
        instructions: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub struct Characters {
    db: Arc<Db>,
}

impl Characters {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Every character, by name.
    pub fn list(&self) -> Result<Vec<Character>> {
        self.db.with(|c| {
            let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM characters ORDER BY name COLLATE NOCASE"))?;
            q.query_map([], from_row)?.collect()
        })
    }

    pub fn get(&self, id: &str) -> Result<Character> {
        self.db
            .with(|c| {
                c.query_row(&format!("SELECT {COLUMNS} FROM characters WHERE id = ?1"), [id], from_row).map(Some).or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })
            })?
            .ok_or_else(|| Error::NotFound(format!("There is no character \"{id}\".")))
    }

    pub fn create(&self, new: &NewCharacter) -> Result<Character> {
        new.check()?;
        let (id, now) = (uuid::Uuid::now_v7().to_string(), now_ms());
        self.db.with(|c| {
            c.execute(
                &format!("INSERT INTO characters ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)"),
                rusqlite::params![id, new.name.trim(), new.about.trim(), new.model, new.thinking, new.temperature, new.instructions.trim(), now],
            )
        })?;
        self.get(&id)
    }

    /// Changes a character. Sessions set to it are told its new instructions from their next answer on.
    pub fn change(&self, id: &str, new: &NewCharacter) -> Result<Character> {
        new.check()?;
        self.get(id)?;
        self.db.with(|c| {
            c.execute(
                "UPDATE characters SET name = ?2, about = ?3, model = ?4, thinking = ?5, temperature = ?6, instructions = ?7, updated_at = ?8 WHERE id = ?1",
                rusqlite::params![id, new.name.trim(), new.about.trim(), new.model, new.thinking, new.temperature, new.instructions.trim(), now_ms()],
            )
        })?;
        self.get(id)
    }

    /// Forgets a character. Its sessions stay, set to none: what was said in them is theirs.
    pub fn delete(&self, id: &str) -> Result<bool> {
        Ok(self.db.with(|c| c.execute("DELETE FROM characters WHERE id = ?1", [id]))? > 0)
    }

    /// What a session set to this character is told, after Demido's own words (E2).
    pub fn told(character: &Character) -> String {
        let about = character.about.trim().trim_end_matches('.');
        let about = if about.is_empty() { String::new() } else { format!(": {about}") };
        if character.instructions.trim().is_empty() {
            return format!("In this session you are \"{}\"{about}.", character.name);
        }
        format!("In this session you are \"{}\"{about}. The person wrote these instructions for it; follow them as theirs:\n{}", character.name, character.instructions.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_is_kept_changed_and_forgotten_and_what_is_wrong_is_said() {
        let characters = Characters::new(Arc::new(Db::memory().unwrap()));
        let made = characters
            .create(&NewCharacter {
                name: " Mira ".into(),
                about: "Reads long documents.".into(),
                model: Some("small".into()),
                temperature: Some(0.3),
                instructions: "Answer with sources.".into(),
                ..NewCharacter::default()
            })
            .unwrap();
        assert_eq!((made.name.as_str(), made.model.as_deref(), made.temperature), ("Mira", Some("small"), Some(0.3)));
        assert_eq!(Characters::told(&made), "In this session you are \"Mira\": Reads long documents. The person wrote these instructions for it; follow them as theirs:\nAnswer with sources.");
        let changed = characters.change(&made.id, &NewCharacter { name: "Mira".into(), instructions: "Be brief.".into(), ..NewCharacter::default() }).unwrap();
        assert_eq!((changed.instructions.as_str(), changed.model, changed.temperature), ("Be brief.", None, None));
        assert_eq!(characters.list().unwrap().len(), 1);
        for (wrong, says) in [
            (NewCharacter { name: "  ".into(), ..NewCharacter::default() }, "needs a name"),
            (NewCharacter { name: "x".repeat(61), ..NewCharacter::default() }, "60 characters"),
            (NewCharacter { name: "Hot".into(), temperature: Some(3.0), ..NewCharacter::default() }, "from 0 to 2"),
            (NewCharacter { name: "Long".into(), instructions: "x".repeat(8_001), ..NewCharacter::default() }, "8000 characters"),
        ] {
            let e = characters.create(&wrong).unwrap_err().to_string();
            assert!(e.contains(says), "{e}");
        }
        assert!(characters.delete(&made.id).unwrap());
        assert!(characters.get(&made.id).is_err() && !characters.delete(&made.id).unwrap());
    }
}
