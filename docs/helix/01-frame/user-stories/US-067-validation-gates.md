---
ddx:
  id: US-067
---

# US-067: Validation Gates

**Feature**: FEAT-019 — Validation Rules and Actionable Errors
**Feature Requirements**: VAL-04, VAL-05, VAL-06, VAL-07, VAL-08, VAL-09, VAL-11, VAL-14
**PRD Requirements**: FR-1
**Priority**: P1
**Status**: Draft

## Story

**As an** agent application developer (Ava) defining progressive validation
**I want** to group rules into named gates (save, complete, review)
**So that** agents can save entities early and validate them incrementally as
they mature

## Context

Agents create entities with minimal fields and fill them in over time. Hard
validation that blocks saves on incomplete data is hostile to that workflow,
but agents still need to know what is missing before an entity can proceed.
This story exercises gate semantics (VAL-04..VAL-07), lifecycle integration
(VAL-08), evaluation on every write path (VAL-09), gate materialization
(VAL-11), and gate status in responses (VAL-14). Gate grammar and semantics
are normative in CONTRACT-010.

## Walkthrough

1. Ava declares `save`, `complete`, and `review` gates with rules, where
   `review` includes `complete`, and a lifecycle transition to `ready`
   requires the `complete` gate.
2. Ava's agent saves a draft entity missing an assignee; the save succeeds
   because only `complete`-gate rules fail.
3. The write response reports `complete: fail` with the failing rule, message,
   and fix; the agent knows exactly what is still needed.
4. The agent attempts the transition to `ready`; Axon blocks it, naming the
   `complete` gate and its failing rules.
5. The agent sets the assignee and retries; the gate passes and the
   transition succeeds.

## Acceptance Criteria

- [ ] **US-067-AC1** — Given a rule with `gate: save`, when an entity
  violating it is written, then the entity is not persisted and no mutation
  audit entry is produced.
- [ ] **US-067-AC2** — Given a rule with `gate: complete`, when an entity
  violating it is written, then the entity is persisted and the gate failure
  is reported in the write response.
- [ ] **US-067-AC3** — Given a rule with `advisory: true`, when an entity
  violating it is written, then the save succeeds and the advisory is
  reported without ever blocking.
- [ ] **US-067-AC4** — Given declared non-save gates, when any entity is
  written, then the write response includes pass/fail status for each of
  those gates.
- [ ] **US-067-AC5** — Given a failing gate, when the write response is
  returned, then it includes the failure details (rule name, field, message,
  fix) for each failing rule.
- [ ] **US-067-AC6** — Given a write that persists, when gate evaluation
  completes, then per-entity gate status is materialized and immediately
  queryable.
- [ ] **US-067-AC7** — Given a schema with validation rules is saved, then
  the declared gates are registered so the system knows which gates exist for
  the collection.
- [ ] **US-067-AC8** — Given `review` declares `includes: [complete]`, when
  an entity fails a `complete` rule, then it also fails the `review` gate.
- [ ] **US-067-AC9** — Given a lifecycle transition declaring
  `requires_gate: complete`, when the entity fails the `complete` gate, then
  the transition is blocked.
- [ ] **US-067-AC10** — Given a blocked transition, when the error is
  returned, then it includes the gate name, the failing rules, and their fix
  suggestions.

## Edge Cases

- **Entity passes all gates from the start**: response reports all gates as
  passing; transitions guarded by those gates proceed without extra steps.
- **Gate rules change on schema save**: existing entities' gate status is
  recomputed in the background (VAL-15); until convergence, status reflects
  the most recent evaluation.
- **Advisory-only collection**: saves never block; advisories are still
  reported and queryable.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Save gate blocks | US-067-AC1 | Rule `gate: save, require bead_type in [task, bug]` | Create `{bead_type: "banana"}` | Rejected; no entity, no mutation audit entry |
| Custom gate allows | US-067-AC2 | Rule `gate: complete, require assignee not_null` | Create draft without assignee | Entity persisted; response shows `complete: fail` with rule details |
| Advisory never blocks | US-067-AC3 | Rule `advisory: true, require title not_match "^TODO"` | Create `{title: "TODO"}` | Saved; advisory reported |
| Gate inclusion | US-067-AC8 | `review includes [complete]`; entity fails a complete rule | Read gate status | Both `complete` and `review` report fail |
| Guarded transition | US-067-AC9/AC10 | Transition `pending → ready requires_gate: complete`; entity fails gate | Attempt transition | Blocked; error names gate, failing rules, fixes |

## Dependencies

- **Stories**: US-066 (rule declaration semantics).
- **Feature Spec**: FEAT-019
- **Feature Requirements**: VAL-04, VAL-05, VAL-06, VAL-07, VAL-08, VAL-09, VAL-11, VAL-14
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010 (gate grammar, gate semantics, response shape); ADR-008 (lifecycles); ADR-010 (gate status storage)

## Out of Scope

- Querying entities by gate status across read surfaces (US-074b).
- Error message enhancement for JSON Schema violations (US-068).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
