---
ddx:
  id: US-050
  review:
    self_hash: 4cd672a9fe2eab97bf2002de5582c4946e4a6dfe085468d61f5ddcc1ad4901be
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-050: Subscribe to Entity Changes

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-18, GQL-19
**PRD Requirements**: FR-20, FR-31
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent built by Ava (agent application developer)
**I want** to receive notifications when entities I care about change
**So that** I can react to state changes without polling

## Context

Agents coordinating over shared records need push-based change notification
with replay semantics: after a disconnect they must resume from where they
left off rather than missing or double-processing changes. This story
exercises FEAT-015's subscription requirements (GQL-18, GQL-19). The
transport, subprotocol, and event shape are normative in CONTRACT-002.

## Walkthrough

1. The agent opens a change subscription for a collection with a filter.
2. Another client creates, updates, and deletes matching entities.
3. The agent receives an event per mutation carrying the mutation type,
   entity data, actor, and an audit cursor.
4. The agent disconnects, reconnects, and uses the last received cursor with
   the audit-log query to catch up before resuming the live stream.

## Acceptance Criteria

- [ ] **US-050-AC1** — Given an active per-collection change subscription
  (transport per CONTRACT-002), when matching entities are created, updated,
  or deleted, then the subscriber receives an event for each mutation.
- [ ] **US-050-AC2** — Given a subscription filter, when non-matching
  entities change, then no event is pushed for them.
- [ ] **US-050-AC3** — Given a received event, when the agent inspects it,
  then it includes the mutation type, new entity data, and actor.
- [ ] **US-050-AC4** — Given a received event, when the agent reconnects
  after a disconnect, then the event's audit cursor can be used with the
  audit-log query to resume without loss.
- [ ] **US-050-AC5** — Given multiple concurrent subscriptions, when changes
  occur, then each subscription delivers independently.
- [ ] **US-050-AC6** — Given an active subscription, when its collection is
  dropped, then the subscription closes with an error event.

## Edge Cases

- **Burst of mutations**: Events arrive in audit order; no event is silently
  dropped within the latency target.
- **Filter references a removed field after schema change**: The
  subscription surfaces an error event rather than silently matching
  nothing.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Live events | US-050-AC1 | Subscription on `beads` | Create, update, delete a bead | 3 events received in order |
| Filtered push | US-050-AC2 | Filter `status = blocked` | Update a non-blocked bead | No event delivered |
| Event payload | US-050-AC3 | Any matching mutation | Inspect event | Mutation type + entity data + actor present |
| Cursor resume | US-050-AC4 | Disconnect after event N | Query audit log after cursor N, resubscribe | Catch-up returns events N+1..; no gap or duplicate |
| Dropped collection | US-050-AC6 | Active subscription | Drop the collection | Error event, subscription closed |

## Dependencies

- **Stories**: US-048 (typed collections in place)
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-18, GQL-19
- **PRD Requirements**: FR-20, FR-31
- **External**: CONTRACT-002 (subscription transport and event shape),
  CONTRACT-005 (audit record/cursor)

## Out of Scope

- MCP resource subscriptions (US-055).
- External change feeds / CDC (FEAT-021).
- Named-query subscriptions (FEAT-009 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
