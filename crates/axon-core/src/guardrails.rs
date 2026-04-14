//! Agent guardrails: per-actor rate limiting and entity scope constraints
//! (FEAT-022, ADR-016).
//!
//! This module is the preventive cousin of the audit log. Where the audit log
//! is reactive (it records what happened), guardrails sit between the auth
//! layer and the storage layer and *reject* mutations that would burst-write
//! beyond an actor's allowed rate, or that target entities outside the
//! actor's declared scope.
//!
//! ## Rate limiting
//!
//! [`GuardrailsLayer::check_rate_limit`] consults a per-actor [`TokenBucket`]
//! whose configuration lives in [`GuardrailsConfig`]. Token buckets are a
//! standard, well-understood algorithm that naturally expresses both a
//! sustained refill rate (`mutations_per_second`) and an allowable burst
//! (`burst_allowance`). The implementation is in-process and lock-protected
//! via [`std::sync::Mutex`]; the critical section is a tiny piece of token
//! arithmetic, satisfying the <1ms overhead requirement from FEAT-022.
//!
//! Time progresses through an injected [`Clock`] rather than through direct
//! `Instant::now()` calls. This is the **critical testability requirement**
//! of FEAT-022: deterministic time-based tests must be able to advance the
//! clock manually instead of sleeping. Production code passes
//! [`crate::clock::SystemClock`]; tests pass [`crate::clock::FakeClock`].
//!
//! ## Scope constraints
//!
//! [`GuardrailsLayer::check_scope`] evaluates the caller's optional
//! [`EntityFilter`] against the entity data being mutated. For updates,
//! patches, and deletes, the caller passes the *pre-mutation* entity state;
//! for creates, the *incoming* data. A non-matching filter produces
//! [`AxonError::ScopeViolation`].
//!
//! ## --no-auth bypass
//!
//! When [`GuardrailsConfig::bypass_anonymous`] is set (the default), all
//! guardrail checks are skipped for the `"anonymous"` actor. This implements
//! the FEAT-022 acceptance criterion that `--no-auth` mode bypasses guardrails
//! without requiring callers to thread a separate "auth disabled" flag through
//! the request pipeline.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::auth::{CallerIdentity, EntityFilter};
use crate::clock::{Clock, SystemClock};
use crate::error::AxonError;

/// Per-actor rate limit configuration.
///
/// A `mutations_per_second` value of `0.0` (or `burst_allowance == 0.0`)
/// disables rate limiting for that actor — used for the anonymous override
/// in `--no-auth` dev mode.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitConfig {
    /// Sustained refill rate in tokens (= mutations) per second.
    pub mutations_per_second: f64,
    /// Bucket capacity (= maximum tokens) — the allowable burst beyond the
    /// sustained rate.
    pub burst_allowance: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            mutations_per_second: 10.0,
            burst_allowance: 20.0,
        }
    }
}

impl RateLimitConfig {
    /// Returns `true` when this configuration disables rate limiting (either
    /// field is zero or negative). Used by the per-actor "unlimited" override.
    pub fn is_unlimited(&self) -> bool {
        self.mutations_per_second <= 0.0 || self.burst_allowance <= 0.0
    }
}

/// Top-level guardrails configuration.
///
/// Rate limit defaults apply to all actors not in `overrides`. The anonymous
/// actor is bypassed entirely when `bypass_anonymous` is set, regardless of
/// any explicit override (this is what makes `--no-auth` mode skip checks).
#[derive(Debug, Clone)]
pub struct GuardrailsConfig {
    /// Master switch — when `false`, guardrails are a no-op.
    pub enabled: bool,
    /// Default per-actor rate limit applied to actors without an override.
    pub default_rate_limit: RateLimitConfig,
    /// Per-actor rate limit overrides keyed by `CallerIdentity.actor`.
    pub overrides: HashMap<String, RateLimitConfig>,
    /// When `true`, the anonymous actor (`--no-auth` mode) bypasses all
    /// guardrail checks. Defaults to `true`.
    pub bypass_anonymous: bool,
}

impl Default for GuardrailsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_rate_limit: RateLimitConfig::default(),
            overrides: HashMap::new(),
            bypass_anonymous: true,
        }
    }
}

/// Per-actor token bucket state.
///
/// The bucket fills at `refill_rate` tokens per second up to `capacity`,
/// and each mutation consumes one token. Time is supplied externally so
/// the bucket has no implicit dependency on the wall clock.
#[derive(Debug)]
pub struct TokenBucket {
    /// Fractional tokens currently available.
    tokens: f64,
    /// Maximum tokens the bucket can hold (= burst allowance).
    capacity: f64,
    /// Tokens added per second of elapsed wall time.
    refill_rate: f64,
    /// Timestamp of the last refill; used to compute elapsed time on the
    /// next refill call.
    last_refill: Instant,
}

impl TokenBucket {
    /// Creates a fresh, full bucket from `config`, anchored at `now`.
    pub fn new(config: &RateLimitConfig, now: Instant) -> Self {
        Self {
            tokens: config.burst_allowance,
            capacity: config.burst_allowance,
            refill_rate: config.mutations_per_second,
            last_refill: now,
        }
    }

    /// Adds tokens earned since the last refill, capped at `capacity`.
    fn refill(&mut self, now: Instant) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = elapsed
                .mul_add(self.refill_rate, self.tokens)
                .min(self.capacity);
            self.last_refill = now;
        }
    }

    /// Attempts to consume one token. On success, returns `Ok(())`. On
    /// failure, returns the number of milliseconds the caller should wait
    /// before enough tokens refill for one more mutation.
    ///
    /// The returned `retry_after_ms` is always `>= 1` so callers cannot
    /// busy-loop with a `0` backoff.
    pub fn try_consume(&mut self, now: Instant) -> Result<(), u64> {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let needed = 1.0 - self.tokens;
            let secs = needed / self.refill_rate;
            let ms = (secs * 1000.0).ceil() as u64;
            Err(ms.max(1))
        }
    }

    /// Read-only view of available tokens (for tests/diagnostics).
    pub fn available_tokens(&self) -> f64 {
        self.tokens
    }
}

/// Structured rejection reason, suitable for audit log metadata.
///
/// Returned by [`GuardrailsLayer::check_rate_limit`] /
/// [`GuardrailsLayer::check_scope`] callers when they want to record *why*
/// a mutation was rejected without re-parsing the [`AxonError`].
#[derive(Debug, Clone)]
pub enum RejectionReason {
    /// The actor's per-second token bucket was empty.
    RateLimitExceeded {
        /// Milliseconds until the next mutation would be allowed.
        retry_after_ms: u64,
    },
    /// The actor's scope filter did not match the target entity.
    ScopeViolation {
        /// Filter that was applied.
        filter: EntityFilter,
    },
}

impl RejectionReason {
    /// Short human-readable label, also used as the `reason` field in the
    /// audit log entry written when guardrails reject a mutation.
    pub fn label(&self) -> &'static str {
        match self {
            RejectionReason::RateLimitExceeded { .. } => "rate_limit_exceeded",
            RejectionReason::ScopeViolation { .. } => "scope_violation",
        }
    }
}

/// Per-actor rate limiting and scope-constraint enforcement.
///
/// Holds:
///
/// - A `HashMap<actor, TokenBucket>` protected by a `std::sync::Mutex`.
///   Buckets are created lazily on first mutation and keyed on the actor
///   string from [`CallerIdentity`].
/// - Configuration for default and per-actor rate limits.
/// - An injected [`Clock`] so tests can advance time deterministically
///   without sleeping.
pub struct GuardrailsLayer {
    config: GuardrailsConfig,
    buckets: Mutex<HashMap<String, TokenBucket>>,
    clock: Arc<dyn Clock>,
}

impl GuardrailsLayer {
    /// Construct a guardrails layer that uses the system clock.
    pub fn new(config: GuardrailsConfig) -> Self {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    /// Construct a guardrails layer that reads time from `clock`.
    ///
    /// This is the constructor tests use to inject a [`crate::clock::FakeClock`]
    /// so they can drive time forward without sleeping.
    pub fn with_clock(config: GuardrailsConfig, clock: Arc<dyn Clock>) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// Read-only access to the configuration.
    pub fn config(&self) -> &GuardrailsConfig {
        &self.config
    }

    /// Returns `true` when the caller should bypass all guardrail checks.
    fn should_bypass(&self, caller: &CallerIdentity) -> bool {
        if !self.config.enabled {
            return true;
        }
        if self.config.bypass_anonymous && caller.actor == "anonymous" {
            return true;
        }
        false
    }

    /// Resolve the effective rate-limit config for `actor`.
    fn rate_config_for(&self, actor: &str) -> RateLimitConfig {
        self.config
            .overrides
            .get(actor)
            .cloned()
            .unwrap_or_else(|| self.config.default_rate_limit.clone())
    }

    /// Consume one rate-limit token for the caller, or reject with
    /// [`AxonError::RateLimitExceeded`].
    ///
    /// Anonymous callers in `--no-auth` mode bypass rate limiting when
    /// [`GuardrailsConfig::bypass_anonymous`] is set. Actors with a
    /// `mutations_per_second == 0.0` override are also unlimited.
    pub fn check_rate_limit(&self, caller: &CallerIdentity) -> Result<(), AxonError> {
        if self.should_bypass(caller) {
            return Ok(());
        }

        let cfg = self.rate_config_for(&caller.actor);
        if cfg.is_unlimited() {
            return Ok(());
        }

        let now = self.clock.now();
        let mut buckets = self.buckets.lock().expect("guardrails mutex poisoned");
        let bucket = buckets
            .entry(caller.actor.clone())
            .or_insert_with(|| TokenBucket::new(&cfg, now));

        bucket
            .try_consume(now)
            .map_err(|retry_after_ms| AxonError::RateLimitExceeded {
                actor: caller.actor.clone(),
                retry_after_ms,
            })
    }

    /// Evaluate the caller's scope filter against entity data, returning
    /// [`AxonError::ScopeViolation`] when the filter does not match.
    ///
    /// `entity_id` is included in the error message so operators can identify
    /// the entity the actor was denied access to. Pass the *pre-mutation*
    /// state for updates/patches/deletes, and the *incoming* data for creates,
    /// per ADR-016 §2.
    pub fn check_scope(
        &self,
        caller: &CallerIdentity,
        entity_id: &str,
        data: &serde_json::Value,
    ) -> Result<(), AxonError> {
        if self.should_bypass(caller) {
            return Ok(());
        }
        let Some(filter) = caller.entity_filter.as_ref() else {
            return Ok(());
        };
        if filter.matches(data) {
            Ok(())
        } else {
            Err(AxonError::ScopeViolation {
                actor: caller.actor.clone(),
                entity_id: entity_id.to_string(),
                filter_field: filter.field.clone(),
                filter_value: filter.value.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Role;
    use crate::clock::FakeClock;
    use serde_json::json;
    use std::time::Duration;

    fn caller(actor: &str) -> CallerIdentity {
        CallerIdentity::new(actor, Role::Write)
    }

    fn fake_clock() -> (FakeClock, Arc<dyn Clock>) {
        let fake = FakeClock::new();
        let clock: Arc<dyn Clock> = Arc::new(fake.clone());
        (fake, clock)
    }

    fn config_with(per_sec: f64, burst: f64) -> GuardrailsConfig {
        GuardrailsConfig {
            enabled: true,
            default_rate_limit: RateLimitConfig {
                mutations_per_second: per_sec,
                burst_allowance: burst,
            },
            overrides: HashMap::new(),
            bypass_anonymous: true,
        }
    }

    // ── Token bucket arithmetic ──────────────────────────────────────────

    #[test]
    fn token_bucket_starts_full() {
        let now = Instant::now();
        let cfg = RateLimitConfig {
            mutations_per_second: 1.0,
            burst_allowance: 5.0,
        };
        let bucket = TokenBucket::new(&cfg, now);
        assert!((bucket.available_tokens() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn token_bucket_refills_over_time() {
        let now = Instant::now();
        let cfg = RateLimitConfig {
            mutations_per_second: 10.0,
            burst_allowance: 10.0,
        };
        let mut bucket = TokenBucket::new(&cfg, now);
        for _ in 0..10 {
            bucket.try_consume(now).unwrap();
        }
        assert!(bucket.try_consume(now).is_err(), "bucket should be empty");

        // Half a second later we should have 5 tokens back.
        bucket.refill(now + Duration::from_millis(500));
        assert!((bucket.available_tokens() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn token_bucket_refill_caps_at_capacity() {
        let now = Instant::now();
        let cfg = RateLimitConfig {
            mutations_per_second: 100.0,
            burst_allowance: 10.0,
        };
        let mut bucket = TokenBucket::new(&cfg, now);
        bucket.try_consume(now).unwrap();
        // After a long wait, tokens should be capped at capacity, not 100+.
        bucket.refill(now + Duration::from_secs(60));
        assert!((bucket.available_tokens() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn token_bucket_retry_after_is_at_least_one_ms() {
        let now = Instant::now();
        let cfg = RateLimitConfig {
            mutations_per_second: 1000.0,
            burst_allowance: 1.0,
        };
        let mut bucket = TokenBucket::new(&cfg, now);
        bucket.try_consume(now).unwrap();
        let err = bucket
            .try_consume(now)
            .expect_err("second consume must fail");
        assert!(err >= 1);
    }

    // ── Rate limiting via the layer (FEAT-022 acceptance test) ──────────

    /// FEAT-022 / axon-0fc26e99 AC: `Rate limit test uses injected clock
    /// (no sleep): inject a clock at T=0; fire 2 mutations (under limit);
    /// advance clock to T=0.1s; fire 3rd mutation — returns retryable error
    /// with retry_after_ms; advance clock to T=1.1s; 3rd mutation now succeeds
    /// (token refilled).`
    ///
    /// Sized so the burst is exactly two tokens. This test runs in
    /// microseconds with no `thread::sleep` calls.
    #[test]
    fn rate_limit_uses_injected_clock_with_no_sleep() {
        let (fake, clock) = fake_clock();
        let config = config_with(/* per_sec */ 1.0, /* burst */ 2.0);
        let layer = GuardrailsLayer::with_clock(config, clock);
        let caller = caller("agent-1");

        // T=0: two mutations under the burst.
        layer.check_rate_limit(&caller).unwrap();
        layer.check_rate_limit(&caller).unwrap();

        // T=0.1s: third mutation must fail because no tokens are available
        // and only 0.1s of refill has elapsed (0.1 tokens, < 1.0).
        fake.advance(Duration::from_millis(100));
        let err = layer
            .check_rate_limit(&caller)
            .expect_err("third mutation must be rejected");
        match err {
            AxonError::RateLimitExceeded {
                actor,
                retry_after_ms,
            } => {
                assert_eq!(actor, "agent-1");
                assert!(
                    retry_after_ms > 0,
                    "retry_after_ms must be positive, got {retry_after_ms}"
                );
            }
            other => panic!("expected RateLimitExceeded, got {other:?}"),
        }

        // T=1.1s: a full token has refilled (refill rate is 1/s, elapsed is
        // 1.0s since the second consume). The third mutation now succeeds.
        fake.advance(Duration::from_secs(1));
        layer
            .check_rate_limit(&caller)
            .expect("third mutation should succeed after refill");
    }

    #[test]
    fn rate_limit_per_actor_buckets_are_independent() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(config_with(1.0, 1.0), clock);

        layer.check_rate_limit(&caller("agent-a")).unwrap();
        // agent-a is now exhausted, but agent-b is independent.
        assert!(layer.check_rate_limit(&caller("agent-a")).is_err());
        layer.check_rate_limit(&caller("agent-b")).unwrap();
    }

    #[test]
    fn rate_limit_per_actor_override_grants_higher_burst() {
        let (_fake, clock) = fake_clock();
        let mut config = config_with(1.0, 1.0);
        config.overrides.insert(
            "agent-reconciler".into(),
            RateLimitConfig {
                mutations_per_second: 100.0,
                burst_allowance: 100.0,
            },
        );
        let layer = GuardrailsLayer::with_clock(config, clock);
        // 50 mutations should comfortably fit inside the override's 100-burst.
        for _ in 0..50 {
            layer
                .check_rate_limit(&caller("agent-reconciler"))
                .expect("override should allow large burst");
        }
    }

    #[test]
    fn anonymous_caller_bypasses_rate_limit() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(config_with(1.0, 1.0), clock);
        let anon = CallerIdentity::anonymous();
        // Far more mutations than the burst allowance — all should pass.
        for _ in 0..100 {
            layer.check_rate_limit(&anon).unwrap();
        }
    }

    #[test]
    fn disabled_layer_is_a_noop() {
        let (_fake, clock) = fake_clock();
        let mut config = config_with(1.0, 1.0);
        config.enabled = false;
        let layer = GuardrailsLayer::with_clock(config, clock);
        for _ in 0..50 {
            layer.check_rate_limit(&caller("agent-x")).unwrap();
        }
    }

    #[test]
    fn unlimited_override_skips_bucket() {
        let (_fake, clock) = fake_clock();
        let mut config = config_with(1.0, 1.0);
        config.overrides.insert(
            "internal-svc".into(),
            RateLimitConfig {
                mutations_per_second: 0.0,
                burst_allowance: 0.0,
            },
        );
        let layer = GuardrailsLayer::with_clock(config, clock);
        for _ in 0..1000 {
            layer.check_rate_limit(&caller("internal-svc")).unwrap();
        }
    }

    // ── Scope checks ────────────────────────────────────────────────────

    /// FEAT-022 / axon-0fc26e99 AC: `agent with scope_filter
    /// 'assignee=agent-1' cannot mutate entity with assignee=agent-2 (returns
    /// ScopeViolation)`.
    #[test]
    fn scope_violation_when_filter_does_not_match() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(GuardrailsConfig::default(), clock);
        let caller = CallerIdentity::new("agent-1", Role::Write)
            .with_entity_filter(EntityFilter::new("assignee", json!("agent-1")));
        let other_data = json!({"assignee": "agent-2", "title": "task"});

        let err = layer
            .check_scope(&caller, "task-7", &other_data)
            .expect_err("scope check must fail");
        match err {
            AxonError::ScopeViolation {
                actor,
                entity_id,
                filter_field,
                filter_value,
            } => {
                assert_eq!(actor, "agent-1");
                assert_eq!(entity_id, "task-7");
                assert_eq!(filter_field, "assignee");
                assert_eq!(filter_value, json!("agent-1"));
            }
            other => panic!("expected ScopeViolation, got {other:?}"),
        }
    }

    #[test]
    fn scope_check_passes_when_filter_matches() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(GuardrailsConfig::default(), clock);
        let caller = CallerIdentity::new("agent-1", Role::Write)
            .with_entity_filter(EntityFilter::new("assignee", json!("agent-1")));
        layer
            .check_scope(&caller, "task-1", &json!({"assignee": "agent-1"}))
            .unwrap();
    }

    #[test]
    fn scope_check_passes_when_no_filter() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(GuardrailsConfig::default(), clock);
        let caller = caller("unfiltered");
        // Whatever the data, no filter means no scope restriction.
        layer.check_scope(&caller, "any", &json!({"x": 1})).unwrap();
    }

    #[test]
    fn scope_check_rejects_missing_field() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(GuardrailsConfig::default(), clock);
        let caller = CallerIdentity::new("agent-1", Role::Write)
            .with_entity_filter(EntityFilter::new("assignee", json!("agent-1")));
        // Field absent — must reject (the alternative would silently allow
        // any entity that happened to omit the scoped field).
        let err = layer.check_scope(&caller, "task", &json!({"title": "x"}));
        assert!(matches!(err, Err(AxonError::ScopeViolation { .. })));
    }

    #[test]
    fn anonymous_bypasses_scope_check() {
        let (_fake, clock) = fake_clock();
        let layer = GuardrailsLayer::with_clock(GuardrailsConfig::default(), clock);
        // Even if anonymous somehow had a filter, --no-auth must bypass it.
        let mut anon = CallerIdentity::anonymous();
        anon.entity_filter = Some(EntityFilter::new("assignee", json!("nobody")));
        layer
            .check_scope(&anon, "task", &json!({"assignee": "agent-2"}))
            .unwrap();
    }

    // ── Rejection reason labels (consumed by the audit log writer) ──────

    #[test]
    fn rejection_reason_labels() {
        assert_eq!(
            RejectionReason::RateLimitExceeded { retry_after_ms: 5 }.label(),
            "rate_limit_exceeded"
        );
        assert_eq!(
            RejectionReason::ScopeViolation {
                filter: EntityFilter::new("assignee", json!("agent-1"))
            }
            .label(),
            "scope_violation"
        );
    }
}
