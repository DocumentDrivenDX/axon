---
ddx:
  id: US-008
  review:
    self_hash: d73daa6707deccc3e6ac5be140a9c07c46c90a7e570cb0253a4727d0b9f652fd
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-008: Revert an Entity to Previous State

**Feature**: FEAT-003 — Audit Log
**Feature Requirements**: AUD-11, AUD-12
**PRD Requirements**: FR-17
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer who discovered an agent made a bad change
**I want** to revert an entity to a previous state using its audit entry
**So that** I can undo agent mistakes without manual data surgery

## Context

Preventive guardrails sometimes fail; when they do, Ava needs a repair path
grounded in the audit log rather than hand-edited records. This story
exercises FEAT-003's entity-revert area (AUD-11, AUD-12): restoring a
recorded before state as an ordinary governed, audited mutation. FEAT-023
extends this primitive into transaction and point-in-time rollback.

## Walkthrough

1. Ava identifies the bad mutation's audit entry (see US-007).
2. She requests a revert of that entry (surface per CONTRACT-001/CONTRACT-008).
3. The system validates the entry's before state against the current schema.
4. The system writes the restored state as a new mutation, producing a new audit entry that references the revert operation.
5. Ava reads the entity and sees the pre-mistake state; the full history, including the mistake and the revert, remains queryable.

## Acceptance Criteria

- [ ] **US-008-AC1** — Given an audit entry for a bad update, when Ava requests a revert of that entry, then the entity's stored state equals the entry's before state.
- [ ] **US-008-AC2** — Given a completed revert, when Ava queries the entity's audit history, then a new revert-operation entry exists — the audit log never loses information.
- [ ] **US-008-AC3** — Given a revert request, when the restored state is checked, then it is validated against the current active schema before commit.
- [ ] **US-008-AC4** — Given the schema has evolved so the before state no longer validates, when Ava requests a revert, then the revert fails with a clear structured error identifying the validation failure.
- [ ] **US-008-AC5** — Given a schema-invalid before state, when Ava requests a revert with the explicit force option, then the revert applies and the response carries a warning.

## Edge Cases

- **Entity deleted since the audit entry**: revert recreates the entity if create-from-revert is valid per ADR-022 semantics, otherwise fails with a clear error.
- **Concurrent mutation during revert**: revert is an OCC write; a version conflict aborts it with current state (FEAT-004 semantics).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-008-AC1 | Entry #42: `status` changed `pending`→`cancelled` | Revert entry #42 | Entity `status` is `pending` again |
| Audit completeness | US-008-AC2 | Revert of entry #42 committed | Query audit for entity | New entry with revert operation present |
| Schema drift | US-008-AC4 | Schema now requires `amount`; before state lacks it | Revert | Structured validation error; entity unchanged |
| Forced revert | US-008-AC5 | Same as above, force option set | Revert with force | Revert applies; warning returned |

## Dependencies

- **Stories**: US-007 (locating the entry)
- **Feature Spec**: FEAT-003
- **Feature Requirements**: AUD-11, AUD-12
- **PRD Requirements**: FR-17
- **External**: CONTRACT-005 (entry shape, revert operation), CONTRACT-001 (revert endpoint), ADR-022 (create semantics for revert-recreate)

## Out of Scope

- Transaction-level and point-in-time rollback, dry-run repair plans (FEAT-023).
- Reverting multiple entities in one operation.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
