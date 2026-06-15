---
ddx:
  id: US-130
  review:
    self_hash: d13c4cc5090c5277d6b541e611e00f8afc0c4336cc87cd692edbda82d1e5ec34
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-130: Emit CDC Events to Kafka

**Feature**: FEAT-021 — Change Feeds (CDC)
**Feature Requirements**: CDC-01, CDC-03, CDC-04, CDC-05, CDC-18
**PRD Requirements**: FR-18
**Priority**: P1
**Status**: Draft

## Story

**As a** data engineer building a pipeline
**I want** Axon changes emitted as Debezium-compatible records on Kafka topics
**So that** my existing Kafka consumers can process Axon data without custom integration

## Context

Renumbered from US-074 (collision with FEAT-009). Downstream pipelines,
analytics engines, and replication targets already speak the
Kafka/Debezium ecosystem. This story exercises the core emission path of
FEAT-021 (CDC-01, CDC-03..05) plus sink configuration (CDC-18): every
committed mutation becomes one standard-envelope event, ordered per
entity, correlated per transaction. The envelope, tenant-aware topic
naming, keys, and `[cdc.*]` configuration keys are normative in
CONTRACT-006.

## Walkthrough

1. The data engineer enables the Kafka sink via the CDC configuration
   keys (CONTRACT-006) and restarts Axon.
2. An agent creates an invoice entity; Axon commits the write and the
   audit record.
3. The CDC producer tails the audit log and publishes a create event in
   the Debezium-compatible envelope on the collection's tenant-aware
   topic.
4. The agent updates, then deletes the invoice; the consumer observes an
   update event with pre- and post-images, then a delete event followed
   by a tombstone, all in order on the same partition.
5. The engineer's existing Debezium-aware consumer processes the events
   with no Axon-specific translation code.

## Acceptance Criteria

- [ ] **US-130-AC1** — Given the Kafka sink is enabled, when an entity is
      created, then a create-operation event in the CONTRACT-006 envelope
      is published on the collection's topic.
- [ ] **US-130-AC2** — Given an existing entity, when it is updated, then
      the emitted event carries the prior state as pre-image and the new
      state as post-image.
- [ ] **US-130-AC3** — Given an existing entity, when it is deleted, then
      a delete-operation event is emitted followed by a null-value
      tombstone.
- [ ] **US-130-AC4** — Given multiple mutations of the same entity, when
      they are published, then all events for that entity land on the same
      partition in mutation order (CONTRACT-006 event keying).
- [ ] **US-130-AC5** — Given any emitted event, when a consumer reads it,
      then it carries the audit cursor field defined in CONTRACT-006 for
      consumer offset tracking.
- [ ] **US-130-AC6** — Given a multi-entity transaction, when its events
      are emitted, then every event shares the same transaction
      identifier.
- [ ] **US-130-AC7** — Given a server configuration, when the operator
      sets the CDC Kafka keys defined in CONTRACT-006, then the sink uses
      the configured brokers, batching, and topic template.

## Edge Cases

- **Kafka unavailable at write time**: the entity write commits normally;
  CDC pauses and catches up from the stored cursor when Kafka recovers.
- **Producer crash mid-batch**: events after the persisted cursor are
  re-emitted (at-least-once); consumers deduplicate by audit cursor.
- **Transaction spanning collections**: events land on different topics
  but share the transaction identifier.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create event | US-130-AC1 | Kafka sink enabled; empty `invoices` collection | Create invoice `INV-1` | One create-op envelope on the `invoices` topic with null pre-image |
| Update pre/post images | US-130-AC2 | `INV-1` at version 1 with `status=draft` | Update `status=approved` | Event pre-image has `draft`, post-image has `approved` |
| Delete + tombstone | US-130-AC3 | `INV-1` exists | Delete `INV-1` | Delete-op event, then tombstone with same key |
| Ordering | US-130-AC4 | `INV-1` mutated 5 times | Consume the topic | 5 events on one partition in version order |
| Transaction correlation | US-130-AC6 | Transaction touching `INV-1` and `INV-2` | Commit transaction | Both events carry the same transaction ID |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-021
- **Feature Requirements**: CDC-01, CDC-03, CDC-04, CDC-05, CDC-18
- **PRD Requirements**: FR-18
- **External**: CONTRACT-006 (envelope, topics, keys, `[cdc.*]` config);
  Kafka-compatible broker

## Out of Scope

- Snapshot and replay behavior (US-132); non-Kafka sinks (US-137); link
  events (US-139); exactly-once delivery (FEAT-021 Out of Scope).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
