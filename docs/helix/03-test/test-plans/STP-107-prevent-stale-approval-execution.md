---
ddx:
  id: STP-107
  review:
    self_hash: 183479492fe052736cd27d8ec3fb79510fc009bdb3b409e75505cd155ed52e6d
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# Story Test Plan: STP-107-prevent-stale-approval-execution

## Story Reference

**User Story**: [[US-107-prevent-stale-approval-execution]] (FEAT-030, P0)
**Technical Design**: [[TD-107-intent-staleness]] — not yet authored; ADR-023 and CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (TOCTOU invariants → L1 DST + L6 contract). This story carries the PRD approval-safety success metric: 100% stale-intent rejection across all five dimensions.

## Scope and Objective

**Goal**: prove every staleness dimension (pre-image, policy, operation hash, plus schema and grant per PRD) rejects commit, multi-entity intents fail atomically, and tokens are single-use.
**Blocking Gate**: `cargo test -p axon-server --test graphql_intents_contract --test mcp_intents_contract`

**In Scope**
- Stale/mismatch commit rejection per dimension; atomic invalidation; token replay/expiry; SDK/GraphQL parity.

**Out of Scope**
- UI stale rendering (STP-118), approval routing (STP-106).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-107-AC1 | Entity version change before commit → stale outcome naming pre-image dimension | `generated_mcp_tools_report_stale_commit_conflict`; `axon_query_intent_commit_conflict_matches_graphql_error_extensions` (stale `{dimension expected actual path}` shape asserted in `graphql_intents_contract.rs`) | Commit fails with stale payload naming the drifted dimension | `@covers US-107-AC1` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/mcp_intents_contract.rs`, `graphql_intents_contract.rs` |
| US-107-AC2 | Policy version change before commit → stale outcome naming policy dimension | `policy_version_drift_before_commit_rejects_as_stale` | Preview at schema v1; `PUT /collections/task/schema` bumps to v2; commit returns `intent_stale` with `policy_version` dimension named | `@covers US-107-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC3 | Commit with operation differing from bound operation hash → mismatch failure | `operation_hash_mismatch_rejects_commit` | Preview with budget 6000; commit supplies budget 7777 in operation; returns `intent_mismatch`; entity unchanged | `@covers US-107-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC4 | Multi-entity intent: one stale entity invalidates the whole intent, no partial commit | `multi_entity_intent_one_stale_entity_invalidates_whole_intent` | Transaction preview over task-a + task-b; out-of-band update bumps task-b version; commit returns `intent_stale` naming task-b pre-image; task-a also unchanged (atomic) | `@covers US-107-AC4` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC5 | Committed token reused → rejected; expired intent cannot commit or be approved | `under_threshold_allow_commit_and_replay_rejects`; `expired_intent_cannot_commit`; `pending_query_materializes_and_audits_expired_intent_lineage` | Replay rejected; expired intents rejected and audited | `@covers US-107-AC5` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC6 | SDK commit helper and GraphQL commit field match for success/stale/mismatch/authz outcomes | `commitIntent returns committed GraphQL success payload @covers US-107-AC6`; `commitIntent preserves stable GraphQL error vocabulary @covers US-107-AC6` | SDK commitIntent returns the committed payload unchanged and preserves stable GraphQL error codes/extensions for stale, mismatch, and forbidden responses | `@covers US-107-AC6` in test bodies | COVERED | L6 parity | `sdk/typescript/test/graphql-client.test.ts` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_intents_contract
cargo test -p axon-server --test mcp_intents_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_intents_contract.rs` (extend: policy-drift, op-hash mismatch, multi-entity atomic invalidation)
- `crates/axon-sim` workload: concurrent commit vs entity mutation race under BUGGIFY (AC4)
- `sdk/typescript/test/graphql-client.test.ts` (extend: governed-workflow outcome assertions for preview/commit/approval/error paths)

### Coverage Focus

- P0: AC2/AC3/AC4 — the PRD metric demands 100% rejection on *every* dimension; only pre-image and expiry/replay are exercised today.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Approved intent + helper to mutate target out-of-band | AC1, AC4 | `preview_budget_patch`, `update_task_amount` helpers |
| Policy-version bump between approve and commit | AC2 | Schema activation helper from STP-109 |
| Multi-entity transaction intent | AC4 | Extend `seed_intent_fixture` |

## Edge Cases and Failure Modes

- Stale rejection must itself be audited with intent lineage (expiry already is).
- Race: commit and out-of-band mutation interleaving — DST seed exploration, not just sequential tests.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC5.
2. Red tests for AC2 (policy drift) and AC3 (op-hash mismatch) — smallest gaps against the PRD metric.
3. Multi-entity atomic invalidation (AC4) at L6, then the DST race workload.
4. AC6 uses the SDK outcome matrix in `sdk/typescript/test/graphql-client.test.ts`; keep the `@covers` citation on the commit helper tests.

**Constraints**
- PRD approval-safety metric (100% stale rejection on pre-image, schema, policy, grant, operation hash); CONTRACT-002 stale/mismatch vocabulary.

**Done When**
- [x] All five staleness dimensions have passing, citing rejection tests
- [x] Token replay and expiry remain green

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
