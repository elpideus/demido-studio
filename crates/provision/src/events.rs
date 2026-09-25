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
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProvisionEvent {
    /// Sent once, first: the steps that will run.
    Plan {
        steps: Vec<StepInfo>,
    },
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
    Log {
        id: StepId,
        line: String,
    },
    /// Sent once, last.
    Finished {
        success: bool,
        failed: Vec<StepId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installer's UI reads these names (apps/installer/src/types.ts).
    #[test]
    fn progress_serializes_the_way_the_installer_reads_it() {
        let event = ProvisionEvent::Progress {
            id: StepId::Model,
            done: 5,
            total: Some(10),
            bytes_per_second: 2.5,
            activity: "Downloading".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "progress");
        assert_eq!(json["id"], "model");
        assert_eq!(json["bytesPerSecond"], 2.5);
        assert_eq!(json["activity"], "Downloading");
    }
}
