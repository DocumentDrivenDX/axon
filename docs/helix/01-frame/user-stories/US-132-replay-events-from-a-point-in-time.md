---
ddx:
  id: US-132
  review:
    self_hash: 9340fa95d85cfe5926076d1414cba2f4bceb71b53fd588269e1cd1b55de336e4
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-132: Replay Events from a Point in Time

**Feature**: FEAT-021 — Change Feeds (CDC)
**Feature Requirements**: CDC-07, CDC-08, CDC-09, CDC-10, CDC-16
**PRD Requirements**: FR-18, FR-31
**Priority**: P1
**Status**: Draft

## Story

**As a** data engineer bootstrapping a new consumer
**I want** to replay all events from a specific audit position
**So that** I can build a complete downstream view without missing data

## Context

Renumbered from US-075 (collision with FEAT-009). New downstream systems
must be able to start from nothing and converge on the full current state,
then follow live changes with no gap. This story exercises the snapshot
and replay area of FEAT-021 (CDC-07..10) and at-least-once cursor
semantics (CDC-16). Cursor token format and replay scoping rules are
normative in CONTRACT-006.

## Walkthrough

1. The data engineer enables CDC on a collection that already holds
   entities.
2. Axon emits a snapshot: every existing entity as a snapshot-read event,
   in entity-ID order.
3. Live mutation events begin from the snapshot boundary cursor with no
   gap.
4. Later, the engineer stands up a second consumer and resets its cursor
   to an earlier audit position scoped to one collection.
5. Axon re-delivers the scoped events from that position; the consumer
   deduplicates any overlap by audit cursor and converges.

## Acceptance Criteria

- [ ] **US-132-AC1** — Given a collection with existing entities, when CDC
      is enabled, then all existing entities are emitted as snapshot-read
      events in entity-ID order.
- [ ] **US-132-AC2** — Given a completed snapshot, when live mutations
      occur, then live events begin from the snapshot boundary cursor with
      no gap or reordering across the boundary.
- [ ] **US-132-AC3** — Given a consumer with a stored cursor, when it
      resets the cursor to any prior audit position, then events from that
      position onward are re-delivered.
- [ ] **US-132-AC4** — Given an opaque cursor token returned by a prior
      event (CONTRACT-006), when a consumer resumes with it, then delivery
      continues from that position even across producer restarts and
      schema changes.
- [ ] **US-132-AC5** — Given a replay request, when it is scoped by
      database, collection, entity/link, or transaction, then only events
      in that scope are delivered, with unchanged envelope semantics.
- [ ] **US-132-AC6** — Given a snapshot interrupted by a crash, when the
      producer restarts, then the snapshot resumes from the last emitted
      entity rather than restarting.
- [ ] **US-132-AC7** — Given at-least-once delivery, when events are
      re-emitted after crash recovery, then each carries the audit cursor
      so consumers can deduplicate.

## Edge Cases

- **Very large snapshot (1M+ entities)**: snapshot batches and yields so
  live CDC for other collections continues; progress observable via
  metrics.
- **Cursor for compacted/expired data**: replay from a position older than
  retained sink data is served from the audit log (the buffer), not the
  sink.
- **Replay during active writes**: replayed historical events and live
  events remain distinguishable and ordered by audit cursor.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Bootstrap snapshot | US-132-AC1 | Collection with 100 entities, CDC off | Enable CDC | 100 snapshot-read events in ID order |
| Boundary continuity | US-132-AC2 | Snapshot completes at boundary cursor B | Write entity | Live event has cursor > B; no missing positions |
| Cursor reset | US-132-AC3 | Consumer at cursor 500 | Reset to cursor 200 | Events from 200 onward re-delivered |
| Scoped replay | US-132-AC5 | History across 2 collections | Replay scoped to `invoices` | Only `invoices` events delivered |
| Crash mid-snapshot | US-132-AC6 | Snapshot crashed after entity 42 | Restart producer | Snapshot resumes at entity 43 |

## Dependencies

- **Stories**: US-130 (emission path)
- **Feature Spec**: FEAT-021
- **Feature Requirements**: CDC-07, CDC-08, CDC-09, CDC-10, CDC-16
- **PRD Requirements**: FR-18, FR-31
- **External**: CONTRACT-006 (cursor tokens, replay scoping, snapshot
  boundary semantics)

## Out of Scope

- Schema discovery (US-135); sink-specific transport behavior (US-130,
  US-137); exactly-once delivery.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
