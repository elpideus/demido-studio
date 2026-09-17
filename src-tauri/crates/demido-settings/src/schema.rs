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
//! **A handful of settings, not thirty.** v2 declared twenty four samplers before
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
    /// How many times one message may send the model back to its tools. Not
    /// the mode's to decide: `docs/rules/tools.md` keeps the mode to
    /// permissions and nothing else.
    pub const STEP_LIMIT: &str = "tools.step_limit";
    /// How many levels of delegation may open under one conversation. Not the
    /// mode's to decide either: `docs/rules/tools.md` keeps the mode to
    /// permissions, and names the depth among the things it does not gate.
    pub const DELEGATION_DEPTH: &str = "tools.delegation_depth";
    /// Which row of the permission matrix is in force. **Permitted**, in
    /// `docs/rules/tools.md`'s two axes: what runs without asking, and read by
    /// the matrix and nothing else.
    pub const TOOLS_MODE: &str = "tools.mode";
    /// Which tools the model is shown. **Offered**, the other axis: the one row
    /// on the ladder that is a set rather than a scalar, and an override of it
    /// replaces the set below rather than merging with it.
    pub const TOOLS_OFFERED: &str = "tools.offered";
    /// Which tools the person said *always for this tool* about.
    ///
    /// Written by the approval row in the transcript and by nothing else, and
    /// **only ever at the chat tier**
    /// ([#55](https://github.com/elpideus/demido-studio/issues/55)): a decision
    /// taken about one call in one conversation is not a decision about every
    /// conversation, and a control that quietly widened it to all of them would
    /// be the nagging this answer exists to end turning into a blanket consent
    /// nobody gave. This crate stores what it is handed; the tier is the
    /// caller's, and `demido-chat/tests/a_tool.rs` is what holds it to the chat.
    pub const TOOLS_ALWAYS: &str = "tools.always";
}

/// The names a mode is stored under, strictest first.
///
/// Written here rather than read from `demido-permission`, because this crate
/// sits below it, and held to that crate's own list by
/// `demido-chat/tests/offered.rs` so the two cannot come apart. The first is the
/// default for the reason the matrix's first row is: the strictest answer wins
/// every ambiguous case.
pub const MODES: &[&str] = &["cautious", "balanced", "autonomous"];

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
    Count {
        default: u64,
        min: u64,
        max: u64,
        /// What the number counts, drawn beside the field.
        ///
        /// Declared per setting because it is not the same word twice: a
        /// context length is tokens, a step limit is steps, a delegation depth
        /// is levels. The control used to write *tokens* for all of them, which
        /// was true of exactly one and is the kind of thing a declaration
        /// carrying its own prose exists to stop.
        unit: &'static str,
    },
    Text {
        /// Always empty in this build. A default with words in it would be host
        /// prompt text, which is a catalog entry and not a schema literal (hard
        /// rule 10,
        /// [`docs/rules/prompts.md`](../../../../../docs/rules/prompts.md)).
        default: &'static str,
        multiline: bool,
    },
    /// One name out of a fixed list.
    Choice {
        default: &'static str,
        options: &'static [&'static str],
    },
    /// A set of names, or nothing, which is **every** name there is.
    ///
    /// Nothing is not the empty set, for the reason off is not zero: nothing is
    /// a tier with no opinion, and the empty set is an opinion, a conversation
    /// offered no tools. The names are not checked against a registry here,
    /// because this crate has none: a name nothing registers offers nothing.
    Set {},
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
            unit: "tokens",
        },
        reloads: true,
    },
    // not-a-prompt: a settings page's own label and caption, as above.
    Setting {
        id: id::STEP_LIMIT,
        section: "Tools",
        title: "Steps per message",
        summary: "How many rounds of tool calls one message may take before the model has to stop.",
        // v2's default, and a whole number rather than off: a loop with no
        // ceiling is the runaway this setting exists to end.
        kind: Kind::Count {
            default: 8,
            min: 1,
            max: 100,
            unit: "steps",
        },
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above.
    Setting {
        id: id::DELEGATION_DEPTH,
        section: "Tools",
        title: "Delegation depth",
        summary: "How far a chain of sub-agents may reach. At 1 a sub-agent cannot delegate further.",
        // The brief's own example is a chain of three, and two is the smallest
        // number that makes a chain exist at all, so the mechanism is driven at
        // its default rather than only when somebody changes a setting
        // ([#64](https://github.com/elpideus/demido-studio/issues/64)). One is
        // the floor because the conversation itself always delegates: turning
        // delegation off is the picker's, which is the control that owns
        // *this model is not shown that tool*.
        //
        // Eight is the ceiling because a delegation blocks the turn that asked
        // for it, so a chain is a stack of turns each holding a model: the
        // number is bounded by how long a person will wait rather than by
        // anything the mechanism needs, and the brief's own example is three.
        // Raising it is one edit here.
        kind: Kind::Count {
            default: 2,
            min: 1,
            max: 8,
            unit: "levels",
        },
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above. The mode
    // is never prose to a model (`docs/rules/tools.md`), and neither is this.
    Setting {
        id: id::TOOLS_MODE,
        section: "Tools",
        title: "Agent mode",
        summary: "What runs without asking. Cautious asks before anything that writes or runs a program.",
        kind: Kind::Choice {
            default: MODES[0],
            options: MODES,
        },
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above.
    Setting {
        id: id::TOOLS_OFFERED,
        section: "Tools",
        title: "Tools",
        summary: "What the model is shown. A tool switched off is not sent to it at all.",
        kind: Kind::Set {},
        reloads: false,
    },
    // not-a-prompt: a settings page's own label and caption, as above. Nothing
    // draws this one today: the approval row writes it and the matrix reads it.
    Setting {
        id: id::TOOLS_ALWAYS,
        section: "Tools",
        title: "Always allowed",
        summary: "The tools you answered always for, in this conversation. Never covers a call that cannot be undone.",
        kind: Kind::Set {},
        reloads: false,
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
            Kind::Text { default, .. } | Kind::Choice { default, .. } => json!(default),
            Kind::Set {} => Value::Null,
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

            Kind::Choice { options, .. } => {
                let name = value
                    .as_str()
                    .ok_or(Invalid::WrongType { expected: "a name" })?;
                if !options.contains(&name) {
                    return Err(Invalid::OutOfRange {
                        allowed: options.join(", "),
                    });
                }
                Ok(json!(name))
            }

            Kind::Set {} => {
                // Nothing is a real answer: every name there is.
                if value.is_null() {
                    return Ok(Value::Null);
                }
                // not-a-prompt: what a refusal says to the person who typed it.
                let wrong = Invalid::WrongType {
                    expected: "a list of names, or nothing for all of them",
                };
                let mut names: Vec<&str> = Vec::new();
                for item in value.as_array().ok_or_else(|| wrong.clone())? {
                    let name = item.as_str().ok_or_else(|| wrong.clone())?;
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                Ok(json!(names))
            }
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

    /// Cautious unless somebody says otherwise, and never a name the matrix
    /// does not have a row for.
    #[test]
    fn the_mode_is_one_of_three_names_and_cautious_by_default() {
        let declared = setting(id::TOOLS_MODE).expect("declared");
        assert_eq!(declared.default_value(), json!("cautious"));
        for name in ["cautious", "balanced", "autonomous"] {
            assert_eq!(declared.accept(&json!(name)), Ok(json!(name)));
        }
        assert!(matches!(
            declared.accept(&json!("Autonomous")),
            Err(Invalid::OutOfRange { .. })
        ));
        assert!(matches!(
            declared.accept(&json!(2)),
            Err(Invalid::WrongType { .. })
        ));
    }

    /// Everything, until somebody names a set. A set is names, each once, and
    /// the empty set is a real answer: a conversation with no tools.
    #[test]
    fn the_offered_set_is_everything_by_default_and_a_list_of_names_otherwise() {
        let declared = setting(id::TOOLS_OFFERED).expect("declared");
        assert_eq!(declared.default_value(), Value::Null);
        assert_eq!(declared.accept(&Value::Null), Ok(Value::Null));
        assert_eq!(declared.accept(&json!([])), Ok(json!([])));
        assert_eq!(
            declared.accept(&json!(["read_file", "run_command"])),
            Ok(json!(["read_file", "run_command"]))
        );
        assert!(matches!(
            declared.accept(&json!(["read_file", 3])),
            Err(Invalid::WrongType { .. })
        ));
        assert!(matches!(
            declared.accept(&json!("read_file")),
            Err(Invalid::WrongType { .. })
        ));
        assert_eq!(
            declared.accept(&json!(["read_file", "read_file"])),
            Ok(json!(["read_file"])),
            "a name twice is the same set"
        );
    }

    /// Only the context length is a flag on the process. Every other setting
    /// is read per turn and takes effect on the next one.
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
