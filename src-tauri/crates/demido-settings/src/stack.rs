//! The tiers, and the one function that collapses them.
//!
//! Precedence is the declaration order of [`Tier`] and nothing else. There is
//! no second place where a value is chosen: a caller asks [`Stack::resolve`]
//! and is handed both the value and where it came from, because a settings page
//! that works out provenance for itself will eventually work it out differently
//! from the request that was actually sent.
//!
//! **The chat is the last word.**
//! [`docs/decisions/0007-a-chat-outranks-its-character.md`](../../../../../docs/decisions/0007-a-chat-outranks-its-character.md)
//! corrected two rule files that described this backwards. It is asserted here
//! rather than described, on [`the_chat_is_the_last_word`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::schema::{id, setting, Invalid, Setting, SCHEMA};

/// Who overrides whom. **Declaration order is precedence**, lowest first: a
/// variant moved in this list changes the behaviour of the whole application.
///
/// Four, of which **two are live in v0.1**. The model and character tiers have
/// nothing to name a subject with yet, and they are declared now because the
/// stored shape has to carry them from the first commit: a ladder that grew a
/// tier later would be a migration of every profile, and the character system
/// is the thing that would be foreclosed by leaving them out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// What the user set for everything. Changing it changes every chat that
    /// has not said otherwise, including every chat made afterwards.
    Global,
    /// What this model wants regardless of who is talking to it. A 4B model
    /// that needs a lower temperature should not make every model need one.
    Model,
    /// Who the model is being. Not live in v0.1.
    Character,
    /// This one conversation. The last word.
    Chat,
}

impl Tier {
    /// Every tier, lowest precedence first.
    pub const ALL: [Tier; 4] = [Tier::Global, Tier::Model, Tier::Character, Tier::Chat];

    /// How the tier is written in files and over the boundary.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Tier::Global => "global",
            Tier::Model => "model",
            Tier::Character => "character",
            Tier::Chat => "chat",
        }
    }
}

/// Where an effective value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Nobody has an opinion; this is the schema's answer.
    Default,
    Tier(Tier),
}

impl Origin {
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Tier(tier) => tier.slug(),
        }
    }
}

impl Serialize for Origin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.slug())
    }
}

/// A stored entry that was not usable, and why. Reported rather than dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    pub id: String,
    pub reason: Invalid,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}: {}", self.id, self.reason)
    }
}

/// What one tier says.
///
/// Values it recognises are validated on the way in, so anything held here is
/// known to fit its schema. Values it does not recognise are kept untouched and
/// written back out again: a profile edited by a newer build must survive being
/// opened by an older one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    known: BTreeMap<&'static str, Value>,
    unknown: Map<String, Value>,
}

impl Values {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set one value, or say why it will not be taken.
    pub fn set(&mut self, id: &str, value: &Value) -> Result<(), Invalid> {
        let setting = setting(id).ok_or(Invalid::Unknown)?;
        self.known.insert(setting.id, setting.accept(value)?);
        Ok(())
    }

    /// Forget this tier's opinion. What the settings page calls revert.
    pub fn clear(&mut self, id: &str) {
        self.known.remove(id);
        self.unknown.remove(id);
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Value> {
        self.known.get(id)
    }

    /// Read a stored object, keeping what does not fit rather than obeying it.
    ///
    /// An unknown id is preserved in the file and reported to the caller: it is
    /// not corruption, and it is not something this build can show either, so
    /// the only honest thing is to say so and leave it alone.
    #[must_use]
    pub fn from_json(object: &Map<String, Value>) -> (Self, Vec<Rejected>) {
        let mut values = Values::new();
        let mut rejected = Vec::new();

        for (id, value) in object {
            match values.set(id, value) {
                Ok(()) => {}
                Err(reason) => {
                    values.unknown.insert(id.clone(), value.clone());
                    rejected.push(Rejected {
                        id: id.clone(),
                        reason,
                    });
                }
            }
        }

        (values, rejected)
    }

    /// Everything this tier holds, including what it did not understand.
    #[must_use]
    pub fn to_json(&self) -> Map<String, Value> {
        let mut object = self.unknown.clone();
        for (id, value) in &self.known {
            object.insert((*id).to_owned(), value.clone());
        }
        object
    }
}

/// The tiers, together, for one conversation.
///
/// Built by [`crate::Settings`] out of the stored document and whichever
/// subjects the ladder names. Nothing else assembles one, so there is one place
/// that knows which tier a set of values belongs to.
#[derive(Debug, Clone, Default)]
pub struct Stack {
    tiers: BTreeMap<Tier, Values>,
}

impl Stack {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a whole tier in place, replacing whatever was there.
    pub fn insert(&mut self, tier: Tier, values: Values) {
        self.tiers.insert(tier, values);
    }

    /// Collapse every tier into one answer per setting, with its provenance.
    #[must_use]
    pub fn resolve(&self) -> Resolved {
        self.resolve_without(None)
    }

    /// What every setting would become if `skip` stopped saying anything: the
    /// value the settings page shows behind a revert.
    fn resolve_without(&self, skip: Option<Tier>) -> Resolved {
        let mut entries = Vec::with_capacity(SCHEMA.len());

        for setting in SCHEMA {
            let mut value = setting.default_value();
            let mut from = Origin::Default;

            // Ascending, so the last writer wins. That is the whole rule, and
            // the last tier in the list is the chat.
            for tier in Tier::ALL {
                if Some(tier) == skip {
                    continue;
                }
                if let Some(found) = self
                    .tiers
                    .get(&tier)
                    .and_then(|values| values.get(setting.id))
                {
                    value = found.clone();
                    from = Origin::Tier(tier);
                }
            }

            entries.push(Entry {
                setting,
                value,
                from,
            });
        }

        Resolved { entries }
    }

    /// Everything a settings page needs to draw one tier: the value in force,
    /// whether this tier is the one saying so, and what reverting would leave.
    #[must_use]
    pub fn view(&self, editing: Tier) -> Vec<Row> {
        let effective = self.resolve();
        let without = self.resolve_without(Some(editing));
        let here = self.tiers.get(&editing);

        // Both resolutions walk `SCHEMA` in order, so they are parallel by
        // construction. Zipping says that; indexing by position only assumed it.
        effective
            .entries
            .into_iter()
            .zip(without.entries)
            .map(|(effective, without)| Row {
                setting: effective.setting,
                value: effective.value,
                from: effective.from,
                set_here: here.is_some_and(|values| values.get(effective.setting.id).is_some()),
                inherited: without.value,
                inherited_from: without.from,
            })
            .collect()
    }
}

/// One setting's answer, and where it came from.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    setting: &'static Setting,
    value: Value,
    from: Origin,
}

/// Every setting's answer.
///
/// Total by construction: a setting in the schema has an entry here whether or
/// not anyone has an opinion about it, so no caller writes a fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    entries: Vec<Entry>,
}

impl Resolved {
    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.setting.id == id)
    }

    #[must_use]
    pub fn get(&self, id: &str) -> &Value {
        static NULL: Value = Value::Null;
        self.entry(id).map_or(&NULL, |entry| &entry.value)
    }

    #[must_use]
    pub fn origin(&self, id: &str) -> Origin {
        self.entry(id).map_or(Origin::Default, |entry| entry.from)
    }

    /// A [`crate::Kind::Amount`]: the value, or nothing where it is switched
    /// off.
    ///
    /// Nothing is not zero. An amount that is off is a parameter the request
    /// does not carry at all, which leaves the decision to the server; zero is
    /// a value it has been told to use.
    #[must_use]
    pub fn amount_of(&self, id: &str) -> Option<f64> {
        self.get(id).as_f64()
    }

    #[must_use]
    pub fn count_of(&self, id: &str) -> Option<u64> {
        self.get(id).as_u64()
    }

    #[must_use]
    pub fn text_of(&self, id: &str) -> &str {
        self.get(id).as_str().unwrap_or_default()
    }

    /// How far the model strays from its likeliest next token, or nothing where
    /// the choice is left to the server.
    #[must_use]
    pub fn temperature(&self) -> Option<f32> {
        // A temperature is one decimal place and the wire takes an f32, so the
        // narrowing is the format rather than a loss.
        #[allow(clippy::cast_possible_truncation)]
        self.amount_of(id::TEMPERATURE).map(|value| value as f32)
    }

    /// The window **one generation** gets. Not what `--ctx-size` is given: see
    /// `demido_inference::llamacpp::arguments`.
    #[must_use]
    pub fn context_length(&self) -> u32 {
        // Never nothing: the setting is a `Count`, which refuses null, and the
        // schema's own default answers when no tier has an opinion.
        u32::try_from(self.count_of(id::CONTEXT_LENGTH).unwrap_or_default()).unwrap_or(u32::MAX)
    }

    /// Who the model is being, or the empty string where nobody has said.
    ///
    /// Empty rather than absent is the honest default: an empty system prompt
    /// is one the assembly leaves out entirely, and a wording nobody chose
    /// would be host prompt text this crate is not the register for.
    #[must_use]
    pub fn system_prompt(&self) -> &str {
        self.text_of(id::SYSTEM_PROMPT)
    }
}

/// One line of a settings page, and of a wizard step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub setting: &'static Setting,
    /// What is actually in force, after every tier.
    pub value: Value,
    pub from: Origin,
    /// Whether the tier being edited is one of the ones with an opinion. A row
    /// can be set here and still not be `from` here, when a later tier wins.
    pub set_here: bool,
    /// What reverting this tier would leave behind.
    pub inherited: Value,
    pub inherited_from: Origin,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use serde_json::json;

    use super::*;
    use crate::schema::id;

    fn values(pairs: &[(&str, Value)]) -> Values {
        let mut values = Values::new();
        for (id, value) in pairs {
            values.set(id, value).expect("the fixture is valid");
        }
        values
    }

    /// The settled decision itself. Editing this test edits
    /// `docs/decisions/0007-a-chat-outranks-its-character.md`.
    #[test]
    fn the_chat_is_the_last_word() {
        let mut sorted = Tier::ALL;
        sorted.sort_unstable();
        assert_eq!(sorted, Tier::ALL, "declaration order is precedence");
        assert_eq!(Tier::ALL.last(), Some(&Tier::Chat));

        let mut stack = Stack::new();
        stack.insert(Tier::Global, values(&[(id::TEMPERATURE, json!(0.2))]));
        stack.insert(Tier::Character, values(&[(id::TEMPERATURE, json!(1.1))]));
        stack.insert(Tier::Chat, values(&[(id::TEMPERATURE, json!(1.4))]));

        let resolved = stack.resolve();
        assert_eq!(resolved.amount_of(id::TEMPERATURE), Some(1.4));
        assert_eq!(resolved.origin(id::TEMPERATURE), Origin::Tier(Tier::Chat));
    }

    #[test]
    fn the_last_tier_with_an_opinion_wins() {
        let mut stack = Stack::new();
        stack.insert(Tier::Global, values(&[(id::TEMPERATURE, json!(0.2))]));
        stack.insert(Tier::Model, values(&[(id::CONTEXT_LENGTH, json!(8192))]));

        let resolved = stack.resolve();
        assert_eq!(resolved.amount_of(id::TEMPERATURE), Some(0.2));
        assert_eq!(resolved.origin(id::TEMPERATURE), Origin::Tier(Tier::Global));
        assert_eq!(resolved.count_of(id::CONTEXT_LENGTH), Some(8192));
    }

    #[test]
    fn a_setting_nobody_mentions_is_still_answered() {
        let resolved = Stack::new().resolve();
        assert_eq!(resolved.amount_of(id::TEMPERATURE), Some(0.7));
        assert_eq!(resolved.count_of(id::CONTEXT_LENGTH), Some(4096));
        assert_eq!(resolved.text_of(id::SYSTEM_PROMPT), "");
        assert_eq!(resolved.origin(id::TEMPERATURE), Origin::Default);
    }

    #[test]
    fn reverting_shows_what_it_would_leave_behind() {
        let mut stack = Stack::new();
        stack.insert(Tier::Global, values(&[(id::TEMPERATURE, json!(0.2))]));
        stack.insert(Tier::Chat, values(&[(id::TEMPERATURE, json!(1.4))]));

        let row = stack
            .view(Tier::Chat)
            .into_iter()
            .find(|row| row.setting.id == id::TEMPERATURE)
            .expect("a row per setting");

        assert!(row.set_here);
        assert_eq!(row.value, json!(1.4));
        assert_eq!(row.inherited, json!(0.2));
        assert_eq!(row.inherited_from, Origin::Tier(Tier::Global));
    }

    /// Editing the global tier while a chat overrides it. Drawing that row as
    /// if it were in force is how a user comes to believe a setting does
    /// nothing.
    #[test]
    fn a_row_can_be_set_here_and_still_not_be_in_force() {
        let mut stack = Stack::new();
        stack.insert(Tier::Global, values(&[(id::TEMPERATURE, json!(0.2))]));
        stack.insert(Tier::Chat, values(&[(id::TEMPERATURE, json!(1.4))]));

        let row = stack
            .view(Tier::Global)
            .into_iter()
            .find(|row| row.setting.id == id::TEMPERATURE)
            .expect("a row per setting");

        assert!(row.set_here);
        assert_eq!(row.from, Origin::Tier(Tier::Chat));
    }

    #[test]
    fn a_value_a_setting_refuses_never_enters_a_tier() {
        let mut values = Values::new();
        assert!(values.set(id::TEMPERATURE, &json!("hot")).is_err());
        assert_eq!(values.get(id::TEMPERATURE), None);
    }

    #[test]
    fn an_unreadable_entry_is_reported_and_kept_but_never_obeyed() {
        let stored: Map<String, Value> = serde_json::from_str(
            r#"{"conversation.temperature": 99, "conversation.invented_later": true,
                "conversation.context_length": 8192}"#,
        )
        .expect("a fixture");

        let (values, rejected) = Values::from_json(&stored);

        assert_eq!(values.get(id::CONTEXT_LENGTH), Some(&json!(8192)));
        assert_eq!(values.get(id::TEMPERATURE), None, "99 is not a temperature");
        assert_eq!(rejected.len(), 2, "both are reported: {rejected:?}");

        let written = values.to_json();
        assert_eq!(
            written.get("conversation.invented_later"),
            Some(&json!(true)),
            "a newer build's setting survives being opened by an older one"
        );
        assert_eq!(
            written.get("conversation.temperature"),
            Some(&json!(99)),
            "what the user typed survives, even unusable"
        );
    }
}
