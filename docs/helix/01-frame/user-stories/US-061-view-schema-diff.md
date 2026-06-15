---
ddx:
  id: US-061
  review:
    self_hash: 1107d05571bbba2d83361c092e779ad6251f1ec277b7d0716177b005712a95b8
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-061: View Schema Diff

**Feature**: FEAT-017 — Schema Evolution and Migration
**Feature Requirements**: EVO-12, EVO-13, EVO-14
**PRD Requirements**: FR-1; PRD Should-Have P1-1 (schema evolution and migration)
**Priority**: P1
**Status**: Draft

## Story

**As a** developer debugging a schema change
**I want** to see exactly what changed between two schema versions
**So that** I can understand the evolution history

## Context

When entities start failing validation or consumers break, the first question is "what changed in the schema and when". This story exercises EVO-12 (structured field-level diff between any two versions), EVO-13 (diff embedded in schema-change audit entries), and EVO-14 (diff available via CLI and query API). Diff surfaces are defined by CONTRACT-008 (CLI) and CONTRACT-002 (GraphQL).

## Walkthrough

1. Developer requests the diff between two schema versions of a collection via the CLI (CONTRACT-008) or the query API (CONTRACT-002).
2. System computes the structured field-level diff: added, removed, and modified fields, including type changes, constraint changes, and enum value changes.
3. Developer inspects the diff and identifies the change that explains the observed behavior.
4. Developer cross-checks the schema-change audit entry, which carries the same field-level diff.

## Acceptance Criteria

- [ ] **US-061-AC1** — Given a collection with at least two schema versions, when the developer requests a diff between two versions via the CLI or query API (CONTRACT-008/CONTRACT-002), then added, removed, and modified fields are shown.
- [ ] **US-061-AC2** — Given a version pair whose changes include type changes, constraint changes, and enum value changes, when the diff is requested, then each of those change kinds is represented in the diff.
- [ ] **US-061-AC3** — Given an applied schema change, when its audit entry is queried, then the entry includes the field-level diff.
- [ ] **US-061-AC4** — Given non-adjacent versions (e.g. v1 and v5), when the diff is requested, then the cumulative diff between those two versions is returned correctly.

## Edge Cases

- **Diff of identical versions**: Requesting a diff of a version against itself returns an empty diff, not an error.
- **Unknown version**: Requesting a diff involving a version that never existed returns a structured not-found error.
- **Metadata-only changes**: Description or index changes appear as modifications without validation impact.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Basic diff | US-061-AC1 | v1: `{id, amount}`; v2: `{id, amount, status}` minus nothing | Diff v1 v2 | `status` listed as added; `id`, `amount` unchanged |
| Change kinds | US-061-AC2 | v2→v3 changes `amount` type, tightens `id` pattern, narrows `status` enum | Diff v2 v3 | Type change, constraint change, and enum change all shown |
| Audit diff | US-061-AC3 | v3 just applied | Query schema-change audit entry | Entry includes the v2→v3 field-level diff |
| Non-adjacent | US-061-AC4 | Versions v1..v5 exist | Diff v1 v5 | Cumulative diff matches composition of v1→v5 changes |

## Dependencies

- **Stories**: US-058 (schema updates create the versions being diffed)
- **Feature Spec**: FEAT-017
- **Feature Requirements**: EVO-12, EVO-13, EVO-14
- **PRD Requirements**: FR-1; Should-Have P1-1
- **External**: CONTRACT-002, CONTRACT-008, CONTRACT-005 (audit record shape)

## Out of Scope

- Visual/graphical schema diff in the admin UI (FEAT-011).
- Diffing schemas across different collections.
- Entity-level diffs (audit/repair views, FEAT-003/FEAT-023).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
