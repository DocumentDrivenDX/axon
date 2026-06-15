---
ddx:
  id: US-007
  review:
    self_hash: ee2328eca68410ca141b209931e5bc80b25d8b0a646229559e4f916909773039
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-007: Query the Audit Trail

**Feature**: FEAT-003 — Audit Log
**Feature Requirements**: AUD-02, AUD-08, AUD-10
**PRD Requirements**: FR-16
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer debugging agent behavior
**I want** to query the audit log for a specific entity, actor, or time range
**So that** I can reconstruct the sequence of events and understand what happened

## Context

When an agent misbehaves, Ava's only recovery path starts with knowing exactly
what changed and in what order. This story exercises FEAT-003's audit-query
area (AUD-08, AUD-10) over the entry shape defined by AUD-02: filterable,
paginated history queries that turn the append-only log into an investigative
tool.

## Walkthrough

1. Ava notices an entity in an unexpected state.
2. She queries audit history filtered by collection and entity ID (via CLI or API; surface per CONTRACT-001/CONTRACT-008).
3. The system returns every mutation of that entity in chronological order, each entry showing operation, actor, timestamp, and diff.
4. She narrows the query with a time-range filter, then pivots to an actor filter to see everything that agent did.
5. She pages through results using the returned cursor until she finds the bad mutation.

## Acceptance Criteria

- [ ] **US-007-AC1** — Given an entity with three committed mutations, when Ava queries audit history filtered by collection and entity ID, then all three entries are returned in chronological order, each showing operation, actor, timestamp, and diff.
- [ ] **US-007-AC2** — Given mutations spread across a day, when Ava queries with a since/until time-range filter, then only entries inside the range are returned.
- [ ] **US-007-AC3** — Given mutations by two different actors, when Ava filters by one actor, then only that actor's entries are returned.
- [ ] **US-007-AC4** — Given more matching entries than the page limit, when Ava queries, then results are cursor-paginated and the cursor retrieves the next page without skipping or duplicating entries.
- [ ] **US-007-AC5** — Given an unsupported filter, when Ava queries, then the request fails with a structured error naming the supported filters (per CONTRACT-001).

## Edge Cases

- **Entity with no history**: query returns an empty result set, not an error.
- **Filter on a dropped collection**: historical entries remain queryable; the drop itself appears as an audited operation.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-007-AC1 | `invoices/inv-1` created, updated, updated | Query audit by collection=`invoices`, entity=`inv-1` | 3 entries, chronological, each with operation/actor/timestamp/diff |
| Time range | US-007-AC2 | Mutations at 09:00, 12:00, 18:00 | Query since=10:00 until=13:00 | Only the 12:00 entry |
| Actor filter | US-007-AC3 | Mutations by `agent-1` and `user-2` | Query actor=`agent-1` | Only `agent-1` entries |
| Pagination | US-007-AC4 | 150 entries, page limit 100 | Query, then query with returned cursor | 100 then 50 entries, no gaps or duplicates |
| Unsupported filter | US-007-AC5 | Filter `metadata.reason=x` | Query | Structured error listing supported filters |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-003
- **Feature Requirements**: AUD-02, AUD-08, AUD-10
- **PRD Requirements**: FR-16
- **External**: CONTRACT-005 (audit entry fields, operation taxonomy), CONTRACT-001 (audit query endpoint and filters), CONTRACT-008 (audit CLI)

## Out of Scope

- Multi-collection streaming tail (US-079).
- PROV-O serialization of results (US-120).
- Reverting state found via the query (US-008).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
