//! The ladder, held over a store.
//!
//! One type, [`Settings`], and it is the only thing in the workspace that
//! assembles a [`Stack`]. Everything else asks it a question: a turn asks what
//! is in force ([`Settings::resolve`]), and a settings page asks what to draw
//! ([`Settings::view`]).
//!
//! **The document is kept in memory and written whole.** Settings are read once
//! per turn and written when a person changes one, so the file is not on the
//! hot path either way, and a write that replaces the document is the only kind
//! that can express a value being cleared.

use std::sync::Mutex;

use serde_json::Value;

use crate::document::{Document, Ladder, Scope};
use crate::schema::Invalid;
use crate::stack::{Resolved, Row, Stack, Tier, Values};
use crate::store::Store;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The value is not one this setting takes. Carried back to whoever typed
    /// it: a settings page that quietly reverted would be indistinguishable
    /// from one that never saved.
    #[error("{id}: {reason}")]
    Refused { id: String, reason: Invalid },

    #[error(transparent)]
    Store(#[from] crate::store::Error),
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Store(store) => store.into(),
            refused @ Error::Refused { .. } => {
                demido_core::Error::invalid("a setting", refused.to_string())
            }
        }
    }
}

/// Every tier's values, and the one place they are collapsed.
pub struct Settings {
    /// **This is the wiring line's cargo.** The composition root names the
    /// implementation ([`crate::Files`]); nothing here knows which one it has.
    store: Box<dyn Store>,
    document: Mutex<Document>,
}

impl Settings {
    /// Open the settings a store holds.
    ///
    /// Never fails. A document that cannot be read is reported to the developer
    /// channel and the defaults are used, because startup never blocks
    /// (`AGENTS.md`) and a settings file is not a reason a window does not
    /// open. Nothing is lost by that: a store that keeps files moves an
    /// unreadable one aside rather than letting the next write land on top of
    /// it ([`crate::Files`]).
    pub fn open(store: impl Store + 'static) -> Self {
        let document = match store.read() {
            Ok(document) => document,
            Err(error) => {
                tracing::warn!(%error, "the settings could not be read; using the defaults");
                Document::default()
            }
        };
        Self {
            store: Box::new(store),
            document: Mutex::new(document),
        }
    }

    /// What is in force for this ladder, and which tier put it there.
    ///
    /// The one call a turn makes. Total: every setting in the schema has an
    /// answer here whether or not anyone has an opinion about it.
    pub fn resolve(&self, ladder: &Ladder) -> Resolved {
        self.stack(ladder).resolve()
    }

    /// What a settings page draws, editing the **top** tier of this ladder.
    ///
    /// The pairing is deliberate rather than an argument: the main settings
    /// window's ladder is the global tier alone, so its rows have no resolution
    /// to read and no provenance chip to draw (`design/windows.md`), and a
    /// chat's ladder ends at that chat, so its rows are either following global
    /// or overridden. A page cannot edit a tier its own ladder does not end at.
    pub fn view(&self, ladder: &Ladder) -> Vec<Row> {
        let editing = ladder
            .scopes()
            .last()
            .map_or(Tier::Global, |scope| scope.tier());
        self.stack(ladder).view(editing)
    }

    /// Set one value on one tier, or say why it will not be taken.
    ///
    /// Written through immediately. A settings change is a deliberate act
    /// rather than the residue of a gesture, so there is nothing to debounce
    /// here the way the desk's layout has (`demido-shell`).
    pub fn set(&self, scope: &Scope, id: &str, value: &Value) -> Result<()> {
        let accepted = crate::schema::setting(id)
            .ok_or(Invalid::Unknown)
            .and_then(|setting| setting.accept(value))
            .map_err(|reason| Error::Refused {
                id: id.to_owned(),
                reason,
            })?;

        self.change(|document| {
            document
                .values_mut(scope)
                .insert(id.to_owned(), accepted.clone());
        })
    }

    /// Forget one tier's opinion. What a settings page calls revert.
    pub fn clear(&self, scope: &Scope, id: &str) -> Result<()> {
        self.change(|document| {
            document.values_mut(scope).remove(id);
            document.prune(scope);
        })
    }

    /// The tiers of this ladder, validated, in precedence order.
    fn stack(&self, ladder: &Ladder) -> Stack {
        let document = self
            .document
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        let mut stack = Stack::new();

        for scope in ladder.scopes() {
            let Some(stored) = document.values(scope) else {
                continue;
            };
            let (values, rejected) = Values::from_json(stored);
            for entry in rejected {
                // Reported rather than dropped, and reported once per read
                // rather than repaired: the value stays in the file, because
                // the build that wrote it may know something this one does not.
                tracing::debug!(
                    tier = scope.tier().slug(),
                    subject = scope.subject().unwrap_or_default(),
                    %entry,
                    "a stored setting is not one this build can use"
                );
            }
            stack.insert(scope.tier(), values);
        }

        stack
    }

    /// Change the document and write it, or leave it exactly as it was.
    ///
    /// The in-memory copy is only replaced once the store has taken it, so a
    /// disk that refuses a write leaves the window showing what is really
    /// saved rather than what was typed.
    fn change(&self, act: impl FnOnce(&mut Document)) -> Result<()> {
        let mut held = self
            .document
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        let mut changed = held.clone();
        act(&mut changed);
        self.store.write(&changed)?;
        *held = changed;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use serde_json::json;

    use super::*;
    use crate::memory::Memory;
    use crate::schema::id;
    use crate::stack::Origin;

    fn settings() -> Settings {
        Settings::open(Memory::new())
    }

    #[test]
    fn nothing_set_anywhere_is_still_a_whole_answer() {
        let resolved = settings().resolve(&Ladder::for_chat("session"));
        assert_eq!(resolved.temperature(), Some(0.7));
        assert_eq!(resolved.context_length(), 4096);
        assert_eq!(resolved.system_prompt(), "");
    }

    /// The acceptance criterion: a global change applies to new chats.
    #[test]
    fn a_global_change_applies_to_a_chat_that_did_not_exist_when_it_was_made() {
        let settings = settings();
        settings
            .set(&Scope::Global, id::TEMPERATURE, &json!(0.2))
            .expect("set");

        for chat in ["one", "made-later", "made-later-still"] {
            let resolved = settings.resolve(&Ladder::for_chat(chat));
            assert_eq!(resolved.temperature(), Some(0.2), "{chat}");
            assert_eq!(
                resolved.origin(id::TEMPERATURE),
                Origin::Tier(Tier::Global),
                "{chat}"
            );
        }
    }

    /// The other half: a per-chat change overrides the global value for that
    /// chat alone and no other.
    #[test]
    fn a_chat_override_reaches_that_chat_and_no_other() {
        let settings = settings();
        settings
            .set(&Scope::Global, id::TEMPERATURE, &json!(0.2))
            .expect("set");
        settings
            .set(&Scope::chat("loud"), id::TEMPERATURE, &json!(1.4))
            .expect("set");

        assert_eq!(
            settings.resolve(&Ladder::for_chat("loud")).temperature(),
            Some(1.4)
        );
        assert_eq!(
            settings.resolve(&Ladder::for_chat("quiet")).temperature(),
            Some(0.2),
            "another chat is untouched by an override made in one"
        );
    }

    /// The system prompt is a ladder value, which means a chat can have its own
    /// without editing the one every other chat uses.
    #[test]
    fn the_system_prompt_is_a_ladder_value_like_any_other() {
        let settings = settings();
        settings
            .set(&Scope::Global, id::SYSTEM_PROMPT, &json!("Be terse."))
            .expect("set");
        settings
            .set(&Scope::chat("pirate"), id::SYSTEM_PROMPT, &json!("Arr."))
            .expect("set");

        assert_eq!(
            settings
                .resolve(&Ladder::for_chat("pirate"))
                .system_prompt(),
            "Arr."
        );
        assert_eq!(
            settings.resolve(&Ladder::for_chat("other")).system_prompt(),
            "Be terse."
        );
    }

    #[test]
    fn reverting_a_chat_falls_back_to_global_rather_than_to_the_default() {
        let settings = settings();
        settings
            .set(&Scope::Global, id::TEMPERATURE, &json!(0.2))
            .expect("set");
        settings
            .set(&Scope::chat("one"), id::TEMPERATURE, &json!(1.4))
            .expect("set");
        settings
            .clear(&Scope::chat("one"), id::TEMPERATURE)
            .expect("cleared");

        assert_eq!(
            settings.resolve(&Ladder::for_chat("one")).temperature(),
            Some(0.2)
        );
    }

    #[test]
    fn a_refused_value_is_reported_and_changes_nothing() {
        let settings = settings();
        let error = settings
            .set(&Scope::Global, id::TEMPERATURE, &json!(9.0))
            .expect_err("9 is not a temperature");
        assert!(matches!(error, Error::Refused { .. }), "{error}");
        assert_eq!(
            settings.resolve(&Ladder::global()).origin(id::TEMPERATURE),
            Origin::Default,
            "a refusal leaves the tier as it was"
        );
    }

    /// The main settings window edits one tier and reads no resolution
    /// (`design/windows.md`), so its ladder is the global tier alone.
    #[test]
    fn the_global_page_edits_global_and_a_chat_page_edits_that_chat() {
        let settings = settings();
        settings
            .set(&Scope::chat("one"), id::TEMPERATURE, &json!(1.4))
            .expect("set");

        let global = settings.view(&Ladder::global());
        let row = global
            .iter()
            .find(|row| row.setting.id == id::TEMPERATURE)
            .expect("a row per setting");
        assert!(!row.set_here);
        assert_eq!(
            row.from,
            Origin::Default,
            "the global page shows the global tier, not what a chat did to it"
        );

        let chat = settings.view(&Ladder::for_chat("one"));
        let row = chat
            .iter()
            .find(|row| row.setting.id == id::TEMPERATURE)
            .expect("a row per setting");
        assert!(row.set_here, "the chat page edits the chat");
        assert_eq!(row.value, json!(1.4));
    }

    /// A change survives the process that made it, which is the whole reason
    /// there is a store under this.
    #[test]
    fn a_change_is_written_through_rather_than_held() {
        let store = Memory::new();
        let settings = Settings::open(store.clone());
        settings
            .set(&Scope::chat("one"), id::CONTEXT_LENGTH, &json!(8192))
            .expect("set");

        let reopened = Settings::open(store.clone());
        assert_eq!(
            reopened.resolve(&Ladder::for_chat("one")).context_length(),
            8192
        );
        assert_eq!(store.writes(), 1, "one deliberate change, one write");
    }
}
