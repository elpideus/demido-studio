//! Which steps remain between a fresh profile and a model that has answered.
//!
//! **The wizard is a plan, not a sequence of screens.** The steps are derived
//! from what is true right now, so re-entering after a half-finished set-up
//! resumes instead of restarting, and the desk's persistent row is the same
//! list rather than a second one.
//!
//! `docs/rules/setup.md` section 1: what is outstanding is **derived from disk
//! rather than remembered**, "so it cannot go stale against what is actually
//! there". That is this file's whole shape: [`Situation`] is observed, never
//! stored, and [`Plan::of`] takes it with the answers and computes. There is
//! no `finished` flag anywhere in this crate, because a flag is a second copy
//! of a fact the disk already holds, and the two would eventually disagree:
//! a person who deletes their runtimes folder has not finished set-up,
//! whatever a flag says.

use demido_runtimes::{Ledger, RowState, LLAMA_CPP};
use serde::Serialize;

use crate::answers::Answers;

/// One thing the person has to settle.
///
/// The order is the order `docs/rules/setup.md` and
/// [#48](https://github.com/elpideus/demido-studio/issues/48) put them in:
/// the accelerator, then what has to be fetched, then where models are read
/// from, then a model that answers.
///
/// There is no route step. v2 asked managed-or-connected first and section 8
/// takes the branch instead of presenting it: "A new user cannot answer
/// whether they run their own inference server, and asking puts the vocabulary
/// of the entire product on screen one." Pointing at a server is a link on the
/// runtime row, which is [`Step::Runtimes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    /// Which accelerator a fetched build is for. Opens already answered.
    Accelerator,
    /// What has to arrive: the required group, and the capabilities beside it.
    Runtimes,
    /// Where models are read from, and which one answers.
    Models,
    /// A model loaded, which is the only thing that finishes set-up.
    FirstAnswer,
}

impl Step {
    /// Every step, in the order a person meets them.
    pub const ALL: [Step; 4] = [
        Step::Accelerator,
        Step::Runtimes,
        Step::Models,
        Step::FirstAnswer,
    ];
}

/// Where a step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    /// Settled. Still shown, with its answer, so it can be revisited.
    Done,
    /// The first one that is not.
    Current,
    /// Waiting on the steps above it.
    Later,
}

/// One step, and where it stands.
///
/// `answer` is a fact rather than a sentence, for the reason
/// `demido_hardware::Note` gives: the window owns the words. What crosses here
/// is a path, an accelerator or a pin, and the wizard writes the line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Planned {
    pub step: Step,
    pub standing: Standing,
}

/// What is true about this profile right now, read from disk by the caller.
///
/// **Facts only.** Nothing here is a choice, which is what keeps this crate
/// testable against a machine nobody owns, and what makes the plan derived
/// rather than remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Situation {
    /// Whether the `llama.cpp` row is one that runs: managed at a pin, or
    /// linked to a binary the person pointed at. Both are runnable, which is
    /// the only question the plan asks.
    pub backend_ready: bool,
    /// Whether the chosen model is a file that is still there.
    pub model_on_disk: bool,
}

impl Situation {
    /// Read the situation out of what the disk says.
    ///
    /// The ledger rather than a boolean, because "is the backend installed" is
    /// a question the runtimes ledger already answers and answering it twice
    /// is how the two answers come apart.
    pub fn observe(ledger: &Ledger, answers: &Answers) -> Self {
        let backend_ready = matches!(
            ledger.state(LLAMA_CPP),
            Some(RowState::Managed { .. } | RowState::Linked { .. })
        );
        let model_on_disk = answers.model.as_ref().is_some_and(|model| model.is_file());
        Self {
            backend_ready,
            model_on_disk,
        }
    }
}

/// The steps, in order, with where each stands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub steps: Vec<Planned>,
}

impl Plan {
    /// The plan for this situation and these answers.
    pub fn of(situation: &Situation, answers: &Answers) -> Self {
        // The accelerator step is done as soon as it has been seen with an
        // answer in it, and it always has one: detection pre-selects, so this
        // step is settled from the moment the wizard draws it and is still a
        // row a person can override (`docs/rules/setup.md` section 3). What
        // marks it settled is the person having chosen, or having moved past
        // it by settling something below it.
        let accelerator = answers.ecosystem.is_some();
        let done = [
            accelerator,
            situation.backend_ready,
            situation.model_on_disk,
            // The first answer is the model being loadable, which is the two
            // above it being true together. It is not a fifth stored fact:
            // a wizard that ticked "answered" would be claiming a model
            // spoke on a machine where the weights have since been deleted.
            situation.backend_ready && situation.model_on_disk,
        ];

        let mut current_taken = false;
        let steps = Step::ALL
            .iter()
            .zip(done)
            .map(|(step, done)| {
                let standing = if done {
                    Standing::Done
                } else if current_taken {
                    Standing::Later
                } else {
                    current_taken = true;
                    Standing::Current
                };
                Planned {
                    step: *step,
                    standing,
                }
            })
            .collect();

        Self { steps }
    }

    /// Every step still to settle, in order. What the desk's persistent row
    /// offers, and it is this same list rather than a second one.
    pub fn outstanding(&self) -> Vec<Step> {
        self.steps
            .iter()
            .filter(|planned| planned.standing != Standing::Done)
            .map(|planned| planned.step)
            .collect()
    }

    /// Nothing is outstanding.
    pub fn complete(&self) -> bool {
        self.outstanding().is_empty()
    }

    /// The step the wizard opens on.
    pub fn current(&self) -> Option<Step> {
        self.steps
            .iter()
            .find(|planned| planned.standing == Standing::Current)
            .map(|planned| planned.step)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use std::path::PathBuf;

    use demido_hardware::Ecosystem;

    use super::*;

    fn nothing() -> Situation {
        Situation {
            backend_ready: false,
            model_on_disk: false,
        }
    }

    #[test]
    fn a_fresh_profile_opens_on_the_accelerator() {
        let plan = Plan::of(&nothing(), &Answers::default());
        assert_eq!(plan.current(), Some(Step::Accelerator));
        assert_eq!(plan.outstanding().len(), 4);
        assert!(!plan.complete());
    }

    #[test]
    fn a_half_finished_set_up_resumes_where_it_stopped() {
        let answers = Answers {
            ecosystem: Some(Ecosystem::Cuda),
            ..Answers::default()
        };
        let situation = Situation {
            backend_ready: true,
            ..nothing()
        };
        let plan = Plan::of(&situation, &answers);
        assert_eq!(plan.current(), Some(Step::Models));
        assert_eq!(plan.outstanding(), vec![Step::Models, Step::FirstAnswer]);
    }

    /// The rule this whole file exists for. A profile that had everything and
    /// then lost its runtimes folder is a profile with set-up outstanding
    /// again, and nothing has to be un-remembered for that to be true.
    #[test]
    fn a_deleted_runtime_puts_the_step_back_without_anything_being_forgotten() {
        let answers = Answers {
            ecosystem: Some(Ecosystem::Cuda),
            model: Some(PathBuf::from("D:/models/gemma.gguf")),
            ..Answers::default()
        };

        let installed = Situation {
            backend_ready: true,
            model_on_disk: true,
        };
        assert!(Plan::of(&installed, &answers).complete());

        let deleted = Situation {
            backend_ready: false,
            ..installed
        };
        let plan = Plan::of(&deleted, &answers);
        assert_eq!(plan.current(), Some(Step::Runtimes));
        assert!(!plan.complete(), "the answers did not change; the disk did");
    }

    #[test]
    fn a_finished_set_up_has_no_current_step() {
        let answers = Answers {
            ecosystem: Some(Ecosystem::Cpu),
            model: Some(PathBuf::from("D:/models/gemma.gguf")),
            ..Answers::default()
        };
        let plan = Plan::of(
            &Situation {
                backend_ready: true,
                model_on_disk: true,
            },
            &answers,
        );
        assert_eq!(plan.current(), None);
        assert!(plan.complete());
    }

    #[test]
    fn the_situation_reads_the_backend_out_of_the_ledger_rather_than_a_flag() {
        let mut ledger = Ledger::default();
        ledger.set(
            LLAMA_CPP,
            RowState::Linked {
                path: PathBuf::from("C:/tools/llama-server.exe"),
                detected_version: None,
            },
        );
        let situation = Situation::observe(&ledger, &Answers::default());
        assert!(
            situation.backend_ready,
            "a linked binary runs, so the step is settled"
        );
    }
}
