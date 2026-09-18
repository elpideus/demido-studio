//! The queue: what is downloading, how far along, and how to stop it.
//!
//! **A failure is a row, never a dialog.** An item that fails keeps its place,
//! says why as a [`Failure`], keeps whatever bytes are still worth resuming
//! from, and gives its slot to the next item: one failure is one failure.
//! [`Queue::resume`] is the retry, and it asks for the bytes still missing.
//!
//! **Per profile, and it survives a restart.** What was asked for is in the
//! profile's `downloads.json` ([`crate::file`]); how far it got is the length
//! of its partial files. [`Queue::open`] reads both and [`Queue::start`] picks
//! up everything nobody paused.
//!
//! Concurrency is a pool rather than a per-item setting: several transfers on
//! one home connection finish no sooner than a few, and make every bar slower
//! to read.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::file::{Entry, Files, Held};
use crate::transfer::{self, Run, Stop};
use crate::{room, Failure, Item};

/// Simultaneous items. Two keeps a domestic link full while each bar still
/// moves fast enough to read, and a model is gigabytes, not megabytes.
const CONCURRENCY: usize = 2;

/// Silence long enough to mean the connection is gone rather than slow.
const STALL: Duration = Duration::from_secs(60);

/// Events buffered for a subscriber that is not keeping up. A slow subscriber
/// loses intermediate progress; [`Queue::rows`] is always whole.
const EVENTS: usize = 256;

/// One item for its lifetime. Kept across a restart, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum State {
    /// Waiting for a slot.
    Queued,
    Running,
    /// Every byte is in and the files are being checked. Seconds of a bar
    /// sitting at its end otherwise.
    Verifying,
    /// Stopped on purpose, with its bytes kept.
    Paused,
    /// Stopped by something else, with the reason, and its bytes kept unless
    /// the reason is that they were wrong.
    Failed {
        failure: Failure,
    },
    /// Verified and in the library.
    Done,
}

/// What the window draws for one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: Id,
    pub repo: String,
    pub name: String,
    /// Bytes on disk, in bytes against [`Row::total`].
    pub received: u64,
    /// What the item costs, as the index stated it.
    pub total: u64,
    #[serde(flatten)]
    pub state: State,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// A row changed, or appeared.
    Row { row: Row },
    /// A row left the queue: cancelled, or a finished one dismissed.
    Gone { id: Id },
}

/// Why a running item is being stopped. The transfer sees one cancelled flag
/// either way; this is how the queue knows which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stopping {
    Pause,
    Cancel,
}

struct Job {
    item: Item,
    state: State,
    received: u64,
    cancel: CancellationToken,
    stopping: Option<Stopping>,
    /// A task is driving this item. Only one ever does.
    live: bool,
}

impl Job {
    fn row(&self, id: Id) -> Row {
        Row {
            id,
            repo: self.item.repo.clone(),
            name: self.item.name.clone(),
            received: self.received,
            total: self.item.bytes(),
            state: self.state.clone(),
        }
    }

    /// What the queue file says about this job, or nothing if it is not one
    /// the file keeps.
    fn entry(&self, id: Id) -> Option<Entry> {
        let held = match (&self.state, self.stopping) {
            (State::Done, _) | (_, Some(Stopping::Cancel)) => return None,
            (State::Paused, _) | (_, Some(Stopping::Pause)) => Some(Held::Paused),
            (State::Failed { failure }, _) => Some(Held::Failed {
                failure: failure.clone(),
            }),
            _ => None,
        };
        Some(Entry {
            id: id.0,
            item: self.item.clone(),
            held,
        })
    }
}

/// Bytes of `item` on disk, finished pieces and partial ones.
fn on_disk(item: &Item) -> u64 {
    item.files.iter().fold(0u64, |total, piece| {
        total.saturating_add(transfer::on_disk(piece))
    })
}

/// The pool, and the record of every item in it.
///
/// Cheap to clone: every clone is the same queue.
#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

struct Inner {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    stall: Duration,
    files: Files,
    next: AtomicU64,
    jobs: Mutex<BTreeMap<Id, Job>>,
    events: broadcast::Sender<Event>,
}

impl Queue {
    /// The queue a profile left, read back. **Starts nothing**: opening a
    /// profile is not a network request, and a caller outside a runtime can
    /// open one. [`Queue::start`] does that.
    pub fn open(files: Files) -> Queue {
        Self::build(files, CONCURRENCY, STALL)
    }

    /// The same, with the pool and the stall timeout chosen. A minute is not a
    /// timeout in a test, it is a hang.
    pub fn with(files: Files, concurrency: usize, stall: Duration) -> Queue {
        Self::build(files, concurrency, stall)
    }

    fn build(files: Files, concurrency: usize, stall: Duration) -> Queue {
        let mut jobs = BTreeMap::new();
        let mut next = 1;
        for entry in files.read() {
            let state = match entry.held {
                None => State::Queued,
                Some(Held::Paused) => State::Paused,
                Some(Held::Failed { failure }) => State::Failed { failure },
            };
            next = next.max(entry.id + 1);
            jobs.insert(
                Id(entry.id),
                Job {
                    received: on_disk(&entry.item),
                    item: entry.item,
                    state,
                    cancel: CancellationToken::new(),
                    stopping: None,
                    live: false,
                },
            );
        }
        let (events, _) = broadcast::channel(EVENTS);
        Queue {
            inner: Arc::new(Inner {
                client: reqwest::Client::new(),
                slots: Arc::new(Semaphore::new(concurrency.max(1))),
                stall,
                files,
                next: AtomicU64::new(next),
                jobs: Mutex::new(jobs),
                events,
            }),
        }
    }

    /// Carry on with everything that was running when the profile last
    /// closed. Called once, from inside the runtime.
    pub fn start(&self) {
        let waiting: Vec<Id> = self
            .jobs()
            .iter()
            .filter(|(_, job)| job.state == State::Queued && !job.live)
            .map(|(id, _)| *id)
            .collect();
        for id in waiting {
            self.spawn(id);
        }
    }

    /// Accept an item and start it when a slot is free.
    ///
    /// An item whose target is already in the queue is that item: two
    /// transfers into one partial file interleave their bytes. Asking again
    /// for one that is paused or failed is the plainest way of saying carry
    /// on, so it resumes.
    pub fn enqueue(&self, item: Item) -> Id {
        let existing = self
            .jobs()
            .iter()
            .find(|(_, job)| job.state != State::Done && job.item.target() == item.target())
            .map(|(id, _)| *id);
        if let Some(id) = existing {
            self.resume(id);
            return id;
        }

        let id = Id(self.inner.next.fetch_add(1, Ordering::SeqCst));
        {
            let mut jobs = self.jobs();
            jobs.insert(
                id,
                Job {
                    received: on_disk(&item),
                    item,
                    state: State::Queued,
                    cancel: CancellationToken::new(),
                    stopping: None,
                    live: false,
                },
            );
            self.settle(&jobs, id);
        }
        self.spawn(id);
        id
    }

    /// Stop an item and keep its bytes. A restart keeps it paused.
    pub fn pause(&self, id: Id) {
        let mut jobs = self.jobs();
        let Some(job) = jobs.get_mut(&id) else { return };
        if !job.live || job.stopping.is_some() {
            return;
        }
        job.stopping = Some(Stopping::Pause);
        job.cancel.cancel();
        // Written now rather than when the transfer notices, so a process
        // killed in between still comes back paused.
        self.persist(&jobs);
    }

    /// Pause every item that is queued or running. A failed item is already
    /// stopped and says why, and pausing it would hide that.
    pub fn pause_all(&self) {
        let live: Vec<Id> = self
            .jobs()
            .iter()
            .filter(|(_, job)| job.live)
            .map(|(id, _)| *id)
            .collect();
        for id in live {
            self.pause(id);
        }
    }

    /// Carry on from the bytes on disk: a paused item, or a failed one, which
    /// is the retry.
    pub fn resume(&self, id: Id) {
        {
            let mut jobs = self.jobs();
            let Some(job) = jobs.get_mut(&id) else { return };
            if job.live || !matches!(job.state, State::Paused | State::Failed { .. }) {
                return;
            }
            // A fresh flag: the old one is cancelled for good.
            job.cancel = CancellationToken::new();
            job.stopping = None;
            job.state = State::Queued;
            job.received = on_disk(&job.item);
            self.settle(&jobs, id);
        }
        self.spawn(id);
    }

    /// Take an item out of the queue and delete its partial files, so a
    /// cancel does not quietly keep gigabytes. A finished item's row is
    /// dismissed and its model left where it is: the library owns it now.
    pub fn cancel(&self, id: Id) {
        let mut jobs = self.jobs();
        let Some(job) = jobs.get_mut(&id) else { return };
        if job.live {
            // The task deletes the partial files once it has let go of them:
            // Windows will not delete a file somebody has open.
            job.stopping = Some(Stopping::Cancel);
            job.cancel.cancel();
            self.persist(&jobs);
            return;
        }
        let Some(job) = jobs.remove(&id) else { return };
        if job.state != State::Done {
            for piece in &job.item.files {
                remove(&piece.partial());
            }
        }
        self.persist(&jobs);
        let _ = self.inner.events.send(Event::Gone { id });
    }

    /// Every row, in the order they were queued.
    pub fn rows(&self) -> Vec<Row> {
        self.jobs().iter().map(|(id, job)| job.row(*id)).collect()
    }

    pub fn row(&self, id: Id) -> Option<Row> {
        self.jobs().get(&id).map(|job| job.row(id))
    }

    /// Every change to every row, as it happens.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Wait until an item stops moving: paused, failed or done, or `None` if
    /// it left the queue. For a caller that needs the model, and for tests.
    pub async fn settled(&self, id: Id) -> Option<State> {
        let mut events = self.subscribe();
        loop {
            match self.row(id).map(|row| row.state) {
                None => return None,
                Some(state @ (State::Paused | State::Failed { .. } | State::Done)) => {
                    if !self.jobs().get(&id).is_some_and(|job| job.live) {
                        return Some(state);
                    }
                }
                Some(_) => {}
            }
            // Any event, or a lag, is a reason to look again.
            if let Err(broadcast::error::RecvError::Closed) = events.recv().await {
                return self.row(id).map(|row| row.state);
            }
        }
    }

    fn jobs(&self) -> MutexGuard<'_, BTreeMap<Id, Job>> {
        // A poisoned lock is a panic elsewhere, and the rows are still whole:
        // every change to them is made in one statement under it.
        self.inner
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn spawn(&self, id: Id) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                id = id.0,
                "a download was queued outside the runtime; it starts with the queue"
            );
            return;
        };
        {
            let mut jobs = self.jobs();
            let Some(job) = jobs.get_mut(&id) else { return };
            if job.live {
                return;
            }
            job.live = true;
        }
        let queue = self.clone();
        runtime.spawn(async move { queue.drive(id).await });
    }

    async fn drive(&self, id: Id) {
        let Some((item, cancel)) = self
            .jobs()
            .get(&id)
            .map(|job| (job.item.clone(), job.cancel.clone()))
        else {
            return;
        };

        // Waiting for a slot is not holding one, and a pause while waiting
        // is a pause.
        let slot = tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            slot = self.inner.slots.clone().acquire_owned() => slot.ok(),
        };
        let Some(_slot) = slot else {
            return self.stopped(id, &item);
        };

        // Against what is still to come, before a byte of it is asked for.
        let needs = item.bytes().saturating_sub(on_disk(&item));
        let folder = item
            .target()
            .and_then(std::path::Path::parent)
            .unwrap_or(std::path::Path::new("."));
        if let Some(free) = room::free(folder).filter(|free| *free < needs) {
            return self.failed(id, &item, Failure::NoRoom { needs, free });
        }

        self.set(id, State::Running, None);
        let run = Run {
            client: &self.inner.client,
            cancel: &cancel,
            stall: self.inner.stall,
        };
        let mut before = 0u64;
        for piece in &item.files {
            let mut report = |bytes: u64| self.set(id, State::Running, Some(before + bytes));
            let fetched = transfer::fetch(piece, run, &mut report).await;
            let verified = match fetched {
                Ok(()) => {
                    self.set(id, State::Verifying, None);
                    transfer::verify(piece, &cancel).await
                }
                Err(stop) => Err(stop),
            };
            match verified {
                Ok(()) => before = before.saturating_add(piece.bytes),
                Err(Stop::Cancelled) => return self.stopped(id, &item),
                Err(Stop::Failed(failure)) => return self.failed(id, &item, failure),
            }
            self.set(id, State::Running, None);
        }

        // Every file passed. The weights' first piece goes last, so the file a
        // backend is handed appears only once everything it needs is beside
        // it.
        for piece in item.files.iter().rev() {
            if let Err(failure) = transfer::promote(piece).await {
                return self.failed(id, &item, failure);
            }
        }
        let mut jobs = self.jobs();
        if let Some(job) = jobs.get_mut(&id) {
            // Too late for a pause or a cancel: the model is in the library.
            job.state = State::Done;
            job.received = item.bytes();
            job.stopping = None;
            job.live = false;
            self.settle(&jobs, id);
        }
    }

    /// A running item's state and figure, published. Progress is not intent,
    /// so it is not written to the queue file.
    fn set(&self, id: Id, state: State, received: Option<u64>) {
        let mut jobs = self.jobs();
        let Some(job) = jobs.get_mut(&id) else { return };
        if job.stopping.is_some() {
            return;
        }
        job.state = state;
        if let Some(received) = received {
            job.received = received;
        }
        let _ = self.inner.events.send(Event::Row { row: job.row(id) });
    }

    fn failed(&self, id: Id, item: &Item, failure: Failure) {
        tracing::warn!(id = id.0, repo = %item.repo, %failure, "a download failed");
        let received = on_disk(item);
        let mut jobs = self.jobs();
        let Some(job) = jobs.get_mut(&id) else { return };
        job.live = false;
        // Stopped on purpose while the failure was arriving: that wins.
        match job.stopping.take() {
            Some(stopping) => {
                drop(jobs);
                self.halt(id, item, stopping);
            }
            None => {
                job.state = State::Failed { failure };
                job.received = received;
                self.settle(&jobs, id);
            }
        }
    }

    /// The task noticed its flag, and has let go of every file. Paused or
    /// cancelled, whichever was asked.
    fn stopped(&self, id: Id, item: &Item) {
        let stopping = {
            let mut jobs = self.jobs();
            let Some(job) = jobs.get_mut(&id) else { return };
            job.live = false;
            job.stopping.take().unwrap_or(Stopping::Pause)
        };
        self.halt(id, item, stopping);
    }

    fn halt(&self, id: Id, item: &Item, stopping: Stopping) {
        let mut jobs = self.jobs();
        match stopping {
            Stopping::Pause => {
                if let Some(job) = jobs.get_mut(&id) {
                    job.state = State::Paused;
                    job.received = on_disk(item);
                    self.settle(&jobs, id);
                }
            }
            Stopping::Cancel => {
                for piece in &item.files {
                    remove(&piece.partial());
                }
                jobs.remove(&id);
                self.persist(&jobs);
                let _ = self.inner.events.send(Event::Gone { id });
            }
        }
    }

    /// A change of intent: written to the queue file and published, under the
    /// one lock, so the file and the window never disagree about the order.
    fn settle(&self, jobs: &BTreeMap<Id, Job>, id: Id) {
        self.persist(jobs);
        if let Some(job) = jobs.get(&id) {
            let _ = self.inner.events.send(Event::Row { row: job.row(id) });
        }
    }

    fn persist(&self, jobs: &BTreeMap<Id, Job>) {
        let entries: Vec<Entry> = jobs.iter().filter_map(|(id, job)| job.entry(*id)).collect();
        if let Err(error) = self.inner.files.write(&entries) {
            tracing::warn!(path = %self.inner.files.path().display(), %error, "the download queue could not be saved");
        }
    }
}

fn remove(path: &std::path::Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "a partial download could not be deleted");
        }
    }
}
