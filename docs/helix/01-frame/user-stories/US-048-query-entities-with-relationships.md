---
ddx:
  id: US-048
  review:
    self_hash: cf367641ca8539b58cf9cbd1711963d53f9e0c380b0961d3e65f6b2cf95eb0d3
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-048: Query Entities with Relationships

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-05, GQL-06, GQL-07, GQL-08
**PRD Requirements**: FR-3, FR-20
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent built by Ava (agent application developer)
**I want** to fetch entities and their related entities in one request
**So that** I can understand the full context without multiple API calls

## Context

Agents reasoning over business records need an entity plus its linked
neighbors (dependencies, vendors, approvals) in one round trip. This story
exercises FEAT-015's query, pagination, and relationship-field requirements
(GQL-05 through GQL-08). The generated field names, filter operators, and
connection shapes are normative in CONTRACT-002.

## Walkthrough

1. The agent sends one GraphQL query for an entity including a relationship
   field with nested selections.
2. Axon resolves the entity and its related entities in a single response.
3. The agent narrows a relationship with filter and sort arguments and pages
   through results using connection cursors.
4. The agent reads the connection's total count to size further work.

## Acceptance Criteria

- [ ] **US-048-AC1** — Given a bead with declared dependency links, when the
  agent queries the bead with its dependency relationship field, then the
  bead and its dependencies return in one response.
- [ ] **US-048-AC2** — Given nested relationships, when the agent queries to
  a depth within the configured limit, then nested relationship resolution
  succeeds at arbitrary depth up to that limit.
- [ ] **US-048-AC3** — Given a relationship field, when the agent supplies
  filter and sort arguments (operators per CONTRACT-002), then only matching
  related entities return in the requested order.
- [ ] **US-048-AC4** — Given any connection-typed field, when the agent
  requests the total count, then the policy-filtered total is available.
- [ ] **US-048-AC5** — Given an invalid filter argument, when the query
  executes, then a GraphQL error returns naming the field path and expected
  type.

## Edge Cases

- **Entity with no links**: The relationship field returns an empty
  connection, not null or an error.
- **Filter on a non-indexed field**: The query still answers correctly via
  scan fallback (FEAT-013 acceleration is transparent).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| One-shot context fetch | US-048-AC1 | Bead `b-1` depends on `b-2`, `b-3` | Query `b-1` with dependency field | Response contains `b-1` plus both dependencies |
| Depth-limited nesting | US-048-AC2 | Chain of 4 linked beads, depth limit 10 | Query 4 levels of nesting | All 4 levels resolve |
| Filtered relationship | US-048-AC3 | `b-1` has 3 deps, one `status=blocked` | Relationship filter `status eq blocked` | Only the blocked dependency returns |
| Total count | US-048-AC4 | 25 related entities, page size 10 | Request first page + total count | 10 edges, total count 25 |
| Bad filter | US-048-AC5 | Filter compares string field to integer | Execute query | GraphQL error with field path and expected type |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-05, GQL-06, GQL-07, GQL-08
- **PRD Requirements**: FR-3, FR-20
- **External**: CONTRACT-002 (GraphQL surface), CONTRACT-007 (traversal
  planning via FEAT-009)

## Out of Scope

- Policy-leak behavior of traversal (US-110).
- Ad-hoc Cypher pattern queries (FEAT-009 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
