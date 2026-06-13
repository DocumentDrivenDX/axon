---
ddx:
  id: STP-118
---

# Story Test Plan: STP-118-handle-stale-and-mismatched-intents-safely

## Story Reference

**User Story**: [[US-118-handle-stale-and-mismatched-intents-safely]] (FEAT-031, P0)
**Technical Design**: [[TD-118-stale-intent-ui]] — not yet authored; FEAT-031 spec + CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E; staleness invariants live in [[STP-107]])

## Scope and Objective

**Goal**: prove the UI renders stale/mismatch/expired intent states safely — commit disabled, specific codes shown, re-preview preserving lineage.
**Blocking Gate**: `bun run test:e2e` filtered to `mutation-intents.spec.ts` and `approval-inbox.spec.ts`

**In Scope**
- Stale-state rendering, mismatch error display, expired-intent gating, re-preview lineage.

**Out of Scope**
- Backend rejection semantics per dimension ([[STP-107]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-118-AC1 | Entity version change after preview → detail shows stale, disables commit, offers re-preview | "renders stale pre-image conflict details after preview drift @US-118 @covers US-118-AC1"; "editing a previewed field invalidates the preview and disables commit until re-preview @covers US-118-AC1" | Stale state rendered, commit disabled, re-preview offered | `@covers US-118-AC1` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |
| US-118-AC2 | Policy/schema/grant/op-hash change → specific stale/mismatch code shown, no partial commit | "renders mismatch GraphQL error payloads returned during commit @covers US-118-AC2"; "intent detail page shows schema-version, policy-version, and grant-version indicators @covers US-116-AC1 @covers US-118-AC2"; "intent detail policy-version increments after policy activation @covers US-118-AC2" | Mismatch codes rendered per dimension indicator | `@covers US-118-AC2` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |
| US-118-AC3 | Expired intent visible in history but not approvable/committable | "shows disabled action states for rejected, expired, committed, and stale intents @US-118 @covers US-118-AC3" | Expired intents render with disabled actions | `@covers US-118-AC3` | COVERED | L7 E2E | `ui/tests/e2e/approval-inbox.spec.ts` |
| US-118-AC4 | Re-preview creates new intent ID and preserves lineage to the prior stale intent | "re-previews after a stale commit, creates a new intent ID, and preserves the lineage link @covers US-118-AC4" | New intent ID + lineage link asserted | `@covers US-118-AC4` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
```

### Planned Test Files

- `ui/tests/e2e/mutation-intents.spec.ts`, `ui/tests/e2e/approval-inbox.spec.ts`, `ui/tests/e2e/intent-audit-lineage.spec.ts` (exist — citation-only pass)

### Coverage Focus

- P0: AC2 — each mismatch dimension must surface its *specific* code, mirroring [[STP-107]]'s backend matrix.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Out-of-band mutation hook to drift a previewed entity | AC1, AC4 | Existing spec drift helpers |
| Policy-activation helper to bump versions mid-flow | AC2 | Shared with policy-authoring spec |
| Expired intent fixture | AC3 | Seeded with short TTL |

## Edge Cases and Failure Modes

- Stale MCP-originated intents render conflict outcomes too (`intent-audit-lineage.spec.ts` @US-118 case).
- A stale intent must never be committable through keyboard shortcuts/double-submit paths.

## Build Handoff

**Implementation Order**
1. Citation-only pass across AC1–AC4.
2. As [[STP-107]] adds backend dimensions (grant drift, op-hash), mirror UI assertions here.

**Constraints**
- UI must render the backend's stale dimension verbatim; no client-side staleness inference.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
