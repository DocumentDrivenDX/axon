---
ddx:
  id: US-077
---

# US-077: Subscribe to a Named Query

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-12, QRY-15
**PRD Requirements**: FR-3, FR-20
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder powering a live admin view (and the DDx server)
**I want** to subscribe to the result of a named query
**So that** consumers see result changes without polling

## Context

Live views and worker queues need push, not poll. This story exercises
QRY-12: named-query subscriptions over the GraphQL subscription path
(FEAT-015), re-evaluated when underlying entities or links change, with
policy filtering per subscriber (QRY-15). Subscription transport and shapes
are normative in CONTRACT-002.

## Walkthrough

1. Wei's client opens a subscription on an activated named query (per CONTRACT-002).
2. The system delivers an initial snapshot of the current result set.
3. A relevant entity or link mutation commits; the system re-evaluates and pushes an update.
4. The client disconnects; the subscription tears down with no leaked watchers.

## Acceptance Criteria

- [ ] **US-077-AC1** — Given a new subscription on a named query, when it is established, then an initial snapshot of the current result set is delivered first.
- [ ] **US-077-AC2** — Given an active subscription, when an entity or link change affects the query's result set, then an update is delivered without client polling.
- [ ] **US-077-AC3** — Given a change that does not affect the result set, when it commits, then no spurious update is delivered.
- [ ] **US-077-AC4** — Given subscribers with different identities, when updates are delivered, then each subscriber's stream is policy-filtered for its own identity (rows it cannot see never appear).
- [ ] **US-077-AC5** — Given a client disconnect, when the connection drops, then the subscription tears down cleanly with no leaked watchers or continued evaluation for that subscriber.

## Edge Cases

- **Schema change deactivating the named query mid-subscription**: the subscription terminates with a documented error, not silence.
- **Burst of mutations**: updates may coalesce, but the final delivered state matches the final result set.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Snapshot first | US-077-AC1 | `ready_beads` returns 3 beads | Subscribe | Initial delivery of the 3 beads |
| Live update | US-077-AC2 | Subscription open | Close a blocking dependency | Update with newly ready bead |
| No spurious push | US-077-AC3 | Subscription open | Mutate an unrelated collection | No update delivered |
| Policy filter | US-077-AC4 | Subscriber B cannot see bead-X | bead-X becomes ready | A's stream shows it; B's does not |
| Teardown | US-077-AC5 | Subscription open | Drop connection | Watcher count returns to baseline |

## Dependencies

- **Stories**: US-075 (named query exists), US-074 (primary consumer pattern)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-12, QRY-15
- **PRD Requirements**: FR-3, FR-20
- **External**: CONTRACT-002 (subscription transport and shapes), FEAT-015 (GraphQL subscriptions), FEAT-021 (change-feed pipeline driving re-evaluation), FEAT-029 (policy)

## Out of Scope

- Subscriptions on ad-hoc queries (V2).
- Durable delivery with replay cursors (FEAT-021 CDC).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
