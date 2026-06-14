---
ddx:
  id: US-122
  review:
    self_hash: 58eb484378a0cc19167c0e2626c6fd089f39764252654b01ecdbad0f32f55559
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-122: Inspect Audit and Recover Entity State

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-19, UI-20, UI-21, UI-22, UI-23
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As an** operator debugging agent behavior (Wei, Business Workflow Builder persona)
**I want** audit history and rollback tools in the entity UI
**So that** I can trace and recover unintended changes

## Context

Renumbered from US-044 (collision with FEAT-012). When an agent writes
something wrong, the operator's job is: find the change, understand the diff,
and put the entity back. This story exercises FEAT-011's audit-and-recovery
requirements (UI-19 through UI-23) end to end.

## Walkthrough

1. Operator opens the database Audit Log from the database sub-navigation.
2. System lists recent entries; the operator filters by collection and opens
   an entry's detail with before/after data and diff.
3. Operator navigates to the affected entity and opens its History tab,
   seeing operation, version, actor, timestamp, and data preview per entry.
4. Operator opens the Rollback tab, which lists prior versions from audit
   history.
5. Operator previews a rollback; the system shows a dry-run diff without
   mutating anything.
6. Operator applies the rollback; the system restores the entity to the
   selected prior version.

## Acceptance Criteria

- [ ] **US-122-AC1** — Given a database workspace, when the operator opens
  the Audit Log from sub-navigation, then recent audit entries are listed.
- [ ] **US-122-AC2** — Given the Audit Log, when the operator filters by
  collection and opens an entry, then the entry detail shows before/after
  data and a diff.
- [ ] **US-122-AC3** — Given an audit update entry, when the operator reverts
  it, then the entity's prior data is restored.
- [ ] **US-122-AC4** — Given an entity with history, when the operator opens
  its History tab, then operation, version, actor, timestamp, and data
  preview are shown per entry.
- [ ] **US-122-AC5** — Given an entity's Rollback tab, when the operator
  selects a prior version and previews, then a dry-run diff is shown with no
  mutation applied.
- [ ] **US-122-AC6** — Given a previewed rollback, when the operator applies
  it, then the entity is mutated to the selected prior version.

## Edge Cases

- **Rollback target conflicts with current state**: the dry-run diff reflects
  the current version; applying against a since-changed entity surfaces the
  structured conflict instead of silently overwriting.
- **Audit entry for a deleted entity**: entry detail still renders before
  data and the operator can recover via the documented recovery flow.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Audit reachability | US-122-AC1 | Database with mutations | Open Audit Log | Entries listed |
| Filter and detail | US-122-AC2 | Entries across 2 collections | Filter one; open entry | Only that collection; detail shows diff |
| Revert update | US-122-AC3 | Update entry for `e1` | Revert | `e1` data restored to prior state |
| History tab | US-122-AC4 | `e1` with 3 versions | Open History | 3 rows with operation/version/actor/time |
| Dry-run preview | US-122-AC5 | Rollback tab on `e1` | Preview version 1 | Diff shown; entity unchanged |
| Apply rollback | US-122-AC6 | Previewed version 1 | Apply | `e1` matches version 1 data |

## Dependencies

- **Stories**: US-042 (entity browsing)
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-19 through UI-23
- **PRD Requirements**: FR-24
- **External**: CONTRACT-001 (rollback and audit endpoints), CONTRACT-005
  (audit record shape)

## Out of Scope

- Transaction-level and point-in-time rollback UI (FEAT-023 scope).
- Policy-filtered audit redaction (FEAT-029/FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
