use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How far ahead of the limit a transfer may run before it waits: smooths out the waits without
/// letting a burst add up to much.
const BURST: Duration = Duration::from_millis(250);

/// A transfer rate shared by every download that carries a clone of it, so several running at
/// once split the limit between them. Changed while they run with [`RateLimit::set`]; 0 is no
/// limit.
#[derive(Clone, Default)]
pub struct RateLimit {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    bytes_per_second: AtomicU64,
    /// When the bytes taken so far will have been sent at the limit.
    paid_until: Mutex<Option<Instant>>,
}

impl RateLimit {
    pub fn new(bytes_per_second: u64) -> Self {
        let limit = Self::default();
        limit.set(bytes_per_second);
        limit
    }

    pub fn get(&self) -> u64 {
        self.inner.bytes_per_second.load(Ordering::Relaxed)
    }

    pub fn set(&self, bytes_per_second: u64) {
        self.inner.bytes_per_second.store(bytes_per_second, Ordering::Relaxed);
        // What was owed at the old rate is forgiven, so a raised limit applies at once.
        *self.inner.paid_until.lock().expect("rate limit lock") = None;
    }

    /// Waits until `bytes` more can be sent within the limit.
    pub async fn take(&self, bytes: usize) {
        if let Some(wait) = self.reserve(bytes, Instant::now()) {
            tokio::time::sleep(wait).await;
        }
    }

    /// Books `bytes` at `now` and returns how long to wait before sending them, if at all.
    fn reserve(&self, bytes: usize, now: Instant) -> Option<Duration> {
        let rate = self.get();
        if rate == 0 {
            return None;
        }
        let mut paid = self.inner.paid_until.lock().expect("rate limit lock");
        let start = paid.filter(|p| *p > now).unwrap_or(now);
        let until = start + Duration::from_secs_f64(bytes as f64 / rate as f64);
        *paid = Some(until);
        until.checked_duration_since(now + BURST).filter(|wait| !wait.is_zero())
    }
}

impl fmt::Debug for RateLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RateLimit").field(&self.get()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_never_waits() {
        let limit = RateLimit::new(0);
        assert_eq!(limit.reserve(usize::MAX, Instant::now()), None);
    }

    #[test]
    fn transfers_sharing_a_limit_wait_for_each_other() {
        let limit = RateLimit::new(1000);
        let other = limit.clone();
        let now = Instant::now();
        // A quarter of a second's worth goes through as the burst.
        assert_eq!(limit.reserve(250, now), None);
        // The next second's worth, from either transfer, waits for the first to be paid.
        assert_eq!(other.reserve(1000, now), Some(Duration::from_secs(1)));
        assert_eq!(limit.reserve(500, now), Some(Duration::from_millis(1500)));
    }

    #[test]
    fn a_new_limit_applies_at_once() {
        let limit = RateLimit::new(10);
        let now = Instant::now();
        assert!(limit.reserve(1000, now).is_some());
        limit.set(0);
        assert_eq!(limit.reserve(1000, now), None);
        limit.set(1000);
        assert_eq!(limit.reserve(100, now), None);
    }
}
