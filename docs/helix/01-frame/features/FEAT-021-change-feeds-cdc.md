---
dun:
  id: FEAT-021
  depends_on:
    - helix.prd
    - FEAT-003
    - FEAT-015
    - ADR-003
    - ADR-014
---
# Feature Specification: FEAT-021 - Change Feeds (Debezium CDC)

**Feature ID**: FEAT-021
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Durable, replayable change feeds that emit Debezium-compatible CDC
records to Kafka topics. A Confluent-compatible Schema Registry serves
entity schemas to downstream consumers. The change feed is a projection
of the audit log — the same data that powers GraphQL subscriptions,
formatted for the Kafka/Debezium ecosystem.

See [ADR-014](../../02-design/adr/ADR-014-change-feeds-debezium-cdc.md)
for the full design.

## Problem Statement

GraphQL subscriptions (FEAT-015) provide ephemeral, real-time push to
connected clients. But data pipelines, analytics engines (DuckDB,
niflheim), search indexes, and replication targets need a durable feed
that:

- Survives client disconnections
- Supports replay from any point in time
- Uses a standard format that existing tooling understands
- Provides schema metadata for consumer code generation

## Requirements

### Functional Requirements

#### Debezium-Compatible Events

- **Envelope format**: Every mutation produces a Debezium envelope with
  `before`, `after`, `source`, `op`, and `ts_ms` fields
- **Operation types**: `c` (create), `u` (update/patch), `d` (delete),
  `r` (snapshot read)
- **Source metadata**: Axon version, instance name, database, schema,
  collection, entity ID, audit log sequence number, transaction ID
- **Tombstone on delete**: Delete events followed by a null-value
  tombstone for Kafka log compaction
- **Link events**: Link create/delete as Debezium events on a per-source-collection `.__links__` topic

#### Kafka Integration

- **One topic per collection**: `{instance}.{db}.{schema}.{collection}`
- **System topic**: `{instance}.__system__` for collection lifecycle events
- **Event key**: `{db, schema, collection, id}` — ensures per-entity
  ordering within a partition
- **Configurable**: Bootstrap servers, batch size, linger, acks, topic template
- **Optional**: Kafka is not required — file and SSE sinks work without it

#### Schema Registry

- **Confluent-compatible REST API**: ~15 endpoints for subject listing,
  schema registration, version management, compatibility checking
- **JSON Schema format**: Entity schemas served as JSON Schema (Axon's
  native format)
- **Facade over Axon schemas**: No separate schema store. The registry
  reads from Axon's `schema_versions` table
- **Compatibility modes**: BACKWARD, FORWARD, FULL, NONE — mapped to
  Axon's schema evolution classification (FEAT-017)
- **Standard port**: Configurable, default 8081 (Confluent convention)

#### Initial Snapshot

- **Bootstrap new consumers**: When CDC is enabled or a new collection
  is created, emit all existing entities as `op: "r"` events
- **Resumable**: Snapshot interrupted by crash resumes from last emitted
  entity ID
- **Ordered**: Entities emitted in ID order within the snapshot
- **Boundary**: Snapshot captures a consistent point; live events begin
  from the snapshot boundary `audit_id`

#### Multi-Sink Support

- **Kafka**: Primary production sink (rdkafka)
- **HTTP SSE**: Shared implementation with GraphQL subscriptions
- **File**: JSONL files with configurable rotation (debugging, replay,
  air-gapped environments)
- **Pluggable**: Sink trait enables future transports (NATS, Pulsar,
  Redis Streams)

#### Cursor Management

- **Persistent cursor**: Last emitted `audit_id` per sink stored in
  `_cdc_cursors` table
- **Resume on restart**: Producer resumes from stored cursor, re-emitting
  any events after the cursor (at-least-once)
- **Per-collection cursors**: Independent progress tracking per collection

### Non-Functional Requirements

- **Latency**: < 1s from entity write to Kafka message availability
  (p99, single-node)
- **Throughput**: Sustain 10K events/second to Kafka with batching
- **Durability**: No event loss if Axon and Kafka are both healthy.
  Audit log is the buffer; CDC catches up after transient Kafka
  unavailability
- **Backpressure**: If Kafka producer buffer is full, CDC pauses tailing.
  Entity writes are never blocked
- **Schema Registry latency**: < 10ms for schema lookups (cached)

## User Stories

### Story US-074: Emit CDC Events to Kafka [FEAT-021]

**As a** data engineer building a pipeline
**I want** Axon changes emitted as Debezium records on Kafka topics
**So that** my existing Kafka consumers can process Axon data without custom integration

**Acceptance Criteria:**
- [ ] Creating an entity produces a Debezium `op: "c"` event on the collection's topic
- [ ] Updating an entity produces `op: "u"` with `before` (old state) and `after` (new state)
- [ ] Deleting an entity produces `op: "d"` followed by a tombstone
- [ ] Events for the same entity land on the same Kafka partition (key-based partitioning)
- [ ] Events include `source.audit_id` for consumer cursor tracking
- [ ] Events in a transaction share the same `source.transaction_id`
- [ ] CDC is configurable via `[cdc.kafka]` in server config

### Story US-075: Replay Events from a Point in Time [FEAT-021]

**As a** data engineer bootstrapping a new consumer
**I want** to replay all events from a specific audit ID
**So that** I can build a complete downstream view without missing data

**Acceptance Criteria:**
- [ ] Initial snapshot emits all existing entities as `op: "r"` events
- [ ] After snapshot, live events begin from the snapshot boundary
- [ ] Consumer can request replay by resetting its cursor to any `audit_id`
- [ ] Snapshot is resumable after crash (resumes from last emitted entity)
- [ ] Re-emitted events (at-least-once) are deduplicated by consumers using `audit_id`

### Story US-076: Discover Entity Schemas via Registry [FEAT-021]

**As a** Kafka consumer developer
**I want** a Confluent-compatible Schema Registry serving Axon entity schemas
**So that** I can generate typed consumer code and validate message formats

**Acceptance Criteria:**
- [ ] `GET /subjects` returns all collection names as subjects
- [ ] `GET /subjects/{subject}/versions/latest` returns the current entity schema as JSON Schema
- [ ] Schema IDs are stable across registry restarts
- [ ] Registering a schema via POST (from a Kafka Connect sink, for example) maps to `put_schema`
- [ ] Compatibility check endpoint validates new schemas against existing versions
- [ ] Registry runs on configurable port (default 8081)

### Story US-077: Stream Changes Without Kafka [FEAT-021]

**As a** developer in a non-Kafka environment
**I want** CDC events written to files or streamed via HTTP
**So that** I can consume Axon changes without Kafka infrastructure

**Acceptance Criteria:**
- [ ] File sink writes Debezium JSONL with configurable rotation
- [ ] SSE sink streams events to HTTP clients (shared with GraphQL subscriptions)
- [ ] File and SSE sinks work independently of Kafka (Kafka can be disabled)
- [ ] All sinks emit the same Debezium envelope format
- [ ] Cursor management works for all sink types

### Story US-078: Link Events in CDC [FEAT-021]

**As a** data engineer tracking entity relationships
**I want** link create/delete events in the change feed
**So that** downstream systems can maintain a replica of the entity graph

**Acceptance Criteria:**
- [ ] Creating a link produces a Debezium `op: "c"` event on `{collection}.__links__` topic
- [ ] Deleting a link produces `op: "d"` event
- [ ] Link events include source/target collection, IDs, link type, and metadata
- [ ] Link events are ordered per-source-entity (same partition key as entity events)

## Edge Cases

- **Kafka unavailable**: CDC pauses. Audit log accumulates. When Kafka
  recovers, CDC catches up from cursor position. Entity writes are never
  blocked
- **Schema change during CDC**: New events use the new schema version.
  The schema registry serves both old and new versions. Consumers using
  the old schema continue working (backward compatibility)
- **Collection dropped**: Final events for all entities (`op: "d"`),
  then a collection-drop event on the system topic. Topic is not
  auto-deleted (Kafka retention handles cleanup)
- **Very large snapshot**: Collections with 1M+ entities. Snapshot
  batches and yields between batches to avoid blocking live CDC.
  Progress reported via metrics
- **Duplicate events**: At-least-once delivery means events may be
  duplicated on producer crash recovery. Events carry `audit_id` for
  consumer-side dedup
- **Transaction spanning multiple collections**: Events land on
  different topics but share `transaction_id`. Consumers that need
  transaction atomicity must correlate by `transaction_id`

## Dependencies

- **FEAT-003** (Audit Log): CDC is a projection of the audit log
- **FEAT-015** (GraphQL): Shares SSE infrastructure with subscriptions
- **FEAT-017** (Schema Evolution): Registry compatibility maps to
  Axon's breaking change detection
- **ADR-003**: Audit log as the source of truth for all mutations
- **ADR-014**: Full design for Debezium format, Kafka integration,
  schema registry

### Crate Dependencies

- `rdkafka` v0.39+ — Kafka producer
- `schema_registry_converter` — Registry client for Avro serialization

## Out of Scope

- **Exactly-once delivery**: Kafka transactional producer. Deferred —
  at-least-once with consumer dedup is sufficient for V1
- **Per-field filtering**: Emit events only when specific fields change.
  Deferred
- **NATS / Pulsar / Redis Streams**: Additional transports beyond
  Kafka + SSE + file. Deferred
- **Downstream schema replication**: Pushing schemas to external
  registries. Deferred
- **CDC admin UI**: Managing CDC configuration via the web UI. Deferred

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #2 (Change feeds)
- **User Stories**: US-074, US-075, US-076, US-077, US-078
- **Architecture**: ADR-014 (Change Feeds — Debezium CDC)
- **Implementation**: `crates/axon-cdc/`, `crates/axon-registry/`

### Feature Dependencies
- **Depends On**: FEAT-003, FEAT-015, FEAT-017
- **Depended By**: niflheim bridge (P2), external analytics consumers
