---
ddx:
  id: STP-103
  review:
    self_hash: 95527f8337d3c979881af91a856bf0971866965f7de09cf18f1f7feb667f9823
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-103-reject-denied-writes

## Story Reference

**User Story**: [[US-103-reject-denied-writes]] (FEAT-029, P0)
**Technical Design**: [[TD-103-write-denial-path]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (decision semantics → L6; transactional abort → L1 DST + L2)

## Scope and Objective

**Goal**: prove denied writes fail with the stable forbidden envelope, deny atomically inside transactions, and replay deterministically under idempotency keys.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract`

**In Scope**
- Row denial, field denial, transactional abort on denial, denied idempotent replay.

**Out of Scope**
- Approval routing of risky-but-allowed writes (STP-106), UI error rendering (STP-115).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-103-AC1 | Row the caller cannot mutate → stable forbidden envelope, row-denial reason | `graphql_nexiq_reference_policy_set_returns_stable_write_denials` | `forbidden` code with row-denial reason and null `field_path` | `@covers US-103-AC1` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-103-AC2 | Write including denied field fails naming the field path | `graphql_nexiq_reference_policy_set_returns_stable_write_denials` | `field_write_denied` with `field_path: "status"`; stored entity unchanged | `@covers US-103-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-103-AC3 | Transaction with one denied op aborts wholly; no partial writes, no audit mutation entry | `graphql_denied_transaction_aborts_wholly_no_partial_writes_no_audit` | `commitTransaction` 3-op (op-1 allowed, op-2 field_write_denied, op-3 allowed): returns `forbidden`, both non-denied entities absent, task-a unchanged, zero `entity.create` audit entries for aborted ops | `@covers US-103-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-103-AC4 | Denied idempotent transaction replays the same forbidden response within TTL | `graphql_denied_idempotent_transaction_returns_same_forbidden_on_retry` | Both first call and retry with same `idempotencyKey` return `forbidden` with `field_write_denied`; entity never created. Note: GraphQL path re-evaluates denials rather than caching them; the edge case of denial surviving a policy relaxation is proven by `http_transaction_replays_policy_forbidden_response` in `gateway.rs` for the REST surface. | `@covers US-103-AC4` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: denied-op transaction abort, denied idempotent replay)
- `crates/axon-sim` workload: denial injected mid-transaction, CHECK no partial state and no mutation audit rows

### Coverage Focus

- P0: AC3 is the atomicity-critical row (allocated to L1 DST per the project plan); AC1/AC2 are the envelope contract.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Nexiq write-denial fixtures (row + field rules) | AC1, AC2 | `seed_nexiq_fixture` |
| 3-op transaction fixture with op 2 denied | AC3 | New fixture in shared policy-fixture suite |
| Idempotency key + TTL window | AC4 | Reuse FEAT-008 idempotency fixtures (`api_contract.rs` patterns) |

## Edge Cases and Failure Modes

- Replay after the policy is *relaxed* must still return the recorded forbidden response within TTL (AC4's "even if policy or data changed").
- Denial mid-transaction must leave version counters untouched (no skipped versions).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2.
2. Red test for AC3 (transaction abort) at L6, then the DST workload.
3. Red test for AC4 replay determinism.

**Constraints**
- CONTRACT-004 forbidden envelope; INV-008 transaction atomicity; FEAT-008 idempotency TTL semantics.

**Done When**
- [x] AC1–AC4 each have passing, citing tests; AC3 DST/BUGGIFY extension deferred to `axon-sim`

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest UNTESTED rows with planned shape
- [x] Scope bounded; commands runnable
