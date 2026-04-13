//! Injectable clock abstraction for deterministic time-based tests.
//!
//! The [`Clock`] trait provides a `now()` method that returns a
//! [`std::time::Instant`]. Production code uses [`SystemClock`], which
//! delegates to [`Instant::now`]. Tests can use [`FakeClock`], whose
//! current time is controlled explicitly via [`FakeClock::advance`] and
//! [`FakeClock::set`].
//!
//! Sharing a single trait across the workspace avoids incompatible
//! per-feature clock abstractions: features such as idempotency keys and
//! rate-limited agent guardrails can both accept `Arc<dyn Clock>` for
//! injection.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Abstract source of monotonic time.
///
/// Implementations must be `Send + Sync` so that a single clock can be
/// shared across threads via `Arc<dyn Clock>`.
pub trait Clock: Send + Sync {
    /// Returns the current instant according to this clock.
    fn now(&self) -> Instant;
}

/// A [`Clock`] that delegates to [`Instant::now`].
///
/// This is the production implementation and carries no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl SystemClock {
    /// Constructs a new [`SystemClock`].
    pub const fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A [`Clock`] whose current time is controlled explicitly.
///
/// Useful in tests that need to observe TTL expiration, rate-limit windows,
/// or other time-based behaviour without calling `thread::sleep`.
#[derive(Debug, Clone)]
pub struct FakeClock {
    inner: Arc<Mutex<Instant>>,
}

impl FakeClock {
    /// Constructs a [`FakeClock`] initialised to [`Instant::now`].
    pub fn new() -> Self {
        Self::at(Instant::now())
    }

    /// Constructs a [`FakeClock`] initialised to the given instant.
    pub fn at(initial: Instant) -> Self {
        Self {
            inner: Arc::new(Mutex::new(initial)),
        }
    }

    /// Advances the clock forward by `duration`.
    pub fn advance(&self, duration: Duration) {
        let mut guard = self.inner.lock().expect("FakeClock mutex poisoned");
        *guard += duration;
    }

    /// Sets the clock to the given instant.
    ///
    /// Useful for tests that need to pin the clock to a specific anchor
    /// rather than drift forward from construction.
    pub fn set(&self, instant: Instant) {
        let mut guard = self.inner.lock().expect("FakeClock mutex poisoned");
        *guard = instant;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.inner.lock().expect("FakeClock mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn traits_are_send_sync() {
        assert_send_sync::<SystemClock>();
        assert_send_sync::<FakeClock>();
        assert_send_sync::<Arc<dyn Clock>>();
    }

    #[test]
    fn system_clock_returns_monotonically_non_decreasing_time() {
        let clock = SystemClock::new();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }

    #[test]
    fn fake_clock_returns_last_set_value() {
        let anchor = Instant::now();
        let clock = FakeClock::at(anchor);
        assert_eq!(clock.now(), anchor);
        // Reading twice yields the same value (no drift).
        assert_eq!(clock.now(), anchor);
    }

    #[test]
    fn fake_clock_advance_moves_forward() {
        let anchor = Instant::now();
        let clock = FakeClock::at(anchor);
        clock.advance(Duration::from_secs(30));
        assert_eq!(clock.now(), anchor + Duration::from_secs(30));
        clock.advance(Duration::from_millis(500));
        assert_eq!(
            clock.now(),
            anchor + Duration::from_secs(30) + Duration::from_millis(500),
        );
    }

    #[test]
    fn fake_clock_set_pins_time() {
        let clock = FakeClock::new();
        let target = Instant::now() + Duration::from_secs(3600);
        clock.set(target);
        assert_eq!(clock.now(), target);
    }

    #[test]
    fn fake_clock_is_shareable_across_arc_dyn_clock() {
        let fake = FakeClock::new();
        let anchor = fake.now();
        let clock: Arc<dyn Clock> = Arc::new(fake.clone());
        fake.advance(Duration::from_secs(10));
        assert_eq!(clock.now(), anchor + Duration::from_secs(10));
    }

    // Simulates how an idempotency store or rate-limit layer would use the
    // clock: evict entries whose `inserted_at + ttl` is older than `now()`.
    #[test]
    fn ttl_eviction_without_sleeping() {
        struct TtlStore {
            clock: Arc<dyn Clock>,
            ttl: Duration,
            entries: Mutex<Vec<(String, Instant)>>,
        }

        impl TtlStore {
            fn insert(&self, key: &str) {
                self.entries
                    .lock()
                    .unwrap()
                    .push((key.to_string(), self.clock.now()));
            }

            fn evict_expired(&self) -> usize {
                let now = self.clock.now();
                let ttl = self.ttl;
                let mut entries = self.entries.lock().unwrap();
                let before = entries.len();
                entries.retain(|(_, inserted)| now.duration_since(*inserted) < ttl);
                before - entries.len()
            }

            fn len(&self) -> usize {
                self.entries.lock().unwrap().len()
            }
        }

        let fake = FakeClock::new();
        let store = TtlStore {
            clock: Arc::new(fake.clone()),
            ttl: Duration::from_secs(60),
            entries: Mutex::new(Vec::new()),
        };

        store.insert("k1");
        store.insert("k2");
        assert_eq!(store.len(), 2);

        // Still inside TTL — nothing evicted.
        fake.advance(Duration::from_secs(30));
        assert_eq!(store.evict_expired(), 0);
        assert_eq!(store.len(), 2);

        // Cross the TTL boundary — both entries must be evicted.
        fake.advance(Duration::from_secs(31));
        assert_eq!(store.evict_expired(), 2);
        assert_eq!(store.len(), 0);
    }
}
