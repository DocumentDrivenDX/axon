---
ddx:
  id: STP-106
  review:
    self_hash: dc09017f7cb3d0ba47be3489da713e9e4f1046bd3c73f8136ef5d2d43df8f6b1
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# Story Test Plan: STP-106-route-risky-writes-for-approval

## Story Reference

**User Story**: [[US-106-route-risky-writes-for-approval]] (FEAT-030, P0)
**Technical Design**: [[TD-106-approval-routing]] — not yet authored; ADR-023 and CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (intent decision semantics → L6 contract)

## Scope and Objective

**Goal**: prove threshold-based approval envelopes route writes correctly (`allow` below, `needs_approval` at/above), approvals/rejections require the configured role, and every approval event is audited.
**Blocking Gate**: `cargo test -p axon-server --test graphql_intents_contract`

**In Scope**
- Threshold routing, `needs_approval` payload, direct-write interception, GraphQL approve/reject, approval audit.

**Out of Scope**
- Staleness/expiry (STP-107), MCP envelopes (STP-108), inbox UI (STP-117).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-106-AC1 | Below threshold → `allow`; at/above → `needs_approval` | `under_threshold_allow_commit_and_replay_rejects`; `over_threshold_intent_can_be_approved_and_committed` | Both routing branches produce the expected decision | `@covers US-106-AC1` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-106-AC2 | `needs_approval` includes required approver role, reason requirement, intent ID | `over_threshold_intent_can_be_approved_and_committed`; `pending_intent_queries_return_pending_reviews` | Needs-approval payload carries route details | `@covers US-106-AC2` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-106-AC3 | Generated direct write hitting the envelope returns approval-required, mutates nothing, no mutation audit entry | `direct_write_intercepted_no_mutation_no_audit` | `patchTask` with `budget_cents > 10000` returns `forbidden`/`needs_approval`; entity state unchanged; no `entity.update` audit entry produced | `@covers US-106-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-106-AC4 | Approver with required role approves/rejects via GraphQL; approval state changes | `over_threshold_intent_can_be_approved_and_committed`; `approval_requires_current_approver_role`; `rejected_intent_cannot_commit`; "approveIntent sends approveMutationIntent mutation with intentId and reason @covers US-106-AC4"; "rejectIntent sends rejectMutationIntent mutation with intentId and reason @covers US-106-AC4" | Role-gated approve/reject transitions intent state; SDK client sends correct mutations | `@covers US-106-AC4` in test bodies + SDK | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs`, `sdk/typescript/test/graphql-client.test.ts` |
| US-106-AC5 | Approval/rejection audited with actor, reason, policy version, intent ID | lineage assertions in `graphql_intents_contract.rs` (approval entries carry `policy_version`, intent lineage) | Audit rows exist for approval/rejection with binding metadata | `@covers US-106-AC5` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_intents_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_intents_contract.rs` (extend: AC3 direct-write interception with pre/post state + audit-count assertions)

### Coverage Focus

- P0: AC3 — bypassing preview must not bypass the envelope.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Approval envelope with numeric threshold (e.g. $10,000) | All ACs | `seed_intent_fixture` |
| Approver and non-approver subjects | AC4 | `update_approval_role` helper |

## Edge Cases and Failure Modes

- Approver losing the role between listing and approving (covered by `approval_rechecks_role_after_preview`).
- Threshold exactly at the boundary value routes to `needs_approval` (at-or-above semantics).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2/AC4/AC5 (verify actor+reason audit assertion while citing).
2. Red test for AC3.

**Constraints**
- CONTRACT-002 approval surface; CONTRACT-005 audit record shape.

**Done When**
- [x] AC1–AC5 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
