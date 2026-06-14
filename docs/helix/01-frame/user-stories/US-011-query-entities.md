---
ddx:
  id: US-011
  review:
    self_hash: 28a107d4af8681008efe4abd2c51ba24126bbc3db3a57c814075740c421ee285
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-011: Query Entities

**Feature**: FEAT-004 — Entity Operations
**Feature Requirements**: ENT-13, ENT-14
**PRD Requirements**: FR-1, FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer
**I want** my agents to find entities matching specific criteria
**So that** they can locate relevant records without knowing entity IDs

## Context

Agents rarely hold entity IDs; they reason in predicates ("pending tasks
assigned to me"). This story exercises FEAT-004's listing surface (ENT-13)
and its boundary (ENT-14): predicate filtering, sorting, and counting are
expressed through the unified read model owned by FEAT-009, whose language
and limits are normative in CONTRACT-007.

## Walkthrough

1. Ava's agent issues a query against a collection: equality and comparison predicates combined with AND (read model per FEAT-009 / CONTRACT-007).
2. The system returns matching entities ordered by the requested sort field.
3. The agent pages through results using the returned cursor.
4. The agent separately requests a count of matches without fetching the rows.

## Acceptance Criteria

- [ ] **US-011-AC1** — Given entities with mixed `status` values, when the agent queries with an equality predicate on `status`, then only matching entities are returned.
- [ ] **US-011-AC2** — Given entities with numeric `priority` values, when the agent queries with a comparison predicate (greater-than), then only entities above the threshold are returned.
- [ ] **US-011-AC3** — Given a query combining two predicates with AND, when executed, then only entities satisfying both are returned.
- [ ] **US-011-AC4** — Given a query with a descending sort on a timestamp field, when executed, then results are ordered newest-first.
- [ ] **US-011-AC5** — Given more matches than the page size, when the agent pages with the returned cursor, then iteration is stable (no gaps or duplicates) while writes continue.
- [ ] **US-011-AC6** — Given a count-style aggregation of matches, when executed, then the count is returned without materializing the matching rows (policy-aware per FEAT-009 QRY-16).

## Edge Cases

- **No matches**: empty result set, not an error.
- **Predicate on an unknown field**: rejected at parse time with a stable error code (CONTRACT-007).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Equality | US-011-AC1 | 3 pending, 2 done tasks | Query `status = "pending"` | 3 entities |
| Comparison | US-011-AC2 | Priorities 1..5 | Query `priority > 3` | Entities with 4 and 5 |
| Conjunction | US-011-AC3 | Mixed status/assignee | Query `status = "pending" AND assignee = "agent-1"` | Only rows satisfying both |
| Stable cursor | US-011-AC5 | 250 matches, page 100 | Page 3 times | 100+100+50, no gaps/duplicates |
| Count | US-011-AC6 | 250 matches | Count query | 250, no row payloads |

## Dependencies

- **Stories**: US-010 (entities exist)
- **Feature Spec**: FEAT-004
- **Feature Requirements**: ENT-13, ENT-14
- **PRD Requirements**: FR-1, FR-3
- **External**: FEAT-009 (unified read model owner), CONTRACT-007 (query language, error codes, limits), CONTRACT-002 (GraphQL query shapes), CONTRACT-001 (HTTP query surface)

## Out of Scope

- Graph traversal and pattern queries (FEAT-009 stories US-023..US-077).
- Aggregations beyond counting (FEAT-018 surfaces, projections of FEAT-009).
- Secondary-index declaration and acceleration (FEAT-013).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
