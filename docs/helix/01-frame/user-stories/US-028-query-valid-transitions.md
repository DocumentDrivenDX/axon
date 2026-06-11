---
ddx:
  id: US-028
---
# US-028: Query Valid Transitions

**Feature**: FEAT-010 — Entity State Machines and Transition Guards
**Feature Requirements**: LCM-09, LCM-10
**PRD Requirements**: FR-10
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer building agents and UIs over governed entities
**I want** to know which state transitions are valid for an entity in its current state
**So that** my agent can present valid options or choose its next action without trial-and-error invalid API calls

## Context

Extracted from FEAT-010. Exercises per-entity transition introspection
(LCM-10) and the collection-level state machine introspection it builds on
(LCM-09). The normative endpoint shapes are owned by CONTRACT-001.

## Walkthrough

1. An agent reads an entity and requests its valid transitions through the introspection API.
2. Axon returns each candidate transition from the entity's current state with the guard status (pass/fail) and a reason for each failing guard.
3. The agent picks a passing transition and applies it, or surfaces the failing-guard reasons to a human.

## Acceptance Criteria

- [ ] **US-028-AC1** — Given an entity in a collection with a declared state machine, when its transitions are queried through the introspection API (endpoint per CONTRACT-001), then the response lists the valid next states from the entity's current state.
- [ ] **US-028-AC2** — Given candidate transitions with guards, when transitions are queried, then each transition reports whether its guard currently passes or fails, with a reason for each failure.
- [ ] **US-028-AC3** — Given an agent consuming the response, when the response is parsed, then it is structured (machine-readable) such that the agent can select a valid transition without further calls.
- [ ] **US-028-AC4** — Given a transition with multiple guards failing simultaneously, when transitions are queried, then all failing guards are reported, each with its specific reason.

## Edge Cases

- **Entity in a terminal state**: the response contains an empty transition list, not an error.
- **No state machine on the collection**: the introspection request reports that no lifecycle is declared rather than fabricating transitions.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| List transitions | US-028-AC1 | Invoice in `submitted` | Query transitions | `approved` (guarded) and other declared next states listed |
| Guard status | US-028-AC2 | `approver_id` null | Query transitions | `approved` reported with guard=fail and reason naming `approver_id` |
| Machine-readable | US-028-AC3 | Any entity with lifecycle | Query transitions | Structured response; agent selects a passing transition |
| Multiple failures | US-028-AC4 | Transition with two unmet guards | Query transitions | Both guards reported with specific reasons |
| Terminal state | edge | Entity in terminal state | Query transitions | Empty transition list |

## Dependencies

- **Stories**: US-026, US-027 (transition and guard semantics being introspected).
- **Feature Spec**: [FEAT-010 — Entity State Machines and Transition Guards](../features/FEAT-010-entity-state-machines.md)
- **Feature Requirements**: LCM-09, LCM-10
- **PRD Requirements**: FR-10
- **External**: CONTRACT-001 (HTTP lifecycle/introspection endpoints)

## Out of Scope

- Policy-based allow/deny on transitions (FEAT-012/FEAT-029); this story covers lifecycle validity only.

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
