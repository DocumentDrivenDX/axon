---
ddx:
  id: FEAT-021
  depends_on:
    - helix.prd
  review:
    self_hash: 6165a271de0b5e5c978f97ab9393596e651a680c51db80153fb85167ed93d993
    deps:
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Specification: FEAT-021 — Change Feeds (CDC)

**Feature ID**: FEAT-021
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Audit, Change Capture, and Repair; API and Deployment Surfaces
**Covered PRD Requirements**: FR-18, FR-31
**Cross-Subsystem Rationale**: The cross-subsystem workflow is the feature:
FR-18 (ordered change-feed records derived from audit) and FR-31 (ordered
streams with stable cursors, replay, and resume exposed as a public surface)
describe one capability — a durable, replayable projection of the audit log
delivered to external consumers. Splitting the projection from its
replay/resume surface would leave neither part shippable.
**FR Prefix**: CDC

## Overview

Durable, replayable change feeds that emit Debezium-compatible CDC records
for every committed entity and link mutation (PRD FR-18). A
Confluent-compatible schema registry facade serves entity schemas to
downstream consumers. The change feed is a projection of the audit log —
the same data that powers GraphQL subscriptions, formatted for the
Kafka/Debezium ecosystem — and supplies Axon's stable replay/resume
interface (PRD FR-31): every emitted event carries a durable audit cursor
and enough scope metadata for consumers to resume or replay by database,
collection, entity/link, or transaction.

The normative event envelope, tenant-aware topic naming and event keys,
registry endpoints, cursor semantics, and `[cdc.*]` configuration keys are
defined in
[CONTRACT-006 — CDC envelope](../../02-design/contracts/CONTRACT-006-cdc-envelope.md).
See [ADR-014](../../02-design/adr/ADR-014-change-feeds-debezium-cdc.md) for
the design rationale.

## Ideal Future State

A data engineer points existing Debezium-aware tooling at Axon and consumes
ordered, schema-described change events without writing custom integration
code. New consumers bootstrap from a consistent snapshot, then follow live
changes from the snapshot boundary. Any consumer can resume after a crash
from its last cursor, or replay a scoped slice of history (one database,
one collection, one entity, one transaction) at any time. Teams without
Kafka get the same events through file or HTTP streaming sinks. The same
cursor vocabulary works across CDC sinks, GraphQL subscriptions, MCP
notifications, and SDK change readers, so a position obtained on one
surface is meaningful on another.

## Problem Statement

- **Current situation**: GraphQL subscriptions (FEAT-015) provide
  ephemeral, real-time push to connected clients only; mutation history is
  queryable solely through Axon's own APIs.
- **Pain points**: Data pipelines, analytics engines (DuckDB, niflheim),
  search indexes, and replication targets need a feed that survives client
  disconnections, supports replay from a point in time, uses a standard
  format existing tooling understands, and provides schema metadata for
  consumer code generation. None of that exists, so every downstream
  integration is bespoke.
- **Desired outcome**: Every committed mutation is observable as an
  ordered, at-least-once, replayable change event in a standard envelope,
  with schema discovery, in environments with or without Kafka.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Change event emission | "Tell me about every committed mutation, in order" | Emit one standard-envelope event per entity/link mutation with ordering and transaction correlation |
| Snapshot and replay | "Bootstrap or rebuild my downstream view without gaps" | Consistent initial snapshots, a defined snapshot/live boundary, and scoped replay from any cursor |
| Schema discovery | "Generate typed consumer code and validate messages" | Registry facade serving entity schemas with compatibility checking |
| Sinks, cursors, and delivery | "Consume changes in my environment and resume after failure" | Kafka, file, and HTTP streaming sinks with persistent per-sink cursors and at-least-once delivery |

## Requirements

### Functional Requirements by Area

#### Change Event Emission

- **CDC-01**. Every committed entity mutation MUST produce exactly one
  change event in the Debezium-compatible envelope defined in CONTRACT-006,
  with operation, pre-image, post-image, source scope, and audit cursor
  populated per that contract.
- **CDC-02**. Link create and delete operations MUST be emitted as change
  events carrying source/target collection, IDs, link type, and metadata,
  on the per-source-collection link topic defined in CONTRACT-006.
- **CDC-03**. Delete events MUST be followed by a null-value tombstone, per
  CONTRACT-006, so log-compacted consumers converge.
- **CDC-04**. Events for the same entity MUST be delivered in mutation
  order; event keying that guarantees per-entity ordering is defined in
  CONTRACT-006. Topic names and keys are tenant-aware per CONTRACT-006.
- **CDC-05**. All events produced by one transaction MUST share the same
  transaction identifier so consumers can correlate cross-collection
  atomicity.
- **CDC-06**. Collection lifecycle events (create, drop) MUST be emitted on
  the system topic defined in CONTRACT-006.

#### Initial Snapshot and Replay

- **CDC-07**. When CDC is enabled, or a new collection is created with CDC
  active, all existing entities MUST be emitted as snapshot-read events in
  entity-ID order.
- **CDC-08**. A snapshot interrupted by a crash MUST resume from the last
  emitted entity rather than restarting.
- **CDC-09**. A snapshot MUST capture a consistent point: live events begin
  from the snapshot boundary cursor, with no gap and no reordering across
  the boundary.
- **CDC-10**. Consumers MUST be able to replay from any audit cursor or
  from an opaque cursor token returned by a prior event, scoped by
  database, collection, entity/link, or transaction, without any change to
  event envelope semantics. Cursor token format and validity rules are
  defined in CONTRACT-006.

#### Schema Discovery

- **CDC-11**. Axon MUST serve entity schemas to consumers through the
  Confluent-compatible registry endpoints defined in CONTRACT-006, in
  Axon's native JSON Schema form.
- **CDC-12**. The registry MUST be a facade over Axon's stored schema
  versions — no separate schema store — and schema IDs MUST be stable
  across restarts.
- **CDC-13**. Registry compatibility checking MUST map to Axon's schema
  evolution classification (FEAT-017): a change Axon classifies as breaking
  fails the corresponding registry compatibility mode.

#### Sinks, Cursors, and Delivery

- **CDC-14**. Kafka MUST be supported as the primary production sink, but
  MUST NOT be required: file and HTTP streaming (SSE) sinks deliver the
  same feed without Kafka infrastructure.
- **CDC-15**. All sinks MUST emit the same event envelope and the same
  cursor fields; the HTTP streaming sink shares delivery semantics with
  GraphQL subscriptions (FEAT-015).
- **CDC-16**. Delivery MUST be at-least-once: each sink persists its last
  emitted cursor per collection, resumes from it on restart, and may
  re-emit events after the cursor. Consumers deduplicate using the audit
  cursor field defined in CONTRACT-006.
- **CDC-17**. GraphQL subscriptions, MCP resource notifications, SDK change
  readers, and CDC sinks MUST use the same audit cursor vocabulary
  (PRD FR-31 audit parity).
- **CDC-18**. CDC and registry behavior MUST be configurable through the
  `[cdc.*]` configuration keys defined in CONTRACT-006 (sink selection,
  connection settings, batching, topic template, registry port).
- **CDC-19**. When a sink cannot accept events (e.g., producer buffer
  full), CDC MUST pause tailing for that sink and catch up later; entity
  writes MUST never be blocked or failed by CDC backpressure.

### Non-Functional Requirements

- **Latency**: < 1 s from entity write to event availability on the sink
  (p99, single node).
- **Throughput**: sustain 10K events/second to Kafka with batching.
- **Durability**: no event loss while Axon and the sink are both healthy;
  the audit log is the buffer, and CDC catches up after transient sink
  unavailability.
- **Reliability**: entity write paths are never blocked by sink failures
  or backpressure (CDC-19).
- **Schema registry latency**: < 10 ms for cached schema lookups.

## User Stories

- [US-130 — Emit CDC Events to Kafka](../user-stories/US-130-emit-cdc-events-to-kafka.md)
- [US-132 — Replay Events from a Point in Time](../user-stories/US-132-replay-events-from-a-point-in-time.md)
- [US-135 — Discover Entity Schemas via Registry](../user-stories/US-135-discover-entity-schemas-via-registry.md)
- [US-137 — Stream Changes Without Kafka](../user-stories/US-137-stream-changes-without-kafka.md)
- [US-139 — Link Events in CDC](../user-stories/US-139-link-events-in-cdc.md)

## Edge Cases and Error Handling

- **Sink unavailable (e.g., Kafka down)**: CDC pauses; the audit log
  accumulates. When the sink recovers, CDC catches up from the stored
  cursor. Entity writes are never blocked.
- **Schema change during CDC**: new events use the new schema version; the
  registry serves both old and new versions, so consumers pinned to the
  old schema keep working for backward-compatible changes.
- **Collection dropped**: final delete events for all entities, then a
  collection-drop event on the system topic. Topics are not auto-deleted;
  retention policy handles cleanup.
- **Very large snapshot (1M+ entities)**: the snapshot batches and yields
  between batches so live CDC for other collections is not starved;
  progress is observable via metrics.
- **Duplicate events**: at-least-once delivery means events may be
  re-emitted after producer crash recovery; consumers deduplicate by audit
  cursor.
- **Transaction spanning multiple collections**: events land on different
  topics but share the transaction identifier; consumers needing
  transactional atomicity correlate by it.

## Success Metrics

- A Debezium-aware consumer (e.g., a Kafka Connect sink) processes Axon
  change events with zero custom envelope-translation code.
- A new consumer bootstraps a complete downstream replica (snapshot + live
  tail) with zero missed or out-of-order per-entity events.
- Replay of a scoped slice (one collection, one transaction) produces
  exactly the audit-recorded mutations for that scope.
- End-to-end change latency meets the < 1 s p99 target under the 10K
  events/second throughput load.

## Constraints and Assumptions

- The audit log (FEAT-003, ADR-003) is the single source of truth; CDC is
  a projection and never invents events the audit log does not contain.
- At-least-once delivery with consumer-side deduplication is acceptable
  for V1; exactly-once is out of scope.
- Consumers are assumed to tolerate envelope-compatible additive fields,
  consistent with CONTRACT-006 compatibility rules.

## Dependencies

- **Other features**:
  - FEAT-003 (Audit Log) — CDC is a projection of the audit log.
  - FEAT-015 (GraphQL) — shared streaming/SSE delivery semantics and
    cursor parity with subscriptions.
  - FEAT-017 (Schema Evolution) — registry compatibility maps to breaking
    change classification.
- **External services**: Kafka-compatible brokers (optional), Confluent
  registry-compatible clients; exact surface in CONTRACT-006.
- **PRD requirements**: FR-18 (P1), FR-31 (P1).

## Out of Scope

- **Exactly-once delivery** (Kafka transactional producer): at-least-once
  with consumer dedup is sufficient for V1.
- **Per-field filtering** (emit only when specific fields change).
- **Additional transports** (NATS, Pulsar, Redis Streams) beyond
  Kafka + SSE + file.
- **Downstream schema replication**: pushing schemas into external
  registries.
- **CDC admin UI**: managing CDC configuration through the web UI.
