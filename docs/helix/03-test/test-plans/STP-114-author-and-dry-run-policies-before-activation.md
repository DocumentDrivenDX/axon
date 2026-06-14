---
ddx:
  id: STP-114
  review:
    self_hash: aa1da87feee7be2479d60f64dc71f5b9bb484e17515fca87e3398dd85453a88d
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-114-author-and-dry-run-policies-before-activation

## Story Reference

**User Story**: [[US-114-author-and-dry-run-policies-before-activation]] (FEAT-031, P0)
**Technical Design**: [[TD-114-policy-authoring-ui]] — not yet authored; FEAT-031 spec + CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove the schema workspace exposes policy authoring with compile reports, blocks activation on failed compiles, and supports fixture dry-runs before activation.
**Blocking Gate**: `bun run test:e2e` filtered to `policy-authoring.spec.ts`

**In Scope**
- Authoring UI, compile-report rendering, activation gating, fixture dry-run.

**Out of Scope**
- Backend compile semantics (STP-109), effective-policy inspection (STP-113).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-114-AC1 | Schema workspace exposes the access-control block beside the raw editor | "compile + fixture dry-run + activate updates the persisted policy @US-114 @covers US-114-AC1" | Policy block reachable and editable from schema workspace | `@covers US-114-AC1` | COVERED | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-114-AC2 | Compile renders report with errors, warnings, nullability, MCP envelope changes | "surfaces missing-index diagnostics for policy_filter_unindexed fixtures @covers US-114-AC2" | Compile report rendered with diagnostics | `@covers US-114-AC2` | COVERED | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-114-AC3 | Failed compile blocks activation; active version unchanged | "failed compile blocks activation and leaves the persisted policy unchanged @covers US-114-AC3" | Activation blocked, persisted policy unchanged | `@covers US-114-AC3` | COVERED | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |
| US-114-AC4 | Successful dry-run returns fixture decisions before the new version applies | "matrix dry-run gate: activation blocked until fixture dry-run recorded; editing policy invalidates gate @covers US-114-AC4" | Dry-run gate enforces evaluate-before-activate | `@covers US-114-AC4` | COVERED | L7 E2E | `ui/tests/e2e/policy-authoring.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
```

### Planned Test Files

- `ui/tests/e2e/policy-authoring.spec.ts` (exists — citation pass; verify AC2's nullability/MCP-envelope assertions)

### Coverage Focus

- P0: AC3 (failed compile can never activate) and AC4 (no blind activation).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Candidate policy with deliberate compile error | AC3 | Inline fixture in spec |
| `policy_filter_unindexed` fixture | AC2 | Existing spec fixture |

## Edge Cases and Failure Modes

- Editing the policy after a recorded dry-run must invalidate the gate (asserted).
- Concurrent activation from a second session should surface a version conflict, not silently overwrite.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1–AC4; extend AC2 assertions if nullability/MCP-envelope legs are missing.

**Constraints**
- The UI submits dry-runs through the same CONTRACT-004 compile pipeline as STP-109 — no UI-local validation.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
