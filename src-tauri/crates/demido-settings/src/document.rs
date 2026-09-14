//! The stored shape: four tiers, whether or not two of them are live.
//!
//! `settings.json`, one per profile, and it carries **global, model, character
//! and chat** from the first commit. Two of those are empty in v0.1 and the
//! file has the room anyway, because the alternative is a migration of every
//! profile on the day the character system lands, and
//! [#32](https://github.com/elpideus/demido-studio/issues/32) named exactly
//! that as the way a slice forecloses characters.
//!
//! A tier holds raw JSON here rather than validated [`crate::Values`]. That is
//! deliberate: the file is what a person can open, and a value this build
//! refuses is kept in it and reported rather than dropped
//! ([`crate::Values::from_json`]). Validation happens on the way into the
//! ladder, once, in [`crate::Settings`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::stack::Tier;

/// The shape of a written settings file.
///
/// Bumped when this shape stops being able to read what the previous one wrote.
/// Unlike the desk's layout, a file of the wrong generation is **not**
/// discarded: settings are typed by a person, and the store moves the file
/// aside rather than deleting somebody's work.
pub const GENERATION: u32 = 1;

/// One tier's stored values, as they sit in the file.
pub type Stored = Map<String, Value>;

/// Which tier, and whose.
///
/// [`Scope::Global`] has no subject because there is one of it. The other three
/// name the model, character or chat whose values these are, which is what lets
/// one file hold every chat's overrides without a file per chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Model(String),
    Character(String),
    Chat(String),
}

impl Scope {
    /// A chat's own tier.
    pub fn chat(id: impl Into<String>) -> Self {
        Scope::Chat(id.into())
    }

    #[must_use]
    pub fn tier(&self) -> Tier {
        match self {
            Scope::Global => Tier::Global,
            Scope::Model(_) => Tier::Model,
            Scope::Character(_) => Tier::Character,
            Scope::Chat(_) => Tier::Chat,
        }
    }

    /// Who this tier's values belong to, or nothing for the global one.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        match self {
            Scope::Global => None,
            Scope::Model(id) | Scope::Character(id) | Scope::Chat(id) => Some(id),
        }
    }
}

/// Which scopes one resolution walks, lowest tier first.
///
/// A conversation is `global` and `chat` in v0.1. The model and character tiers
/// are absent from the ladder rather than empty in it, because nothing yet
/// names a model or a character to resolve against, and a ladder that carried a
/// subject nobody chose would resolve against values nobody set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ladder {
    scopes: Vec<Scope>,
}

impl Ladder {
    /// The global tier alone: what the main settings window edits.
    #[must_use]
    pub fn global() -> Self {
        Self {
            scopes: vec![Scope::Global],
        }
    }

    /// The ladder one conversation resolves through: global, then this chat.
    pub fn for_chat(id: impl Into<String>) -> Self {
        Self {
            scopes: vec![Scope::Global, Scope::chat(id)],
        }
    }

    /// The scopes, lowest tier first.
    #[must_use]
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
}

/// Every tier's values, as one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub generation: u32,
    /// What the user set for everything.
    #[serde(default)]
    pub global: Stored,
    /// Per model, per character, per chat. Empty maps in v0.1 for the first
    /// two, and the file says so rather than saying nothing.
    #[serde(default)]
    pub models: BTreeMap<String, Stored>,
    #[serde(default)]
    pub characters: BTreeMap<String, Stored>,
    #[serde(default)]
    pub chats: BTreeMap<String, Stored>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            generation: GENERATION,
            global: Stored::new(),
            models: BTreeMap::new(),
            characters: BTreeMap::new(),
            chats: BTreeMap::new(),
        }
    }
}

impl Document {
    /// What one scope holds, if anything does.
    #[must_use]
    pub fn values(&self, scope: &Scope) -> Option<&Stored> {
        match scope {
            Scope::Global => Some(&self.global),
            Scope::Model(id) => self.models.get(id),
            Scope::Character(id) => self.characters.get(id),
            Scope::Chat(id) => self.chats.get(id),
        }
    }

    /// What one scope holds, ready to be changed. Creates the entry.
    pub fn values_mut(&mut self, scope: &Scope) -> &mut Stored {
        match scope {
            Scope::Global => &mut self.global,
            Scope::Model(id) => self.models.entry(id.clone()).or_default(),
            Scope::Character(id) => self.characters.entry(id.clone()).or_default(),
            Scope::Chat(id) => self.chats.entry(id.clone()).or_default(),
        }
    }

    /// Drop a subject that has nothing left to say.
    ///
    /// A chat that overrode one value and then reverted it leaves an empty
    /// object behind, and a file that accumulates one of those per conversation
    /// is a file nobody can read. The global tier is never removed: there is
    /// one of it, and an empty object there is the ordinary state.
    pub fn prune(&mut self, scope: &Scope) {
        let (map, id) = match scope {
            Scope::Global => return,
            Scope::Model(id) => (&mut self.models, id),
            Scope::Character(id) => (&mut self.characters, id),
            Scope::Chat(id) => (&mut self.chats, id),
        };
        if map.get(id).is_some_and(Stored::is_empty) {
            map.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use serde_json::json;

    use super::*;

    /// The acceptance criterion, at the level of the file: two tiers are live
    /// and four are stored.
    #[test]
    fn the_stored_shape_carries_all_four_tiers() {
        let json = serde_json::to_value(Document::default()).expect("serialised");
        for tier in ["global", "models", "characters", "chats"] {
            assert!(json.get(tier).is_some(), "{tier} is missing from {json}");
        }
    }

    #[test]
    fn a_file_written_by_an_older_build_still_reads() {
        // Only the generation, which is what a first version might have held.
        let document: Document =
            serde_json::from_str(r#"{"generation": 1}"#).expect("read an older file");
        assert_eq!(document, Document::default());
    }

    #[test]
    fn a_chat_with_nothing_left_to_say_is_not_kept() {
        let mut document = Document::default();
        document
            .values_mut(&Scope::chat("session"))
            .insert("conversation.temperature".into(), json!(0.2));
        assert!(document.chats.contains_key("session"));

        document.values_mut(&Scope::chat("session")).clear();
        document.prune(&Scope::chat("session"));
        assert!(document.chats.is_empty(), "an empty chat is not a chat");
    }

    #[test]
    fn a_ladder_for_a_chat_is_global_then_that_chat() {
        let ladder = Ladder::for_chat("session");
        assert_eq!(
            ladder.scopes(),
            [Scope::Global, Scope::Chat("session".into())]
        );
    }
}
