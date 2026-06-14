---
ddx:
  id: US-019
  review:
    self_hash: ca317b29072be09db1c8ad4abfbd974afa5a873f5ef24c9f02ec39f3e3dba4af
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-019: Query Across Entity-Link Graph

**Feature**: FEAT-007 — Entity-Graph Data Model
**Feature Requirements**: GRF-05, GRF-06, GRF-07
**PRD Requirements**: FR-2, FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer
**I want** my agents to combine entity predicates with link traversal in one query
**So that** they can answer questions like "find all pending beads that depend on completed beads" in a single round-trip

## Context

The payoff of a first-class link model is queries that mix record predicates
and graph shape. This story demonstrates FEAT-007's model (typed directional
links, cross-collection reach) through the unified read model: the query
language, planner, and limits are owned by FEAT-009 and are normative in
CONTRACT-007. This story validates that the *model* supports combined
entity-link queries end-to-end.

## Walkthrough

1. Ava's agent submits one query: entities in `beads` with `status = "pending"` linked via `depends-on` to entities with `status = "done"` (language per CONTRACT-007).
2. The planner matches entity predicates and link patterns in one execution.
3. The result contains the matching entities and the traversal path that satisfied the pattern.
4. The agent acts on the result without further round-trips.

## Acceptance Criteria

- [ ] **US-019-AC1** — Given pending and done beads connected by `depends-on` links, when the combined predicate+traversal query runs, then exactly the pending beads whose dependencies are done are returned.
- [ ] **US-019-AC2** — Given a matching result, when path projection is requested, then the result includes the traversal path alongside the matched entities.
- [ ] **US-019-AC3** — Given a 10K-entity collection, when a 3-hop combined query runs, then it completes within 500 ms p99 (FEAT-009 NFR budget).
- [ ] **US-019-AC4** — Given a pattern with no matches, when the query runs, then an empty result set is returned, not an error.

## Edge Cases

- **Links crossing collections**: the pattern matches across collections naturally (GRF-07).
- **Dangling links** (target force-deleted): skipped by traversal, never an error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-019-AC1 | 2 pending beads dep-on done beads; 1 pending dep-on open bead | Combined query | Only the 2 qualifying beads |
| Path projection | US-019-AC2 | Same graph | Query with path projection | Entities + paths returned |
| Latency | US-019-AC3 | 10K beads, 3-hop pattern | Timed query | < 500 ms p99 |
| Empty | US-019-AC4 | No qualifying beads | Combined query | Empty result, no error |

## Dependencies

- **Stories**: US-017, US-018 (model populated)
- **Feature Spec**: FEAT-007
- **Feature Requirements**: GRF-05, GRF-06, GRF-07
- **PRD Requirements**: FR-2, FR-3
- **External**: FEAT-009 (query language, planner, limits — the owning feature for read semantics), CONTRACT-007 (Cypher surface), CONTRACT-002 (GraphQL result shapes)

## Out of Scope

- Query-language clause coverage, named queries, subscriptions (FEAT-009 stories).
- Policy-filtered visibility during traversal (FEAT-029 / FEAT-009 QRY-15..16).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
