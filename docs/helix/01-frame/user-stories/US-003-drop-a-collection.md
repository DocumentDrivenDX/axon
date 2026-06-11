---
ddx:
  id: US-003
---

# US-003: Drop a Collection

**Feature**: FEAT-001 — Collections
**Feature Requirements**: COL-06
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As a** developer managing application lifecycle
**I want** to remove a collection that is no longer needed
**So that** I can clean up unused data structures without losing the audit history

## Context

Applications retire data structures over time, but a destructive drop must not be casual and must not erase lineage. This story exercises COL-06: drop requires explicit confirmation, removes the collection and its entities, retains all audit records, and records the drop event itself with the entity count at drop time. Drop surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API).

## Walkthrough

1. Developer issues a drop-collection request via the CLI (CONTRACT-008) or HTTP API (CONTRACT-001).
2. System refuses the request unless it carries the explicit confirmation required by the surface contract.
3. Developer re-issues the request with explicit confirmation.
4. System removes the collection and its entities, records a drop event in the audit log including the entity count at drop time, and retains all prior audit records for the collection and its entities.

## Acceptance Criteria

- [ ] **US-003-AC1** — Given an existing collection, when the developer requests a drop without the explicit confirmation required by CONTRACT-008/CONTRACT-001, then the request is rejected and the collection is unchanged.
- [ ] **US-003-AC2** — Given an existing collection, when the developer requests a drop with explicit confirmation, then the collection and its entities are removed.
- [ ] **US-003-AC3** — Given a collection was just dropped, when the audit log is queried, then a drop event exists recording the actor and the entity count at the time of the drop.
- [ ] **US-003-AC4** — Given a collection was dropped, when the audit history of the dropped collection or its former entities is queried, then the pre-drop audit records are still retrievable.

## Edge Cases

- **Drop non-existent collection**: Returns a structured not-found error, not a crash.
- **Concurrent drop**: Two confirmed drops of the same collection — one succeeds; the other receives a not-found error.
- **Drop during active writes**: In-flight writes to the collection fail with a structured error once the drop commits; none partially apply afterward.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Unconfirmed drop | US-003-AC1 | `invoices` exists with 3 entities | Drop without confirmation | Rejected; collection and entities intact |
| Confirmed drop | US-003-AC2 | `invoices` exists with 3 entities | Drop with explicit confirmation | Collection and 3 entities removed |
| Drop audit event | US-003-AC3 | `invoices` just dropped (had 3 entities) | Query audit log | Drop event present with actor and entity count 3 |
| Audit retention | US-003-AC4 | `invoices` dropped; entities had prior mutations | Query audit history for a former entity | Pre-drop mutation records returned |

## Dependencies

- **Stories**: US-001 (a collection must exist to be dropped)
- **Feature Spec**: FEAT-001
- **Feature Requirements**: COL-06
- **PRD Requirements**: FR-1
- **External**: CONTRACT-001, CONTRACT-008; FEAT-003 (audit log retention semantics)

## Out of Scope

- Restoring a dropped collection from audit history (rollback/recovery is FEAT-023).
- Authorization for who may drop collections (FEAT-012/FEAT-029).
- Renaming collections (COL-05).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
