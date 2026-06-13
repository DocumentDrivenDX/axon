---
ddx:
  id: STP-107
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
- UI stale rendering ([[STP-118]]), approval routing ([[STP-106]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-107-AC1 | Entity version change before commit → stale outcome naming pre-image dimension | `generated_mcp_tools_report_stale_commit_conflict`; `axon_query_intent_commit_conflict_matches_graphql_error_extensions` (stale `{dimension expected actual path}` shape asserted in `graphql_intents_contract.rs`) | Commit fails with stale payload naming the drifted dimension | `@covers US-107-AC1` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/mcp_intents_contract.rs`, `graphql_intents_contract.rs` |
| US-107-AC2 | Policy version change before commit → stale outcome naming policy dimension | none (lineage records policy_version, but no test drifts the policy and asserts rejection) | n/a | planned `@covers US-107-AC2` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC3 | Commit with operation differing from bound operation hash → mismatch failure | none | n/a | planned `@covers US-107-AC3` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC4 | Multi-entity intent: one stale entity invalidates the whole intent, no partial commit | `graphql_preview_mutation_binds_versions_for_all_operation_shapes` proves binding; atomic-rejection-on-drift case itself absent | Bindings exist for all op shapes; partial-commit rejection unproven | planned `@covers US-107-AC4` | UNTESTED | L1 DST + L6 | planned in `crates/axon-server/tests/graphql_intents_contract.rs`; DST workload in `crates/axon-sim` |
| US-107-AC5 | Committed token reused → rejected; expired intent cannot commit or be approved | `under_threshold_allow_commit_and_replay_rejects`; `expired_intent_cannot_commit`; `pending_query_materializes_and_audits_expired_intent_lineage` | Replay rejected; expired intents rejected and audited | `@covers US-107-AC5` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-107-AC6 | SDK commit helper and GraphQL commit field match for success/stale/mismatch/authz outcomes | none (no SDK intent helper tests; MCP parity exists but AC names SDK per CONTRACT-009) | n/a | planned `@covers US-107-AC6` | UNTESTED | L6 parity | planned in `sdk/typescript/test/` once the SDK commit helper lands |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_intents_contract
cargo test -p axon-server --test mcp_intents_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_intents_contract.rs` (extend: policy-drift, op-hash mismatch, multi-entity atomic invalidation)
- `crates/axon-sim` workload: concurrent commit vs entity mutation race under BUGGIFY (AC4)
- `sdk/typescript/test/intents.test.ts` (planned, AC6)

### Coverage Focus

- P0: AC2/AC3/AC4 — the PRD metric demands 100% rejection on *every* dimension; only pre-image and expiry/replay are exercised today.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Approved intent + helper to mutate target out-of-band | AC1, AC4 | `preview_budget_patch`, `update_task_amount` helpers |
| Policy-version bump between approve and commit | AC2 | Schema activation helper from [[STP-109]] |
| Multi-entity transaction intent | AC4 | Extend `seed_intent_fixture` |

## Edge Cases and Failure Modes

- Stale rejection must itself be audited with intent lineage (expiry already is).
- Race: commit and out-of-band mutation interleaving — DST seed exploration, not just sequential tests.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC5.
2. Red tests for AC2 (policy drift) and AC3 (op-hash mismatch) — smallest gaps against the PRD metric.
3. Multi-entity atomic invalidation (AC4) at L6, then the DST race workload.
4. AC6 when the SDK helper exists; record a manual exception until then if the story must close.

**Constraints**
- PRD approval-safety metric (100% stale rejection on pre-image, schema, policy, grant, operation hash); CONTRACT-002 stale/mismatch vocabulary.

**Done When**
- [ ] All five staleness dimensions have passing, citing rejection tests
- [ ] Token replay and expiry remain green

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
