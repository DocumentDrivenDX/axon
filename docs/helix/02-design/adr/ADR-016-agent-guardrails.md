---
dun:
  id: ADR-016
  depends_on:
    - ADR-005
    - FEAT-012
    - FEAT-022
    - FEAT-003
---
# ADR-016: Agent Guardrails — Rate Limiting and Scope Constraints

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-13 | Accepted | Erik LaBianca | FEAT-022, FEAT-012, FEAT-003 | High |

## Context

Audit trails (FEAT-003) are reactive — they record what happened after the
fact. In agentic workflows, a misbehaving or misconfigured agent can submit
structurally valid mutations that are semantically wrong, or it can bulk-
mutate thousands of entities in a tight loop before a human can intervene.

Axon needs *preventive* controls that sit between the auth layer (FEAT-012)
and the storage layer, enforcing two classes of per-agent constraint:

1. **Rate limiting**: cap how many mutations a given agent identity can
   commit per second and per minute, with configurable burst headroom.

2. **Scope constraints**: reject any mutation that targets an entity the
   agent's credential does not cover, as determined by a filter annotation
   on the identity.

Both controls operate on `CallerIdentity` (already threaded through the
request context by the auth middleware, per FEAT-012 / bead axon-e0817efb).
They are separate from RBAC — RBAC decides *what kind* of operation an actor
may perform; guardrails decide *how much* and *on which specific entities*.

Semantic validation hooks (allowing external validators to examine proposed
mutations in context before commit) are architecturally desirable but are
explicitly deferred by the FEAT-022 spec. This ADR does not design them.

| Aspect | Description |
|--------|-------------|
| Problem | No preventive controls to cap agent mutation rate or constrain entity scope |
| Current State | Auth middleware (FEAT-012) provides RBAC but no per-agent rate or scope limits |
| Requirements | Token-bucket rate limiter per actor; entity-filter scope constraint per credential; audit entries for all rejections |

## Decision

### 1. Rate Limiting Strategy

**Token-bucket algorithm** per agent identity.

Token buckets are a standard, well-understood algorithm for rate limiting.
They naturally express both a sustained rate and an allowable burst, which
matches the FEAT-022 requirement ("burst allowance configurable separately
from sustained rate"). A simple in-memory implementation is sufficient for
V1 — no cross-process coordination is needed because each Axon instance is
single-process.

#### Bucket Configuration (per actor)

```toml
[guardrails.rate_limit]
# Default limits applied to all agents unless overridden
mutations_per_second = 10
mutations_per_minute = 200
burst_allowance = 20
```

Per-actor overrides can specify the same fields under a named stanza:

```toml
[guardrails.rate_limit.overrides."agent-reconciler"]
mutations_per_second = 100
mutations_per_minute = 2000
burst_allowance = 50
```

#### Data Structure

```rust
/// Per-actor rate limiting state.
struct TokenBucket {
    /// Fractional tokens available (sub-second precision).
    tokens: f64,
    /// Maximum tokens (= burst_allowance).
    capacity: f64,
    /// Tokens added per second (= mutations_per_second).
    refill_rate: f64,
    /// Separate per-minute counter (sliding window).
    minute_count: u64,
    /// Timestamp of the start of the current minute window.
    minute_window_start: Instant,
    /// Per-minute limit.
    mutations_per_minute: u64,
    /// Last refill timestamp (for computing elapsed time).
    last_refill: Instant,
}
```

The guardrails layer holds:

```rust
struct GuardrailsLayer {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    config: GuardrailsConfig,
    audit: Arc<dyn AuditLog>,
}
```

- `HashMap<String, TokenBucket>` is keyed by `CallerIdentity.actor`.
- The `Mutex` is a `std::sync::Mutex` (not async). Lock contention is
  negligible because the critical section is tiny (token arithmetic + map
  lookup). This satisfies the <1ms overhead requirement.
- Buckets are created lazily on first mutation from an actor.
- Buckets for actors not seen recently are evicted (LRU or time-based)
  to bound memory usage.

#### Token Consumption

Each mutation call (create, update, patch, delete) consumes one token from
the per-second bucket and increments the per-minute counter. Queries are not
rate-limited — guardrails apply to write operations only.

#### Rate Limit Exceeded Response

When a bucket is empty (or the per-minute cap is reached), the operation is
rejected with a **retryable** error:

```rust
AxonError::RateLimitExceeded {
    actor: String,
    retry_after_ms: u64,
}
```

`retry_after_ms` is computed as the time until enough tokens refill for one
more mutation. The HTTP layer maps this to `429 Too Many Requests` with a
`Retry-After` header; the gRPC layer uses `RESOURCE_EXHAUSTED` with a
`google.rpc.RetryInfo` detail.

### 2. Scope Constraints

**Entity-filter annotation on `CallerIdentity`**.

For V1, scope is expressed as an optional `entity_filter` field on
`CallerIdentity`. The filter is a simple field-equality predicate that the
guardrails layer evaluates against the target entity's data before committing
the mutation.

#### CallerIdentity Extension

```rust
pub struct CallerIdentity {
    pub actor: String,
    pub role: Role,
    /// Optional scope constraint. If present, this actor may only mutate
    /// entities that match this filter. `None` means no scope restriction.
    pub entity_filter: Option<EntityFilter>,
}

/// A simple field-equality filter for V1 scope constraints.
/// E.g., `{ field: "assignee", value: "agent-reconciler" }`
pub struct EntityFilter {
    pub field: String,
    pub value: serde_json::Value,
}
```

The `entity_filter` is populated by the identity provider at request
authentication time. For Tailscale, it can be derived from a node
attribute or ACL tag annotation. For OIDC, it maps from a JWT claim.
In `--no-auth` mode, `entity_filter` is always `None` (no restriction).

#### Scope Check Logic

Before any mutation is executed, the guardrails layer checks:

1. Does the caller have an `entity_filter`? If not, proceed.
2. Fetch the target entity (it must exist for updates/patches/deletes; for
   creates, apply the filter to the incoming data).
3. Evaluate `entity.data[filter.field] == filter.value`.
4. If the filter does not match, reject with:

```rust
AxonError::ScopeViolation {
    actor: String,
    entity_id: String,
    filter: EntityFilter,
}
```

**Filter evaluation is zero-extra-read for updates and deletes**: the storage
layer already reads the entity to apply the mutation. The guardrails layer
receives the pre-mutation entity data as part of the normal mutation flow and
evaluates the filter against it without an additional storage read, satisfying
the non-functional requirement.

For **creates**, the incoming entity data is evaluated against the filter
before writing. If the new entity does not match the filter, the create is
rejected. This prevents an agent from creating entities outside its scope.

#### Scope vs. RBAC

RBAC (from FEAT-012) is enforced *before* guardrails in the middleware stack:

```
Request → Auth middleware (identity + RBAC) → Guardrails layer → Handler
```

An actor must first pass RBAC (have the `write` role) before guardrails
are evaluated. Guardrails add per-agent preventive controls on top of
role-based access — they do not replace RBAC.

### 3. Middleware Position

The guardrails layer is a wrapper around `AxonHandler` (not an axum
middleware layer), implemented as a `GuardrailsHandler` that delegates to
the inner handler after passing checks:

```rust
impl AxonHandler for GuardrailsHandler {
    async fn create_entity(&self, req: CreateEntityRequest, caller: &CallerIdentity)
        -> Result<Entity, AxonError>
    {
        self.check_rate_limit(caller)?;
        self.check_scope_create(caller, &req.data)?;
        let result = self.inner.create_entity(req, caller).await?;
        Ok(result)
    }

    async fn update_entity(&self, req: UpdateEntityRequest, caller: &CallerIdentity)
        -> Result<Entity, AxonError>
    {
        self.check_rate_limit(caller)?;
        // Scope check happens inside inner handler after entity fetch;
        // GuardrailsHandler passes the filter to inner via context.
        self.inner.update_entity_with_filter(req, caller).await
    }
    // ... patch, delete similarly
}
```

This keeps guardrails independent of the transport layer (HTTP, gRPC, MCP,
GraphQL all share the same `AxonHandler` interface).

### 4. Audit Trail for Rejections

All guardrail rejections produce an audit log entry with:

| Field | Value |
|-------|-------|
| `operation` | `guardrail_rejection` |
| `actor` | `caller.actor` |
| `entity_id` | target entity ID (if known) |
| `collection` | target collection |
| `reason` | `rate_limit_exceeded` or `scope_violation` |
| `detail` | structured JSON: `{ retry_after_ms }` or `{ filter, actual_value }` |

Audit entries are written even when the mutation is rejected — rejection is
an observable event that operators need to detect misbehaving agents. The
`guardrail_rejection` operation is treated as a read-level audit event (it
does not change entity data) and does not itself consume a rate-limit token.

### 5. Configuration

All guardrail configuration lives in `axon-server.toml` (or equivalent
config file). There is no API to configure limits in V1 — configuration
changes require a server restart.

```toml
[guardrails]
enabled = true

[guardrails.rate_limit]
# Default limits for all actors not explicitly overridden
mutations_per_second = 10
mutations_per_minute = 200
burst_allowance = 20

# Per-actor overrides (keyed by CallerIdentity.actor)
[guardrails.rate_limit.overrides."agent-reconciler"]
mutations_per_second = 100
mutations_per_minute = 2000
burst_allowance = 50

[guardrails.rate_limit.overrides."anonymous"]
# Disable rate limiting for anonymous (--no-auth dev mode)
mutations_per_second = 0  # 0 = unlimited
mutations_per_minute = 0
burst_allowance = 0
```

Scope constraints (`entity_filter`) are not configured in `axon-server.toml`
— they are injected by the identity provider at authentication time and live
on `CallerIdentity`. The identity provider configuration (Tailscale tag
mappings, OIDC claim mappings) is where scope filters are bound to actors.

### 6. Crate Location

The guardrails implementation lives in `crates/axon-core/src/guardrails.rs`
(or a `guardrails/` submodule). It has no new crate dependencies — it uses
`std::sync::Mutex`, `std::collections::HashMap`, and `std::time::Instant`
from the standard library, plus the existing `AxonError` and `CallerIdentity`
types from `axon-core`.

## Consequences

**Positive**:
- Rate limiting is in-process with no external dependencies (Redis, Memcached,
  etc.), keeping the <1ms overhead requirement achievable
- Token buckets naturally express burst + sustained rates with a single data
  structure
- Scope constraints require zero additional storage reads for updates/deletes
  (reuse the pre-mutation entity fetch)
- Guardrails are transport-agnostic (operate on `AxonHandler`, not HTTP/gRPC)
- All rejections are auditable, giving operators visibility into misbehaving
  agents
- Clear separation: RBAC grants access by role; guardrails constrain by
  identity (actor string) and entity attributes

**Negative**:
- In-process rate limiting means limits are per-instance, not per-cluster. In
  a multi-instance deployment, an agent can exceed the intended global rate by
  routing to multiple instances. Acceptable for V1 (single-instance deployments
  are the target for now)
- `entity_filter` V1 supports only field-equality predicates. Complex
  conditions (range checks, multi-field conjunctions) require V2 work
- Rate limit state is lost on server restart. Agents can burst immediately
  after a restart. Acceptable for V1
- Scope check for creates evaluates incoming data, not persisted data — an
  agent could create an entity matching the filter, then have another actor
  update the filter field. Post-creation scope drift is not prevented by V1
  guardrails

**Deferred**:
- **Semantic validation hooks**: custom validators that examine proposed
  mutations in business context. Explicitly deferred by FEAT-022 spec
- **Configuring limits via API**: V1 is config-file only; live reconfiguration
  without restart is deferred
- **Distributed rate limiting**: cross-instance coordination (Redis token
  bucket) for multi-instance deployments
- **Compound entity filters**: multi-field and range predicates beyond simple
  field equality
- **Entities-affected-per-transaction limit**: FEAT-022 mentions this as a
  requirement; deferred to a follow-on bead after the per-second/per-minute
  limits are proven

## References

- [FEAT-022: Agent Guardrails](../../01-frame/features/FEAT-022-agent-guardrails.md)
- [FEAT-012: Authorization](../../01-frame/features/FEAT-012-authorization.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [ADR-005: Authentication (Tailscale tsnet)](ADR-005-authentication-tailscale-tsnet.md)
- [Token Bucket Algorithm](https://en.wikipedia.org/wiki/Token_bucket)
