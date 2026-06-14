---
ddx:
  id: STP-115
  review:
    self_hash: 043cc223901bec44c4122288d411f1b49ce57c9f7b9d8c8e4035b743de3f0db3
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# Story Test Plan: STP-115-browse-entities-with-policy-safe-ui-semantics

## Story Reference

**User Story**: [[US-115-browse-entities-with-policy-safe-ui-semantics]] (FEAT-031, P0)
**Technical Design**: [[TD-115-policy-safe-browsing]] — not yet authored; FEAT-031 spec + CONTRACT-002/004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove entity browsing for a policy-limited viewer matches policy-filtered GraphQL results, renders redaction as explicit state with no DOM leakage, and shows denied writes without optimistic updates.
**Blocking Gate**: `bun run test:e2e` filtered to `policy-enforcement.spec.ts`

**In Scope**
- List/detail/relationship/audit rendering under row filtering and field redaction; denied-write error display.

**Out of Scope**
- Backend filtering semantics (STP-101, STP-102), intent preview flows (STP-116).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-115-AC1 | Lists, traversal, cursors, totals match policy-filtered GraphQL | "contractor list surfaces policy-filtered totalCount and policy version @covers US-115-AC1"; "contractor links tab surfaces backend-filtered totalCount and group totals @covers US-115-AC1"; "point-read of hidden entity renders not-found without existence leakage @covers US-115-AC1"; "no-visible-rows empty state shows policy version with no hidden counts @covers US-115-AC1" | Rendered counts/rows equal backend-filtered results; hidden rows absent | `@covers US-115-AC1` | COVERED | L7 E2E | `ui/tests/e2e/policy-enforcement.spec.ts` |
| US-115-AC2 | Redaction shown as explicit state; original value absent from DOM | "contractor sees redacted commercial_terms across list, detail, and audit views @US-115 @covers US-115-AC2 @covers US-115-AC4"; "contractor links tab inline preview redacts target invoice fields @covers US-115-AC2" | Redacted state rendered; value not in DOM | `@covers US-115-AC2` | COVERED | L7 E2E | `ui/tests/e2e/policy-enforcement.spec.ts` |
| US-115-AC3 | Denied write shows error code, field path, policy explanation; no optimistic update | "denied delete surfaces stable code, reason, and policy explanation @covers US-115-AC3"; "real backend denies contractor delete with stable code, reason, and policy @covers US-115-AC3" | Denial rendered with stable code/reason; state unchanged | `@covers US-115-AC3` | COVERED | L7 E2E | `ui/tests/e2e/policy-enforcement.spec.ts` |
| US-115-AC4 | Audit views apply the same redaction rules as entity reads | "contractor sees redacted commercial_terms across list, detail, and audit views @US-115 @covers US-115-AC2 @covers US-115-AC4" (audit leg) | Audit view redacts the same fields | `@covers US-115-AC4` | COVERED | L7 E2E | `ui/tests/e2e/policy-enforcement.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
```

### Planned Test Files

- `ui/tests/e2e/policy-enforcement.spec.ts` (exists — citation-only pass)

### Coverage Focus

- P0: AC2 DOM-leak prevention; AC1 count parity (UI must not recompute counts).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Contractor viewer + invoices with hidden rows and redacted commercial fields | All ACs | Seeded nexiq-style fixtures via E2E helpers |
| Live-mutation hooks (subscription inserts) | AC1 hardening | Existing "live insertion…" cases in the spec |

## Edge Cases and Failure Modes

- Live insertion of hidden rows must never flash into the viewer's list (asserted).
- Forbidden audit/links loads collapse to a uniform error string — no leak via differentiated errors (asserted).

## Build Handoff

**Implementation Order**
1. Citation-only pass: map each AC to its existing Playwright titles.

**Constraints**
- UI renders backend decisions; any client-side filtering is a defect, not coverage.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
