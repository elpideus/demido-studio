//! Which tools exist, and what happens when the model calls one.
//!
//! The registry is where a call stops being text and becomes an outcome. Four
//! things can go wrong before a tool ever runs, and all four come back as a
//! sentence the model can act on rather than as an error somebody has to
//! translate later:
//!
//! - there is no workspace, so there is nothing to do anything to
//! - a name that is not a tool, answered with the names that are
//! - arguments that are not JSON, answered with what the parser objected to
//! - arguments that do not fit the schema, answered with every fault at once
//!
//! Doing this here rather than in each tool is the point. It is the difference
//! between one implementation of "explain this to a model" and one per tool,
//! and the per-tool version is written well the first two times.
//!
//! **The registry holds the workspace, and a tool never does.** Every tool in
//! this slice needs one, so a registry without one offers nothing rather than
//! offering tools that cannot succeed however they are called. The day a tool
//! arrives that needs no workspace (a web search, say) is when that distinction
//! should be built, with the tool that justifies it in front of whoever builds
//! it.
//!
//! **Nothing here decides whether a call may happen.** [`Registry::plan`] stops
//! one step short, holding an understood call and its [`Intent`], which is
//! where the capability matrix
//! ([#53](https://github.com/elpideus/demido-studio/issues/53)) and the
//! approval ([#55](https://github.com/elpideus/demido-studio/issues/55)) go. A
//! registry that ran a tool as soon as it understood the call would leave
//! nowhere for either of them to stand.

use std::sync::Arc;

use demido_prompts::{Document, Tools};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::arguments;
use crate::command::RunCommand;
use crate::delegate::{DelegateTask, Delegating};
use crate::files::{DeleteFile, ReadFile, WriteFile};
use crate::listing::ListDirectory;
use crate::search::SearchFiles;
use crate::tool::{Context, Failure, Intent, Outcome, Tool};
use crate::workspace::Workspace;

/// A call as the model made it.
///
/// The arguments are the text the model produced, not a parsed value: what a
/// malformed call needs is an objection naming what the parser found, and a
/// call that never parsed has no value to carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// What the backend called this call, so a result can be tied to it.
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One tool as a payload carries it: its shape, with its document's prose on it.
///
/// The two halves come from two places on purpose. `parameters` is the shape
/// the tool declares, the one [`arguments::faults`] parses a call against, with
/// a `description` added to each property the document gives prose to and
/// nothing else changed. `description` and that prose are the tool register's
/// ([`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md)),
/// and `document` is kept beside them so that what is recorded as offered is the
/// name and the hash of exactly the wording that was merged.
///
/// `shape` is the same schema with no prose on it, which is the half the
/// session log records beside the hash: the wording is already there once per
/// session, and the rebuild merges the two exactly as `parameters` was merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub shape: Value,
    pub document: Document,
}

/// Tools that are switched on and off together, under one name.
///
/// The grain the picker offers
/// ([`docs/rules/tools.md`](../../../../docs/rules/tools.md)): a group is one
/// row, expandable to its tools. The name is the fact, and the words a window
/// draws for it are the window's.
pub struct Group {
    pub name: &'static str,
    tools: Vec<Box<dyn Tool>>,
}

impl IntoIterator for Group {
    type Item = Box<dyn Tool>;
    type IntoIter = std::vec::IntoIter<Box<dyn Tool>>;

    fn into_iter(self) -> Self::IntoIter {
        self.tools.into_iter()
    }
}

/// The Files group: `read_file`, `list_directory`, `search_files`,
/// `write_file` and `delete_file`.
///
/// Written down once, here, so that a caller assembling a registry names the
/// group rather than five tools, and the day a sixth joins the group it joins
/// every caller.
pub fn files() -> Group {
    Group {
        name: "files",
        tools: vec![
            Box::new(ReadFile),
            Box::new(ListDirectory),
            Box::new(SearchFiles),
            Box::new(WriteFile),
            Box::new(DeleteFile),
        ],
    }
}

/// The Shell group: `run_command`.
///
/// A group of one, and a group anyway: the picker offers groups
/// ([`docs/rules/tools.md`](../../../../docs/rules/tools.md)), and "files but
/// no shell" is a set it has to be able to say.
pub fn shell() -> Group {
    Group {
        name: "shell",
        tools: vec![Box::new(RunCommand)],
    }
}

/// The Delegation group: `delegate_task`.
///
/// One row in the picker and one entry in the registry, switchable like any
/// other ([#61](https://github.com/elpideus/demido-studio/issues/61)), so that
/// delegating is a thing a person turns off in the place they turn the shell
/// off rather than a capability that arrives beside the machinery.
///
/// It takes what carries a task out, because a tool may not know about a
/// session, a backend or a ladder, and all three are what a sub-agent is made
/// of. See [`crate::delegate`].
pub fn delegation(delegating: Delegating) -> Group {
    Group {
        name: "delegation",
        tools: vec![Box::new(DelegateTask::to(delegating))],
    }
}

/// One registered tool, and the group it was registered in.
#[derive(Clone)]
struct Registered {
    group: &'static str,
    tool: Arc<dyn Tool>,
}

/// The tools on offer, and where they may work.
#[derive(Clone, Default)]
pub struct Registry {
    tools: Vec<Registered>,
    workspace: Option<Workspace>,
}

impl Registry {
    /// An empty registry over `workspace`. `None` means no workspace is set,
    /// which is the ordinary state of a fresh profile.
    pub fn open(workspace: Option<Workspace>) -> Self {
        Self {
            tools: Vec::new(),
            workspace,
        }
    }

    /// A registry holding the Files group, which is what v0.1 ships.
    pub fn of_files(workspace: Option<Workspace>) -> Self {
        Self::open(workspace).with_group(files())
    }

    /// Add a whole group, as `files()` or `shell()` hands it over.
    ///
    /// **A name is offered once.** A later registration of a name replaces the
    /// earlier one rather than sitting beside it, because two tools with one
    /// name in a payload is a model choosing between them by position and a log
    /// that cannot say which ran. It is an invariant rather than an override
    /// feature: nothing in v0.1 registers a name twice, and this is what keeps
    /// the day something does from being the day it is discovered.
    pub fn with_group(mut self, group: Group) -> Self {
        let name = group.name;
        for tool in group {
            let tool: Arc<dyn Tool> = Arc::from(tool);
            self.tools
                .retain(|existing| existing.tool.name() != tool.name());
            self.tools.push(Registered { group: name, tool });
        }
        self
    }

    /// Every group registered, in registration order, each with its tools'
    /// names. Whether or not there is a workspace: this is what a person may
    /// switch on, not what the model is shown.
    pub fn groups(&self) -> Vec<(&'static str, Vec<String>)> {
        let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
        for entry in &self.tools {
            let name = entry.tool.name().to_owned();
            match groups.iter_mut().find(|(group, _)| *group == entry.group) {
                Some((_, names)) => names.push(name),
                None => groups.push((entry.group, vec![name])),
            }
        }
        groups
    }

    /// The same registry holding only the tools named, in their registered
    /// order.
    ///
    /// What the picker's set becomes. **Left out is absent**: not offered, and
    /// not planned, so a call naming one is a name this registry has never
    /// heard of (`docs/rules/tools.md`: disabled means absent). A name nothing
    /// registers narrows to nothing rather than failing.
    #[must_use]
    pub fn only(&self, names: &[String]) -> Self {
        Self {
            tools: self
                .tools
                .iter()
                .filter(|entry| names.iter().any(|name| name == entry.tool.name()))
                .cloned()
                .collect(),
            workspace: self.workspace.clone(),
        }
    }

    /// The same registry, with `delegate_task` answering on `delegating`
    /// instead of wherever it answered before.
    ///
    /// **One rendezvous per agent**, which is what the asynchronous path needs
    /// and the synchronous one never did
    /// ([#66](https://github.com/elpideus/demido-studio/issues/66)). While a
    /// delegation blocked there was one loop awaiting a delegation at a time,
    /// so one channel could not be ambiguous about whose delegation it was
    /// carrying. Above the default a sub-agent runs beside the turn that asked
    /// for it, and two agents sharing a channel is an ask answered by whichever
    /// one happened to poll first: the task would be carried out correctly and
    /// recorded under the wrong parent.
    ///
    /// It replaces in place rather than registering the name again, so the
    /// order the set is offered in is the order it was registered in. A
    /// registry with no `delegate_task` in it is returned unchanged, which is
    /// the case that matters: a sub-agent at the depth limit has no delegation
    /// tool, and giving it a rendezvous would be giving it one back.
    #[must_use]
    pub fn delegating_to(&self, delegating: Delegating) -> Self {
        let mut rebound = self.clone();
        for entry in &mut rebound.tools {
            if entry.tool.name() == DelegateTask::NAME {
                entry.tool = Arc::new(DelegateTask::to(delegating.clone()));
            }
        }
        rebound
    }

    /// Whether a tool by this name is on offer.
    ///
    /// Asked of the whole registry and of a narrowed one, it is how the turn
    /// loop tells a tool the person switched off from a name the model guessed.
    pub fn offers(&self, name: &str) -> bool {
        self.on_offer().iter().any(|tool| tool.name() == name)
    }

    /// Everything the model may be told about, in the words `documents` holds
    /// for it right now.
    ///
    /// Empty when there is no workspace: a model that is shown a tool will call
    /// it, and a call that cannot succeed however it is written is worse than a
    /// tool that was never mentioned.
    ///
    /// A registered tool with no document is not offered. Every host tool has
    /// one, and `tests/documents.rs` fails the commit that adds a tool without
    /// one, so this is the state nothing reaches rather than a fallback: a tool
    /// offered with no words on it is the one thing a 4B model picks worst, and
    /// a tool offered with words nobody can edit or log is rule 10 broken.
    pub fn offered(&self, documents: &Tools) -> Vec<Spec> {
        self.on_offer()
            .into_iter()
            .filter_map(|tool| {
                let document = documents.get(tool.name())?;
                let shape = tool.parameters();
                Some(Spec {
                    name: tool.name().to_owned(),
                    description: document.description().to_owned(),
                    parameters: document.describe(shape.clone()),
                    shape,
                    document,
                })
            })
            .collect()
    }

    /// Everything on offer, by name, in the registry's own order.
    ///
    /// What a sub-agent's set is intersected against (`demido_permission`'s
    /// `inherit`): the parent's offered set, as the thing that decides what a
    /// child may be shown. On offer rather than registered,
    /// so a conversation with no workspace passes an empty ceiling down rather
    /// than a list of names nothing could have called anyway.
    pub fn offered_names(&self) -> Vec<String> {
        self.on_offer()
            .into_iter()
            .map(|tool| tool.name().to_owned())
            .collect()
    }

    /// Is anything on offer? Answers "should this turn carry tools at all".
    pub fn is_empty(&self) -> bool {
        self.on_offer().is_empty()
    }

    /// Work out what a call would do, without doing it.
    pub fn plan(&self, call: &Call) -> std::result::Result<Planned<'_>, Failure> {
        // Where a tool may act, before which tool: with no workspace nothing
        // was ever offered, so a call naming anything at all is answered with
        // why there is nothing rather than with a name.
        let Some(workspace) = self.workspace.as_ref() else {
            return Err(self.no_such_tool(&call.name));
        };

        // Found among the tools that were on offer, not among the ones that
        // exist, so a call naming a tool the model was never shown is answered
        // the way every other guessed name is.
        let on_offer = self.on_offer();
        let Some(tool) = on_offer.iter().find(|tool| tool.name() == call.name) else {
            return Err(self.no_such_tool(&call.name));
        };

        let parsed: Value = serde_json::from_str(&call.arguments).map_err(|err| {
            // The arguments as the model wrote them are not repeated back: a
            // small model shown its own malformed output tends to produce it
            // again. What it needs is the objection and the shape.
            Failure::retryable(format!(
                "those arguments are not valid JSON ({err}). {} takes {}.",
                tool.name(),
                shape(&tool.parameters())
            ))
        })?;

        let faults = arguments::faults(&parsed, &tool.parameters());
        if !faults.is_empty() {
            return Err(Failure::retryable(format!(
                "{} was called wrongly: {}. It takes {}.",
                tool.name(),
                faults.join("; "),
                shape(&tool.parameters())
            )));
        }

        let context = Context::over(workspace);
        let intent = tool.intent(&parsed, &context);

        Ok(Planned {
            tool: Arc::clone(tool),
            arguments: parsed,
            context,
            intent,
        })
    }

    /// Plan a call and run it, with nothing in between.
    ///
    /// For a caller with no policy to apply: tests, and anywhere the answer to
    /// "may this happen" is already yes. The turn loop uses `plan` instead, so
    /// that the matrix and the approval have somewhere to stand.
    pub async fn run(&self, call: &Call) -> Outcome {
        self.plan(call)?.run().await
    }

    /// What the model may be shown right now.
    fn on_offer(&self) -> Vec<&Arc<dyn Tool>> {
        match self.workspace {
            Some(_) => self.tools.iter().map(|entry| &entry.tool).collect(),
            None => Vec::new(),
        }
    }

    /// The answer to a name that is not a tool.
    ///
    /// It lists what there is, because a model that guessed a name will guess
    /// another one unless it is shown the set.
    fn no_such_tool(&self, asked: &str) -> Failure {
        let on_offer = self.on_offer();
        let names: Vec<&str> = on_offer.iter().map(|tool| tool.name()).collect();

        if names.is_empty() {
            // Why there is nothing, not just that there is nothing. Almost
            // always it is the workspace, and a model told to answer directly
            // without being told why cannot pass the reason on to the user.
            if self.workspace.is_none() {
                return Failure::final_(format!(
                    "there is no tool called {asked}. No workspace is set, so nothing that works \
                     on files is available. Tell the user to set one in Settings, and answer them \
                     directly in the meantime."
                ));
            }
            return Failure::final_(format!(
                "there is no tool called {asked}, and no tools are available at all. Answer the \
                 user directly."
            ));
        }

        Failure::retryable(format!(
            "there is no tool called {asked}. The tools you have are: {}.",
            names.join(", ")
        ))
    }
}

/// A call that has been understood, and not yet made.
///
/// Everything that happens between understanding a call and making it happens
/// here: the matrix reads [`Planned::intent`], the person may be asked about
/// it, and whatever wants to take a copy of what it will touch reads the same
/// declaration.
pub struct Planned<'a> {
    tool: Arc<dyn Tool>,
    arguments: Value,
    context: Context<'a>,
    /// What it will do.
    pub intent: Intent,
}

impl Planned<'_> {
    /// Which tool. Named separately from the intent because a decision is
    /// recorded against a tool, and a summary is prose.
    pub fn tool(&self) -> &str {
        self.tool.name()
    }

    pub async fn run(&self) -> Outcome {
        self.tool.run(&self.arguments, &self.context).await
    }
}

/// By hand, because a tool is a trait object and nothing is gained by making
/// every implementation of it printable. What a reader of a failed test wants
/// is which tool, with what, and what it said it would do.
impl std::fmt::Debug for Planned<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Planned")
            .field("tool", &self.tool.name())
            .field("arguments", &self.arguments)
            .field("intent", &self.intent)
            .finish()
    }
}

/// A schema, said in one line, for a model that has just got it wrong.
///
/// The schema itself was already in the request; repeating it verbatim spends
/// tokens on something the model has demonstrably not read. A sentence naming
/// the properties and which are required is what it needs.
fn shape(schema: &Value) -> String {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "no arguments".to_owned();
    };
    if properties.is_empty() {
        return "no arguments".to_owned();
    }

    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();

    properties
        .iter()
        .map(|(name, schema)| {
            let kind = schema
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("value");
            let need = match required.contains(&name.as_str()) {
                true => "required",
                false => "optional",
            };
            format!("{name} ({kind}, {need})")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::tool::Ability;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    fn call(name: &str, arguments: &str) -> Call {
        Call {
            id: "call-1".to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    #[test]
    fn the_files_group_is_five_tools_and_they_are_the_five_named() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));
        let prompts = tempfile::tempdir().expect("a directory");

        let names: Vec<String> = registry
            .offered(&Tools::open(prompts.path()))
            .into_iter()
            .map(|it| it.name)
            .collect();
        assert_eq!(
            names,
            [
                "read_file",
                "list_directory",
                "search_files",
                "write_file",
                "delete_file"
            ]
        );
    }

    // What every tool promises about its schema and its paths is
    // `contract::holds_for`, called for each of them by
    // `tests/confinement.rs`. Asserting it a second time here would be two
    // places to keep in step, and the tool that skipped one of them would be
    // the tool nobody added to both.

    #[test]
    fn delete_file_is_the_only_tool_that_declares_itself_destructive() {
        // The floor under every mode, read off the declarations rather than
        // off a list of names anybody has to keep in step.
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let destructive: Vec<String> = ["read_file", "list_directory", "write_file", "delete_file"]
            .into_iter()
            .filter(|name| {
                let planned = registry
                    .plan(&call(name, r#"{"path": "README.md", "content": ""}"#))
                    // Only `write_file` takes content, so the others are
                    // planned without it.
                    .or_else(|_| registry.plan(&call(name, r#"{"path": "README.md"}"#)))
                    .expect("a plan");
                planned.intent.destructive
            })
            .map(str::to_owned)
            .collect();

        assert_eq!(destructive, ["delete_file"]);
    }

    #[test]
    fn a_name_that_is_not_a_tool_is_answered_with_the_names_that_are() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let failure = registry
            .plan(&call("read_files", "{}"))
            .expect_err("no such tool");

        assert!(failure.retryable);
        assert!(failure.message.contains("read_file"), "{failure}");
    }

    #[test]
    fn arguments_that_are_not_json_are_answered_with_the_shape_rather_than_the_schema() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let failure = registry
            .plan(&call("read_file", "{path: README.md"))
            .expect_err("not JSON");

        assert!(
            failure.message.contains("path (string, required)"),
            "{failure}"
        );
    }

    #[test]
    fn every_fault_in_a_call_comes_back_at_once() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let failure = registry
            .plan(&call(
                "read_file",
                r#"{"from_line": "two", "colour": "red"}"#,
            ))
            .expect_err("wrong");

        assert!(failure.message.contains("path is required"), "{failure}");
        assert!(
            failure.message.contains("colour is not one of"),
            "{failure}"
        );
        assert!(failure.message.contains("from_line has to be"), "{failure}");
    }

    #[tokio::test]
    async fn a_call_that_fits_runs_and_answers() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let answer = registry
            .run(&call("read_file", r#"{"path": "README.md"}"#))
            .await
            .unwrap();
        assert!(answer.contains("Hello"), "{answer}");
    }

    #[test]
    fn a_registry_with_no_workspace_offers_nothing_and_says_why() {
        // A model shown a tool will call it, and a call that cannot succeed
        // however it is written is worse than a tool never mentioned.
        let registry = Registry::of_files(None);

        assert!(registry.is_empty());
        let failure = registry
            .plan(&call("read_file", r#"{"path": "README.md"}"#))
            .expect_err("nothing on offer");
        assert!(!failure.retryable, "no wording of the arguments fixes this");
        assert!(failure.message.contains("No workspace is set"), "{failure}");
    }

    #[tokio::test]
    async fn run_command_is_registered_as_the_shell_group_and_runs_from_a_call() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace)).with_group(shell());

        let planned = registry
            .plan(&call("run_command", r#"{"command": "echo registered"}"#))
            .expect("a plan");
        assert_eq!(planned.intent.ability, Ability::Shell);

        let answer = planned.run().await.expect("it ran");
        assert!(answer.contains("registered"), "{answer}");
    }

    #[test]
    fn a_later_registration_of_a_name_replaces_the_earlier_one() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace)).with_group(files());
        let prompts = tempfile::tempdir().expect("a directory");

        assert_eq!(registry.offered(&Tools::open(prompts.path())).len(), 5);
    }

    /// What the picker draws: groups in the order they were registered, each
    /// with its tools, whether or not a workspace is set. Switching a tool on
    /// is a choice a person can make before there is anywhere for it to act.
    #[test]
    fn the_groups_are_named_and_listed_whether_or_not_there_is_a_workspace() {
        let registry = Registry::of_files(None)
            .with_group(shell())
            .with_group(delegation(nothing()));

        assert_eq!(
            registry.groups(),
            vec![
                (
                    "files",
                    vec![
                        "read_file".to_owned(),
                        "list_directory".to_owned(),
                        "search_files".to_owned(),
                        "write_file".to_owned(),
                        "delete_file".to_owned()
                    ]
                ),
                ("shell", vec!["run_command".to_owned()]),
                ("delegation", vec!["delegate_task".to_owned()]),
            ]
        );
    }

    /// Delegation is one row and one entry, ruled on like any other call: the
    /// registry hands back an intent declaring the shell, and stops there.
    #[tokio::test]
    async fn delegate_task_is_one_registry_entry_declaring_the_shell() {
        let (_dir, workspace) = workspace();
        let registry =
            Registry::of_files(Some(workspace)).with_group(delegation(crate::delegating(
                |task: String| async move { Ok(format!("the sub-agent read {task}")) },
            )));

        let planned = registry
            .plan(&call("delegate_task", r#"{"task": "the changelog"}"#))
            .expect("a plan");
        assert_eq!(planned.intent.ability, Ability::Shell);
        assert!(!planned.intent.destructive);

        let answer = planned.run().await.expect("it ran");
        assert!(answer.contains("the changelog"), "{answer}");
    }

    /// The same absence the picker produces for every other tool: switched off
    /// is not offered and not planned either.
    #[test]
    fn a_narrowed_registry_leaves_delegation_out_the_way_it_leaves_any_group_out() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace)).with_group(delegation(nothing()));
        let narrowed = registry.only(&["read_file".to_owned()]);

        assert!(registry.offers("delegate_task"));
        assert!(!narrowed.offers("delegate_task"));
        assert!(narrowed
            .plan(&call("delegate_task", r#"{"task": "anything"}"#))
            .is_err());
    }

    fn nothing() -> crate::Delegating {
        crate::delegating(|_| async { Ok(String::new()) })
    }

    /// Narrowed is absent: not offered, and not planned either, so a call
    /// naming a tool switched off never reaches the matrix.
    #[test]
    fn a_narrowed_registry_neither_offers_nor_plans_what_it_left_out() {
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace)).with_group(shell());
        let prompts = tempfile::tempdir().expect("a directory");
        let narrowed = registry.only(&["read_file".to_owned()]);

        let names: Vec<String> = narrowed
            .offered(&Tools::open(prompts.path()))
            .into_iter()
            .map(|it| it.name)
            .collect();
        assert_eq!(names, ["read_file"]);
        assert!(narrowed
            .plan(&call("run_command", r#"{"command": "echo no"}"#))
            .is_err());
        assert!(registry.offers("run_command"));
        assert!(!narrowed.offers("run_command"));
        assert!(
            !registry.offers("read_files"),
            "a guessed name is not a tool"
        );
    }

    #[test]
    fn a_plan_carries_the_intent_and_stops_there() {
        // The registry understands a call and does not decide about it. What
        // it hands back is what the matrix reads.
        let (_dir, workspace) = workspace();
        let registry = Registry::of_files(Some(workspace));

        let planned = registry
            .plan(&call(
                "write_file",
                r#"{"path": "new.txt", "content": "hi"}"#,
            ))
            .expect("a plan");

        assert_eq!(planned.tool(), "write_file");
        assert_eq!(planned.intent.ability, Ability::Write);
        assert!(!planned.intent.destructive);
    }
}
