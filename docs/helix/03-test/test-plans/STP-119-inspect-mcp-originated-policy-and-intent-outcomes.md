---
ddx:
  id: STP-119
---

# Story Test Plan: STP-119-inspect-mcp-originated-policy-and-intent-outcomes

## Story Reference

**User Story**: [[US-119-inspect-mcp-originated-policy-and-intent-outcomes]] (FEAT-031, P0)
**Technical Design**: [[TD-119-mcp-intent-inspection-ui]] — not yet authored; FEAT-031 spec + CONTRACT-003 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (browser workflows → L7 E2E)

## Scope and Objective

**Goal**: prove operators can inspect MCP tool envelopes in the policy workspace and see agent identity, delegated authority, and structured outcomes for MCP-originated intents in inbox/audit views.
**Blocking Gate**: `bun run test:e2e` filtered to `mcp-envelope-preview.spec.ts` and `intent-audit-lineage.spec.ts`

**In Scope**
- MCP envelope preview, MCP-originated intent detail, reason-code parity with UI policy explanations.

**Out of Scope**
- MCP protocol semantics ([[STP-108]]), generic inbox flows ([[STP-117]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-119-AC1 | Workspace shows MCP tool envelope for selected subject/collection/operation | "mirrors explainPolicy outcomes for read, needs_approval, and denied flows @US-119 @covers US-119-AC1 @covers US-119-AC3" | Envelope preview matches explainPolicy outcomes | `@covers US-119-AC1` | COVERED | L7 E2E | `ui/tests/e2e/mcp-envelope-preview.spec.ts` |
| US-119-AC2 | MCP intent detail shows agent identity, delegated authority, credential/grant version, tool name, args summary, outcome | "shows delegated MCP intent metadata in the inbox and detail panels @US-119 @covers US-119-AC2"; "shows stdio command/config status and redacted env for an MCP-originated intent @covers US-119-AC2" | Delegated metadata and tool provenance rendered (env redacted) | `@covers US-119-AC2` | COVERED | L7 E2E | `ui/tests/e2e/intent-audit-lineage.spec.ts`, `ui/tests/e2e/mcp-envelope-preview.spec.ts` |
| US-119-AC3 | Denied MCP tool result and UI policy explanation use the same stable reason code | "mirrors explainPolicy outcomes for read, needs_approval, and denied flows @US-119 @covers US-119-AC1 @covers US-119-AC3"; "opens an axon.query bridge in the GraphQL console matching the envelope outcome @covers US-119-AC3" | Reason codes match between MCP result and UI explanation | `@covers US-119-AC3` | COVERED | L7 E2E | `ui/tests/e2e/mcp-envelope-preview.spec.ts` |
| US-119-AC4 | needs-approval/denied/conflict MCP outcomes visible in inbox and audit lineage | "shows conflict outcomes for stale MCP-originated intent commits @US-118 @covers US-119-AC4" | Each MCP outcome class surfaces in inbox/lineage views | `@covers US-119-AC4` | COVERED | L7 E2E | `ui/tests/e2e/intent-audit-lineage.spec.ts` |

## Executable Proof

### Primary Commands

```bash
cd ui && bun run test:e2e
```

### Planned Test Files

- `ui/tests/e2e/mcp-envelope-preview.spec.ts`, `ui/tests/e2e/intent-audit-lineage.spec.ts` (exist — citation-only pass)

### Coverage Focus

- P0: AC2 delegated-authority visibility (audit-grade agent attribution) and AC3 reason-code parity.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| MCP-originated intents (needs_approval, denied, conflict) | AC2, AC4 | Seeded via MCP tool calls in E2E setup |
| Human-originated intent for provenance contrast | AC2 | "hides the stdio provenance panel for human-originated intents" fixture |

## Edge Cases and Failure Modes

- Stdio provenance panel hidden for human-originated intents (asserted) — no false agent attribution.
- Redacted env vars in provenance must never render secret values.

## Build Handoff

**Implementation Order**
1. Citation-only pass across AC1–AC4.

**Constraints**
- Envelope semantics per CONTRACT-003; UI mirrors MCP outcomes, never re-derives them.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
