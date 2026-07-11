---
ddx:
  id: CONTRACT-006
  depends_on:
    - ADR-014
    - ADR-018
    - FEAT-021
    - FEAT-003
  review:
    self_hash: 13e5724753d3343edd73481dd9458874fd1c3c676be5005a53022e77e751da39
    deps:
      ADR-014: 6b9f2190081dd7dae202942b25247ee638b0359a4ead7109987b5bc4440c7347
      ADR-018: 6282a6ac66a0dcfd400663681132c9f5f85ed7c78793a1cf7f8bf06853cf1d97
      FEAT-003: 15881e4941cec74cf6e0be6d023da0a34cb4f1f4efb5efbb6a9b8246e037010f
      FEAT-021: 6165a271de0b5e5c978f97ab9393596e651a680c51db80153fb85167ed93d993
    reviewed_at: "2026-07-11T02:26:23Z"
---

# Contract

**Contract ID**: CONTRACT-006
**Type**: event + HTTP API (CDC envelope, topics, registry, cursors)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-014, ADR-018, FEAT-021, FEAT-003, FEAT-014, FEAT-017, CONTRACT-005

## Purpose

Defines the normative change-data-capture surface: the Debezium-compatible
event envelope, tenant-aware topic naming and event keys, the
Confluent-compatible schema-registry REST endpoints, cursor semantics, and
the `[cdc]` configuration keys. Any CDC consumer, sink, or registry client
implements against this document.

## Scope and Boundaries

- In scope: envelope fields, operation mapping, tombstones, topic and key
  schemes, registry endpoints and wire format, cursor and replay semantics,
  configuration keys.
- Out of scope: audit entry record itself (CONTRACT-005), GraphQL
  subscription wire shape (FEAT-015), sink implementations and producer
  internals, Kafka broker configuration.
- Owning system: `axon-audit` (cdc module, the CDC publisher) / `axon-registry` (the registry facade).

## Normative Surface

### Envelope fields

Every mutation produces one Debezium-compatible envelope:

| Element | Type / Shape | Required | Rules | Notes |
|---------|--------------|----------|-------|-------|
| `before` | JSON object \| null | yes | Pre-image; `null` for `c` and `r` | |
| `after` | JSON object \| null | yes | Post-image; `null` for `d` | |
| `op` | enum | yes | `c` (create), `u` (update/patch), `d` (delete), `r` (snapshot read) | |
| `ts_ms` | integer | yes | Event timestamp, milliseconds since epoch | |
| `source.version` | string | yes | Axon server version | |
| `source.connector` | string | yes | MUST be `"axon"` | |
| `source.name` | string | yes | Instance name (e.g. `"axon-prod"`) | |
| `source.ts_ms` | integer | yes | Source timestamp (ms) | |
| `source.tenant` | string | yes | Tenant name (ADR-018) | Tenant-aware extension; see Precedence |
| `source.db` | string | yes | Database name (FEAT-014 namespace) | |
| `source.schema` | string | yes | Schema/namespace name | |
| `source.collection` | string | yes | Collection name | |
| `source.entity_id` | string | yes | Entity ID | |
| `source.audit_id` | integer | yes | Monotonic audit sequence number; the consumer offset | Dedup key for at-least-once |
| `source.transaction_id` | string \| null | yes | Transaction ID if part of a transaction, else `null` | Shared by all events in a transaction |

Operation mapping (normative):

| Axon mutation | `op` | `before` | `after` |
|---|---|---|---|
| `entity.create` | `c` | null | full entity |
| `entity.update` | `u` | entity before update | entity after update |
| `entity.patch` | `u` | entity before patch | entity after merge |
| `entity.delete` | `d` | entity before delete | null |
| `link.create` | `c` | null | link data |
| `link.delete` | `d` | link data | null |
| (initial snapshot) | `r` | null | full entity |

Deletes MUST be followed by a tombstone event (same key, null value) for
Kafka log compaction.

### Topic naming (tenant-aware)

One Kafka topic per collection:

```
{instance}.{tenant}.{db}.{schema}.{collection}
```

Example: `axon-prod.acme.finance.default.invoices`.

| Topic | Pattern | Carries |
|---|---|---|
| Collection topic | `{instance}.{tenant}.{db}.{schema}.{collection}` | Entity events |
| Link topic | `{instance}.{tenant}.{db}.{schema}.{collection}.__links__` | Link events for the source collection, same envelope format |
| System topic | `{instance}.__system__` | Collection lifecycle events (create, drop, schema change) across tenants; events carry `source.tenant` |

> Reconciliation note (normative): ADR-014's original
> `{instance}.{db}.{schema}.{collection}` scheme predates ADR-018's tenant
> boundary. The tenant-prefixed form above is the normative scheme; the
> tenant segment MUST appear in the topic name, the event key, and
> `source.tenant` so that no two tenants share a topic or key space.

### Event key

The Kafka message key ensures per-entity partition ordering:

```json
{ "tenant": "acme", "db": "finance", "schema": "default",
  "collection": "invoices", "id": "inv_001" }
```

All events for the same entity MUST land on the same partition.

### Schema registry REST endpoints (Confluent-compatible)

Served on a configurable port (default 8081), as a facade over Axon's
`schema_versions` store — no separate schema database.

| Method | Path | Behavior |
|---|---|---|
| GET | `/subjects` | List collections as subjects (`{tenant}.{db}.{schema}.{collection}-value`) |
| POST | `/subjects/{subject}/versions` | Register schema — maps to `put_schema` |
| GET | `/subjects/{subject}/versions` | List schema versions |
| GET | `/subjects/{subject}/versions/{version}` | Get specific version |
| GET | `/subjects/{subject}/versions/latest` | Get latest version |
| GET | `/schemas/ids/{id}` | Get schema by global ID |
| PUT | `/config` | Set/get global compatibility mode |
| PUT | `/config/{subject}` | Per-subject compatibility |
| POST | `/compatibility/subjects/{subject}/versions/{version}` | Test compatibility |

- Schemas are served as JSON Schema 2020-12 (`schemaType: "JSON"`) in the
  Confluent wire format.
- The global schema ID is derived from `(collection_id, version)` packed as
  an integer and MUST be stable across registry restarts.
- Compatibility modes map to FEAT-017 classification: BACKWARD → compatible
  changes only; FORWARD → additive only; FULL → compatible + additive;
  NONE → no check. Default: BACKWARD.

### Cursor semantics

| Element | Rule |
|---|---|
| `_cdc_cursors` table | Persists the last emitted `audit_id` per sink; per-collection cursors track independent progress |
| Resume | On restart the producer resumes from the stored cursor; events after the cursor MAY be re-emitted (at-least-once). Consumers MUST be idempotent or deduplicate by `source.audit_id` |
| Cursor token | External APIs expose an opaque, random, server-resolved cursor handle derived from audit sequence, sink, and scope. When the scope is transaction-aware, replay is transaction-framed. Tokens MUST remain valid across producer restarts and schema-compatible migrations; incompatible schema, policy, or auth epoch changes purge outstanding tokens and require rebootstrap |
| Scoped replay | Replay MAY be scoped by database, schema, collection, entity/link ID, or transaction ID without changing envelope semantics; a consumer MAY reset its cursor to any `audit_id` |
| Snapshot | Initial snapshot emits all existing entities as `op: "r"` in entity-ID order; the snapshot boundary is the max `audit_id` recorded at start; live tailing begins from the boundary. Snapshots are resumable from the last emitted entity ID |
| Cursor vocabulary parity | GraphQL subscriptions, MCP resource notifications, SDK change readers, and CDC sinks use the same audit cursor vocabulary |

### Configuration keys

```toml
[cdc]
enabled = true
instance_name = "axon-prod"

[cdc.kafka]
enabled = true
bootstrap_servers = "kafka:9092"
topic_template = "{instance}.{tenant}.{db}.{schema}.{collection}"
system_topic = "{instance}.__system__"
serialization = "json"          # "json" | "avro" (avro requires registry)
batch_size = 100
linger_ms = 10
acks = "all"

[cdc.kafka.schema_registry]
url = "http://localhost:8081"

[cdc.file]
enabled = false
directory = "/var/axon/cdc"
rotate_size_mb = 100
```

Kafka is optional: file and SSE sinks MUST work without it.

## Precedence and Compatibility

- Versioning: the envelope is Debezium-compatible; new `source` fields are
  additive only. Consumers MUST ignore unknown fields.
- Tenant reconciliation: the tenant-aware topic/key/source scheme supersedes
  ADR-014's pre-tenant form. Deployments created before tenancy MAY map to a
  `default` tenant segment; the template is configurable via
  `topic_template`, but any template MUST include `{tenant}` when more than
  one tenant exists.
- Cursor epoch compatibility: producer restarts and schema-compatible
  migrations preserve cursor validity. Incompatible schema, policy, or auth
  epoch changes trigger a hard cursor cut before 1.0, but the cut MUST be
  explicit: existing cursors are purged and clients rebootstrap from a fresh
  snapshot.
- Delivery: at-least-once. `source.audit_id` is the offset enabling
  consumer-side exactly-once.
- Ordering: per-entity ordering within a partition is guaranteed by the
  event key; cross-entity ordering follows `audit_id` within a database.
- Backpressure: producer pauses tailing when sink buffers fill; entity
  writes are never blocked (the audit log is the buffer).

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|------------------|-------|----------------------|
| Sink unavailable (e.g. Kafka down) | Producer pauses; cursor not advanced; no event loss while audit log retained | yes (automatic) | CDC catches up after sink recovers |
| Producer crash mid-stream | Events after cursor re-emitted on restart | yes | Consumers deduplicate by `audit_id` |
| Producer crash mid-snapshot | Snapshot resumes from last emitted entity ID | yes | None required from consumers |
| Unknown subject on registry lookup | 40401 subject-not-found (Confluent error convention) | no | Use a listed subject |
| Incompatible schema registration | 409 conflict per compatibility mode | yes (after change) | Adjust schema or compatibility mode |
| Incompatible schema, policy, or auth epoch change | Existing cursor tokens are purged; client must rebootstrap from a fresh snapshot | no | Obtain a fresh cursor after the incompatible change |
| Cursor token invalid for scope | Request rejected | no | Obtain a fresh cursor from a recent event |

## Examples

```json
{
  "before": null,
  "after": {
    "id": "bead-42",
    "version": 1,
    "data": { "bead_type": "task", "status": "draft",
              "title": "Implement auth middleware", "priority": 2 },
    "created_at": "2026-04-05T14:30:00Z",
    "created_by": "agent-1"
  },
  "source": {
    "version": "0.1.0",
    "connector": "axon",
    "name": "axon-prod",
    "ts_ms": 1743865800000,
    "tenant": "acme",
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

Registry wire format:

```json
{
  "subject": "acme.default.default.beads-value",
  "version": 3,
  "id": 42,
  "schemaType": "JSON",
  "schema": "{\"type\":\"object\",\"required\":[\"bead_type\",\"status\",\"title\"],\"properties\":{...}}"
}
```

## Non-Normative Notes

The subject naming above keeps the instance name out of registry subjects;
deployments that need instance-qualified subjects may prefix them, but the
tenant segment is the load-bearing isolation boundary. NATS/Pulsar/Redis
sinks, exactly-once Kafka transactions, and per-field filtering are deferred
per ADR-014.

## Validation Checklist

- [ ] Normative fields and rules are explicit.
- [ ] Compatibility and precedence rules are explicit.
- [ ] Error handling is explicit.
- [ ] At least one executable test can be derived from this contract.
- [ ] Non-normative notes cannot be mistaken for contract requirements.
