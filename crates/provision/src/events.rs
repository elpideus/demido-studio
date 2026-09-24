use serde::{Deserialize, Serialize};

use crate::plan::{StepId, StepInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

/// What the engine reports while it works. Serialized as-is to the installer's UI.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProvisionEvent {
    /// Sent once, first: the steps that will run.
    Plan { steps: Vec<StepInfo> },
    Step {
        id: StepId,
        state: StepState,
        /// Failure reason, or a short note for skipped steps.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Progress {
        id: StepId,
        done: u64,
        total: Option<u64>,
        bytes_per_second: f64,
        /// What is happening right now, e.g. `Downloading cudart-llama-bin-win-cuda-13.4-x64.zip`.
        activity: String,
    },
    Log { id: StepId, line: String },
    /// Sent once, last.
    Finished {
        success: bool,
        failed: Vec<StepId>,
    },
}
