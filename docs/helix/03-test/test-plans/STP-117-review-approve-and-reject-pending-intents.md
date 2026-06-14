---
ddx:
  id: STP-117
  review:
    self_hash: f5790350929f7018dbc6ff42cca59d5a2075b55312fdd9ea03bab7610dcf3416
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-117-review-approve-and-reject-pending-intents

## Story Reference

**User Story**: [[US-117-review-approve-and-reject-pending-intents]] (FEAT-031, P0)
**Technical Design**: [[TD-117-approval-inbox-ui]] — not yet authored; FEAT-031 spec + CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove the approval inbox lists pending intents with full review context, supports role-gated approve/reject with reasons, records audit, and blocks self-approval under separation of duties.
**Blocking Gate**: `bun run test:e2e` filtered to `approval-inbox.spec.ts`

**In Scope**
- Inbox listing/filtering, intent detail review, approve/reject flows, separation-of-duties UI behavior.

**Out of Scope**
- Stale/expired action gating (STP-118), backend approval semantics (STP-106).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-117-AC1 | Inbox lists intents with status, requester, subject, collection, operation, reason, role, age, expiry, MCP origin | "lists scoped intents across review states and opens detail @US-117 @covers US-117-AC1"; "supports dense filters, keyboard selection, and inline review without leaving inbox @covers US-117-AC1" | Inbox rows render the review metadata and filter correctly | `@covers US-117-AC1` | COVERED | L7 E2E | `ui/tests/e2e/approval-inbox.spec.ts` |
| US-117-AC2 | Detail view shows canonical operation, diff, explanation, pre-images, version bindings, route, audit links | "shows schema_version, policy_version, and grant_version in inline detail panel @covers US-117-AC2" | Detail panel renders bindings and review context | `@covers US-117-AC2` | COVERED | L7 E2E | `ui/tests/e2e/approval-inbox.spec.ts` |
| US-117-AC3 | Approver with configured role approves with reason → approval audit entry, intent commit-eligible | "approves and rejects pending intents from the detail route @covers US-117-AC3 @covers US-117-AC4"; audit lineage: "shows preview and approval events for approved intent in chronological order @covers US-117-AC3" | Approval transitions state and produces audit lineage | `@covers US-117-AC3` | COVERED | L7 E2E | `ui/tests/e2e/approval-inbox.spec.ts`, `ui/tests/e2e/intent-audit-lineage.spec.ts` |
| US-117-AC4 | Rejection records actor, reason, policy version, intent ID; intent can never commit | "approves and rejects pending intents from the detail route @covers US-117-AC3 @covers US-117-AC4"; "shows rejection event for rejected intent in chronological order @covers US-117-AC4"; backend: `rejected_intent_cannot_commit` | Rejection recorded and terminal | `@covers US-117-AC4` | COVERED | L7 E2E + L6 | `ui/tests/e2e/approval-inbox.spec.ts`, `ui/tests/e2e/intent-audit-lineage.spec.ts` |
| US-117-AC5 | Separation of duties: requester cannot approve own intent; structured error surfaced | UI: "shows authorization failures without clearing the entered reason @covers US-117-AC5" | Self-approval blocked with structured error; UI preserves input | `@covers US-117-AC5` | COVERED | L7 E2E | `ui/tests/e2e/approval-inbox.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
cargo test -p axon-server --test graphql_intents_contract
```

### Planned Test Files

- Existing specs need a citation pass; verify the UI self-approval case specifically (AC5) renders the separation-of-duties error, not a generic failure.

### Coverage Focus

- P0: AC4 terminal rejection and AC5 separation of duties.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Pending intents in multiple review states | AC1 | Seeded E2E intent fixtures |
| Approver, non-approver, and requester identities | AC3–AC5 | Role fixtures via E2E helpers |

## Edge Cases and Failure Modes

- Loading/empty/error inbox states are distinguishable (asserted).
- Reason field must be required where policy demands it; failure must not clear entered text (asserted).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1–AC5; confirm the AC5 UI leg targets self-approval specifically.

**Constraints**
- Approve/reject must round-trip through GraphQL — no UI-side state transitions.

**Done When**
- [x] AC1–AC5 passing with citations across UI and backend legs

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
