---
ddx:
  id: US-016
  review:
    self_hash: d526a4e4e6fdfb4f2b0204a5b4c07938b6bc5d29e5a65df62cc3278fbb28a2e1
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---
# US-016: Track Bead Dependencies

**Feature**: FEAT-006 — Bead Storage Adapter
**Feature Requirements**: BED-06, BED-07, BED-08, BED-10
**PRD Requirements**: None directly (dogfooding extension; builds on FR-2)
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer whose agent manages a work queue
**I want** to declare and query bead dependencies
**So that** the agent can determine which beads are ready to work on without computing the dependency graph itself

## Context

Extracted from FEAT-006. Exercises typed bead dependencies (BED-07), cycle
rejection (BED-08), the derived ready predicate (BED-06), and the ready-queue
query (BED-10). "Ready" mirrors DDx semantics: status `open` with every
blocking dependency `closed` — it is a derived predicate, never a stored
status.

## Walkthrough

1. Ava creates beads A, B, and C, declaring that A blocks B and B blocks C.
2. The agent queries the ready queue; only A is ready.
3. The agent closes A; the ready queue now returns B.
4. Ava inspects C's dependency tree and sees the chain C → B → A with each bead's status.
5. An attempt to add a dependency from A to C is rejected as a cycle.

## Acceptance Criteria

- [ ] **US-016-AC1** — Given existing beads, when a bead is created or updated declaring dependencies on other bead ids, then the dependencies persist and are returned when the bead is read.
- [ ] **US-016-AC2** — Given a dependency chain A→B→C, when a dependency from A to C is added (closing the cycle A→B→C→A), then the operation is rejected with a cycle-detection error identifying the cycle, and the graph is unchanged.
- [ ] **US-016-AC3** — Given a bead with transitive dependencies, when its dependency tree is queried, then the full tree is returned with each dependency's current status.
- [ ] **US-016-AC4** — Given beads in mixed states, when the ready queue is queried, then it returns exactly the beads whose status is `open` and whose blocking dependencies are all `closed`.
- [ ] **US-016-AC5** — Given a bead with a dependency, when that dependency is removed, then the removal succeeds and is captured in the audit trail.

## Edge Cases

- **Dependency on a terminal bead**: depending on a `cancelled` bead never satisfies the ready predicate unless the lifecycle declaration counts `cancelled` as satisfying; by default only `closed` satisfies.
- **Self-dependency**: a bead declaring a dependency on itself is rejected as a one-node cycle.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Declare dependencies | US-016-AC1 | Beads A, B exist | Create C with dependency on B | C persisted; reading C shows dependency on B |
| Cycle rejected | US-016-AC2 | Chain A→B→C | Add dependency A→C | Cycle error naming A→B→C→A; graph unchanged |
| Dependency tree | US-016-AC3 | Chain C→B→A | Query dependency tree of C | Tree C→B→A with statuses |
| Ready queue | US-016-AC4 | A `closed`, B `open` (blocks: A), C `open` (blocks: B) | Query ready queue | Returns exactly B |
| Audited removal | US-016-AC5 | C depends on B | Remove the dependency | Removal succeeds; audit trail records the change |

## Dependencies

- **Stories**: US-015 (bead creation and lifecycle).
- **Feature Spec**: [FEAT-006 — Bead Storage Adapter](../features/FEAT-006-bead-storage-adapter.md)
- **Feature Requirements**: BED-06, BED-07, BED-08, BED-10
- **PRD Requirements**: None directly (dogfooding extension)
- **External**: CONTRACT-001 (HTTP routes), CONTRACT-008 (CLI command tree)

## Out of Scope

- Lifecycle transition validation (US-015).
- General-purpose graph traversal queries (FEAT-009).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
