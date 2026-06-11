---
ddx:
  id: US-139
---

# US-139: Link Events in CDC

**Feature**: FEAT-021 — Change Feeds (CDC)
**Feature Requirements**: CDC-02, CDC-04
**PRD Requirements**: FR-18
**Priority**: P1
**Status**: Draft

## Story

**As a** data engineer tracking entity relationships
**I want** link create/delete events in the change feed
**So that** downstream systems can maintain a replica of the entity graph

## Context

Renumbered from US-078 (collision with FEAT-015). Links are first-class
objects (PRD FR-2); a downstream replica that only sees entity events
cannot reconstruct the graph. This story exercises CDC-02: link create and
delete events on the per-source-collection link topic, ordered with the
source entity's events. Link topic naming is normative in CONTRACT-006.

## Walkthrough

1. The data engineer's consumer subscribes to a collection's link topic
   (CONTRACT-006).
2. An application creates an `INVOICED_BY` link from an invoice to a
   vendor; the consumer receives a create-operation link event carrying
   source/target collections and IDs, link type, and metadata.
3. The link is later deleted; the consumer receives a delete-operation
   event.
4. The downstream replica applies both and its graph matches Axon's.

## Acceptance Criteria

- [ ] **US-139-AC1** — Given a link is created, when CDC emits it, then a
      create-operation event appears on the source collection's link topic
      (CONTRACT-006).
- [ ] **US-139-AC2** — Given a link is deleted, when CDC emits it, then a
      delete-operation event appears on the same topic.
- [ ] **US-139-AC3** — Given any link event, when a consumer reads it,
      then it includes source and target collections and IDs, link type,
      and link metadata.
- [ ] **US-139-AC4** — Given multiple link mutations for the same source
      entity, when they are published, then they are ordered per source
      entity (same partitioning principle as entity events).

## Edge Cases

- **Link whose target entity is later deleted**: the link delete event and
  the entity delete event each appear on their own topics, correlated by
  transaction ID when deleted together.
- **Replay including links**: scoped replay (US-132) covers link events so
  a graph replica can be rebuilt from any cursor.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Link create | US-139-AC1 | Invoice `INV-1`, vendor `V-1` | Create `INVOICED_BY` link | Create-op event on `invoices` link topic |
| Link payload | US-139-AC3 | Link with metadata `{po: "PO-9"}` | Consume event | Event carries both endpoints, type `INVOICED_BY`, metadata |
| Per-source ordering | US-139-AC4 | 3 links created then 1 deleted on `INV-1` | Consume topic | 4 events in mutation order on one partition |

## Dependencies

- **Stories**: US-130 (envelope/emission semantics)
- **Feature Spec**: FEAT-021
- **Feature Requirements**: CDC-02, CDC-04
- **PRD Requirements**: FR-18 (and FR-2 link model)
- **External**: CONTRACT-006 (link topic naming, envelope)

## Out of Scope

- Entity events (US-130); graph traversal queries (FEAT-009); link schema
  declaration (FEAT-007).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
