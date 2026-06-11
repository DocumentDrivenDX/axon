---
ddx:
  id: US-074
---

# US-074: Pattern Query for Ready/Blocked Queue

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-01, QRY-04, QRY-12, QRY-13
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer running DDx workers (consumer case axon-05c1019d)
**I want** to retrieve all ready beads — open, with no open dependencies — in one round-trip
**So that** worker pickup decisions don't dominate queue latency

## Context

The motivating consumer case for the Cypher unification: readiness is a
negative existence predicate over outgoing-link target state, inexpressible
in per-pattern read surfaces without two phases. This story exercises
negative existence patterns (QRY-01), named-query declaration (QRY-04),
subscriptions (QRY-12), and the DDx latency budgets. After it lands, DDx
drops its two-phase fallback.

## Walkthrough

1. Ava declares `ready_beads` (open beads with no non-closed `DEPENDS_ON` targets) and a complementary `blocked_beads` named query (grammar per CONTRACT-007).
2. A DDx worker invokes `ready_beads` and receives the full ready set in one round-trip.
3. The DDx server subscribes to `ready_beads` and receives updates as beads and links change.
4. Workers pick beads from the live set without polling.

## Acceptance Criteria

- [ ] **US-074-AC1** — Given open beads with mixed dependency states, when `ready_beads` runs, then exactly the open beads with no non-closed dependencies are returned in one round-trip.
- [ ] **US-074-AC2** — Given the same data, when `blocked_beads` runs, then it returns exactly the open beads excluded from `ready_beads`.
- [ ] **US-074-AC3** — Given 1K beads (≈500 open, varied dependency states), when `ready_beads` runs, then it completes in under 100 ms p99.
- [ ] **US-074-AC4** — Given 10K beads, when `ready_beads` runs, then it completes in under 500 ms p99.
- [ ] **US-074-AC5** — Given an active subscription on `ready_beads`, when a bead or dependency link changes the result set, then an update is delivered (per QRY-12).

## Edge Cases

- **Bead with a closed-only dependency chain**: ready (negative existence over non-closed targets).
- **Dependency cycle among open beads**: all participants are blocked; the query terminates correctly.
- **Empty queue**: returns empty rows, not an error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Ready set | US-074-AC1 | b1 open no deps; b2 open dep on closed; b3 open dep on open | `ready_beads` | b1, b2 |
| Blocked set | US-074-AC2 | Same | `blocked_beads` | b3 |
| 1K latency | US-074-AC3 | 1K beads seeded | Timed run | < 100 ms p99 |
| 10K latency | US-074-AC4 | 10K beads seeded | Timed run | < 500 ms p99 |
| Live update | US-074-AC5 | Subscription active; b3's dep closes | Commit the close | Update delivers b3 as ready |

## Dependencies

- **Stories**: US-075 (named-query machinery), US-077 (subscription machinery)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-01, QRY-04, QRY-12, QRY-13
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (negative existence, named-query grammar), CONTRACT-002 (subscription delivery), FEAT-006 (bead adapter consumer)

## Out of Scope

- Bead claim/assignment semantics (FEAT-006 / DDx server logic).
- Priority-based scheduling policy (consumer concern; ordering is just a query clause).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
