//! Written after the gesture settles, not during it.
//!
//! Dragging the rail from one edge to the other is one decision by the user and
//! can be dozens of state changes in the window. Writing each of them is a file
//! rewritten dozens of times for an arrangement nobody has finished making, and
//! it is the shape that makes people say an application "hits the disk". So the
//! window says what the desk looks like as often as it likes, and this decides
//! when that becomes a file: once, [`SETTLES_AFTER`] the last thing it heard.
//!
//! **Nothing is lost on the way out.** Dropping the handle flushes whatever is
//! still pending before the thread joins, so quitting inside the quiet window
//! saves the arrangement rather than discarding it. That is the one case a
//! naive debouncer gets wrong, and it is exactly the case a user hits: move the
//! rail, then close the window.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::layout::Shell;
use crate::store::Store;

/// How long the desk has to be still before the arrangement is written.
///
/// Long enough that a drag is one write, short enough that it has happened by
/// the time anybody could kill the process by hand.
pub const SETTLES_AFTER: Duration = Duration::from_millis(400);

/// A store that coalesces writes.
///
/// Reads pass through, so the desk always reads back what it last said even
/// when that has not reached the disk yet.
#[derive(Debug)]
pub struct Debounced<S: Store> {
    store: Arc<S>,
    shared: Arc<Shared>,
    quiet: Duration,
    /// `None` when the operating system refused a thread. Startup never blocks
    /// (`AGENTS.md`), so that is reported and the writes simply stop being
    /// deferred.
    worker: Option<JoinHandle<()>>,
}

#[derive(Debug, Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct Pending {
    /// The arrangement the window last reported, until it is written.
    latest: Option<Shell>,
    /// When it stops being touched long enough to be worth writing.
    settles_at: Option<Instant>,
    stopping: bool,
}

impl<S: Store + 'static> Debounced<S> {
    /// Defer writes to `store` until the desk has been still for
    /// [`SETTLES_AFTER`].
    pub fn new(store: S) -> Self {
        Self::settling_after(store, SETTLES_AFTER)
    }

    /// The same, with the quiet window named. Tests use it; the application
    /// uses [`Debounced::new`], so there is one number and it is above.
    pub fn settling_after(store: S, quiet: Duration) -> Self {
        let store = Arc::new(store);
        let shared = Arc::new(Shared::default());

        let worker = std::thread::Builder::new()
            .name("demido-shell-layout".to_owned())
            .spawn({
                let store = Arc::clone(&store);
                let shared = Arc::clone(&shared);
                move || {
                    while let Some(shell) = shared.next_due() {
                        if let Err(error) = store.write(&shell) {
                            tracing::warn!(%error, "the desk arrangement was not saved");
                        }
                    }
                }
            });

        let worker = match worker {
            Ok(worker) => Some(worker),
            Err(error) => {
                tracing::warn!(%error, "no thread for the layout; writing it in place instead");
                None
            }
        };

        Self {
            store,
            shared,
            quiet,
            worker,
        }
    }

    /// The arrangement to draw.
    ///
    /// Whatever the window last reported, if that has not been written yet, and
    /// otherwise whatever the store has. Reading the store alone would let the
    /// desk read back a stale layout inside the quiet window, which is a bug
    /// that only appears under a fast hand.
    pub fn read(&self) -> Option<Shell> {
        let latest = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .latest;
        latest.or_else(|| self.store.read())
    }

    /// Report what the desk looks like now. It reaches the disk once the desk
    /// stops changing.
    pub fn remember(&self, shell: Shell) {
        if self.worker.is_none() {
            // No thread to defer to. Writing in place is worse and it is not
            // nothing, which is the trade a subsystem that failed is supposed
            // to make.
            if let Err(error) = self.store.write(&shell) {
                tracing::warn!(%error, "the desk arrangement was not saved");
            }
            return;
        }

        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(|held| held.into_inner());
            pending.latest = Some(shell);
            pending.settles_at = Some(Instant::now() + self.quiet);
        }
        self.shared.wake.notify_all();
    }
}

impl Shared {
    /// Block until there is an arrangement worth writing, or until there is
    /// nothing left to do.
    fn next_due(&self) -> Option<Shell> {
        let mut pending = self.pending.lock().unwrap_or_else(|held| held.into_inner());
        loop {
            match (pending.latest, pending.settles_at) {
                // On the way out, whatever is pending is written now. The quiet
                // window is a courtesy to the disk, not a promise to the user
                // that their last gesture can be dropped.
                (Some(shell), _) if pending.stopping => {
                    pending.latest = None;
                    pending.settles_at = None;
                    return Some(shell);
                }
                (Some(shell), Some(at)) => {
                    let now = Instant::now();
                    if now >= at {
                        pending.latest = None;
                        pending.settles_at = None;
                        return Some(shell);
                    }
                    pending = self
                        .wake
                        .wait_timeout(pending, at - now)
                        .unwrap_or_else(|held| held.into_inner())
                        .0;
                }
                (Some(shell), None) => {
                    pending.latest = None;
                    return Some(shell);
                }
                (None, _) if pending.stopping => return None,
                (None, _) => {
                    pending = self
                        .wake
                        .wait(pending)
                        .unwrap_or_else(|held| held.into_inner());
                }
            }
        }
    }
}

impl<S: Store> Drop for Debounced<S> {
    fn drop(&mut self) {
        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(|held| held.into_inner());
            pending.stopping = true;
        }
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use crate::layout::Side;
    use crate::memory::Memory;

    /// Short enough to keep the suite quick, long enough that a loaded machine
    /// does not settle in the middle of a burst of gestures.
    const QUIET: Duration = Duration::from_millis(120);

    fn arranged(rail: Side) -> Shell {
        Shell { rail }
    }

    #[test]
    fn a_burst_of_gestures_is_one_write() {
        let store = Memory::new();
        {
            let desk = Debounced::settling_after(store.clone(), QUIET);
            for _ in 0..20 {
                desk.remember(arranged(Side::Right));
                desk.remember(arranged(Side::Left));
            }
            std::thread::sleep(QUIET * 8);
            assert_eq!(
                store.writes(),
                1,
                "forty reports of one decision are one file"
            );
        }
        assert_eq!(store.read(), Some(arranged(Side::Left)));
    }

    #[test]
    fn nothing_is_written_while_the_desk_is_still_moving() {
        let store = Memory::new();
        let desk = Debounced::settling_after(store.clone(), QUIET);
        desk.remember(arranged(Side::Right));
        assert_eq!(
            store.writes(),
            0,
            "the gesture has not settled, so nothing has reached the store"
        );
        assert_eq!(
            desk.read(),
            Some(arranged(Side::Right)),
            "the desk still reads back what it just said"
        );
    }

    #[test]
    fn quitting_inside_the_quiet_window_still_saves_the_arrangement() {
        // Move the rail, then close the window. A debouncer that drops this is
        // a desk that forgets exactly the gesture the user just made.
        let store = Memory::new();
        {
            let desk = Debounced::settling_after(store.clone(), Duration::from_secs(60));
            desk.remember(arranged(Side::Right));
            assert_eq!(store.writes(), 0);
        }
        assert_eq!(store.read(), Some(arranged(Side::Right)));
        assert_eq!(store.writes(), 1);
    }

    #[test]
    fn a_desk_nobody_arranged_writes_nothing_at_all() {
        let store = Memory::new();
        drop(Debounced::settling_after(store.clone(), QUIET));
        assert_eq!(store.writes(), 0);
        assert_eq!(store.read(), None);
    }

    #[test]
    fn an_unarranged_desk_reads_through_to_the_store() {
        let store = Memory::new();
        store.write(&arranged(Side::Right)).expect("wrote");
        let desk = Debounced::settling_after(store.clone(), QUIET);
        assert_eq!(desk.read(), Some(arranged(Side::Right)));
    }
}
