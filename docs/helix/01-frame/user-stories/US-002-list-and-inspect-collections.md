---
ddx:
  id: US-002
  review:
    self_hash: 93b7c3b7323b4119e75f08333b5dda9808c399f8975cf3d15873305bf48bc129
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-002: List and Inspect Collections

**Feature**: FEAT-001 — Collections
**Feature Requirements**: COL-08, COL-09, COL-10, COL-11
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As a** developer or agent
**I want** to list all collections and inspect their metadata
**So that** I can discover what data is available and its structure without out-of-band knowledge

## Context

Agents and developers arriving at an Axon database need to discover what collections exist and what each holds before reading or writing. This story exercises COL-08 (list with metadata), COL-09 (describe with full metadata), COL-10 (equivalent CLI and API surfaces), and COL-11 (metadata tracking). Discovery surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-001 (HTTP API).

## Walkthrough

1. Developer or agent issues a list-collections request via the CLI (CONTRACT-008) or HTTP API (CONTRACT-001).
2. System returns every collection in scope with its name, schema version, entity count, and created/updated timestamps.
3. The caller picks a collection and issues a describe request for it.
4. System returns the full metadata for that collection, including its schema, declared indexes, and statistics.

## Acceptance Criteria

- [ ] **US-002-AC1** — Given a database with collections, when the caller lists collections via the CLI or HTTP API (CONTRACT-008/CONTRACT-001), then every collection in scope is returned with name, schema version, and entity count.
- [ ] **US-002-AC2** — Given an existing collection, when the caller describes it, then the response includes the full metadata: schema, declared indexes, statistics, and created/last-modified timestamps.
- [ ] **US-002-AC3** — Given the same collection, when it is listed or described via CLI and via API, then both surfaces return the same information.
- [ ] **US-002-AC4** — Given a database with no collections, when the caller lists collections, then an empty list is returned, not an error.

## Edge Cases

- **Empty database**: List returns an empty list (US-002-AC4), not an error.
- **Describe non-existent collection**: Returns a structured not-found error.
- **Stale entity count**: Entity count in list/describe reflects committed state at read time; concurrent writes may change it immediately after.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path list | US-002-AC1 | Database with `invoices` (3 entities) and `vendors` (1 entity) | List collections | Both collections returned with names, schema versions, entity counts 3 and 1 |
| Describe | US-002-AC2 | `invoices` exists with schema v2 and one index | Describe `invoices` | Full metadata returned: schema v2 document, index, statistics, timestamps |
| Surface parity | US-002-AC3 | `invoices` exists | List/describe via CLI and via API | Same fields and values on both surfaces |
| Empty database | US-002-AC4 | Database with zero collections | List collections | Empty list, success status |

## Dependencies

- **Stories**: US-001 (collections must be creatable to be discoverable)
- **Feature Spec**: FEAT-001
- **Feature Requirements**: COL-08, COL-09, COL-10, COL-11
- **PRD Requirements**: FR-1
- **External**: CONTRACT-001, CONTRACT-008

## Out of Scope

- Filtering discovery results by access policy (FEAT-029 owns visibility).
- Querying entities inside a collection (FEAT-004).
- Schema retrieval semantics beyond the embedded schema in describe (FEAT-002, US-006).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
