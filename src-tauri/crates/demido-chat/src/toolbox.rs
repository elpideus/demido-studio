//! What a conversation may call, and what a person is shown when a call waits
//! on them.
//!
//! Two axes, kept apart the way `docs/rules/tools.md` keeps them. **Offered** is
//! the registry narrowed to the set the ladder resolved, in the tool register's
//! words: what the model is shown. **Permitted** is the mode, also off the
//! ladder, read by the matrix and by nothing else (`demido_permission`). A
//! toolbox holds neither setting and decides neither; the turn loop resolves
//! both per turn and asks each its own question.

use std::path::PathBuf;

use demido_prompts::{Paragraphs, Prompt, Tools};
use demido_tools::{Ability, Registry, Spec};
use serde::Serialize;

/// The tools one conversation could offer, and the words they are offered in.
pub struct Toolbox {
    /// Everything registered. The set a turn offers is this, narrowed.
    registry: Registry,
    documents: Tools,
    /// Where a refusal's wording comes from: a declined call, a stopped one, a
    /// call past the step limit, a call to a tool switched off. Host prompt
    /// text, so a catalog entry (hard rule 10), and the same directory the tool
    /// documents are edited in.
    paragraphs: Paragraphs,
}

impl Toolbox {
    /// The tools `registry` holds, described in the words the prompts directory
    /// holds for them. Opens nothing and creates nothing.
    pub fn open(registry: Registry, prompts: impl Into<PathBuf>) -> Self {
        let prompts = prompts.into();
        Self {
            registry,
            documents: Tools::open(prompts.clone()),
            paragraphs: Paragraphs::open(prompts),
        }
    }

    /// What the picker draws: every group, with its tools' names.
    pub fn groups(&self) -> Vec<Offering> {
        self.registry
            .groups()
            .into_iter()
            .map(|(group, tools)| Offering {
                group: group.to_owned(),
                tools,
            })
            .collect()
    }

    /// Every registered tool's schema shape, by name: what the tool register's
    /// editor draws beside a document as the part that is not editable.
    pub fn shapes(&self) -> Vec<(String, serde_json::Value)> {
        self.registry.shapes()
    }

    /// The registry as one turn offers it: narrowed to `set`, or all of it when
    /// nobody on the ladder named one.
    pub(crate) fn narrowed(&self, set: Option<&[String]>) -> Registry {
        match set {
            Some(names) => self.registry.only(names),
            None => self.registry.clone(),
        }
    }

    /// Everything `registry` offers, each with its document and its shape.
    pub(crate) fn offered(&self, registry: &Registry) -> Vec<Spec> {
        registry.offered(&self.documents)
    }

    /// Everything registered, before any set narrowed it.
    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn paragraph(&self, id: &str) -> Option<Prompt> {
        self.paragraphs.get(id)
    }
}

/// One row of the picker: a group, and the tools a person can switch in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offering {
    /// The group's name. The words drawn for it are the window's.
    pub group: String,
    pub tools: Vec<String>,
}

/// One call waiting on a person: the tool, what it declares, and the exact
/// arguments that will run if they allow it.
///
/// Handed to the approval callback [`crate::Chat::ask`] takes. A callback rather
/// than a trait, because the window is the one real implementation and a trait
/// would buy a second one that exists only in tests
/// ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md)); this struct is the
/// interface that trait would have.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asking {
    pub turn: u32,
    /// Where the call is on the log, which is what the decision is recorded
    /// against.
    pub call: u64,
    /// What the backend called the call.
    pub id: String,
    pub tool: String,
    pub ability: Ability,
    /// The tool's one line about what this call will do.
    pub summary: String,
    /// Asked about in every mode, and not something *always* can cover.
    pub destructive: bool,
    /// The arguments as they will run: already parsed and checked against the
    /// tool's schema.
    pub arguments: serde_json::Value,
}
