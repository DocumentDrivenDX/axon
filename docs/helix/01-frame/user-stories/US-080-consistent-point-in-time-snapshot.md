---
ddx:
  id: US-080
  review:
    self_hash: 07544292293e7592f9a5c18f2c94f83e6481e4c3585c64d3088b50355aee5b02
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-080: Consistent Point-in-Time Snapshot

**Feature**: FEAT-004 — Entity Operations
**Feature Requirements**: ENT-02, ENT-07, ENT-13
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer syncing Axon state into another system
**I want** a read-only, consistent point-in-time bulk export of a database
**So that** my export never interleaves halves of concurrent writes

## Context

> **Reconstructed story (2026-06-10).** This story ID is claimed by live code
> and test coverage tags but had no spec heading; its content is reconstructed
> from FEAT-004's consistency requirements (read-after-write consistency, the
> system-metadata envelope, stable listing) per the user-story ID registry.
> Treat the acceptance criteria as the governing statement going forward.

Paging through collections row-by-row while writers are active yields a
torn view. This story provides a single consistent cut: a bulk read whose
contents all reflect one point in time. The snapshot surface is normative in
CONTRACT-001 (snapshot endpoint).

## Walkthrough

1. Ava requests a snapshot of a database, optionally scoped to collections (surface per CONTRACT-001).
2. The system selects a consistent point-in-time view and exports every entity (with envelope) and link visible at that point.
3. Concurrent writers proceed unblocked; none of their mid-snapshot commits appear in the export.
4. Ava loads the export downstream, trusting that cross-collection invariants hold within it.

## Acceptance Criteria

- [ ] **US-080-AC1** — Given concurrent writes during the export, when Ava requests a snapshot, then every exported record reflects the same point-in-time cut — no record shows state from after the cut.
- [ ] **US-080-AC2** — Given a transaction (FEAT-008) committing two related entities around the cut, when the snapshot is taken, then the export contains either both post-transaction states or neither (no torn transaction).
- [ ] **US-080-AC3** — Given an in-progress snapshot, when writers continue, then their writes succeed — the snapshot is read-only and acquires no write locks.
- [ ] **US-080-AC4** — Given a snapshot scoped to named collections, when exported, then only those collections' entities are included, each carrying its full system-metadata envelope.

## Edge Cases

- **Empty database**: snapshot succeeds with an empty export, not an error.
- **Collection dropped mid-snapshot**: the snapshot reflects the cut — the collection's pre-drop contents appear if the cut precedes the drop.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Consistent cut | US-080-AC1 | Writer updating `tasks` continuously | Snapshot during writes | All rows ≤ cut versions; no post-cut state |
| Transaction atomicity | US-080-AC2 | Txn updates `accounts/a` + `accounts/b` | Snapshot racing the commit | Both or neither updated state in export |
| Non-blocking | US-080-AC3 | Snapshot in progress | Concurrent write | Write commits normally |
| Scoping | US-080-AC4 | Snapshot of `tasks` only | Export | Only `tasks` entities, envelopes present |

## Dependencies

- **Stories**: US-010 (entities exist)
- **Feature Spec**: FEAT-004
- **Feature Requirements**: ENT-02, ENT-07, ENT-13
- **PRD Requirements**: FR-1
- **External**: CONTRACT-001 (snapshot endpoint), FEAT-008 (transactional commit atomicity the cut must respect)

## Out of Scope

- Incremental/differential exports and resumable change streams (FEAT-021 CDC).
- Snapshot restore/import tooling.
- Cross-database snapshots.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
