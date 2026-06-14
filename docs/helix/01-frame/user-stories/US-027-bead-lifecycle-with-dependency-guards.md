---
ddx:
  id: US-027
  review:
    self_hash: 71ee69088385439537f1e566bfed3bb000942eaab03e0416a8ce34f187ac7e96
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---
# US-027: Bead Lifecycle with Dependency Guards

**Feature**: FEAT-010 — Entity State Machines and Transition Guards
**Feature Requirements**: LCM-05, LCM-06, LCM-09
**PRD Requirements**: FR-10
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer running an agentic framework on Axon
**I want** beads to enforce lifecycle transitions with dependency-aware guards
**So that** no bead starts execution until all of its blocking dependencies are complete

## Context

Extracted from FEAT-010. Exercises link-based guard conditions (LCM-05),
guard failure reporting (LCM-06), and state machine introspection for a
collection (LCM-09), using the FEAT-006 bead lifecycle (DDx vocabulary:
proposed, open, in_progress, blocked, closed, cancelled) with a guard that a
bead may only leave `open` for `in_progress` when every `blocks` dependency
is `closed`.

## Walkthrough

1. Ava's bead collection declares a guard on `open` → `in_progress`: all `blocks` link targets must be `closed`.
2. An agent attempts to claim a bead whose dependency is still `open`; Axon rejects the transition, naming the failing guard and the offending bead.
3. The dependency closes; the agent retries and the transition succeeds.
4. The agent fetches the collection's state machine definition to plan its queue behavior.

## Acceptance Criteria

- [ ] **US-027-AC1** — Given a bead in `open` with a `blocks` dependency whose status is not `closed`, when a transition to `in_progress` is attempted, then the transition is rejected.
- [ ] **US-027-AC2** — Given that rejection, when the error is returned, then it identifies the specific guard condition and the failing entity/field (the unmet dependency).
- [ ] **US-027-AC3** — Given all `blocks` dependencies are `closed`, when the transition to `in_progress` is attempted, then it succeeds.
- [ ] **US-027-AC4** — Given a collection with a declared state machine, when the state machine definition is requested through the API, then the full definition (states, transitions, guards) is returned (endpoint per CONTRACT-001).
- [ ] **US-027-AC5** — Given a link-based guard, when it is evaluated, then evaluation uses the same traversal semantics as the FEAT-009 query primitive (identical visibility and link-type matching).

## Edge Cases

- **Dependency target deleted**: the guard fails with a link-target-not-found reason; the bead cannot transition until the dependency reference is resolved.
- **Mixed terminal states**: a dependency in `cancelled` does not satisfy a guard requiring `closed`.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Blocked claim | US-027-AC1 | Bead B `open`, blocks-dep A `open` | Transition B to `in_progress` | Rejected |
| Guard failure detail | US-027-AC2 | Same as above | Inspect rejection | Error names the dependency guard and bead A |
| Unblocked claim | US-027-AC3 | Dep A `closed` | Transition B to `in_progress` | Succeeds |
| Introspect machine | US-027-AC4 | Bead collection with lifecycle | Request state machine definition | Definition with states, transitions, guards returned |
| Deleted target | edge | Dep A deleted | Transition B to `in_progress` | Guard fails with link-target-not-found reason |

## Dependencies

- **Stories**: US-015/US-016 (FEAT-006 bead model), US-028 (per-entity transition introspection).
- **Feature Spec**: [FEAT-010 — Entity State Machines and Transition Guards](../features/FEAT-010-entity-state-machines.md)
- **Feature Requirements**: LCM-05, LCM-06, LCM-09
- **PRD Requirements**: FR-10
- **External**: CONTRACT-001 (HTTP lifecycle endpoints), CONTRACT-010 (lifecycle declaration grammar)

## Out of Scope

- The bead schema and ready-queue semantics themselves (FEAT-006).
- Graph traversal query surface (FEAT-009).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
