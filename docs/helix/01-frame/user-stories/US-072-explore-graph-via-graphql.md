---
ddx:
  id: US-072
---

# US-072: Explore Graph via GraphQL

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-08, QRY-12
**PRD Requirements**: FR-3, FR-20
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder creating a relationship explorer UI
**I want** to traverse the entity graph through typed GraphQL fields
**So that** I can build interactive drill-down views without N+1 fanout

## Context

Inherited from the retired FEAT-020. UIs consume the graph through GraphQL,
not raw Cypher. This story exercises the generated named-query surface
(QRY-08): typed connections with pagination, multi-hop results in one
resolver execution, and depth protection. Connection and field shapes are
normative in CONTRACT-002.

## Walkthrough

1. Wei declares a named query for the exploration pattern; schema activation generates a typed GraphQL field (per CONTRACT-002).
2. Her UI queries the field with connection arguments for paging.
3. The system executes the underlying plan once — multi-hop results arrive without per-node resolver fanout.
4. The UI drills down by re-querying with new roots, protected by the depth limit.

## Acceptance Criteria

- [ ] **US-072-AC1** — Given an activated named query, when the UI queries its GraphQL field, then results arrive as a typed connection (edges, page info, total count) per CONTRACT-002.
- [ ] **US-072-AC2** — Given a multi-hop named query, when executed, then it runs as one planned execution — no N+1 per-node resolution.
- [ ] **US-072-AC3** — Given nested traversal requests beyond the depth limit (default 10), when submitted, then the query is rejected with the documented error rather than executing unboundedly.
- [ ] **US-072-AC4** — Given connection arguments (forward and backward pagination), when used on a named-query connection, then paging behaves per CONTRACT-002.

## Edge Cases

- **Empty page**: a page past the last result returns an empty connection with correct page info.
- **Policy-hidden nodes**: excluded consistently from edges and total counts (QRY-15..16).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Typed connection | US-072-AC1 | Named query active | GraphQL query with `first: 10` | Edges + pageInfo + totalCount |
| No fanout | US-072-AC2 | 3-hop named query, 1K nodes | Execute; count plan executions | Single planned execution |
| Depth guard | US-072-AC3 | Request nesting depth 11 | Execute | Documented rejection |
| Back-paging | US-072-AC4 | 25 results, page size 10 | Page forward twice, then back | Stable, consistent pages |

## Dependencies

- **Stories**: US-075 (named-query declaration)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-08, QRY-12
- **PRD Requirements**: FR-3, FR-20
- **External**: CONTRACT-002 (field/connection shapes, depth limits), CONTRACT-007 (underlying query semantics), FEAT-015 (GraphQL layer)

## Out of Scope

- Graph visualization UI itself (FEAT-011).
- Ad-hoc Cypher over GraphQL (US-076).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
