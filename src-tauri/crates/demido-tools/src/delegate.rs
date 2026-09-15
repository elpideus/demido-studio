//! `delegate_task`: handing a piece of work to a sub-agent, as a tool.
//!
//! The Delegation group is one tool, and it is a registry entry like any other
//! ([#61](https://github.com/elpideus/demido-studio/issues/61)). That is the
//! whole point of it being here: delegation arrives through the machinery S2
//! built rather than beside it, so a person switches it off in the same picker
//! row they switch the shell off in, a model that names it while it is off is
//! told the user turned it off by the same path, and the mode rules on it
//! through the same matrix.
//!
//! **It declares [`Ability::Shell`]**, per
//! [`docs/rules/tools.md`](../../../../docs/rules/tools.md): *"`delegate_task`
//! declares `Ability::Shell`, so Cautious and Balanced ask before the first
//! delegation of a turn and Autonomous does not."* No fifth ability joins the
//! matrix, and this tool reads no mode, exactly like every other one.
//!
//! **What actually happens when it runs is not here.** The child session, its
//! log, the depth limit and the pool are the turn loop's
//! ([#63](https://github.com/elpideus/demido-studio/issues/63) onwards), and
//! they need a session, a backend and a settings ladder, none of which a tool
//! may know about. So this tool holds a [`Delegating`], the callback that
//! carries a task out and answers, in the shape
//! [`crate::Tool`] already speaks: a task description in, an [`Outcome`] out.
//! A callback rather than a trait, for the reason the approval is one
//! ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md)): there is one real
//! implementation and a trait would buy a second that exists only in tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::tool::{Ability, Context, Failure, Intent, Outcome, Tool};

/// A delegation in flight. Boxed because it outlives the call that built it.
pub type Delegated = Pin<Box<dyn Future<Output = Outcome> + Send>>;

/// How a delegated task is actually carried out: the task, and what the
/// sub-agent answered.
///
/// Shared rather than owned, because the registry a turn narrows is cloned per
/// turn and every clone delegates to the same place.
pub type Delegating = Arc<dyn Fn(String) -> Delegated + Send + Sync>;

/// Build a [`Delegating`] out of an ordinary async function.
///
/// Here so that a caller writes what a delegation does rather than how a future
/// is boxed, and so the boxing is spelled once.
pub fn delegating<F, Fut>(run: F) -> Delegating
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Outcome> + Send + 'static,
{
    Arc::new(move |task| Box::pin(run(task)))
}

/// A person approving a delegation is reading one line, so the task it names
/// has to stop being a task description and start being a phrase.
const SUMMARY_LIMIT: usize = 120;

/// Hand a self-contained piece of work to a sub-agent.
///
/// What it says to the model is the tool register's
/// ([`0008`](../../../../docs/decisions/0008-a-tool-description-is-a-prompt.md)),
/// under the id `delegate_task`, which is also this tool's name. Of every
/// document Demido ships this is the one most likely to be reworded, which is
/// why it is an entry from its first commit rather than a literal somebody
/// retrofits later.
pub struct DelegateTask {
    delegating: Delegating,
}

impl DelegateTask {
    /// What the model calls it, what the log records, and what the tool
    /// register keys its document by.
    ///
    /// A constant rather than a literal in three crates: `demido-permission`
    /// needs it to know which tool a person is asked about once per turn, and a
    /// name spelled twice is a name that can be spelled differently.
    pub const NAME: &'static str = "delegate_task";

    /// A delegation tool that hands its tasks to `delegating`.
    pub fn to(delegating: Delegating) -> Self {
        Self { delegating }
    }
}

#[async_trait]
impl Tool for DelegateTask {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" },
            },
            "required": ["task"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, _: &Context<'_>) -> Intent {
        let task = arguments["task"].as_str().unwrap_or_default();
        Intent {
            ability: Ability::Shell,
            summary: format!("Delegate: {}", phrase(task)),
            // Not at this declaration. What the sub-agent itself does is ruled
            // on call by call, by the same matrix and the same person, and the
            // destructive floor is under the child as much as under the parent.
            // Declaring the delegation destructive would ask twice for one
            // thing and still not cover the call that mattered.
            destructive: false,
            // Nothing this call changes can be named before the sub-agent has
            // decided what to do, so nothing is claimed. The child's own calls
            // declare what they touch, as any call does.
            touches: Vec::new(),
        }
    }

    async fn run(&self, arguments: &Value, _: &Context<'_>) -> Outcome {
        let task = arguments["task"].as_str().unwrap_or_default().trim();
        if task.is_empty() {
            // not-a-prompt: a tool result naming what was wrong with this call.
            return Err(Failure::retryable("there is no task to delegate."));
        }
        (self.delegating)(task.to_owned()).await
    }
}

/// A task description, cut to something a person can read in a row.
///
/// Cut on a character boundary and on a word where there is one nearby, because
/// the approval row is the one place a person decides and half a word there
/// reads as a broken window rather than as a long task.
fn phrase(task: &str) -> String {
    let task = task.split_whitespace().collect::<Vec<_>>().join(" ");
    if task.chars().count() <= SUMMARY_LIMIT {
        return task;
    }

    let kept: String = task.chars().take(SUMMARY_LIMIT).collect();
    let cut = match kept.rsplit_once(' ') {
        Some((head, _)) if head.chars().count() >= SUMMARY_LIMIT / 2 => head,
        _ => kept.as_str(),
    };
    format!("{}...", cut.trim_end())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::workspace::Workspace;
    use serde_json::json;

    fn answering(text: &'static str) -> DelegateTask {
        DelegateTask::to(delegating(move |_| async move { Ok(text.to_owned()) }))
    }

    fn rig() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    #[test]
    fn a_delegation_declares_the_shell_and_nothing_it_cannot_put_back() {
        // The whole of the mode's involvement in delegating: one ability the
        // matrix already has a column for, and no new axis.
        let (_dir, workspace) = rig();
        let tool = answering("done");

        let intent = tool.intent(
            &json!({ "task": "Find every caller of resolve" }),
            &Context::over(&workspace),
        );

        assert_eq!(intent.ability, Ability::Shell);
        assert!(!intent.destructive);
        assert!(intent.touches.is_empty());
        assert!(
            intent.summary.contains("Find every caller"),
            "{}",
            intent.summary
        );
    }

    #[test]
    fn a_long_task_is_one_line_by_the_time_a_person_reads_it() {
        let (_dir, workspace) = rig();
        let long = "Read every file under src and report back ".repeat(20);

        let summary = answering("done")
            .intent(&json!({ "task": long }), &Context::over(&workspace))
            .summary;

        assert!(summary.chars().count() <= SUMMARY_LIMIT + 16, "{summary}");
        assert!(summary.ends_with("..."), "{summary}");
    }

    #[test]
    fn a_task_written_over_several_lines_still_summarises_to_a_row() {
        let (_dir, workspace) = rig();
        let summary = answering("done")
            .intent(
                &json!({ "task": "Find the bug.\n\nThen fix it." }),
                &Context::over(&workspace),
            )
            .summary;

        assert_eq!(summary, "Delegate: Find the bug. Then fix it.");
    }

    #[tokio::test]
    async fn a_task_reaches_whatever_carries_it_out_and_its_answer_is_the_result() {
        let (_dir, workspace) = rig();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let taken = seen.clone();
        let tool = DelegateTask::to(delegating(move |task: String| {
            let taken = taken.clone();
            async move {
                taken.lock().unwrap().push(task);
                Ok("the sub-agent's answer".to_owned())
            }
        }));

        let answer = tool
            .run(
                &json!({ "task": "  Count the tests  " }),
                &Context::over(&workspace),
            )
            .await
            .expect("an answer");

        assert_eq!(answer, "the sub-agent's answer");
        assert_eq!(seen.lock().unwrap().as_slice(), ["Count the tests"]);
    }

    #[tokio::test]
    async fn an_empty_task_is_refused_before_anything_is_delegated() {
        let (_dir, workspace) = rig();
        let tool = DelegateTask::to(delegating(|_| async {
            panic!("nothing should have been delegated")
        }));

        let failure = tool
            .run(&json!({ "task": "   " }), &Context::over(&workspace))
            .await
            .expect_err("nothing to do");

        assert!(failure.retryable, "writing a task fixes this");
    }
}
