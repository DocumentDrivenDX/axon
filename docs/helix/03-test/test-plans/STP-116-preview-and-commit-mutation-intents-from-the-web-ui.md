---
ddx:
  id: STP-116
  review:
    self_hash: 5d44980d8fcab2d6de954fbcd79b987ef76b2311af8cffc121410125abd3b432
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# Story Test Plan: STP-116-preview-and-commit-mutation-intents-from-the-web-ui

## Story Reference

**User Story**: [[US-116-preview-and-commit-mutation-intents-from-the-web-ui]] (FEAT-031, P0)
**Technical Design**: [[TD-116-intent-preview-ui]] — not yet authored; FEAT-031 spec + CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove UI write flows preview before commit (diff, versions, decision, expiry, intent ID), commit allowed changes, render denials without tokens, and link successful commits to audit.
**Blocking Gate**: `bun run test:e2e` filtered to `mutation-intents.spec.ts`

**In Scope**
- Preview modal content, allowed commit, denied preview, post-commit audit linkage.

**Out of Scope**
- Approval inbox (STP-117), stale handling (STP-118), backend preview semantics (STP-105).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-116-AC1 | Submit shows preview: entities, diff, pre-image versions, decision, explanation, expiry, intent ID | "renders and commits an allowed mutation intent without showing the token @US-116 @covers US-116-AC1 @covers US-116-AC2"; "renders a needs-approval preview with approval route details @covers US-116-AC1"; "intent detail page shows schema-version, policy-version, and grant-version indicators @covers US-116-AC1 @covers US-118-AC2" | Preview modal renders diff + bindings + decision before commit | `@covers US-116-AC1` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |
| US-116-AC2 | Under-threshold allowed change commits via GraphQL without approval | "renders and commits an allowed mutation intent without showing the token @US-116 @covers US-116-AC1 @covers US-116-AC2" | Allowed commit completes from preview | `@covers US-116-AC2` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |
| US-116-AC3 | Denied preview shows reason; exposes no executable intent token | "renders a denied preview without an executable intent token @covers US-116-AC3"; "denied previews preserve the user draft input across modal close @covers US-116-AC3" | Denial reason rendered, token absent | `@covers US-116-AC3` | COVERED | L7 E2E | `ui/tests/e2e/mutation-intents.spec.ts` |
| US-116-AC4 | Successful commit links to resulting audit entry and updated entity | "deep link filters /audit by intent ID and pre-populates filter @US-116 @covers US-116-AC4" | Commit confirmation links into audit by intent ID | `@covers US-116-AC4` | COVERED | L7 E2E | `ui/tests/e2e/intent-audit-lineage.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
```

### Planned Test Files

- `ui/tests/e2e/mutation-intents.spec.ts`, `ui/tests/e2e/intent-audit-lineage.spec.ts` (exist — citation-only pass)

### Coverage Focus

- P0: AC3 (no token on denial) and AC2 (commit path integrity, single `commitMutationIntent` call — double-submit guard already asserted).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Write flow with approval-capable policy + thresholds | All ACs | Seeded E2E intent fixtures |
| Audit deep-link route | AC4 | `/audit` filter by intent ID |

## Edge Cases and Failure Modes

- Editing a previewed field must invalidate the preview and disable commit (asserted).
- Double-submit must emit exactly one commit call (asserted).

## Build Handoff

**Implementation Order**
1. Citation-only pass across AC1–AC4.

**Constraints**
- The intent token must never render in the DOM (asserted in the allowed-commit test).

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
