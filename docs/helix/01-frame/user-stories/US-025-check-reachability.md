---
ddx:
  id: US-025
  review:
    self_hash: 9b4894fb3524997a1bc34c67ff97d57f3a64c2af7451dbe21afc1b3b089b965e
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-025: Check Reachability

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-01, QRY-13, QRY-16
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder on a project management tool
**I want** to check whether issue A is transitively blocked by issue B
**So that** I can warn users about hidden dependencies without materializing paths

## Context

Reachability is a boolean question; materializing every path to answer it is
wasteful. This story exercises existence-check evaluation (QRY-01), index
probing with short-circuit (QRY-13), and policy-aware existence (QRY-16 —
hidden records must not leak through a true/false answer).

## Walkthrough

1. Wei's tool submits an existence check: does a bounded path of `BLOCKS` or `DEPENDS_ON` links lead from issue A to issue B? (language per CONTRACT-007)
2. The planner probes link indexes and short-circuits on the first path found.
3. The tool receives a boolean answer without any path payload.
4. The tool warns the user when the answer is true.

## Acceptance Criteria

- [ ] **US-025-AC1** — Given a transitive chain from A to B, when the existence check runs, then it returns true without materializing the path.
- [ ] **US-025-AC2** — Given multiple paths from A to B, when the existence check runs, then evaluation short-circuits on the first path found.
- [ ] **US-025-AC3** — Given a pattern alternating two link types, when the existence check runs, then paths through either link type satisfy it.
- [ ] **US-025-AC4** — Given the connecting path runs through a record hidden from the caller by policy, when the existence check runs, then the answer does not reveal the hidden record's existence (per FEAT-029 / QRY-16).

## Edge Cases

- **No path**: returns false, not an error.
- **A equals B**: a zero-length path does not satisfy a `*1..N` bound; result is false unless a real cycle exists.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Reachable | US-025-AC1 | A→X→B via `DEPENDS_ON` | `RETURN EXISTS { MATCH (a {id:$a})-[:BLOCKS\|DEPENDS_ON*1..10]->(b {id:$b}) }` | `true`, no path payload |
| Short-circuit | US-025-AC2 | 1K paths A→B | Same check, timed | Latency consistent with first-hit, not full enumeration |
| Alternation | US-025-AC3 | A-BLOCKS→X-DEPENDS_ON→B | Same check | `true` |
| Policy-safe | US-025-AC4 | Connecting node hidden from caller | Same check as caller | Leak-safe answer per policy contract |

## Dependencies

- **Stories**: US-018 (links exist)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-01, QRY-13, QRY-16
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (existence semantics, alternation, limits), FEAT-029 (policy visibility rules)

## Out of Scope

- Returning the witnessing path (use a traversal query, US-023).
- Shortest-path queries (V2).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
