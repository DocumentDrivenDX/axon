//! In-process idempotency key store for `POST /transactions` (FEAT-008 US-081).
//!
//! Holds a TTL-bounded map keyed by `(database_id, idempotency_key)`. The
//! HTTP gateway calls [`IdempotencyStore::try_reserve`] before executing a
//! transaction; on success it calls [`IdempotencyStore::store_response`] to
//! cache the response so a retry within the TTL returns the same body
//! without re-executing the transaction.
//!
//! Failed transactions deliberately do **not** call `store_response`, which
//! lets the client retry with the same key after correcting the payload —
//! see the [`IdempotencyStore::release`] helper for releasing the in-flight
//! reservation.
//!
//! The store accepts an injectable [`Clock`] so TTL expiry can be observed
//! in tests without `thread::sleep`. This mirrors the pattern used by the
//! agent guardrails work.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axon_core::Clock;

/// Default TTL applied to idempotency entries.
///
/// FEAT-008 US-081 specifies 5 minutes.
pub const DEFAULT_IDEMPOTENCY_TTL: Duration = Duration::from_secs(5 * 60);

/// Outcome of [`IdempotencyStore::try_reserve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservationResult<R> {
    /// Caller is the first to claim this key — proceed and call
    /// [`IdempotencyStore::store_response`] on success or
    /// [`IdempotencyStore::release`] on failure.
    Reserved,
    /// A previous request with this key already completed; return the
    /// cached response without re-executing.
    AlreadyCached(R),
    /// A previous request with this key is still in flight. The caller
    /// should respond with HTTP 409 + `retryable: true` and a hint.
    InFlight {
        /// How long the caller should wait before retrying, in
        /// milliseconds. Derived from the configured TTL.
        retry_after_ms: u64,
    },
}

#[derive(Debug)]
enum EntryStatus<R> {
    Pending,
    Complete(R),
}

#[derive(Debug)]
struct IdempotencyEntry<R> {
    status: EntryStatus<R>,
    expires_at: Instant,
}

/// In-process TTL-bounded idempotency dedup store.
///
/// Generic over the cached response payload `R` so the same primitive can
/// back the HTTP gateway (cached JSON body) and a future gRPC integration.
pub struct IdempotencyStore<R: Clone> {
    clock: Arc<dyn Clock>,
    ttl: Duration,
    entries: Mutex<HashMap<(String, String), IdempotencyEntry<R>>>,
}

impl<R: Clone> IdempotencyStore<R> {
    /// Construct a store with an explicit clock and TTL.
    pub fn new(clock: Arc<dyn Clock>, ttl: Duration) -> Self {
        Self {
            clock,
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Construct a store with the FEAT-008 US-081 default TTL of 5 minutes.
    pub fn with_default_ttl(clock: Arc<dyn Clock>) -> Self {
        Self::new(clock, DEFAULT_IDEMPOTENCY_TTL)
    }

    /// Return the configured TTL (useful for `retry_after_ms` hints).
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    fn evict_expired_locked(
        entries: &mut HashMap<(String, String), IdempotencyEntry<R>>,
        now: Instant,
    ) {
        entries.retain(|_, entry| entry.expires_at > now);
    }

    /// Drop all entries whose TTL has elapsed. Returns the number removed.
    pub fn evict_expired(&self) -> usize {
        let now = self.clock.now();
        let mut entries = self
            .entries
            .lock()
            .expect("idempotency store mutex poisoned");
        let before = entries.len();
        Self::evict_expired_locked(&mut entries, now);
        before - entries.len()
    }

    /// Attempt to claim `(db_id, key)` for execution.
    ///
    /// Performs lazy eviction before the lookup, so a key whose TTL has
    /// elapsed is treated as absent.
    pub fn try_reserve(&self, db_id: &str, key: &str) -> ReservationResult<R> {
        let now = self.clock.now();
        let mut entries = self
            .entries
            .lock()
            .expect("idempotency store mutex poisoned");
        Self::evict_expired_locked(&mut entries, now);

        let composite = (db_id.to_string(), key.to_string());
        match entries.get(&composite) {
            Some(entry) => match &entry.status {
                EntryStatus::Pending => ReservationResult::InFlight {
                    retry_after_ms: self.ttl.as_millis() as u64,
                },
                EntryStatus::Complete(response) => {
                    ReservationResult::AlreadyCached(response.clone())
                }
            },
            None => {
                entries.insert(
                    composite,
                    IdempotencyEntry {
                        status: EntryStatus::Pending,
                        expires_at: now + self.ttl,
                    },
                );
                ReservationResult::Reserved
            }
        }
    }

    /// Cache `response` against `(db_id, key)` and refresh the TTL deadline.
    ///
    /// Call this after a successful commit. A subsequent `try_reserve` with
    /// the same key returns `AlreadyCached(response)` until the TTL elapses.
    pub fn store_response(&self, db_id: &str, key: &str, response: R) {
        let now = self.clock.now();
        let mut entries = self
            .entries
            .lock()
            .expect("idempotency store mutex poisoned");
        let composite = (db_id.to_string(), key.to_string());
        entries.insert(
            composite,
            IdempotencyEntry {
                status: EntryStatus::Complete(response),
                expires_at: now + self.ttl,
            },
        );
    }

    /// Release a `Pending` reservation without caching a response.
    ///
    /// Called when the underlying transaction fails — the failure is *not*
    /// cached, so the client may retry with a corrected payload using the
    /// same key (FEAT-008 US-081 AC4).
    pub fn release(&self, db_id: &str, key: &str) {
        let mut entries = self
            .entries
            .lock()
            .expect("idempotency store mutex poisoned");
        let composite = (db_id.to_string(), key.to_string());
        if let Some(entry) = entries.get(&composite) {
            if matches!(entry.status, EntryStatus::Pending) {
                entries.remove(&composite);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("idempotency store mutex poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::FakeClock;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CannedResponse(&'static str);

    fn store_with_clock(
        clock: FakeClock,
        ttl: Duration,
    ) -> (IdempotencyStore<CannedResponse>, FakeClock) {
        let store = IdempotencyStore::<CannedResponse>::new(Arc::new(clock.clone()), ttl);
        (store, clock)
    }

    #[test]
    fn fresh_key_reserves_then_caches_response() {
        let (store, _clock) = store_with_clock(FakeClock::new(), Duration::from_secs(60));

        // First call: fresh key reserves.
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved
        );

        // While pending, a second call returns InFlight.
        assert!(matches!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::InFlight { .. }
        ));

        // After the response is stored, subsequent calls return the cached body.
        store.store_response("db1", "key-A", CannedResponse("ok"));
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::AlreadyCached(CannedResponse("ok"))
        );
        // A third call still returns the cached response.
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::AlreadyCached(CannedResponse("ok"))
        );
    }

    #[test]
    fn ttl_expiry_evicts_cached_response_via_clock() {
        let clock = FakeClock::new();
        let (store, clock) = store_with_clock(clock, Duration::from_secs(60));

        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved
        );
        store.store_response("db1", "key-A", CannedResponse("v1"));

        // Just inside the TTL: still cached.
        clock.advance(Duration::from_secs(59));
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::AlreadyCached(CannedResponse("v1"))
        );

        // Cross the TTL boundary — the entry must be evicted on lookup.
        clock.advance(Duration::from_secs(2));
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved,
            "key must be re-issued after TTL expiry without thread::sleep",
        );
    }

    #[test]
    fn in_flight_concurrent_reservation_returns_inflight_with_retry_after() {
        let (store, _clock) = store_with_clock(FakeClock::new(), Duration::from_secs(60));

        // First request reserves.
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved
        );

        // Second request before store_response — must return InFlight with a
        // retry-after hint derived from the TTL.
        match store.try_reserve("db1", "key-A") {
            ReservationResult::InFlight { retry_after_ms } => {
                assert_eq!(retry_after_ms, 60_000);
            }
            other => panic!("expected InFlight, got {:?}", other),
        }
    }

    #[test]
    fn failed_transaction_release_allows_retry() {
        let (store, _clock) = store_with_clock(FakeClock::new(), Duration::from_secs(60));

        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved
        );
        // Transaction failed — release the reservation without caching.
        store.release("db1", "key-A");

        // A retry must be able to re-execute (no cached failure).
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::Reserved
        );
    }

    #[test]
    fn release_does_not_evict_completed_entry() {
        let (store, _clock) = store_with_clock(FakeClock::new(), Duration::from_secs(60));

        store.try_reserve("db1", "key-A");
        store.store_response("db1", "key-A", CannedResponse("ok"));

        // Calling release on a completed key must be a no-op.
        store.release("db1", "key-A");
        assert_eq!(
            store.try_reserve("db1", "key-A"),
            ReservationResult::AlreadyCached(CannedResponse("ok"))
        );
    }

    #[test]
    fn keys_are_scoped_per_database() {
        let (store, _clock) = store_with_clock(FakeClock::new(), Duration::from_secs(60));

        store.try_reserve("db-a", "shared-key");
        store.store_response("db-a", "shared-key", CannedResponse("a"));

        // Same key in a different database is independent.
        assert_eq!(
            store.try_reserve("db-b", "shared-key"),
            ReservationResult::Reserved
        );
        store.store_response("db-b", "shared-key", CannedResponse("b"));

        assert_eq!(
            store.try_reserve("db-a", "shared-key"),
            ReservationResult::AlreadyCached(CannedResponse("a"))
        );
        assert_eq!(
            store.try_reserve("db-b", "shared-key"),
            ReservationResult::AlreadyCached(CannedResponse("b"))
        );
    }

    #[test]
    fn evict_expired_returns_removed_count() {
        let clock = FakeClock::new();
        let (store, clock) = store_with_clock(clock, Duration::from_secs(60));

        store.try_reserve("db1", "k1");
        store.try_reserve("db1", "k2");
        store.store_response("db1", "k1", CannedResponse("c1"));
        store.store_response("db1", "k2", CannedResponse("c2"));
        assert_eq!(store.len(), 2);

        // Within TTL — nothing to evict.
        clock.advance(Duration::from_secs(30));
        assert_eq!(store.evict_expired(), 0);
        assert_eq!(store.len(), 2);

        // Past TTL — both entries evicted.
        clock.advance(Duration::from_secs(31));
        assert_eq!(store.evict_expired(), 2);
        assert_eq!(store.len(), 0);
    }
}
