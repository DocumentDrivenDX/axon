---
ddx:
  id: US-101
  review:
    self_hash: f123cc330863d116e553ae3ec9c2407da8e4ed18d97f8e227d39ad9dc7c23fc2
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-101: Hide Inaccessible Entities

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-08, ACL-09, ACL-11
**PRD Requirements**: FR-13
**Priority**: P0
**Status**: Approved

## Story

**As a** consultant using a direct browser-to-Axon app (end user of an
application built by Ava, Agent Application Developer persona)
**I want** Axon to omit engagements I am not assigned to
**So that** bypassing the UI cannot reveal other client work

## Context

Browser-side filtering is not a security boundary: a tailnet user can call
GraphQL directly. Row visibility must be enforced before data leaves Axon,
and hidden rows must be indistinguishable from missing ones — including in
pagination, counts, and link traversal.

## Walkthrough

1. Consultant authenticates and queries the engagements collection.
2. System evaluates the read policy and returns only rows whose membership
   includes the consultant.
3. Consultant point-reads an engagement they are not assigned to.
4. System responds exactly as it would for a missing entity.

## Acceptance Criteria

- [ ] **US-101-AC1** — Given a hidden entity, when the caller point-reads it,
  then the response is identical to a missing entity (not-found / null per
  CONTRACT-004 read-denial semantics), never a forbidden response.
- [ ] **US-101-AC2** — Given list and GraphQL connection queries, when
  results are returned, then hidden rows are omitted without any policy
  error.
- [ ] **US-101-AC3** — Given pagination and total counts, when a filtered
  list is requested, then page windows and counts are computed after policy
  filtering.
- [ ] **US-101-AC4** — Given link traversal or relationship resolution, when
  a target entity is hidden, then it is not materialized for the caller.

## Edge Cases

- **Aggregates**: aggregate results include only caller-visible rows.
- **Existence probing**: no error-shape, timing-independent count, or
  nullability difference distinguishes hidden from missing entities.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Hidden point read | US-101-AC1 | Engagement without caller in members | Point read | Same shape as missing entity |
| List omission | US-101-AC2 | 10 engagements, 3 visible | List query | Exactly 3 rows, no errors |
| Post-filter pagination | US-101-AC3 | 3 visible rows, limit 100 | Paginated query | Count and pages over 3 rows |
| Hidden traversal target | US-101-AC4 | Link to hidden engagement | Traverse relationship | Target omitted |

## Dependencies

- **Stories**: US-109 (policy must be authorable)
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-08, ACL-09, ACL-11
- **PRD Requirements**: FR-13
- **External**: CONTRACT-004 (read-denial semantics), CONTRACT-002 (GraphQL
  shapes), CONTRACT-001 (REST compatibility shapes)

## Out of Scope

- Field-level redaction on visible rows (US-102).
- UI rendering of filtered results (FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
