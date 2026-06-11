---
ddx:
  id: US-113
---

# US-113: Inspect Effective Policy In The Web UI

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-03, PUI-04, PUI-05, PUI-06, PUI-07
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As an** operator (Wei, Business Workflow Builder persona)
**I want** to inspect the effective policy for a subject, row, and operation
**So that** I can understand why Axon allows, denies, redacts, or routes a write for approval

## Context

Operators need to read the same policy decisions the engine produces, without
hand-writing GraphQL. The policy workspace renders effective-policy results
on two surfaces: an impact matrix for the five entity-CRUD operations and an
explain panel covering all eight operations with fixture context.

## Walkthrough

1. Operator opens the database policy workspace and selects a subject and
   collection.
2. System renders the effective policy: row visibility, redacted fields,
   denied fields, approval envelopes, and policy version.
3. Operator drills into the explain panel for a specific entity and
   operation, supplying fixture context where needed.
4. System returns the decision, rule IDs, reason code, field paths, and
   required approver role.

## Acceptance Criteria

- [ ] **US-113-AC1** — Given a database policy workspace, when the operator
  selects a subject and collection, then effective-policy results render for
  that selection (GraphQL fields per CONTRACT-002).
- [ ] **US-113-AC2** — Given the impact matrix, when it renders, then it
  covers the five entity-CRUD operations (read, create, update, patch,
  delete) per subject × entity cell.
- [ ] **US-113-AC3** — Given the explain panel, when the operator evaluates a
  selected entity, sample row, or JSON fixture, then all eight operations are
  supported — the five CRUD operations plus transition (with a from/to state
  pair), rollback (with a target version), and transaction (with an operation
  list).
- [ ] **US-113-AC4** — Given any explanation, when it renders, then it
  includes stable rule IDs, decision, reason code, field paths, policy
  version, and the required approver role when applicable.
- [ ] **US-113-AC5** — Given a transaction fixture, when it is explained,
  then per-step outcomes are shown with entity state threaded forward across
  steps alongside the aggregate decision.
- [ ] **US-113-AC6** — Given the GraphQL console, when the operator issues
  the same effective-policy and explanation operations, then the results
  match what the policy panel shows.

## Edge Cases

- **Subject with no application attributes**: the panel explains the
  fallback decision rather than erroring.
- **Hidden fixture entity**: explanations never confirm existence of entities
  the evaluating operator's chosen subject cannot see beyond what policy
  introspection allows.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Effective policy render | US-113-AC1 | Contractor subject, engagements | Select in workspace | Visibility, redactions, version shown |
| Matrix coverage | US-113-AC2 | 2 subjects × 2 entities | Open matrix | 5 CRUD ops per cell |
| Explain all ops | US-113-AC3 | Entity + transition fixture | Evaluate transition draft→active | Decision with rule ID |
| Explanation detail | US-113-AC4 | Denied update | Explain | Rule ID, reason code, field paths, version |
| Transaction threading | US-113-AC5 | create→update fixture chain | Explain transaction | Step 2 sees step 1's created state |
| Console parity | US-113-AC6 | Same selection | Run in GraphQL console | Same results as panel |

## Dependencies

- **Stories**: US-104 (backend explanation), US-109 (active policy)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-03 through PUI-07
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-002 (policy GraphQL fields), CONTRACT-004 (reason
  codes)

## Out of Scope

- Policy editing and dry-run activation (US-114).
- MCP envelope preview (US-119).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
