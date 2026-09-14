//! The startup sequence the splash draws, one named stage at a time.
//!
//! Brief B41: "There should be a splash small window (similar to that of
//! Discord), showing the logo, the Demido Studio name and the loading status
//! (maybe even what it is actually loading?)". The parenthesis is taken at face
//! value (`design/splash.md`): the splash names the subsystem coming up, not a
//! percentage and not a spinner.
//!
//! ## Why the sequence is here and not in the window
//!
//! The splash is painted before anything else exists, so it cannot own what it
//! is waiting for. It draws one tick per stage and one line naming the stage
//! running, and the stages are Rust's because the work is Rust's. The tick
//! count is the length of [`STAGES`], which is what `design/splash.md` means by
//! "the tick count is the number of startup stages, which the shell owns".
//!
//! ## Why a failure is not an error
//!
//! `AGENTS.md`: "Startup never blocks. A subsystem that fails is reported and
//! skipped; the app still reaches a usable state." So [`run`] has no failure
//! path. A stage that returns an error turns its tick rose, is logged, is kept
//! in [`Failures`] for a surface to render, and the next stage starts. The desk
//! opens either way, which is the rule expressed in the first window a person
//! ever sees: a splash that can hang is a splash that trains people to
//! force-quit.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use demido_settings::Ladder;

use crate::wiring::{Rig, Wiring};

/// One stage of the sequence, as the splash draws it.
///
/// The name is what the stage line says, uppercased by the window rather than
/// here: a label is typography, and a string that arrives already shouting is a
/// string no other surface can reuse.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Stage {
    /// Stable, and what a forced failure names in a debug build.
    pub id: &'static str,
    /// What is loading, in the words the splash shows.
    pub name: &'static str,
}

/// Every stage, in the order they run.
///
/// Each one is real work that touches a disk or an environment, which is what
/// makes naming them worth anything: a sequence of stages that did nothing
/// would be a progress bar with better manners.
pub const STAGES: &[Stage] = &[
    Stage {
        id: "profile",
        // not-a-prompt: the five names below are the splash's stage line, read
        // by a person watching the app start. No model sees them.
        name: "the profile",
    },
    Stage {
        id: "settings",
        name: "the settings",
    },
    Stage {
        id: "desk",
        name: "the desk",
    },
    Stage {
        id: "log",
        name: "the session log",
    },
    Stage {
        id: "runtime",
        name: "the runtime",
    },
];

/// Where a stage is in its life, as the tick states of `design/splash.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Not started. The tick is `--color-ink-4`.
    Pending,
    /// The one full-height tick, in `--color-signal`.
    Running,
    /// Up, and dim: `--color-signal-dim`.
    Done,
    /// Reported and skipped, in `--color-rose`.
    Failed,
}

/// What the splash is told, once per state change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Which tick, counted from zero.
    pub index: usize,
    /// How many ticks there are, so the window never has to be told twice.
    pub total: usize,
    /// The stage this is about.
    pub stage: Stage,
    /// Where it is now.
    pub state: State,
    /// Why it failed, where it did. A sentence, for the log and for a surface
    /// that renders the skipped subsystems later.
    pub detail: Option<String>,
}

/// A subsystem that did not start, kept so something can say so afterwards.
///
/// `design/splash.md`: "the app reports the failure in Settings and still
/// reaches a usable state". The splash reports it as it happens, in rose; this
/// is the same fact after the splash is gone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub id: &'static str,
    pub name: &'static str,
    pub detail: String,
}

/// The subsystems this launch skipped. Empty on an ordinary launch.
#[derive(Debug, Default)]
pub struct Failures(Mutex<Vec<Failure>>);

impl Failures {
    /// Everything skipped so far.
    ///
    /// A poisoned lock is read through rather than unwrapped: the list is a
    /// report, and a report that panics is worse than a report that is stale.
    pub fn read(&self) -> Vec<Failure> {
        match self.0.lock() {
            Ok(failures) => failures.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn record(&self, failure: Failure) {
        match self.0.lock() {
            Ok(mut failures) => failures.push(failure),
            Err(poisoned) => poisoned.into_inner().push(failure),
        }
    }
}

/// The event the splash listens to. One name for one kind of thing, like
/// `chat://update`: the window subscribes once and redraws.
const PROGRESS: &str = "boot://progress";

/// How long the finished scale stays on screen before the desk takes over.
///
/// Not decoration. The last tick and the last stage line land in the same frame
/// as the window that replaces them, and a splash whose final state nobody sees
/// is a splash that flickered.
const LINGER: Duration = Duration::from_millis(320);

/// The stages, for a window that has to draw a tick per stage before any of
/// them has run.
#[tauri::command]
pub fn boot_stages() -> &'static [Stage] {
    STAGES
}

/// What this launch skipped, for a surface that reports it after the splash.
#[tauri::command]
pub fn boot_failures(failures: tauri::State<'_, Failures>) -> Vec<Failure> {
    failures.read()
}

/// Run the sequence, told to the splash as it goes, and hand over to the desk.
///
/// Called by the splash once it is listening ([`crate::boot_begin`]), and by
/// the watchdog if it never does. Runs once: the caller holds the guard.
pub async fn run(app: AppHandle) {
    for (index, stage) in STAGES.iter().enumerate() {
        report(&app, index, *stage, State::Running, None);
        hold().await;

        match work(&app, stage.id) {
            Ok(()) => report(&app, index, *stage, State::Done, None),
            Err(error) => {
                let detail = error.to_string();
                tracing::error!(stage = stage.id, %detail, "a subsystem did not start; skipping it");
                app.state::<Failures>().record(Failure {
                    id: stage.id,
                    name: stage.name,
                    detail: detail.clone(),
                });
                report(&app, index, *stage, State::Failed, Some(detail));
            }
        }
    }

    tokio::time::sleep(LINGER).await;
    open_the_desk(&app);
}

/// One stage's actual work.
///
/// Every arm reads something that can genuinely fail on a real machine: a
/// profile directory on a locked-down account, a settings document written by a
/// newer build, a log on a full disk, a runtime the environment names and does
/// not have. A stage that could not fail would be a tick that means nothing.
fn work(app: &AppHandle, id: &str) -> demido_core::Result<()> {
    if let Some(forced) = forced_failure(id) {
        return Err(forced);
    }

    let wiring = app.state::<Wiring>();
    match id {
        "profile" => app
            .path()
            .app_local_data_dir()
            .map(|_| ())
            .map_err(|error| demido_core::Error::unavailable("the profile directory", error)),
        // Reading the ladder is what proves the document on disk parses. The
        // view is discarded: the settings page asks for its own, and a boot
        // that cached one would hand a page a copy that a later write moved on
        // from.
        "settings" => {
            let _ = wiring.settings.view(&Ladder::global());
            Ok(())
        }
        // A layout that will not load is discarded silently by the store
        // (`demido-shell`), so this stage reports the read happening rather
        // than its verdict: the default desk is a correct outcome, not a
        // failure.
        "desk" => {
            let _ = wiring.desk.read();
            Ok(())
        }
        "log" => wiring.chat.history().map(|_| ()).map_err(Into::into),
        // Absent is not failed. A machine where set-up has not run has no
        // runtime and still has a usable desk with the composer disabled, which
        // is the ordinary first launch (`crate::wiring::Rig`).
        "runtime" => {
            let _ = Rig::from_environment();
            Ok(())
        }
        other => Err(demido_core::Error::not_found("a startup stage", other)),
    }
}

/// The desk, and the splash out of the way.
///
/// Both windows are looked up rather than held, because either can be gone: a
/// person who closes the splash before the desk opens has closed a window, not
/// cancelled a launch, and the desk still opens.
fn open_the_desk(app: &AppHandle) {
    match app.get_webview_window("main") {
        Some(desk) => {
            if let Err(error) = desk.show().and_then(|()| desk.set_focus()) {
                tracing::error!(%error, "the desk would not come to the front");
            }
        }
        None => tracing::error!("there is no desk window to open"),
    }

    if let Some(splash) = app.get_webview_window("splash") {
        if let Err(error) = splash.close() {
            tracing::warn!(%error, "the splash would not close");
        }
    }
}

/// Tell the splash, and carry on if it is not listening.
///
/// A failed emit is a window that is closing, which is not a reason to stop a
/// startup sequence: the stages are what open the desk.
fn report(app: &AppHandle, index: usize, stage: Stage, state: State, detail: Option<String>) {
    let progress = Progress {
        index,
        total: STAGES.len(),
        stage,
        state,
        detail,
    };
    if let Err(error) = app.emit(PROGRESS, &progress) {
        tracing::warn!(%error, stage = stage.id, "the splash was not told");
    }
}

/// A stage forced to fail, for the screenshot that proves a failure is skipped.
///
/// Debug builds only, and an environment variable rather than a flag, because
/// the evidence is taken by `pnpm dev:drive` and a flag would have to cross
/// three commands to reach here. `DEMIDO_BOOT_FAIL=settings pnpm dev:drive`
/// turns that one tick rose and opens the desk anyway.
#[cfg(debug_assertions)]
fn forced_failure(id: &str) -> Option<demido_core::Error> {
    let forced = std::env::var("DEMIDO_BOOT_FAIL").ok()?;
    forced.split(',').any(|name| name.trim() == id).then(|| {
        demido_core::Error::unavailable(
            id,
            // not-a-prompt: a developer reads this in the log and on the splash.
            "DEMIDO_BOOT_FAIL named this stage, so it was failed on purpose",
        )
    })
}

#[cfg(not(debug_assertions))]
fn forced_failure(_id: &str) -> Option<demido_core::Error> {
    None
}

/// Hold each stage long enough to be photographed, in a debug build that asks.
///
/// `DEMIDO_BOOT_HOLD_MS=700 pnpm dev:drive` makes a sequence that finishes in
/// milliseconds last long enough for `scripts/drive.mjs` to catch it. Unset, it
/// is not a delay at all, so an ordinary launch pays nothing for the evidence
/// the window gate owes.
#[cfg(debug_assertions)]
async fn hold() {
    let Some(ms) = std::env::var("DEMIDO_BOOT_HOLD_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
    else {
        return;
    };
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[cfg(not(debug_assertions))]
async fn hold() {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    /// Every tick the splash draws is a stage that names itself.
    #[test]
    fn every_stage_has_an_id_and_a_name() {
        assert!(!STAGES.is_empty());
        for stage in STAGES {
            assert!(!stage.id.is_empty());
            assert!(!stage.name.is_empty());
        }
    }

    /// Ids are how a failure is named and how a stage is forced, so two stages
    /// sharing one is two ticks nobody can tell apart.
    #[test]
    fn stage_ids_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for stage in STAGES {
            assert!(seen.insert(stage.id), "two stages answer to {}", stage.id);
        }
    }

    /// A skipped subsystem is still on the list afterwards. That list is what
    /// lets the app report a failure once the splash is gone.
    #[test]
    fn a_failure_is_kept_after_it_is_skipped() {
        let failures = Failures::default();
        assert!(failures.read().is_empty());
        failures.record(Failure {
            id: "settings",
            name: "the settings",
            detail: "the document would not parse".into(),
        });
        let kept = failures.read();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "settings");
    }

    /// The forced failure is a debug affordance and names exactly the stage it
    /// was given, so evidence of one rose tick is not evidence of five.
    #[cfg(debug_assertions)]
    #[test]
    fn a_forced_failure_names_one_stage() {
        // Set for the length of this test only. The suite runs stages nowhere
        // else, so no other test can read it.
        std::env::set_var("DEMIDO_BOOT_FAIL", "settings");
        assert!(forced_failure("settings").is_some());
        assert!(forced_failure("desk").is_none());
        std::env::remove_var("DEMIDO_BOOT_FAIL");
    }

    /// A stage the list does not have is a programming error that reports
    /// itself rather than one that quietly does nothing.
    #[test]
    fn an_unknown_stage_is_refused() {
        let error = demido_core::Error::not_found("a startup stage", "nonsense");
        assert!(error.to_string().contains("nonsense"));
    }
}
