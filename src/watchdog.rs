//! The watchdog's decision, as plain data (research S19): when has the event loop been silent
//! for too long. The thread that acts on it lives in `backend::watchdog`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The event loop's sign of life: milliseconds on the monotonic clock (which does not advance
/// while the machine is suspended, FR-030) since the heartbeat was created.
#[derive(Debug, Clone)]
pub struct Heartbeat {
    start: Instant,
    last_ms: Arc<AtomicU64>,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

impl Heartbeat {
    pub fn new() -> Heartbeat {
        Heartbeat {
            start: Instant::now(),
            last_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Milliseconds since the heartbeat was created.
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The loop made progress now.
    pub fn beat(&self) {
        self.last_ms.store(self.now_ms(), Ordering::Relaxed);
    }

    /// When the loop last made progress.
    pub fn last_ms(&self) -> u64 {
        self.last_ms.load(Ordering::Relaxed)
    }
}

/// Whether a loop last seen at `last_ms` is frozen at `now_ms`: silent for longer than
/// `timeout`.
pub fn expired(last_ms: u64, now_ms: u64, timeout: Duration) -> bool {
    let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    now_ms.saturating_sub(last_ms) > timeout_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_only_after_the_timeout() {
        let t = Duration::from_secs(10);
        assert!(!expired(1000, 11_000, t), "exactly the timeout");
        assert!(expired(1000, 11_001, t), "one millisecond more");
        assert!(!expired(5000, 4000, t), "a beat after now never expires");
    }

    #[test]
    fn a_beat_is_stored_and_shared() {
        let h = Heartbeat::new();
        let copy = h.clone();
        std::thread::sleep(Duration::from_millis(5));
        h.beat();
        assert!(copy.last_ms() >= 5, "{}", copy.last_ms());
        assert!(copy.now_ms() >= copy.last_ms());
    }
}
