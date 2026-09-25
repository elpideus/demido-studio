//! The installation engine.
//!
//! An [`InstallPlan`] says what to install and where; [`run`] executes it step by step,
//! reporting [`ProvisionEvent`]s as it goes, and returns the [`InstallManifest`] it wrote.
//!
//! Only the app itself is critical. Every other step (runtime, Python, Node, the model) fails
//! on its own: the installation completes, the manifest records what is missing, and the step
//! can be retried. A broken download should cost a retry, never a reinstall.
//!
//! [`InstallManifest`]: demido_core::InstallManifest

mod events;
pub mod folder;
mod plan;
mod process;
mod runner;
mod steps;
pub mod system;

pub use events::{ProvisionEvent, StepState};
pub use plan::{AppPayload, InstallPlan, StepId, StepInfo};
pub use runner::run;
pub use steps::uninstall::{UninstallOptions, uninstall};
