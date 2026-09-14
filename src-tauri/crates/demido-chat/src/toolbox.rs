//! What a conversation may call, and what a person is shown when a call waits
//! on them.
//!
//! Two axes, kept apart the way `docs/rules/tools.md` keeps them. **Offered** is
//! the registry and the tool register: what the model is shown. **Permitted** is
//! the mode: what runs without asking, read by the matrix and by nothing else
//! (`demido_permission`). A toolbox holds both and decides neither; the turn loop
//! asks each its own question.

use std::path::PathBuf;

use demido_permission::Mode;
use demido_prompts::{Paragraphs, Prompt, Tools};
use demido_tools::{Ability, Registry, Spec};
use serde::Serialize;

/// The tools one conversation offers, the words they are offered in, and the
/// mode their calls are ruled on under.
pub struct Toolbox {
    registry: Registry,
    documents: Tools,
    /// Where a refusal's wording comes from: a declined call, a stopped one, a
    /// call past the step limit. Host prompt text, so a catalog entry (hard
    /// rule 10), and the same directory the tool documents are edited in.
    paragraphs: Paragraphs,
    /// The stored name of the mode, never a [`Mode`]: only the matrix reads a
    /// mode. Empty until something names one, which the matrix reads as an
    /// unknown name and so as Cautious.
    mode: String,
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
            mode: String::new(),
        }
    }

    /// Rule on calls under the mode stored as `name`.
    ///
    /// Fixed for the life of the conversation until the mode is a value on the
    /// settings ladder, which is
    /// [#56](https://github.com/elpideus/demido-studio/issues/56)'s.
    #[must_use]
    pub fn in_mode(mut self, name: &str) -> Self {
        name.clone_into(&mut self.mode);
        self
    }

    pub(crate) fn mode(&self) -> Mode {
        Mode::named(&self.mode)
    }

    /// Everything on offer this turn, each with its document and its shape.
    pub(crate) fn offered(&self) -> Vec<Spec> {
        self.registry.offered(&self.documents)
    }

    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn paragraph(&self, id: &str) -> Option<Prompt> {
        self.paragraphs.get(id)
    }
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
