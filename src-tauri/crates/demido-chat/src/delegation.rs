//! The two ends of a delegation: the tool's, and the turn loop's.
//!
//! [#61](https://github.com/elpideus/demido-studio/issues/61) built
//! `delegate_task` as a registry entry holding a
//! [`demido_tools::Delegating`], "the callback that carries a task out and
//! answers". This module is the real one, and it is a pair rather than a
//! closure for one reason: **what carries a task out is the turn that is
//! running**.
//!
//! A sub-agent needs the conversation's backend, its ladder, its session, the
//! cancellation token of the turn it was asked for in, the sink the window is
//! reading and the person the approval goes to. The last two are borrowed for
//! the length of one `Chat::ask` call, and a registry entry outlives every turn
//! it is offered in, so a closure in the registry cannot hold them. Rather than
//! widen every callback in this crate to an owned, shared thing so that a tool
//! can reach them, the tool asks and the loop answers: [`Delegating`]'s end
//! sends the task, [`Delegations`]'s end is polled by the loop beside the call
//! it is running, and the answer comes back on the same rendezvous the approval
//! already uses.
//!
//! So the child runs on the parent's own stack, inside the turn that asked for
//! it, with everything that turn has. "At the default parallelism the call
//! **blocks** and the answer is the tool's result the ordinary way" is then not
//! a rule anybody keeps: it is the only shape this pair has.
//!
//! **Both ends are made together** ([`delegations`]) because a tool wired to
//! one conversation's loop and registered on another's is a delegation that
//! answers in the wrong session. The composition root splits the pair between
//! the registry and the chat in the two lines that follow each other.

use demido_tools::{delegating, Delegating, Failure, Outcome};
use tokio::sync::{mpsc, oneshot};

/// One delegation the tool has asked for: the task, and where its answer goes.
///
/// The answer is a `oneshot` for the reason the approval's is: a delegation is
/// a question with one answer, and a channel that could deliver two would be a
/// second sub-agent nobody asked for.
pub struct Asked {
    task: String,
    answered: oneshot::Sender<Outcome>,
}

impl Asked {
    /// The task, as the call wrote it and as the person approved it.
    #[must_use]
    pub fn task(&self) -> &str {
        &self.task
    }

    /// Hand the sub-agent's answer back to the call that asked for it.
    ///
    /// A send that fails is a turn that stopped waiting, which a stop does. The
    /// answer is dropped and nothing is recorded twice, which is the right
    /// outcome and not an error worth reporting.
    pub fn answer(self, outcome: Outcome) {
        let _ = self.answered.send(outcome);
    }
}

/// The turn loop's end: the delegations `delegate_task` has asked for, in
/// order.
///
/// Held by the [`crate::Chat`] and borrowed for the length of a turn, which is
/// what makes it a queue of one in practice: the loop dispatches calls one at a
/// time, so there is never a second task waiting while the first is carried
/// out.
pub struct Delegations {
    /// A sender of its own, so the queue never closes.
    ///
    /// A conversation whose registry holds no Delegation group would otherwise
    /// have a receiver whose only sender was dropped, and that answers `None`
    /// at once, every time: an arm of the `select!` beside a running tool that
    /// resolves instantly is a turn that spins instead of waiting. Holding one
    /// end here makes "nobody will ask" pend forever, which is what that arm
    /// needs it to do.
    ///
    /// Underscored because it is held for its effect on the channel and never
    /// read: nothing is ever sent on it, and that is the point.
    _never: mpsc::Sender<Asked>,
    asked: mpsc::Receiver<Asked>,
}

impl Delegations {
    /// The loop's end of a pair whose tool end nothing holds: a conversation
    /// with no Delegation group in its registry.
    ///
    /// Named rather than reached by taking one half of [`delegations`] and
    /// dropping the other, because "this conversation delegates nowhere" is
    /// what a reader needs to see. Nothing ever asks on it, and waiting on it
    /// waits forever, which is what a turn's `select!` arm needs.
    #[must_use]
    pub fn none() -> Self {
        delegations().1
    }

    /// The next delegation asked for. Pends forever when nothing will ask.
    pub(crate) async fn next(&mut self) -> Asked {
        match self.asked.recv().await {
            Some(asked) => asked,
            // Unreachable while `_never` is held, and a pend rather than a
            // panic if it ever is not: a turn that waits is recoverable by the
            // stop that is already wired, and a turn that panics takes the
            // window with it.
            None => std::future::pending().await,
        }
    }
}

/// Both ends of a delegation, made together: the tool's, for the registry, and
/// the loop's, for the chat.
///
/// ```ignore
/// let (delegating, delegations) = demido_chat::delegations();
/// let registry = Registry::open(workspace).with_group(demido_tools::delegation(delegating));
/// let chat = Chat::new(id, open, supervisor, model, settings, toolbox, delegations);
/// ```
#[must_use]
pub fn delegations() -> (Delegating, Delegations) {
    // One at a time, because the loop dispatches one call at a time. A deeper
    // queue would be a buffer for something that cannot queue.
    let (asks, asked) = mpsc::channel::<Asked>(1);
    let never = asks.clone();
    let delegating = delegating(move |task: String| {
        let asks = asks.clone();
        async move {
            let (answer, answered) = oneshot::channel();
            asks.send(Asked {
                task,
                answered: answer,
            })
            .await
            // not-a-prompt: the objection a tool result carries when the two
            // halves of this pair were never put where they belong, which is a
            // build that wired one and not the other.
            .map_err(|_| Failure::final_("delegation is not available here."))?;
            // The turn is what answers. A receiver dropped without one is a
            // turn that ended under the call, which a stop does: the call is
            // answered as stopped by the loop and this result is never
            // recorded.
            answered
                .await
                // not-a-prompt: as above, for a turn that went away mid
                // delegation.
                .unwrap_or_else(|_| Err(Failure::final_("the sub-agent did not answer.")))
        }
    });
    (
        delegating,
        Delegations {
            _never: never,
            asked,
        },
    )
}
