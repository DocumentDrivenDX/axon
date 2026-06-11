---
ddx:
  id: STP-113
---

# Story Test Plan: STP-113-inspect-effective-policy-in-the-web-ui

## Story Reference

**User Story**: [[US-113-inspect-effective-policy-in-the-web-ui]] (FEAT-031, P0)
**Technical Design**: [[TD-113-policy-workspace-ui]] — not yet authored; FEAT-031 spec + CONTRACT-002 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove the policy workspace renders effective policy, impact matrix, and explanations for all eight operations, matching GraphQL console results.
**Blocking Gate**: `bun run test:e2e` (from `ui/`) filtered to `policy-authoring.spec.ts` and `graphql-policy-console.spec.ts`

**In Scope**
- Policy workspace inspect/explain panels; GraphQL console parity.

**Out of Scope**
- Authoring/dry-run/activation flow ([[STP-114]]), backend explain semantics ([[STP-104]]).

## Acceptance Criteria Test Mapping

Playwright titles carry story-level `@US-113` tags but no AC-level `@covers` citations yet.

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-113-AC1 | Selecting subject + collection renders effective-policy results | "renders subject × operation × fixture-row outcomes for the active policy" | Workspace renders per-selection outcomes | missing — add `@covers US-113-AC1` to title | UNCITED_COVERAGE | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-113-AC2 | Impact matrix covers five CRUD ops per subject × entity cell | "surfaces active-vs-proposed deltas across read\|create\|update\|patch\|delete fixture rows"; "impact matrix renders active-vs-proposed delta for transaction-row cells" | Matrix cells render all five operations | missing — add `@covers US-113-AC2` | UNCITED_COVERAGE | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-113-AC3 | Explain panel supports all eight ops (CRUD + transition, rollback, transaction) | "runs read, patch, and transaction policy evaluations from the workspace @US-113 @US-114" | Read/patch/transaction evaluations run — transition and rollback legs need explicit assertions before full coverage | missing — add `@covers US-113-AC3` | UNCITED_COVERAGE (partial: 3 of 8 ops asserted) | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-113-AC4 | Explanations include rule IDs, decision, reason code, field paths, policy version, approver role | "policy-version testid increments after policy activation" + explain assertions in the same spec | Policy version and outcome fields rendered — verify rule-ID/field-path assertions while citing | missing — add `@covers US-113-AC4` | UNCITED_COVERAGE | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-113-AC5 | Transaction fixture explained per-step with state threaded forward + aggregate decision | "renders structured transaction fixture editor and updates the evaluator" | Transaction fixture editor drives per-step evaluation | missing — add `@covers US-113-AC5` | UNCITED_COVERAGE | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-113-AC6 | GraphQL console reproduces the same effective-policy/explain results | "opens an effectivePolicy preset from the policy workspace @US-113"; "opens an explainPolicy preset…" | Console presets mirror workspace results | missing — add `@covers US-113-AC6` | UNCITED_COVERAGE | L7 E2E | `ui/tests/e2e/graphql-policy-console.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e   # scripts/test-ui-e2e-docker.sh, postgres storage
```

### Planned Test Files

- `ui/tests/e2e/policy-authoring.spec.ts`, `ui/tests/e2e/graphql-policy-console.spec.ts` (exist — need `@covers` citations; AC3 needs transition + rollback legs)

### Coverage Focus

- P0: AC3 full eight-operation coverage; AC6 console parity.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Seeded tenant/database with reference policy set | All ACs | E2E fixtures via `scripts/test-ui-e2e-docker.sh` + `ui/tests/e2e/helpers.ts` |
| Transaction fixture JSON | AC5 | Inline in policy-authoring spec |

## Edge Cases and Failure Modes

- Explanation for a subject with no grants must render without leaking hidden structure.
- Policy activation during an open workspace session must refresh versions (covered by version-increment tests).

## Build Handoff

**Implementation Order**
1. Citation pass: add `@covers US-113-ACm` to the mapped Playwright titles.
2. Add transition and rollback explain legs to close AC3.

**Constraints**
- UI must render backend decisions verbatim — no client-side policy re-derivation.

**Done When**
- [ ] AC1–AC6 passing with citations; AC3 covers all eight operations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
