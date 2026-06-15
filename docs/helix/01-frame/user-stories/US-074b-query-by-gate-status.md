---
ddx:
  id: US-074b
  review:
    self_hash: 34eca988f0fd0c9556a7538d8f394592aaa104935e78c8301e11d62129823268
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-074b: Query by Gate Status

**Feature**: FEAT-019 — Validation Rules and Actionable Errors
**Feature Requirements**: VAL-12, VAL-13
**PRD Requirements**: FR-1, FR-3
**Priority**: P1
**Status**: Draft

## Story

**As an** agent application developer (Ava) operating agents and reviewing
their work
**I want** to find entities that pass or fail a specific validation gate
**So that** I can find items ready for processing and items that still need
attention

## Context

Gate status is only useful at scale if it is queryable: "show me all orders
ready for processing" must be a fast indexed filter, not a per-entity rule
re-evaluation. This story exercises gate-status querying across read
surfaces (VAL-12), exclusion of entities without gate evaluations (VAL-13),
and the gate-filter latency NFR. This story keeps its legacy suffixed ID
`US-074b` per the user-story ID registry; it is distinct from US-074.

## Walkthrough

1. Ava's collection has a `complete` gate; some entities pass it and some do
   not.
2. Ava queries for entities passing `complete` combined with
   `status = pending`.
3. Axon answers from the materialized gate status index, returning only
   pending entities that pass the gate.
4. Ava flips the gate filter to failing entities to build a "needs
   attention" work list.

## Acceptance Criteria

- [ ] **US-074b-AC1** — Given entities with materialized gate status, when a
  query filters on a gate passing, then only entities passing that gate are
  returned.
- [ ] **US-074b-AC2** — Given the same data, when a query filters on the gate
  failing, then only entities failing that gate are returned.
- [ ] **US-074b-AC3** — Given a query combining a gate filter with field
  filters, when it executes, then both filters apply in one query.
- [ ] **US-074b-AC4** — Given the GraphQL read surface, when a gate filter is
  used, then it returns the same results as the structured API
  (surface per CONTRACT-002).
- [ ] **US-074b-AC5** — Given the MCP query surface, when a gate filter is
  used, then it returns the same results as GraphQL.
- [ ] **US-074b-AC6** — Given a collection of 100K entities, when a
  gate-status query executes, then it is served by the materialized gate
  index in under 50 ms.
- [ ] **US-074b-AC7** — Given entities with no gate evaluations (for example,
  a collection without validation rules), when gate filters run, then those
  entities are not returned.

## Edge Cases

- **Gate name not declared for the collection**: the query is rejected with
  a structured error rather than silently returning nothing.
- **Stale status during recomputation**: after a rules change, results
  reflect the most recent materialized evaluation until background
  recomputation converges (VAL-15).
- **Advisory findings**: advisory status is queryable the same way gate
  status is, supporting "entities with advisory warnings" work lists.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Passing filter | US-074b-AC1 | 3 entities pass `complete`, 2 fail | Query gate `complete` = pass | Exactly the 3 passing entities |
| Combined filter | US-074b-AC3 | Passing entities with mixed `status` values | Query `complete` = pass AND `status = pending` | Only pending passers |
| Surface parity | US-074b-AC4/AC5 | Same data | Same gate filter via structured API, GraphQL, MCP | Identical result sets |
| Index latency | US-074b-AC6 | 100K entities | Gate-status query | < 50 ms, served from gate index |
| No gate rows | US-074b-AC7 | Collection without validation rules | Gate filter query | No entities returned |

## Dependencies

- **Stories**: US-067 (gate status materialization).
- **Feature Spec**: FEAT-019
- **Feature Requirements**: VAL-12, VAL-13
- **PRD Requirements**: FR-1, FR-3
- **External**: CONTRACT-002 (GraphQL filter surface), CONTRACT-007 (Cypher
  read surface), CONTRACT-010 (gate semantics); ADR-010 (gate index design)

## Out of Scope

- Defining gates and rules (US-066, US-067).
- General field-filter query capabilities (FEAT-004, FEAT-009).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
