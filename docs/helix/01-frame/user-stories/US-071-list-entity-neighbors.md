---
ddx:
  id: US-071
  review:
    self_hash: 17f93303aed3c2b8384b03941eaf7ae1394bb3e30b4cc4843ddd42b0a2f15dc5
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-071: List Entity Neighbors

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-01, QRY-13
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose agent is orienting itself around an entity
**I want** the agent to see all entities linked to and from a record in one query
**So that** it understands the record's context in the graph before acting

## Context

Inherited from the retired FEAT-020. "What is connected to this?" is the
first question an agent asks about an unfamiliar record. This story
exercises undirected and directed single-hop matching with link-type
filters (QRY-01) over link-storage indexes (QRY-13).

## Walkthrough

1. Ava's agent submits a single-hop match around an entity with no direction constraint (language per CONTRACT-007).
2. The system returns each neighbor with the connecting relationship's type.
3. The agent narrows to outbound-only, then to a specific link type, refining the picture.

## Acceptance Criteria

- [ ] **US-071-AC1** — Given an entity with inbound and outbound links, when an undirected single-hop match runs, then all neighbors are returned with their relationship types.
- [ ] **US-071-AC2** — Given a direction constraint (outbound or inbound), when the match runs, then only neighbors in that direction are returned.
- [ ] **US-071-AC3** — Given a link-type filter, when the match runs, then only neighbors connected by that type are returned.
- [ ] **US-071-AC4** — Given an entity with fewer than 100 links, when the neighbor query runs, then results return in under 20 ms p99.

## Edge Cases

- **Isolated entity**: returns empty, not an error.
- **Neighbors hidden by policy**: excluded from results without leaking existence (FEAT-029 / QRY-15..16).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| All neighbors | US-071-AC1 | B→A, A→C | `MATCH (a {id:$id})-[r]-(b) RETURN type(r), b` | B and C with types |
| Outbound only | US-071-AC2 | Same | Outbound-constrained match | C only |
| Type filter | US-071-AC3 | A→C `depends-on`, A→D `authored-by` | Filter `depends-on` | C only |
| Latency | US-071-AC4 | Entity with 80 links | Timed query | < 20 ms p99 |

## Dependencies

- **Stories**: US-018 (links exist)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-01, QRY-13
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (match semantics, direction/type filters), FEAT-007 (link model), FEAT-029 (visibility)

## Out of Scope

- Multi-hop exploration (US-023, US-072).
- Neighbor counts as schema metadata.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
