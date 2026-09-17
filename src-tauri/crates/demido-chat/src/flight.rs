//! The delegations a turn started and has not folded in yet.
//!
//! Above the default parallelism `delegate_task` does not block
//! ([#66](https://github.com/elpideus/demido-studio/issues/66)): the call is
//! answered at once, because a request whose assistant message asks for a call
//! nothing answered is one no compatible server accepts, and the sub-agent runs
//! beside the turn that asked for it. What it answered arrives later, as a
//! message rather than as a second result for a call that already has one.
//!
//! **A step boundary is the only place that answer may arrive.** That is what
//! this module exists to make true, and it is why the determinism here comes
//! from ordering rather than from timing: a background answer is buffered the
//! moment the child finishes and written only where the loop asks for it, which
//! is after every call of a step has been answered. Nothing holds a clock still
//! and nothing reaches into a scheduler.
//!
//! **Nothing may be dropped.** A run whose model stops asking for tools waits
//! for what is still in flight, and so does one that has used its last step: a
//! delegation nobody mentions again is a silent loss, and a ceiling that ate an
//! answer would be the worse of the two. [`Flight::settle`] is that wait, and
//! the loop has no path to an ending that does not go through it.
//!
//! **A sub-agent advances while the conversation is talking to the model.** It
//! is set down while the conversation is running a tool of its own, because
//! that is where the parent is awaiting a future of its own and the two would
//! otherwise interleave for no gain anybody could observe. [`Flight::beside`]
//! is the one place a child is polled at all.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use demido_tools::Outcome;
use demido_trace::AgentId;

use crate::chat::Result;

/// How many sub-agents may run beside the conversation at once.
///
/// **The number counts slots and the conversation is the first of them**
/// (`docs/rules/tools.md`), so what is here is one fewer than the slots the
/// backend reported opening. At one slot there is nothing to hold and every
/// delegation blocks, which is the default and the only path the reference
/// model can afford on the rig.
///
/// The slots are the ones `Presence::Ready` carries, which are the ones the
/// running backend says it opened rather than the ones `tools.parallel_agents`
/// asked for. A parallelism the card could not honour is already a queue by the
/// time it reaches here.
pub(crate) struct Slots {
    /// `None` at the default, so "is this on" is one check rather than a
    /// capacity of zero every caller has to remember means something else.
    free: Option<Arc<Semaphore>>,
}

impl Slots {
    /// The sub-agent slots under a backend that opened `slots` of them.
    pub(crate) fn under(slots: u32) -> Self {
        let spare = slots.saturating_sub(1) as usize;
        Self {
            free: (spare > 0).then(|| Arc::new(Semaphore::new(spare))),
        }
    }

    /// Take a slot, if delegating here runs beside the conversation at all and
    /// one is free.
    ///
    /// `None` is the synchronous path, and it is the answer in two different
    /// situations on purpose: at the default there is no pool, and at a pool
    /// that is full there is no room in it. Both end in a delegation that
    /// blocks and answers as its call's own result, which is a path the loop
    /// already has. The model is never told there is no room, because *try
    /// again later* is not something a small model does anything sensible with.
    pub(crate) fn take(&self) -> Option<OwnedSemaphorePermit> {
        self.free.clone()?.try_acquire_owned().ok()
    }
}

/// What a background sub-agent's run leaves for the turn that asked for it.
pub(crate) struct Harvest {
    /// The `tool/call` that asked for the delegation. Also what orders two
    /// answers folded in at one boundary: a call's position is the order the
    /// model asked in, which is the order the log already reads in.
    pub(crate) call: u64,
    /// Whose answer it is.
    pub(crate) agent: AgentId,
    /// The task as the call wrote it, quoted back in the frame so a model with
    /// two sub-agents out can tell which one is talking.
    pub(crate) task: String,
    /// The child's own answer, by position on its half of the log. What
    /// `agent/returned` names rather than copies.
    ///
    /// `None` only where the child's log refused the event that would have been
    /// its answer, which is the staged full disk of `tests/delegated.rs`. There
    /// is then nothing on the log to name, so nothing claims there is: the
    /// answer is still framed for the model, and the fact that it came back is
    /// the one thing that goes unrecorded, because recording it would mean
    /// pointing at an event that is not there.
    pub(crate) answer: Option<u64>,
    /// What it said, or what went wrong with it.
    pub(crate) outcome: Outcome,
}

impl Harvest {
    /// The text the frame is filled with, which is the answer or the objection.
    pub(crate) fn text(&self) -> &str {
        match &self.outcome {
            Ok(text) => text,
            Err(failure) => &failure.message,
        }
    }
}

/// The delegations of one agent's run: what is still going, and what has
/// finished and is waiting for a boundary.
pub(crate) struct Flight<'a> {
    running: FuturesUnordered<Pin<Box<dyn Future<Output = Result<Harvest>> + Send + 'a>>>,
    /// Finished, unwritten. Never longer than the delegations one turn asked
    /// for, and drained at the next boundary.
    done: Vec<Result<Harvest>>,
}

impl<'a> Flight<'a> {
    pub(crate) fn empty() -> Self {
        Self {
            running: FuturesUnordered::new(),
            done: Vec::new(),
        }
    }

    /// Whether anything is still running or waiting to be written down.
    ///
    /// The question a run asks before it is allowed to end.
    pub(crate) fn is_busy(&self) -> bool {
        !self.running.is_empty() || !self.done.is_empty()
    }

    /// Put one sub-agent's run in the air.
    pub(crate) fn start(&mut self, work: impl Future<Output = Result<Harvest>> + Send + 'a) {
        self.running.push(Box::pin(work));
    }

    /// Await `main`, letting whatever is in flight advance beside it.
    ///
    /// Nothing is written here: a child that finishes mid-generation is put in
    /// `done` and stays there until the loop asks. That is the whole of *a step
    /// boundary is the only place a background answer may arrive*, and it is an
    /// ordering rather than a timing, so a test asserts it on the log without
    /// touching the clock.
    pub(crate) async fn beside<T>(&mut self, main: impl Future<Output = T>) -> T {
        let mut main = std::pin::pin!(main);
        loop {
            if self.running.is_empty() {
                return main.await;
            }
            let finished = tokio::select! {
                // The turn's own work first, so a generation that is ready is
                // read rather than made to wait behind a sub-agent.
                biased;
                out = &mut main => return out,
                Some(finished) = self.running.next() => finished,
            };
            self.done.push(finished);
        }
    }

    /// Wait for everything still running, and keep its answers.
    ///
    /// What stands between a background delegation and a silent loss. A run
    /// that has nothing left to say does not end while a sub-agent it started
    /// is still talking, and neither does one that has used its last step.
    pub(crate) async fn settle(&mut self) {
        while let Some(finished) = self.running.next().await {
            self.done.push(finished);
        }
    }

    /// Everything that has finished since the last boundary, in the order the
    /// model asked for it.
    ///
    /// Ordered by the call rather than by which child finished first, so what
    /// the log reads is what the conversation asked for and not what the
    /// scheduler happened to do. Two answers folded in at one boundary are in
    /// the order their delegations are on the log.
    pub(crate) fn take(&mut self) -> Result<Vec<Harvest>> {
        let mut harvested: Vec<Harvest> = std::mem::take(&mut self.done)
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        harvested.sort_by_key(|one| one.call);
        Ok(harvested)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    /// The conversation's own slot is not a sub-agent's, so the default holds
    /// nothing and every delegation blocks.
    #[test]
    fn the_default_is_not_a_pool_at_all() {
        assert!(Slots::under(1).take().is_none());
        assert!(Slots::under(0).take().is_none());
    }

    /// Two slots is one sub-agent beside the conversation, and a second
    /// delegation while it is out blocks rather than being refused.
    #[test]
    fn a_pool_that_is_full_sends_the_next_delegation_down_the_blocking_path() {
        let slots = Slots::under(2);
        let held = slots.take().expect("one sub-agent beside the conversation");
        assert!(slots.take().is_none(), "the second waits its turn");
        drop(held);
        assert!(slots.take().is_some(), "and gets the slot back");
    }

    /// The buffer is ordered by the call, never by which child answered first.
    #[tokio::test]
    async fn two_answers_at_one_boundary_are_in_the_order_they_were_asked_for() {
        let mut flight = Flight::empty();
        flight.start(async { Ok(harvest(9)) });
        flight.start(async { Ok(harvest(4)) });
        flight.settle().await;

        let calls: Vec<u64> = flight.take().unwrap().iter().map(|one| one.call).collect();
        assert_eq!(calls, [4, 9]);
    }

    /// Nothing is taken until it is asked for, which is what makes the boundary
    /// the only place an answer arrives.
    #[tokio::test]
    async fn a_child_that_finished_waits_for_the_loop_to_ask() {
        let mut flight = Flight::empty();
        flight.start(async { Ok(harvest(1)) });

        let said = flight
            .beside(async {
                // A generation is not ready the instant it is asked for, and
                // this is what stands in for that without a clock: the child is
                // polled where a real one would be, between two chunks.
                for _ in 0..4 {
                    tokio::task::yield_now().await;
                }
                "the generation finished"
            })
            .await;

        assert_eq!(said, "the generation finished");
        assert!(flight.is_busy(), "it finished, and it is still unwritten");
        assert_eq!(flight.take().unwrap().len(), 1);
        assert!(!flight.is_busy());
    }

    fn harvest(call: u64) -> Harvest {
        Harvest {
            call,
            agent: AgentId::delegated(call),
            task: "look something up".to_owned(),
            answer: Some(call + 1),
            outcome: Ok("done".to_owned()),
        }
    }
}
