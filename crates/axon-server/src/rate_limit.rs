//! Per-actor in-memory write rate limiting.
//!
//! Tracks write operations per actor using a sliding window. When the limit is
//! breached, returns a `Retry-After` duration so the gateway can respond with
//! HTTP 429 Too Many Requests.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of write operations allowed per actor within the window.
    pub max_writes: u64,
    /// Duration of the sliding window.
    pub window: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_writes: 1000,
            window: Duration::from_secs(60),
        }
    }
}

/// Result of a rate limit check when the limit has been exceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimited {
    /// How many seconds the caller should wait before retrying.
    pub retry_after_secs: u64,
}

/// Per-actor sliding-window write rate limiter.
///
/// Each actor gets an independent window of timestamps. When `check()` is
/// called, expired entries are pruned and the current count is compared against
/// the configured maximum. If the limit is exceeded, a [`RateLimited`] error
/// is returned with the number of seconds until the oldest entry in the window
/// expires.
#[derive(Clone)]
pub struct WriteRateLimiter {
    config: RateLimitConfig,
    windows: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
}

impl WriteRateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            windows: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check whether the given actor is allowed to perform a write operation.
    ///
    /// On success, the current instant is recorded in the actor's window.
    /// On failure, returns [`RateLimited`] with the retry-after duration.
    pub async fn check(&self, actor: &str) -> Result<(), RateLimited> {
        self.check_at(actor, Instant::now()).await
    }

    /// Check with an explicit timestamp (for testing).
    pub(crate) async fn check_at(&self, actor: &str, now: Instant) -> Result<(), RateLimited> {
        let mut windows = self.windows.lock().await;
        let window = windows.entry(actor.to_string()).or_default();

        // Prune expired entries from the front of the deque.
        let cutoff = now.checked_sub(self.config.window).unwrap_or(now);
        while window.front().is_some_and(|&ts| ts < cutoff) {
            window.pop_front();
        }

        // Check the count.
        if window.len() as u64 >= self.config.max_writes {
            // Calculate retry-after: time until the oldest entry expires.
            let oldest = window.front().copied().unwrap_or(now);
            let expires_at = oldest + self.config.window;
            let retry_after = if expires_at > now {
                expires_at - now
            } else {
                Duration::from_secs(1)
            };
            // Round up to whole seconds so the Retry-After header is correct.
            let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
            return Err(RateLimited {
                retry_after_secs: secs,
            });
        }

        // Record the new write.
        window.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max_writes: u64, window_secs: u64) -> WriteRateLimiter {
        WriteRateLimiter::new(RateLimitConfig {
            max_writes,
            window: Duration::from_secs(window_secs),
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn allows_writes_under_limit() {
        let rl = limiter(3, 60);
        assert!(rl.check("alice").await.is_ok());
        assert!(rl.check("alice").await.is_ok());
        assert!(rl.check("alice").await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_write_at_limit() {
        let rl = limiter(2, 60);
        assert!(rl.check("alice").await.is_ok());
        assert!(rl.check("alice").await.is_ok());
        let err = rl.check("alice").await.expect_err("should be rate limited");
        assert!(err.retry_after_secs > 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tracks_actors_independently() {
        let rl = limiter(1, 60);
        assert!(rl.check("alice").await.is_ok());
        assert!(rl.check("bob").await.is_ok());
        assert!(rl.check("alice").await.is_err());
        assert!(rl.check("bob").await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn expired_entries_are_pruned() {
        let rl = limiter(2, 10);
        let now = Instant::now();

        // Two writes at t=0.
        assert!(rl.check_at("alice", now).await.is_ok());
        assert!(rl.check_at("alice", now).await.is_ok());
        assert!(rl.check_at("alice", now).await.is_err());

        // After the window has elapsed, entries are pruned and new writes allowed.
        let after_window = now + Duration::from_secs(11);
        assert!(rl.check_at("alice", after_window).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn retry_after_reflects_oldest_entry_expiry() {
        let rl = limiter(2, 60);
        let now = Instant::now();

        assert!(rl.check_at("alice", now).await.is_ok());
        assert!(
            rl.check_at("alice", now + Duration::from_secs(10))
                .await
                .is_ok()
        );

        // Third write should fail. The oldest entry is at `now`, so it expires
        // at `now + 60s`. If we're at `now + 15s`, retry-after should be ~45s.
        let err = rl
            .check_at("alice", now + Duration::from_secs(15))
            .await
            .expect_err("should be rate limited");
        assert_eq!(err.retry_after_secs, 45);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sliding_window_allows_after_partial_expiry() {
        let rl = limiter(2, 10);
        let now = Instant::now();

        // Two writes at t=0 and t=5.
        assert!(rl.check_at("alice", now).await.is_ok());
        assert!(
            rl.check_at("alice", now + Duration::from_secs(5))
                .await
                .is_ok()
        );

        // At t=11, the first entry (t=0) has expired but the second (t=5) is
        // still within the window. One more write should be allowed.
        let t11 = now + Duration::from_secs(11);
        assert!(rl.check_at("alice", t11).await.is_ok());

        // But a second write at t=11 would exceed the limit (t=5 and t=11 in window).
        assert!(rl.check_at("alice", t11).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn default_config_is_1000_per_60s() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_writes, 1000);
        assert_eq!(config.window, Duration::from_secs(60));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clone_shares_state() {
        let rl = limiter(2, 60);
        let rl2 = rl.clone();

        assert!(rl.check("alice").await.is_ok());
        assert!(rl2.check("alice").await.is_ok());
        // Both clones share the same window, so the third check fails on either.
        assert!(rl.check("alice").await.is_err());
    }
}
