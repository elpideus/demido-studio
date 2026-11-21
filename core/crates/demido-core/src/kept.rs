//! Keeping an answer for a short while.
//!
//! Some questions cost something to answer (reading the hardware, asking the desktop) and are asked in
//! bursts: a screen opens and three parts of it want the same thing. The answer is kept for a few seconds,
//! and when several ask at the same moment the work is done once.
//!
//! This is for answers that go stale by themselves. What changes only when someone changes it (settings)
//! is not kept this way: it is held in memory and updated at the moment of the change.

use std::future::Future;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

pub struct Kept<T> {
    slot: Mutex<Option<(Instant, T)>>,
    keeps: Duration,
}

impl<T: Clone> Kept<T> {
    pub fn new(keeps: Duration) -> Self {
        Self { slot: Mutex::new(None), keeps }
    }

    /// The kept answer if it is young enough. Otherwise `read` is run: once, however many are waiting.
    /// `fresh` skips the kept answer, for when a person asks to look again.
    pub async fn get<F, Fut>(&self, fresh: bool, read: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        // Held while reading: whoever asks meanwhile waits here and then finds the answer.
        let mut slot = self.slot.lock().await;
        if !fresh
            && let Some((when, kept)) = &*slot
            && when.elapsed() < self.keeps
        {
            return kept.clone();
        }
        let answer = read().await;
        *slot = Some((Instant::now(), answer.clone()));
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn many_askers_cost_one_reading_until_it_is_old_or_a_fresh_one_is_wanted() {
        // Wide gaps on both sides of the lifetime, so a slow build machine does not change the outcome.
        let kept = Arc::new(Kept::new(Duration::from_millis(1500)));
        let readings = Arc::new(AtomicUsize::new(0));
        let read = |readings: Arc<AtomicUsize>| async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            readings.fetch_add(1, Ordering::Relaxed) + 1
        };
        // Five at the same moment: one reading, the same answer for all.
        let all = futures_like_join((0..5).map(|_| {
            let (kept, readings) = (kept.clone(), readings.clone());
            tokio::spawn(async move { kept.get(false, || read(readings)).await })
        }))
        .await;
        assert_eq!(all, [1, 1, 1, 1, 1]);
        assert_eq!(readings.load(Ordering::Relaxed), 1);
        // Asked to look again: a new reading, even though the kept one is young.
        assert_eq!(kept.get(true, || read(readings.clone())).await, 2);
        // Once it is old, the next asker reads again.
        tokio::time::sleep(Duration::from_millis(1700)).await;
        assert_eq!(kept.get(false, || read(readings.clone())).await, 3);
    }

    async fn futures_like_join(tasks: impl Iterator<Item = tokio::task::JoinHandle<usize>>) -> Vec<usize> {
        let mut out = Vec::new();
        for t in tasks.collect::<Vec<_>>() {
            out.push(t.await.unwrap());
        }
        out
    }
}
