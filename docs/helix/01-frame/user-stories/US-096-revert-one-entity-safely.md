---
ddx:
  id: US-096
  review:
    self_hash: 3b30b9e14970750cf4974bc5a4f33a7d1129e082c3d8236483f0e56ad32cf692
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-096: Revert One Entity Safely

**Feature**: FEAT-023 — Rollback and Recovery
**Feature Requirements**: RBK-01, RBK-02, RBK-03, RBK-12
**PRD Requirements**: FR-19, FR-30
**Priority**: P1
**Status**: Draft

## Story

**As a** developer repairing one corrupted entity (Ava, agent application
developer)
**I want** to restore that entity to a previous version from the audit log
**So that** recovery is precise, audited, and does not rewrite history

## Context

The most common repair is one entity damaged by one bad write. Entity-level
rollback restores a known-good version as a new governed write (RBK-01,
RBK-02), audited with the entity revert operation from CONTRACT-005's
taxonomy (RBK-03), and inspectable through diff/log/blame-style views
(RBK-12).

## Walkthrough

1. Ava locates the corrupted invoice and identifies version 5 as the last
   good state in its audit history.
2. Ava commits an entity rollback to version 5.
3. Axon applies the restore as a new write at the current version; the
   entity's version advances and history is intact.
4. The audit log shows a revert entry referencing the source audit entry,
   and the diff view shows exactly which fields were restored.

## Acceptance Criteria

- [ ] **US-096-AC1** — Given an entity with audit history, when a rollback
  to a prior version (by version number or audit entry ID) is committed,
  then the entity's current state equals the target version's state.
- [ ] **US-096-AC2** — Given a committed rollback, when history is
  inspected, then the rollback exists as a new mutation at an advanced
  version — old versions are not rewritten or deleted.
- [ ] **US-096-AC3** — Given a committed entity rollback, when its audit
  entry is read, then it uses the entity revert operation defined in
  CONTRACT-005 and references the audit entry it restored from.
- [ ] **US-096-AC4** — Given a committed rollback, when inspected through
  diff/log/blame-style audit views, then the reverted fields and source
  audit entry are visible.
- [ ] **US-096-AC5** — Given the entity was modified between dry-run and
  commit, when the rollback commits, then it fails with an OCC conflict
  reported clearly and current state is unchanged.
- [ ] **US-096-AC6** — Given a previously committed rollback, when that
  rollback is itself rolled back, then the later state is re-applied as
  another new write with its own audit entry.

## Edge Cases

- **Restored payload no longer valid**: if the target version violates the
  active schema, the rollback is rejected by normal write validation; the
  dry-run surfaces this first.
- **Target version is a delete**: compensating semantics follow FEAT-023
  edge-case rules — recreation/deletion both carry original audit
  references.
- **Approval-routed revert**: if policy classifies the revert as
  `needs_approval`, commit follows the FEAT-030 intent flow.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Restore v5 | US-096-AC1 | Invoice at v7; v5 known good | Commit rollback to v5 | State equals v5 content; version advances to v8 |
| Forward-only history | US-096-AC2 | Same | Inspect history | v5..v7 unchanged; v8 is the revert |
| Audit linkage | US-096-AC3 | Same | Read v8 audit entry | Entity revert operation per CONTRACT-005; references v5's audit entry |
| OCC conflict | US-096-AC5 | Another writer commits v8 first | Commit rollback expecting v7 | Conflict error; state remains v8 |
| Rollback of rollback | US-096-AC6 | v8 is a revert to v5 | Roll back v8 | v9 re-applies v7 state with its own audit entry |

## Dependencies

- **Stories**: US-095 (dry-run precedes commit).
- **Feature Spec**: FEAT-023
- **Feature Requirements**: RBK-01, RBK-02, RBK-03, RBK-12
- **PRD Requirements**: FR-19, FR-30
- **External**: CONTRACT-005 (operation taxonomy, audit references),
  CONTRACT-001/002/008/009 (surfaces); FEAT-003 (audit history),
  FEAT-008 (OCC writes)

## Out of Scope

- Multi-entity and time-window recovery (US-097).
- Schema version rollback (FEAT-017).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
