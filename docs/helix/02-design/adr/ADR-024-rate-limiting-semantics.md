---
ddx:
  id: ADR-024
  depends_on:
    - ADR-016
    - ADR-018
    - ADR-019
    - FEAT-022
  review:
    self_hash: 4b62772a355802aa5e4972eb22ad23c908eded8459621ee56d26958eb7c0642f
    deps:
      ADR-016: d023701c0bedc5ada8a9121fa850a6b78d7b2b2f39d2b7ac41d7d2c48de7a1b9
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      ADR-019: 3d6482363128cb8e6bc2cb86023a0a66c6a1c3027fab72ad99938d8136bb9732
      FEAT-022: 63ecd2aff32e4cc0aa516c6cc8632ffb5ed3a004a6b633edf60dfc0b038f0fc6
    reviewed_at: "2026-06-14T03:52:45Z"
---
# ADR-024: Rate Limiting Semantics — Per-Actor Sliding Window

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-10 | Accepted | Erik LaBianca | ADR-016, ADR-018, ADR-019, FEAT-022, CONTRACT-001 | High |

## Context

ADR-016 selected a token-bucket algorithm for per-agent write rate limiting.
The implementation that actually shipped uses a different mechanism: a
per-server, per-actor **sliding window** whose accounting is shared across
tenant, database, and write route. The semantics were recorded in
`api-contracts.md` (now [CONTRACT-001](../contracts/CONTRACT-001-http-api-surface.md))
without a decision record, leaving ADR-016 describing an algorithm Axon does
not run. This ADR records the implemented semantics as the deliberate
decision and supersedes ADR-016's algorithm section.

| Aspect | Description |
|--------|-------------|
| Problem | The shipped limiter (sliding window, shared scope, `retry_after_seconds` envelope) diverged from ADR-016's accepted token-bucket design with no governing decision record |
| Current State | CONTRACT-001 documents the sliding-window contract as normative; ADR-016 §1 still describes token buckets |
| Requirements | FEAT-022 preventive rate limiting; deterministic, retryable 429 envelope; <1ms overhead; one limiter behavior across HTTP, GraphQL, and MCP write routes |
| Decision Drivers | Implementation simplicity; one shared accounting state per actor instead of bucket-per-(tenant, database, route); contract already normative in CONTRACT-001 and exercised by clients |

## Decision

We will rate-limit writes with a **per-server, per-actor sliding window**
whose accounting is **shared across tenant, database, and write route** —
one window per actor per server process, not per-tenant or per-route buckets.
Each write endpoint call consumes one slot; one transaction request consumes
exactly one slot regardless of operation count; read/query operations are not
rate-limited. Rejections return HTTP 429 with a `Retry-After` header (whole
seconds) and the body envelope:

```json
{
  "code": "rate_limit_exceeded",
  "detail": {
    "message": "write rate limit exceeded",
    "retry_after_seconds": 60,
    "scope": "actor_write"
  }
}
```

The `retry_after_seconds` envelope recorded in
[CONTRACT-001](../contracts/CONTRACT-001-http-api-surface.md) (formerly
`api-contracts.md`) is hereby ratified as the deliberate decision, not an
implementation accident. CONTRACT-001 owns the normative envelope; this ADR
owns the rationale.

**Key Points**: Sliding window per actor per server | Accounting shared
across tenant/database/write-route | `retry_after_seconds` + `Retry-After`
envelope normative in CONTRACT-001

### Boundary with ADR-019 row policies

Rate limiting and guardrail **scope-constraint checks** (ADR-016 §2) run at
**evaluation step 2** of ADR-019's fixed evaluation order — after FEAT-012
identity, tenant membership, and credential grants (step 1), and before any
policy evaluation (steps 3-7). They are cheap, policy-independent gates:
the limiter consumes a slot and the credential's scope filter is checked
without compiling or evaluating collection policy. Row predicate policy
(ADR-019 step 4) remains the authority for record-level visibility and
mutation filtering; guardrail scope constraints do not replace, pre-empt, or
duplicate row policies — a write must pass both.

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Token bucket per actor (ADR-016) | Natural burst + sustained-rate expression; previously accepted | Implementation chose sliding window for simplicity and shared-state behavior; bucket-per-scope state multiplies with tenants/routes | Rejected: not what shipped; sliding window met requirements with less state |
| Per-tenant (or per-tenant-per-actor) scoping | Satisfies FEAT-022 GRD-06 tenant isolation of write capacity | More accounting state; tenant fairness policy questions unresolved; not needed at V1 single-instance scale | Rejected for V1: FEAT-022's tenant-scoping requirement remains a future refinement (see Risks) |
| Distributed limiter (Redis or shared store) | Cluster-accurate global limits | External dependency; latency; contradicts ADR-016's in-process constraint | Rejected: per-server is the accepted V1 boundary |
| **Per-server, per-actor sliding window, shared scope** | Simple; one state per actor; deterministic `retry_after_seconds`; already implemented and contract-tested | Coarse: no per-tenant fairness; burst shaping weaker than token bucket | **Selected: ratifies the implemented, contract-normative behavior** |

## Consequences

| Type | Impact |
|------|--------|
| Positive | ADR record matches running code and CONTRACT-001; clients get a deterministic, retryable 429 envelope; single per-actor state keeps overhead <1ms |
| Positive | One limiter behavior across HTTP, GraphQL, and MCP write routes — no per-surface divergence |
| Negative | Shared cross-tenant accounting means one actor's writes in tenant A consume capacity it might need in tenant B, and FEAT-022 GRD-06 (tenant write-capacity isolation) is not satisfied in V1 |
| Negative | Per-server limits still under-enforce global rates in multi-instance deployments (unchanged from ADR-016) |
| Neutral | ADR-016's scope constraints, middleware position, and rejection auditing are unaffected |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| **Tension with FEAT-022 GRD-06**: shared cross-tenant accounting lets one tenant's agents consume capacity affecting another tenant's agents via a shared actor, deferring the tenant-scoping requirement | M | M | Explicitly flagged: per-tenant scoping is a future refinement; revisit when multi-tenant production load arrives; GRD-06 remains an open requirement against this ADR |
| Sliding window allows boundary bursts up to 2x within adjacent windows | M | L | Window granularity tuning; acceptable for V1 |
| Envelope drift between server and CONTRACT-001 | L | H | Contract tests assert the 429 envelope shape |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| 429 responses carry `Retry-After` and `retry_after_seconds` matching CONTRACT-001 | Any contract-test failure |
| One transaction request consumes exactly one slot | Slot accounting regression |
| Limiter overhead <1ms p99 on write paths | Benchmark regression |
| Cross-tenant starvation reports | Reconsider per-tenant scoping (GRD-06) |

## Supersession

- **Supersedes**: ADR-016 §1 (rate-limiter algorithm: token bucket). ADR-016's
  scope constraints, middleware position, and rejection auditing stand.
- **Superseded by**: None

## Concern Impact

- **Concern selection**: Constrains the preventive-safety concern (ADR-016) to
  per-server, per-actor shared accounting for V1; records the GRD-06 tenant
  fairness gap as an accepted, flagged limitation.
- **Practice override**: None.

## References

- [ADR-016: Agent Guardrails](ADR-016-agent-guardrails.md)
- [ADR-018: Tenant, User, and Credential Model](ADR-018-tenant-user-credential-model.md)
- [ADR-019: Policy Authoring and Intents](ADR-019-policy-authoring-and-intents.md)
- [CONTRACT-001: HTTP API Surface](../contracts/CONTRACT-001-http-api-surface.md)
- [FEAT-022: Agent Guardrails](../../01-frame/features/FEAT-022-agent-guardrails.md) (GRD-06)
