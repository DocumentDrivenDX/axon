---
ddx:
  id: US-055
  review:
    self_hash: 5eeeb476a17ea61bb9d33642a36a01bd66c6086dd0f9a1deb401b735b71a9f19
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-055: Agent Subscribes to Changes via MCP

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-08, MCP-10
**PRD Requirements**: FR-21, FR-31
**Priority**: P1
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority) monitoring a
collection
**I want** to be notified when entities change
**So that** I can react to state changes without polling

## Context

Agents coordinating long-running work need change notification with resume
semantics through the standard MCP resource-subscription mechanism. This
story exercises MCP-08 and MCP-10; the URI grammar (tenant-aware four-level
form) and notification semantics are normative in CONTRACT-003.

## Walkthrough

1. The agent subscribes to a collection resource (URI per CONTRACT-003).
2. Another client mutates entities in that collection.
3. The agent receives a resource-updated notification per mutation, each
   carrying an audit cursor.
4. The agent re-reads the resource to get the new state, and after a
   disconnect uses the cursor with the audit resource to catch up.

## Acceptance Criteria

- [ ] **US-055-AC1** — Given a subscription to a collection resource, when
  an entity in the collection is created, updated, or deleted, then the
  agent receives a resource-updated notification.
- [ ] **US-055-AC2** — Given a received notification, when the agent
  inspects it, then it includes the audit cursor needed to resume through
  the audit resource after reconnect.
- [ ] **US-055-AC3** — Given a notification, when the agent re-reads the
  subscribed resource, then it observes the new state.
- [ ] **US-055-AC4** — Given multiple subscriptions, when changes occur,
  then each subscription delivers independently.
- [ ] **US-055-AC5** — Given a subscription to a single entity resource,
  when that entity is deleted, then the agent receives a resource-updated
  notification.
- [ ] **US-055-AC6** — Given an active subscription, when the collection's
  schema changes, then tool definitions update but the subscription
  continues uninterrupted.

## Edge Cases

- **Agent disconnects**: The subscription is cleaned up server-side; no
  dangling pollers.
- **Subscribed collection dropped**: The agent's next interaction returns
  not-found; a list-changed notification is sent.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Collection notification | US-055-AC1 | Subscription on `beads` collection URI | Create a bead elsewhere | resource-updated received |
| Cursor resume | US-055-AC2 | Notification N received, then disconnect | Read audit resource after cursor N | Changes after N returned; no gap |
| Re-read state | US-055-AC3 | Notification received | Re-read resource | New entity state present |
| Entity deletion | US-055-AC5 | Subscription on entity URI | Delete the entity | resource-updated received |
| Schema survival | US-055-AC6 | Active subscription | Put a new schema version | Subscription still delivering afterward |

## Dependencies

- **Stories**: US-052 (resource template discovery)
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-08, MCP-10
- **PRD Requirements**: FR-21, FR-31
- **External**: CONTRACT-003 (URI grammar, notifications), CONTRACT-005
  (audit cursors)

## Out of Scope

- GraphQL subscriptions (US-050).
- External CDC feeds (FEAT-021).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
