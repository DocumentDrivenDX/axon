---
ddx:
  id: US-062
  review:
    self_hash: 5f33bacda8e1f7cc5a050853df4e59aa8192391505eedd04a193fd823274fcf9
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-062: Count Entities by Field

**Feature**: FEAT-018 — Aggregation Queries
**Feature Requirements**: AGG-01, AGG-02, AGG-08, AGG-09
**PRD Requirements**: FR-3
**Priority**: P1
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority)
**I want** to count entities grouped by a field
**So that** I can understand the distribution of data without fetching all
entities

## Context

"How many beads per status?" is the canonical summary question agents ask
before planning work. This story exercises FEAT-018's counting and grouping
semantics (AGG-01, AGG-02) as projections of the unified planner (AGG-08)
with policy-safe totals (AGG-09). Projection surfaces are normative in
CONTRACT-002/CONTRACT-003/CONTRACT-007.

## Walkthrough

1. The agent requests a count grouped by a field over a collection.
2. Axon compiles the aggregation through the unified planner and returns one
   count per group plus a total.
3. The agent narrows the count with a filter and re-runs it.
4. Entities missing the grouped field appear in their own null-labeled
   group; hidden rows never contribute.

## Acceptance Criteria

- [ ] **US-062-AC1** — Given beads in several statuses, when the agent runs
  a count grouped by status, then each status group returns its count.
- [ ] **US-062-AC2** — Given no grouping, when the agent runs a count, then
  a single total count returns.
- [ ] **US-062-AC3** — Given a filter, when the agent runs a filtered count,
  then only matching entities are counted.
- [ ] **US-062-AC4** — Given an empty collection, when the agent runs a
  grouped count, then a zero total with empty groups returns.
- [ ] **US-062-AC5** — Given entities with a null or missing grouped field,
  when the agent runs a grouped count, then those entities form their own
  null-labeled group.

## Edge Cases

- **Grouping by a non-existent field**: All entities fall into a single
  null-labeled group.
- **Policy-hidden rows**: Excluded from every group count and the total — no
  hidden-row inference.
- **High-cardinality grouping**: The group-limit parameter caps the number
  of groups returned.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Grouped count | US-062-AC1 | 5 draft, 12 pending, 3 in_progress beads | Count grouped by status | Groups: draft 5, pending 12, in_progress 3 |
| Total only | US-062-AC2 | Same 20 beads | Count without grouping | Total 20 |
| Filtered count | US-062-AC3 | 8 of 20 beads have `bead_type=task` | Count with task filter | Total 8 |
| Empty collection | US-062-AC4 | No entities | Grouped count | Total 0, groups [] |
| Null group | US-062-AC5 | 2 beads missing status | Count grouped by status | Null-labeled group with count 2 |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-018
- **Feature Requirements**: AGG-01, AGG-02, AGG-08, AGG-09
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (planner/limits), CONTRACT-002 and CONTRACT-003
  (projection surfaces)

## Out of Scope

- Numeric aggregations (US-063).
- Surface-specific projection behavior (US-064, US-065).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
