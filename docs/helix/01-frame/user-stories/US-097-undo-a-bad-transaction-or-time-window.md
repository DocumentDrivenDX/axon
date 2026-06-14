---
ddx:
  id: US-097
  review:
    self_hash: c0243cd5b82bb2449537898632313b12609999fc03137c03f4479bfd7cdc6126
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-097: Undo a Bad Transaction or Time Window

**Feature**: FEAT-023 — Rollback and Recovery
**Feature Requirements**: RBK-04, RBK-05, RBK-06, RBK-07, RBK-08, RBK-15, RBK-16
**PRD Requirements**: FR-19, FR-30
**Priority**: P1
**Status**: Draft

## Story

**As an** operator recovering from a bad automation run (Wei, business
workflow builder)
**I want** transaction-level and point-in-time rollback
**So that** I can recover a coherent set of related mutations atomically

## Context

A rogue automation rarely damages one entity; it damages a transaction's
worth of related records, or everything it touched for an hour. This story
exercises atomic transaction rollback (RBK-04, RBK-05), point-in-time
rollback (RBK-07), the reserved audit operations and original-mutation
references (RBK-06, RBK-08), governed commits (RBK-15), and stable audit
references in responses (RBK-16).

## Walkthrough

1. Wei identifies the bad automation's transaction ID from the audit log.
2. After a dry-run (US-095), Wei commits a transaction rollback; Axon
   reverts every entity and link the transaction touched in one atomic
   compensating transaction.
3. For the wider incident, Wei commits a point-in-time rollback of the
   collection to just before the automation started.
4. Both rollbacks appear in the audit log under the reserved rollback
   operations, referencing the original mutations they compensate, and the
   responses carry stable audit references Wei pastes into the incident
   report.

## Acceptance Criteria

- [ ] **US-097-AC1** — Given a transaction that touched multiple entities
  and links, when a transaction rollback commits, then all of its changes
  are reversed atomically in one compensating transaction.
- [ ] **US-097-AC2** — Given any compensating operation conflicts, when a
  transaction rollback commits, then the whole rollback fails as a unit and
  no partial reversal is applied.
- [ ] **US-097-AC3** — Given a collection (or database) and a timestamp,
  when a point-in-time rollback commits, then all mutations after that
  timestamp in that scope are reverted.
- [ ] **US-097-AC4** — Given committed rollbacks, when their audit entries
  are read, then point-in-time rollback uses the collection rollback
  operation and transaction rollback uses the transaction rollback
  operation reserved in CONTRACT-005.
- [ ] **US-097-AC5** — Given any committed rollback, when audit is queried,
  then the rollback entry references the original audit IDs it compensates.
- [ ] **US-097-AC6** — Given rollback dry-run and commit responses, when
  consumed by SDK, CLI, GraphQL, or operator UI clients, then they expose
  stable, machine-readable audit references.

## Edge Cases

- **Entity touched by a later unrelated transaction**: the compensating
  operation conflicts; per AC2 the transaction rollback fails as a unit and
  reports the conflicting entities.
- **Approval-routed rollback**: when policy returns `needs_approval` for
  the compensating transaction, commit follows the FEAT-030 intent flow
  (RBK-15); a stale intent requires a fresh dry-run.
- **Window includes already-reverted mutations**: compensations compose —
  reverting a revert re-applies the later state; references chain through
  each layer.
- **Cross-collection transaction, collection-scoped window**: point-in-time
  rollback scope is the selected collection/database; a dry-run shows any
  transaction whose effects are only partially inside the scope.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Atomic undo | US-097-AC1 | `tx-42` updated 3 entities, created 1 link | Commit transaction rollback | All 4 changes reversed in one transaction |
| Unit failure | US-097-AC2 | One of the 3 entities modified since | Commit transaction rollback | Whole rollback fails; nothing reversed; conflict reported |
| Window revert | US-097-AC3 | 14 mutations after 09:00 in `invoices` | Point-in-time rollback to 09:00 | All 14 reverted via inverse transaction |
| Reserved operations | US-097-AC4 | Both rollbacks committed | Read audit entries | Collection/transaction rollback operations per CONTRACT-005 |
| Original references | US-097-AC5 | Same | Query audit | Each rollback entry lists compensated original audit IDs |

## Dependencies

- **Stories**: US-095 (dry-run precedes commit), US-096 (entity-level
  semantics).
- **Feature Spec**: FEAT-023
- **Feature Requirements**: RBK-04, RBK-05, RBK-06, RBK-07, RBK-08, RBK-15, RBK-16
- **PRD Requirements**: FR-19, FR-30
- **External**: CONTRACT-005 (reserved rollback operations),
  CONTRACT-001/002/008/009 (surfaces); FEAT-008 (atomic transactions);
  FEAT-029/FEAT-030 (policy and approval on commit)

## Out of Scope

- Graph-wide arbitrary point-in-time rollback across all collections and
  databases (FEAT-023 Out of Scope; also excluded by FEAT-030).
- Physical backup/restore.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
