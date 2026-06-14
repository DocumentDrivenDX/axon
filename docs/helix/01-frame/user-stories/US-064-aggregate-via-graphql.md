---
ddx:
  id: US-064
  review:
    self_hash: dc684b0fbee81e70b7344a80b69671948304b335fc1481662d686a54954a90b0
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-064: Aggregate via GraphQL

**Feature**: FEAT-018 — Aggregation Queries
**Feature Requirements**: AGG-05, AGG-07
**PRD Requirements**: FR-3, FR-20
**Priority**: P1
**Status**: Draft

## Story

**As** Wei, a business workflow builder creating a dashboard
**I want** to query aggregations via GraphQL
**So that** I can build summary views without client-side computation

## Context

Dashboards are GraphQL clients; aggregations must be first-class generated
fields alongside entity queries. This story exercises FEAT-018's GraphQL
projection (AGG-05) and cross-surface parity (AGG-07). The field naming,
arguments, and result types are normative in CONTRACT-002's Aggregations
section.

## Walkthrough

1. Wei discovers the generated per-collection aggregation field through
   introspection.
2. Wei queries it with filter, grouping, and aggregation-function arguments.
3. The response returns the total count and groups with key values and
   aggregated results.
4. Wei combines the aggregation with a regular entity query in the same
   GraphQL request to render a summary header plus a detail list.

## Acceptance Criteria

- [ ] **US-064-AC1** — Given a registered collection, when Wei introspects
  the schema, then a per-collection aggregation query field is
  auto-generated (naming per CONTRACT-002).
- [ ] **US-064-AC2** — Given the aggregation field, when Wei queries it,
  then filter, grouping, and aggregation-function arguments are available
  (shapes per CONTRACT-002).
- [ ] **US-064-AC3** — Given a grouped aggregation query, when it executes,
  then the response includes the total count and groups with their keys and
  aggregated values.
- [ ] **US-064-AC4** — Given a single GraphQL request containing an
  aggregation query and a regular entity query, when it executes, then both
  resolve correctly in one response.

## Edge Cases

- **Invalid aggregation argument** (numeric function on a string field):
  Structured error with the aggregation category code (per CONTRACT-002).
- **Policy-hidden rows**: GraphQL aggregation totals match the policy-safe
  planner results — identical to the same aggregation via MCP or the
  structured API.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Generated field | US-064-AC1 | `beads` registered | Introspect | Aggregation field present for beads |
| Full arguments | US-064-AC2 | 20 beads | Filtered, grouped count + avg | Correct groups and values |
| Result shape | US-064-AC3 | Grouped query | Execute | totalCount + groups with keys and values |
| Mixed request | US-064-AC4 | Aggregation + entity list in one document | Execute | Both results in one response |

## Dependencies

- **Stories**: US-062, US-063 (aggregation semantics), US-049
  (introspection)
- **Feature Spec**: FEAT-018
- **Feature Requirements**: AGG-05, AGG-07
- **PRD Requirements**: FR-3, FR-20
- **External**: CONTRACT-002 (GraphQL aggregation projection), CONTRACT-007
  (planner)

## Out of Scope

- MCP aggregation tools (US-065).
- Ad-hoc Cypher aggregation queries (FEAT-009 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
