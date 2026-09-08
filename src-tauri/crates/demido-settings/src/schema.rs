//! What a setting is, declared once.
//!
//! A setting exists here or it does not exist. Nothing else may invent a
//! default, decide a range, or accept a value this file would refuse: a second
//! opinion about what a temperature may be is how a settings page and a request
//! come to disagree about what the user asked for.
//!
//! The declaration carries its own prose because one declaration is what lets
//! the settings page, the set-up wizard and (later) the Navigator draw the same
//! control without registering it three times. None of that prose is ever sent
//! to a model, which is why every entry is accounted for against hard rule 10.
//!
//! **Three settings, not thirty.** v2 declared twenty four samplers before
//! anything sent one. What is here is what this slice actually resolves, sends
//! and can be held to; the rest is additive, and a setting added later is an
//! entry in [`SCHEMA`] and nothing else.

use serde::Serialize;
use serde_json::{json, Value};

/// The stable identifiers, so nothing spells one out twice.
///
/// The prefix is the **section** a setting is drawn in, not the tier it is set
/// at: any of these can be set globally or on one chat. Stored in files, so
/// renaming one is a migration rather than a rename.
pub mod id {
    /// Who the model is being, which is a ladder value from the first commit.
    /// See
    /// [`docs/decisions/0007-a-chat-outranks-its-character.md`](../../../../../docs/decisions/0007-a-chat-outranks-its-character.md).
    pub const SYSTEM_PROMPT: &str = "conversation.system_prompt";
    pub const TEMPERATURE: &str = "conversation.temperature";
    pub const CONTEXT_LENGTH: &str = "conversation.context_length";
}

/// One setting: what it is called, what it means, and what it will accept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    /// Stable identifier, `section.name`.
    pub id: &'static str,
    /// Which page it is drawn on, and how a search will one day group it.
    pub section: &'static str,
    pub title: &'static str,
    /// One sentence, shown under the control.
    pub summary: &'static str,
    pub kind: Kind,
    /// Whether changing it means the server has to be started again.
    ///
    /// Said in the declaration rather than worked out by a caller, because the
    /// window has to be able to tell somebody that a number they just typed
    /// costs them a reload. Only the context length does: it is a flag on the
    /// process, and the other two are fields of a request.
    pub reloads: bool,
}

/// What a setting accepts, and what it is when nobody has said otherwise.
///
/// The default lives inside the kind rather than beside it, so a setting cannot
/// be declared with a default its own range would reject.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "control", rename_all = "kebab-case")]
pub enum Kind {
    /// A number that is either **off** or a value: a switch, and a field beside
    /// it that only means anything while the switch is on.
    ///
    /// **Off is not zero, and that is the whole reason this kind exists.** A
    /// sampler that is not sent is one the server decides for itself; a sampler
    /// sent as zero is one it has been told to use with a value of zero, and on
    /// `llama.cpp` those are different generations. v2 stored both as zero and
    /// so could express neither.
    Amount {
        /// What it is when it is on and nobody has chosen a value.
        default: f64,
        min: f64,
        max: f64,
        /// Whether it is on before anybody says.
        on: bool,
    },
    /// A whole number that is always a number. Absence is not an answer here:
    /// something has to reach the process on its command line.
    Count { default: u64, min: u64, max: u64 },
    Text {
        /// Always empty in this build. A default with words in it would be host
        /// prompt text, which is a catalog entry and not a schema literal (hard
        /// rule 10,
        /// [`docs/rules/prompts.md`](../../../../../docs/rules/prompts.md)).
        default: &'static str,
        multiline: bool,
    },
}

/// Why a value was refused.
///
/// Carried to the user, never swallowed: a setting that quietly reverts to its
/// default is indistinguishable from one that was never saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// No setting by that id. The value is kept in the file it came from all
    /// the same, because the build that wrote it may know something this one
    /// does not.
    Unknown,
    WrongType {
        expected: &'static str,
    },
    OutOfRange {
        allowed: String,
    },
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Unknown => out.write_str("no setting by that name"),
            Invalid::WrongType { expected } => write!(out, "expected {expected}"),
            Invalid::OutOfRange { allowed } => write!(out, "outside {allowed}"),
        }
    }
}

/// Every setting this build has, in the order the page draws them.
pub static SCHEMA: &[Setting] = &[
    // not-a-prompt: a settings page's own label and caption, drawn beside a
    // control and never put in front of a model. The one string here a model
    // does read is this setting's *value*, which the user types.
    Setting {
        id: id::SYSTEM_PROMPT,
        section: "Conversation",
        title: "System prompt",
        summary: "Sent ahead of the conversation, every turn. Empty until you write one.",
        kind: Kind::Text {
            default: "",
            multiline: true,
        },
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above.
    Setting {
        id: id::TEMPERATURE,
        section: "Conversation",
        title: "Temperature",
        summary: "How far the model strays from its likeliest next token. Off leaves the choice to the server.",
        kind: Kind::Amount {
            default: 0.7,
            min: 0.0,
            max: 2.0,
            on: true,
        },
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above.
    Setting {
        id: id::CONTEXT_LENGTH,
        section: "Conversation",
        title: "Context length",
        summary: "The window one generation gets. This is the number reserved, not the number shared out between slots.",
        kind: Kind::Count {
            default: 4096,
            min: 512,
            max: 262_144,
        },
        reloads: true,
    },
];

/// The setting with this id, if this build has one.
#[must_use]
pub fn setting(id: &str) -> Option<&'static Setting> {
    SCHEMA.iter().find(|setting| setting.id == id)
}

impl Setting {
    /// What this setting is when no tier has an opinion.
    #[must_use]
    pub fn default_value(&self) -> Value {
        match self.kind {
            Kind::Amount { default, on, .. } => {
                if on {
                    json!(default)
                } else {
                    Value::Null
                }
            }
            Kind::Count { default, .. } => json!(default),
            Kind::Text { default, .. } => json!(default),
        }
    }

    /// Accept a value, or say why not.
    ///
    /// Returns the value in its canonical shape, so `0.7` typed as an integer
    /// and as a float are stored identically. Out of range is refused rather
    /// than clamped: silently moving a number somebody typed is a lie the
    /// settings page then tells back to them.
    pub fn accept(&self, value: &Value) -> Result<Value, Invalid> {
        match self.kind {
            Kind::Amount { min, max, .. } => {
                // Off, which is a real answer and always allowed: it is what
                // "let the server decide" is stored as.
                if value.is_null() {
                    return Ok(Value::Null);
                }
                // not-a-prompt: what a refusal says to the person who typed it.
                let number = value.as_f64().ok_or(Invalid::WrongType {
                    expected: "a number, or nothing to leave it to the server",
                })?;
                if !number.is_finite() || number < min || number > max {
                    return Err(Invalid::OutOfRange {
                        allowed: format!("{min} to {max}, or nothing"),
                    });
                }
                Ok(json!(number))
            }

            Kind::Count { min, max, .. } => {
                let count = value.as_u64().ok_or(Invalid::WrongType {
                    expected: "a whole number",
                })?;
                if count < min || count > max {
                    return Err(Invalid::OutOfRange {
                        allowed: format!("{min} to {max}"),
                    });
                }
                Ok(json!(count))
            }

            Kind::Text { .. } => value
                .as_str()
                .map(|text| json!(text))
                .ok_or(Invalid::WrongType { expected: "text" }),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn the_schema_is_a_table_of_distinct_ids() {
        for declared in SCHEMA {
            assert_eq!(
                setting(declared.id).map(|found| found.id),
                Some(declared.id)
            );
        }
        let mut ids: Vec<&str> = SCHEMA.iter().map(|setting| setting.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two settings share an id");
    }

    /// A default its own range would refuse is a schema that cannot be saved
    /// back the way it was read.
    #[test]
    fn every_default_is_a_value_its_own_setting_accepts() {
        for declared in SCHEMA {
            let default = declared.default_value();
            assert_eq!(
                declared.accept(&default),
                Ok(default.clone()),
                "{} refuses its own default {default}",
                declared.id
            );
        }
    }

    #[test]
    fn the_system_prompt_ships_empty() {
        let declared = setting(id::SYSTEM_PROMPT).expect("declared");
        assert_eq!(declared.default_value(), json!(""));
    }

    #[test]
    fn a_temperature_outside_the_range_is_refused_rather_than_clamped() {
        let declared = setting(id::TEMPERATURE).expect("declared");
        assert!(matches!(
            declared.accept(&json!(9.0)),
            Err(Invalid::OutOfRange { .. })
        ));
    }

    /// Off is not zero. Both are storable, and they are different requests.
    #[test]
    fn a_temperature_can_be_off_and_can_be_zero() {
        let declared = setting(id::TEMPERATURE).expect("declared");
        assert_eq!(declared.accept(&Value::Null), Ok(Value::Null));
        assert_eq!(declared.accept(&json!(0.0)), Ok(json!(0.0)));
    }

    /// A context length is always a number, because a number is what reaches
    /// the process on its command line.
    #[test]
    fn a_context_length_is_never_nothing() {
        let declared = setting(id::CONTEXT_LENGTH).expect("declared");
        assert!(matches!(
            declared.accept(&Value::Null),
            Err(Invalid::WrongType { .. })
        ));
        assert!(matches!(
            declared.accept(&json!(16)),
            Err(Invalid::OutOfRange { .. })
        ));
        assert_eq!(declared.accept(&json!(8192)), Ok(json!(8192)));
    }

    /// Only the context length is a flag on the process. The other two are
    /// fields of a request and take effect on the next turn.
    #[test]
    fn the_only_setting_that_costs_a_reload_is_the_one_the_server_starts_with() {
        let reloading: Vec<&str> = SCHEMA
            .iter()
            .filter(|setting| setting.reloads)
            .map(|setting| setting.id)
            .collect();
        assert_eq!(reloading, vec![id::CONTEXT_LENGTH]);
    }
}
