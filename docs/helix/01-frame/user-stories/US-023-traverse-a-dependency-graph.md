---
ddx:
  id: US-023
  review:
    self_hash: 8f95484229d01eede9128e4c7b787e6aff5a2c3014b479c38366f37c45636fdd
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-023: Traverse a Dependency Graph

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-01, QRY-02, QRY-13
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose agents manage a work queue
**I want** my agents to find all transitive dependencies of a bead in one query
**So that** readiness decisions are correct and don't require client-side graph walking

## Context

Readiness is a transitive property: a bead is blocked by anything anywhere
below it. This story exercises bounded variable-length path traversal with
per-hop filtering (QRY-01), schema-checked references (QRY-02), and
link-index-backed planning (QRY-13). Language and limits are normative in
CONTRACT-007.

## Walkthrough

1. Ava's agent submits a bounded variable-length traversal from a bead along `DEPENDS_ON` links (language per CONTRACT-007).
2. The planner expands hops via link indexes and returns every transitive dependency.
3. The agent adds a per-hop predicate to keep only incomplete dependencies.
4. With path projection, the result also carries the dependency paths.

## Acceptance Criteria

- [ ] **US-023-AC1** — Given a bead with multi-level dependencies, when a bounded variable-length traversal (depth 1..10) runs from it, then all transitive dependencies are returned.
- [ ] **US-023-AC2** — Given path projection is requested, when the traversal runs, then each result row includes the path from root to dependency.
- [ ] **US-023-AC3** — Given a dependency cycle in the graph, when the traversal runs, then it terminates safely with correct results (no infinite loop, no timeout).
- [ ] **US-023-AC4** — Given a per-hop predicate (for example, status not done), when the traversal runs, then only dependencies satisfying the predicate are returned.

## Edge Cases

- **No outgoing links**: traversal returns empty, not an error.
- **Depth bound exceeded by the graph**: results are truncated at the bound; deeper nodes are simply absent (bounds per CONTRACT-007 §Limits).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Transitive set | US-023-AC1 | A→B→C, A→D | `MATCH (a:Bead {id:'A'})-[:DEPENDS_ON*1..10]->(d) RETURN d` | B, C, D |
| Path projection | US-023-AC2 | Same graph | `MATCH p = (a)-[:DEPENDS_ON*1..10]->(d) RETURN p` | Paths A→B, A→B→C, A→D |
| Cycle safety | US-023-AC3 | A→B→A cycle | Traverse from A | Terminates; B (and A via cycle rules) without loop |
| Hop filter | US-023-AC4 | C is `done`, B is `open` | `... WHERE d.status <> 'done' RETURN d` | B only |

## Dependencies

- **Stories**: US-018 (links exist)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-01, QRY-02, QRY-13
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (clause subset, depth limits, error codes), FEAT-007 (link model), FEAT-013 (index acceleration)

## Out of Scope

- Shortest-path or weighted traversal (V2; FEAT-009 Out of Scope).
- Ready/blocked queue composition (US-074).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
