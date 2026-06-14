---
ddx:
  id: US-070
  review:
    self_hash: e0d887e39e72fafa4e59dbae9b67b1a14f18ac4fcd0532231e782c31d3f93c15
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-070: Find Link Targets

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-04, QRY-05, QRY-08, QRY-13
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose agent is creating a dependency link
**I want** the agent to discover which entities it can link to, with search and an already-linked indicator
**So that** it picks the right target without fetching the entire collection

## Context

Inherited from the retired FEAT-020. Before creating a link, an agent needs
candidates from the link type's target collection, filtered by search text,
annotated with whether a link already exists. This story exercises named
queries (QRY-04, QRY-05), their generated surfaces (QRY-08), and index-backed
planning (QRY-13).

## Walkthrough

1. Ava declares a link-candidates named query taking source ID, link type, search text, and limit (declaration grammar per CONTRACT-007).
2. Her agent invokes the generated surface with a search string.
3. The system returns candidates from the target collection, each annotated with an already-linked indicator computed via optional matching against the source's existing links.
4. The agent presents candidates and creates the link (US-018).

## Acceptance Criteria

- [ ] **US-070-AC1** — Given a declared link-candidates named query, when invoked with a source ID and link type, then entities from that link type's target collection are returned.
- [ ] **US-070-AC2** — Given search text and filters, when the query runs, then matching combines in one index-backed match (no client-side filtering).
- [ ] **US-070-AC3** — Given the source already links to some candidates, when the query runs, then each row carries an already-linked indicator derived via optional matching.
- [ ] **US-070-AC4** — Given the link type's cardinality declaration, when clients need it, then it is available as schema metadata (CONTRACT-010), not computed in the query result.
- [ ] **US-070-AC5** — Given a 10K-entity target collection with an indexed predicate, when the query runs, then results return in under 50 ms p99.

## Edge Cases

- **Empty target collection**: returns empty rows, not an error.
- **Search with no matches**: empty result set.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Candidates | US-070-AC1 | `depends-on` targets `beads` | Invoke `link_candidates(source, 'depends-on', '', 20)` | Bead entities returned |
| Search | US-070-AC2 | Beads titled "auth", "ui" | Search "auth" | Only "auth" bead |
| Already linked | US-070-AC3 | Source already links bead-B | Invoke query | bead-B row flagged linked |
| Latency | US-070-AC5 | 10K beads, indexed title | Timed invoke | < 50 ms p99 |

## Dependencies

- **Stories**: US-075 (named-query declaration), US-018 (link creation follows)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-04, QRY-05, QRY-08, QRY-13
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (named-query grammar, planning), CONTRACT-010 (link-type cardinality metadata), CONTRACT-002/003 (generated surfaces), FEAT-013 (indexes)

## Out of Scope

- Creating the link itself (US-018, FEAT-007).
- Full-text relevance ranking (P2 search features).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
