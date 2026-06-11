---
ddx:
  id: US-063
---

# US-063: Compute Numeric Aggregations

**Feature**: FEAT-018 — Aggregation Queries
**Feature Requirements**: AGG-01, AGG-03, AGG-08
**PRD Requirements**: FR-3
**Priority**: P1
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority) analyzing entity
data
**I want** to compute sum, average, minimum, and maximum over numeric fields
**So that** I can derive summary statistics without fetching all entities

## Context

Numeric summaries (total invoice amount, average priority) power agent
decisions and dashboards. This story exercises FEAT-018's numeric function
and type-handling semantics (AGG-01, AGG-03) compiled through the unified
planner (AGG-08). Error codes are normative in CONTRACT-002/CONTRACT-007.

## Walkthrough

1. The agent requests an average over a numeric field grouped by another
   field and reads per-group averages.
2. It requests a sum over an invoice amount field and reads the total.
3. It requests minimum and maximum over a priority field.
4. A request to sum a string field fails with a clear structured type
   error; null-valued entities are excluded from numeric results.

## Acceptance Criteria

- [ ] **US-063-AC1** — Given beads with priorities across statuses, when the
  agent computes an average of priority grouped by status, then each group
  returns its correct average.
- [ ] **US-063-AC2** — Given an invoices collection, when the agent computes
  a sum of the amount field, then the correct total returns.
- [ ] **US-063-AC3** — Given a numeric field, when the agent computes
  minimum and maximum, then the correct extreme values return.
- [ ] **US-063-AC4** — Given a non-numeric field, when the agent requests a
  sum over it, then a clear structured type error returns (codes per
  CONTRACT-002/CONTRACT-007).
- [ ] **US-063-AC5** — Given entities whose aggregated field is null, when
  computing sum/average/minimum/maximum, then those entities are excluded
  rather than treated as zero.
- [ ] **US-063-AC6** — Given an integer source field, when the agent
  computes an average, then the result is a non-integer numeric value.

## Edge Cases

- **All values null**: Numeric aggregates return a null/absent result with
  the excluded count visible, not zero.
- **Single entity**: Min, max, average, and sum all equal that entity's
  value.
- **Mixed groups after filtering**: Per-group exclusion counts let the
  caller see how many entities contributed.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Grouped average | US-063-AC1 | pending: priorities 2,4; done: 6 | AVG(priority) by status | pending 3.0, done 6.0 |
| Sum total | US-063-AC2 | Amounts 100, 250, 650 | SUM(amount) | 1000 |
| Extremes | US-063-AC3 | Priorities 1..9 | MIN/MAX(priority) | 1 and 9 |
| Type error | US-063-AC4 | `title` is a string field | SUM(title) | Structured type error |
| Null exclusion | US-063-AC5 | Amounts 100, null, 200 | SUM and AVG(amount) | Sum 300; avg 150.0 (2 contributors) |
| Float average | US-063-AC6 | Integer priorities 1, 2 | AVG(priority) | 1.5 |

## Dependencies

- **Stories**: US-062 (grouping semantics)
- **Feature Spec**: FEAT-018
- **Feature Requirements**: AGG-01, AGG-03, AGG-08
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (planner), CONTRACT-002/CONTRACT-003
  (projection surfaces)

## Out of Scope

- Filters on aggregated results (HAVING-style; FEAT-018 out of scope).
- Approximate or windowed aggregation.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
