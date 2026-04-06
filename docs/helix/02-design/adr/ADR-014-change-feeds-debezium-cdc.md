---
dun:
  id: ADR-014
  depends_on:
    - ADR-003
    - ADR-010
    - ADR-012
    - FEAT-003
    - FEAT-015
---
# ADR-014: Change Feeds — Debezium-Compatible CDC with Kafka and Schema Registry

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | ADR-003, ADR-010, FEAT-003, FEAT-015 | High |

## Context

Axon produces a rich audit log on every mutation (ADR-003). GraphQL
subscriptions (ADR-012/FEAT-015) provide real-time push to connected
clients. But downstream systems — data pipelines, analytics engines,
search indexes, replication targets — need a durable, replayable change
feed that survives client disconnections and supports at-least-once
delivery.

The audit log *is* the change feed data. The question is how to expose
it to external consumers in a standard format over a durable transport.

| Aspect | Description |
|--------|-------------|
| Problem | No durable, replayable change feed for downstream consumers. GraphQL subscriptions are ephemeral (lost on disconnect). Audit log is queryable but not streamable |
| Current State | Audit log stored in database. GraphQL subscriptions poll audit log. No external integration |
| Requirements | Debezium-compatible CDC records on Kafka topics. Confluent-compatible schema registry. Multi-transport support (Kafka, HTTP SSE, file) |

## Decision

### 1. Debezium Envelope Format

All change events use the Debezium envelope format — the de facto
standard for CDC records. This gives Axon compatibility with the entire
Debezium/Kafka Connect ecosystem: consumers built for Debezium can
consume Axon change feeds without custom code.

#### Event Structure

```json
{
  "before": null,
  "after": {
    "id": "bead-42",
    "version": 1,
    "data": {
      "bead_type": "task",
      "status": "draft",
      "title": "Implement auth middleware",
      "priority": 2
    },
    "created_at": "2026-04-05T14:30:00Z",
    "created_by": "agent-1"
  },
  "source": {
    "version": "0.1.0",
    "connector": "axon",
    "name": "axon-prod",
    "ts_ms": 1743865800000,
    "db": "default",
    "schema": "default",
    "collection": "beads",
    "entity_id": "bead-42",
    "audit_id": 42,
    "transaction_id": null
  },
  "op": "c",
  "ts_ms": 1743865800000
}
```

#### Operation Mapping

| Axon mutation | Debezium op | before | after |
|---|---|---|---|
| `entity.create` | `c` | null | full entity |
| `entity.update` | `u` | entity before update | entity after update |
| `entity.patch` | `u` | entity before patch | entity after merge |
| `entity.delete` | `d` | entity before delete | null |
| `link.create` | `c` | null | link data |
| `link.delete` | `d` | link data | null |
| (initial snapshot) | `r` | null | full entity |

For deletes, a **tombstone event** (same key, null value) follows the
delete event, enabling Kafka log compaction.

#### Source Metadata

The `source` block carries Axon-specific metadata:

| Field | Description |
|---|---|
| `version` | Axon server version |
| `connector` | Always `"axon"` |
| `name` | Instance name (configurable, e.g., `"axon-prod"`) |
| `ts_ms` | Event timestamp (milliseconds since epoch) |
| `db` | Database name (FEAT-014 namespace) |
| `schema` | Schema/namespace name |
| `collection` | Collection name |
| `entity_id` | Entity ID |
| `audit_id` | Monotonically increasing audit log sequence number |
| `transaction_id` | Transaction ID if the mutation was part of a transaction, null otherwise |

The `audit_id` serves as the **offset** — consumers can resume from a
specific `audit_id` to get exactly-once delivery semantics (combined
with consumer-side deduplication).

#### Event Key

The Kafka message key is:
```json
{
  "db": "default",
  "schema": "default",
  "collection": "beads",
  "id": "bead-42"
}
```

This ensures all events for the same entity land on the same Kafka
partition, preserving per-entity ordering.

### 2. Topic Naming

One Kafka topic per collection:

```
{instance-name}.{database}.{schema}.{collection}
```

Examples:
- `axon-prod.default.default.beads`
- `axon-prod.prod.billing.invoices`

This follows the Debezium convention of
`{connector-name}.{database}.{table}`, extended with Axon's namespace
hierarchy.

A special **system topic** carries collection lifecycle events (create,
drop, schema change):
- `axon-prod.__system__`

### 3. Kafka Schema Registry (Confluent-Compatible)

Axon implements the Confluent Schema Registry REST API as a thin facade
over its existing schema management. This allows Kafka consumers to
discover entity schemas using standard tooling (Schema Registry clients,
Kafka Connect, stream processors).

#### API Endpoints

| Method | Path | Implementation |
|---|---|---|
| GET | `/subjects` | List collections as subjects (`{db}.{schema}.{collection}-value`) |
| POST | `/subjects/{subject}/versions` | Register schema — maps to `put_schema` |
| GET | `/subjects/{subject}/versions` | List schema versions from `schema_versions` table |
| GET | `/subjects/{subject}/versions/{version}` | Get specific version |
| GET | `/subjects/{subject}/versions/latest` | Get latest version |
| GET | `/schemas/ids/{id}` | Get schema by global ID |
| PUT | `/config` | Set/get compatibility mode |
| PUT | `/config/{subject}` | Per-subject compatibility |
| POST | `/compatibility/subjects/{subject}/versions/{version}` | Test compatibility |

#### Schema Format

Schemas are registered as **JSON Schema** (not Avro) — Axon's entity
schemas are already JSON Schema 2020-12 documents. The registry serves
them in the Confluent wire format:

```json
{
  "subject": "axon-prod.default.default.beads-value",
  "version": 3,
  "id": 42,
  "schemaType": "JSON",
  "schema": "{\"type\":\"object\",\"required\":[\"bead_type\",\"status\",\"title\"],\"properties\":{...}}"
}
```

The global schema ID is derived from `(collection_id, version)` packed
as an integer, ensuring stability across schema registry restarts.

#### Compatibility Modes

Axon maps Confluent compatibility modes to its own schema evolution
classification (FEAT-017):

| Confluent mode | Axon classification |
|---|---|
| BACKWARD | New schema can read old data → compatible changes only |
| FORWARD | Old schema can read new data → additive changes only |
| FULL | Both directions → compatible + additive only |
| NONE | No compatibility check (force-apply) |

Default mode: BACKWARD (matching Confluent default).

#### Serving

The schema registry is served by axum on a configurable port:

```
axon-server --http-port 3000 --grpc-port 50051 --registry-port 8081
```

Port 8081 is the Confluent Schema Registry default. The registry shares
the same schema store as the main server — no separate database.

### 4. CDC Producer Architecture

```
Entity Write Path
    │
    ├── StorageAdapter.put()
    ├── AuditLog.append()
    │
    ▼
CDC Producer
    │
    ├── Read audit entry
    ├── Format as Debezium envelope
    ├── Serialize (JSON or Avro with registry)
    │
    ├──► Kafka topic (if configured)
    ├──► HTTP SSE stream (GraphQL subscriptions)
    └──► File (optional, for debugging/replay)
```

#### Producer Implementation

The CDC producer is a background task that tails the audit log and
emits Debezium-formatted events to configured sinks:

1. **Cursor-based tailing**: The producer maintains a cursor (last
   emitted `audit_id`). On startup, it resumes from the stored cursor.
   The cursor is persisted in a `_cdc_cursors` table

2. **At-least-once delivery**: The producer emits the event, waits for
   the sink acknowledgment (Kafka ack, SSE client receipt), then
   advances the cursor. On crash, events after the cursor may be
   re-emitted. Consumers must be idempotent or deduplicate by
   `audit_id`

3. **Batching**: Events are batched for Kafka efficiency (configurable
   batch size and linger time)

4. **Backpressure**: If the Kafka producer's buffer is full, the CDC
   producer pauses audit log tailing until buffer space is available.
   Entity writes are not blocked — the audit log acts as the buffer

#### Sink Configuration

```toml
[cdc]
enabled = true
instance_name = "axon-prod"

[cdc.kafka]
enabled = true
bootstrap_servers = "kafka:9092"
# Topic naming template
topic_template = "{instance}.{db}.{schema}.{collection}"
# System events topic
system_topic = "{instance}.__system__"
# Serialization: "json" or "avro" (requires schema registry)
serialization = "json"
# Producer tuning
batch_size = 100
linger_ms = 10
acks = "all"

[cdc.kafka.schema_registry]
# URL of the schema registry (Axon's own or external)
url = "http://localhost:8081"

[cdc.file]
# Optional: write CDC events to JSONL files (for debugging/replay)
enabled = false
directory = "/var/axon/cdc"
rotate_size_mb = 100
```

### 5. Initial Snapshot

When CDC is first enabled (or when a new collection is created), the
producer emits a **snapshot** of all existing entities as `op: "r"`
(read) events. This brings downstream consumers to a consistent state
before live events begin.

Snapshot procedure:
1. Record the current max `audit_id` as the snapshot boundary
2. Scan all entities in the collection
3. Emit each entity as an `op: "r"` event with `before: null`
4. After the snapshot completes, begin tailing from the snapshot
   boundary `audit_id`

Snapshots are resumable: if the producer crashes mid-snapshot, it
resumes from the last emitted entity ID (entities are scanned in
ID order).

### 6. Relationship to GraphQL Subscriptions

GraphQL subscriptions (ADR-012) and CDC are complementary:

| Aspect | GraphQL Subscriptions | CDC/Kafka |
|---|---|---|
| Transport | WebSocket | Kafka / SSE / file |
| Durability | Ephemeral (lost on disconnect) | Durable (Kafka retention) |
| Replay | No | Yes (from any audit_id) |
| Format | GraphQL response (typed) | Debezium envelope (standard) |
| Consumers | UI, agents (connected) | Pipelines, analytics, search, replication |
| Filtering | Per-query field selection | Per-topic (collection granularity) |

Both are backed by the same audit log. GraphQL subscriptions poll the
audit log and push to WebSocket clients. CDC tails the audit log and
pushes to Kafka (or other sinks). They can run simultaneously.

### 7. Link Events

Links have their own Debezium events on a dedicated topic per
link source collection:

```
axon-prod.default.default.beads.__links__
```

Link events use the same envelope format:

```json
{
  "before": null,
  "after": {
    "source_collection": "beads",
    "source_id": "bead-42",
    "target_collection": "beads",
    "target_id": "bead-99",
    "link_type": "depends-on",
    "metadata": null
  },
  "source": { ... },
  "op": "c",
  "ts_ms": 1743865800000
}
```

### 8. Crate Dependencies

- **`rdkafka`** v0.39+ — Kafka producer (wraps librdkafka)
- **`schema_registry_converter`** — Client-side schema serialization
  with registry integration (for Avro mode)
- Implementation lives in `crates/axon-cdc/` — new workspace crate

```
crates/
  axon-cdc/
    src/
      producer.rs       # Audit log tailer + event emitter
      envelope.rs       # Debezium envelope construction
      kafka.rs          # Kafka sink (rdkafka)
      sse.rs            # HTTP SSE sink (shared with GraphQL)
      file.rs           # File sink (JSONL)
      snapshot.rs       # Initial snapshot logic
      cursor.rs         # Cursor persistence
  axon-registry/
    src/
      api.rs            # Confluent Schema Registry REST API
      mapping.rs        # Axon schema → registry schema translation
```

## Consequences

**Positive**:
- Standard CDC format — any Debezium consumer works with Axon
- Durable, replayable change feed via Kafka
- Schema registry enables schema evolution for consumers
- Schema registry is a facade over existing Axon schema management —
  no separate database, no schema drift
- Multi-sink: Kafka + SSE + file from the same producer
- Audit log is the single source of truth — CDC is a projection
- Initial snapshot enables bootstrapping new consumers
- `audit_id` as offset enables exactly-once consumer semantics

**Negative**:
- Kafka dependency for production CDC (optional — can use file sink
  or SSE only)
- rdkafka requires librdkafka C library (cmake build dependency)
- Schema registry implementation is a new API surface to maintain
  (~15 endpoints)
- At-least-once delivery means consumers must handle duplicates
- CDC producer is a background task with its own lifecycle (crash
  recovery, cursor management)

**Deferred**:
- **Exactly-once delivery**: Kafka transactional producer with
  idempotent writes. Requires careful cursor + Kafka transaction
  coordination. At-least-once is sufficient for V1
- **Per-field CDC filtering**: Emitting events only when specific
  fields change. V1 emits all mutations
- **NATS / Pulsar / Redis Streams sinks**: Additional transports
  beyond Kafka. The producer architecture supports pluggable sinks
- **CDC for schema changes**: Schema change events on the system
  topic are informational. Full schema replication to downstream
  registries is deferred

## References

- [ADR-003: Backing Store Architecture](ADR-003-backing-store-architecture.md)
- [ADR-010: Physical Storage and Secondary Indexes](ADR-010-physical-storage-and-secondary-indexes.md)
- [ADR-012: GraphQL Query Layer](ADR-012-graphql-query-layer.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [FEAT-015: GraphQL Query Layer](../../01-frame/features/FEAT-015-graphql-query-layer.md)
- [Debezium Connector Documentation](https://debezium.io/documentation/)
- [Confluent Schema Registry API](https://docs.confluent.io/platform/current/schema-registry/develop/api.html)
- [rdkafka crate](https://crates.io/crates/rdkafka)
