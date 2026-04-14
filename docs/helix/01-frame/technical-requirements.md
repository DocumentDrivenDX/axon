---
dun:
  id: helix.technical-requirements
  depends_on:
    - helix.prd
    - helix.principles
---
# Technical Requirements

**Version**: 0.2.0
**Date**: 2026-04-04
**Revised**: 2026-04-06
**Status**: Draft

---

## 1. Implementation Language

Axon is implemented in **Rust**. See [ADR-001](../02-design/adr/ADR-001-implementation-language.md).

---

## 2. Architecture: Stateless Servers

Axon servers are **stateless**. They do not implement consensus protocols (Raft, Paxos, etc.) internally. Durability, replication, and distributed consistency are delegated to the backing store.

| Concern | Axon's Responsibility | Backing Store's Responsibility |
|---------|----------------------|-------------------------------|
| Transaction logic | OCC, version checks, conflict detection, atomic commit | Durable write, crash recovery |
| Schema validation | Validate entities/links against schemas | Store schema metadata durably |
| Audit log | Produce audit entries, enforce append-only | Persist audit entries durably |
| Query execution | Parse, plan, execute queries over stored data | Index storage, range scans, point lookups |
| Replication | None — delegates to backing store | Replication, failover, read replicas |
| Consensus | None | Raft, Paxos, or equivalent (if distributed) |

### Implications

- Any Axon server instance can serve any request — no leader election, no shard assignment
- Horizontal scaling = more Axon server instances behind a load balancer
- Backing store determines the durability and availability guarantees
- Embedded mode = Axon library + embedded backing store (SQLite) in one process
- Server mode = Axon server + external backing store (PostgreSQL, FoundationDB, etc.)

---

## 2a. EAV Storage Model

Axon uses an **Entity-Attribute-Value (EAV)** pattern for **secondary indexes**, not for primary entity storage. Entity data is stored opaquely (JSONB/TEXT/raw bytes by backend) as a single blob per entity. The EAV pattern is used exclusively for the index tables that accelerate queries.

This design enables:

- **Flexible schema evolution** without database migrations (DDL changes) — entity storage is schema-agnostic.
- **Uniform indexing** across all entity types — the same EAV index tables serve all collections.
- **Clean separation** between entity storage and index structures, allowing new index types (vector, BM25) to be added without changing the storage layer or API.

Users never interact with the EAV layer directly — it is an implementation detail of the indexing strategy. All structured query access goes through declared secondary indexes.

---

## 3. Multi-Backend Storage

Axon abstracts storage behind a **Storage Adapter** trait. Multiple backing stores are supported, selected at deployment time.

### Required Adapters (V1)

| Backend | Mode | Use Case |
|---------|------|----------|
| **SQLite / libSQL** | Embedded | Development, testing, single-user, edge deployment |
| **PostgreSQL** | Server | Production, multi-user, existing infrastructure |

### Candidate Adapters (Spike Required)

| Backend | Mode | Use Case | Spike |
|---------|------|----------|-------|
| **FoundationDB** | Server | Scale-out, strong consistency, proven under Apple/Snowflake | [SPIKE-001](../02-design/spikes/SPIKE-001-backing-store-evaluation.md) |
| **TiKV** | Server | Distributed KV with ACID, Raft-based | SPIKE-001 |
| **fjall / RocksDB** | Embedded | High write throughput for audit logs | SPIKE-001 |
| **DuckDB** | Embedded | Aggregation/analytics queries (read path) | SPIKE-001 |

### Storage Adapter Trait (Conceptual)

The adapter must support:
- **Key-value operations**: get, put, delete, range scan
- **Transactions**: begin, commit, abort with snapshot isolation (V1); storage backends may provide serializable at the transaction layer.
- **Ordered iteration**: scan by key prefix with start/end bounds
- **Atomic batch writes**: multiple puts/deletes in one atomic operation
- **Compare-and-swap**: conditional write for OCC (write if version matches)

---

## 4. Data Shape Limits

Derived from niflheim's production-proven limits, adapted for Axon's entity-graph model.

### Entity Limits

| Constraint | Limit | Rationale |
|-----------|-------|-----------|
| Maximum nesting depth | **8 levels** | Matches niflheim. Enforced at schema definition and write time |
| Maximum fields per entity level | **65,535** (u16) | Matches niflheim STRUCT field encoding |
| Maximum array/list elements | **4,294,967,295** (u32) | Matches niflheim LIST/MAP encoding |
| Maximum entity size (serialized) | **1 MB default, 10 MB hard max** | Derived from niflheim's event size limits. Configurable per collection |
| Maximum string/blob field size | **Limited by entity size** | No independent field size limit beyond entity max |
| Minimum entity fields | **1** (beyond system metadata) | At least one user-defined field |

### Collection Limits

| Constraint | Limit | Rationale |
|-----------|-------|-----------|
| Entities per collection | **No hard limit** | Bounded by backing store capacity. Designed for 100K–10M entities |
| Collections per database | **No hard limit** | Expected 5–50 in typical deployments |
| Schema fields total (across nesting) | **No hard limit** | Bounded by nesting depth × fields per level |

### Link Limits

| Constraint | Limit | Rationale |
|-----------|-------|-----------|
| Links per entity (outgoing) | **No hard limit** | Bounded by backing store. Expected <1,000 per entity |
| Link metadata size | **64 KB** | Links are lightweight; metadata should be small |
| Link types per database | **No hard limit** | Expected <100 in typical deployments |
| Traversal depth (default max) | **10 hops** | Configurable. Warning emitted at >10 |

### Transaction Limits

| Constraint | Limit | Rationale |
|-----------|-------|-----------|
| Operations per transaction | **100** | Prevents runaway transactions. Sufficient for all expected use cases |
| Transaction timeout | **30 seconds** | Configurable. Prevents resource leaks |
| Concurrent transactions | **Bounded by backing store** | OCC means no lock contention; throughput scales with store |

### Performance Targets

| Operation | Target (p99) | Measurement |
|-----------|-------------|-------------|
| Single entity read | **<5 ms** | Point lookup by ID |
| Single entity write | **<10 ms** | Create or update with schema validation + audit |
| Multi-entity transaction (5 ops) | **<20 ms** | Atomic commit with OCC |
| Collection scan (1000 entities) | **<100 ms** | Filter + sort + paginate |
| Audit log append | **<2 ms overhead** | Additional latency beyond the mutation itself |
| Link traversal (3 hops) | **<50 ms** | Follow typed links with filters |
| Aggregation query (10K entities) | **<500 ms** | COUNT/SUM/GROUP BY |

---

## 4a. Physical Storage Architecture

The logical data model from Section 3 is materialized with the following
physical design principles. See [ADR-010](../02-design/adr/ADR-010-physical-storage-and-secondary-indexes.md).

### Numeric Collection IDs

Collections use surrogate integer primary keys. All storage tables reference
collections by integer, not by name. Collection renames are O(1) — update
one row, no data rewrite.

### Native UUID Entity IDs

Entity IDs are stored as 16-byte UUIDs (native type on PostgreSQL, BLOB on
SQLite, raw bytes on KV stores). UUIDv7 is the default server-generated
format (time-ordered). Client-supplied non-UUID strings are mapped via UUID v5
deterministic hashing.

### Dedicated Links Table

Links are stored in a dedicated table (not as entities in pseudo-collections)
with database-enforced referential integrity:
- `ON DELETE RESTRICT` — cannot delete an entity that has links pointing at it
- Reverse-lookup index replaces the `__axon_links_rev__` pseudo-collection
- All backends implement the same referential integrity semantics (DB-enforced
  on SQL, application-enforced on KV)

### Entity Data Opacity

Entity data is opaque to the storage layer. The data column type varies by
backend (JSONB in PostgreSQL, TEXT in SQLite, raw bytes in KV stores) but is
not used for query execution. All structured queries go through declared
secondary indexes.

### No Backend-Specific Query Operators

The storage layer does not use GIN indexes, JSONB containment operators, or
any backend-specific query features. This keeps the query path portable across
PostgreSQL, SQLite, and KV stores (FoundationDB, Fjall).

---

## 4b. Secondary Indexes

Axon uses the EAV (Entity-Attribute-Value) pattern for secondary indexes.
See [ADR-010](../02-design/adr/ADR-010-physical-storage-and-secondary-indexes.md)
and [FEAT-013](features/FEAT-013-secondary-indexes.md).

### Index Types

| Type | Storage | Use Case |
|------|---------|----------|
| `string` | TEXT | Status, name, category lookups |
| `integer` | BIGINT | Priority, count, quantity ranges |
| `float` | DOUBLE PRECISION | Scores, percentages, measurements |
| `datetime` | TIMESTAMPTZ / epoch nanos | Time-range queries, sorting by date |
| `boolean` | BOOLEAN | Flag-based filtering |

### Index Varieties

- **Single-field**: One EAV table per type, shared across all collections.
  PK: `(collection_id, field_path, value, entity_id)`
- **Compound**: Binary-encoded sort key preserving multi-field sort order.
  Uses FoundationDB-compatible tuple encoding. PK:
  `(collection_id, index_name, sort_key, entity_id)`
- **Unique**: Single-field or compound with uniqueness constraint. Enforced
  at the storage level

### Index Lifecycle

Indexes go through states: `building` → `ready` → `dropping`. The query
planner only uses `ready` indexes. Background workers handle build and
cleanup. A rebuild operation returns a `ready` index to `building` for
reindexing.

### Future Index Types (Not Scheduled)

Because entity storage and indexing are decoupled, new index types can be added without changes to the storage layer or API:

| Index Type | Use Case | Notes |
|-----------|----------|-------|
| **Vector** | Semantic search, embedding-based retrieval | Would surface as a `near` filter. Must maintain transactional guarantees |
| **BM25 / full-text** | Full-text search with ranking and faceting | Tantivy-based. Significant integration effort |

**Risk**: If vector/BM25 indexes are hosted as separate services (off-node), maintaining ACID guarantees across them becomes a distributed systems problem. Keep indexes co-located as long as possible.

### Query Planner

Rules-based (not cost-based). Checks filter fields against declared indexes:
1. Exact match on indexed field → use index table
2. Prefix match on compound index → use compound index with range scan
3. Sort field matches index → use index scan order
4. No match → full scan with application-layer filter

---

## 4c. Tenancy, Namespace Hierarchy, and Path-Based Addressing

See [ADR-018](../02-design/adr/ADR-018-tenant-user-credential-model.md)
(governing), [ADR-011](../02-design/adr/ADR-011-multi-tenancy-and-namespace-hierarchy.md)
(node placement and migration), and [FEAT-014](features/FEAT-014-multi-tenancy.md).

### Four-Level Conceptual Hierarchy

```
tenant  (global account boundary — owns users, credentials, databases)
├── users            (M:N via tenant_users)
├── credentials      (tenant-scoped JWTs with grants)
└── databases        (N per tenant — placed on nodes)
     └── schemas     (logical namespace within a database)
          └── collections  (entity containers)
               └── entities

node  (physical placement only — invisible from the data path)
```

### Wire Addressing (Path-Based)

Every data-plane route is nested under a fixed prefix:

```
/tenants/{tenant}/databases/{database}/{resource...}
```

An entity's canonical URL is simultaneously its identifier, its routing
key, and its HTTP cache key. There is no `X-Axon-Database` header, no
`X-Axon-Tenant` header, and no un-prefixed legacy routes. See ADR-018
for the rationale.

### Defaults

- On a fresh deployment, the first successful authenticated request
  auto-creates a `default` tenant with the authenticating user as its
  admin, plus a `default` database and a `default` schema inside it.
  This is idempotent — runs only when `tenants` is empty.
- `--no-auth` mode synthesizes the default context in memory without
  persisting any rows.

### Node Topology (P2)

Nodes carry region and zone metadata. A `database_placement` table maps
`(tenant, database)` pairs to nodes. Database migration is a routing
table update plus data replication — no key-space rewrite, no URL
change, no client reconfiguration. ADR-011 governs the migration
protocol.

### Access Control Scoping

Every data-plane request is authorized against a `(user, tenant,
database)` triple:

1. **Membership** — the caller must have a `tenant_users` row for the
   URL's tenant. Roles: `admin | write | read`.
2. **Grant** — if the caller is using a JWT credential, the credential's
   `grants.databases[]` claim must cover the URL's database with an
   `ops` entry matching the request method (read vs. write).

Grants are always ≤ role: an admin can issue narrow credentials, but
a `read` member cannot issue a `write` credential. See FEAT-012 for
the full model and ADR-018 for the JWT claim shape.

---

## 5. Schema System

### Axon Entity Schema Format (ESF)

Axon defines its own schema format, **Entity Schema Format (ESF)**, native to the entity-graph-relational model. ESF supports:

- **Entity schemas**: deeply nested structures with recursive type definitions
- **Link-type schemas**: typed relationships with metadata schemas and cardinality constraints
- **Validation rules**: field-level (type, range, pattern, enum), cross-field, and cross-entity constraints
- **Context-specific constraints**: rules that vary by context (inspired by tablespec/UMF's per-LOB nullability)
- **Severity levels**: error (reject write), warning (accept with flag), info (log only)

### Schema Bridges

ESF bridges bidirectionally to other schema formats:

| Bridge | Direction | Purpose |
|--------|-----------|---------|
| ESF ↔ JSON Schema | Bidirectional | Interoperability with standard tooling |
| ESF → SQL DDL | Export | Generate backing store tables |
| ESF ↔ UMF (tablespec) | Bidirectional | Data pipeline integration, leverage existing UMF schemas |
| ESF → Protobuf | Export | Generate gRPC message definitions |
| ESF → TypeScript | Export | Client SDK type generation |

### Concepts Incorporated from UMF/tablespec

| UMF Concept | Axon ESF Equivalent |
|-------------|-------------------|
| Context-specific nullability (`nullable: {MD: false, ME: true}`) | Context-specific constraints with configurable context dimension |
| Relationships with cardinality and confidence | Link-type definitions with cardinality (1:1, 1:N, N:M) |
| Validation rules with severity | Validation rules with error/warning/info severity |
| Type mappings (UMF → SQL, PySpark, JSON Schema) | ESF bridges to multiple output formats |
| Domain types (`domain_type: "email"`, `"phone_number"`) | Semantic type annotations for enhanced validation |
| Derivation/survivorship strategies | Entity derivation rules for MDM/golden-record use cases |
| Quality checks (post-write) | Post-commit validation hooks |

### Open Questions

1. **ESF format**: YAML like UMF, or something else? JSON Schema as the base with extensions (current PRD direction) vs. a custom SDL (like EdgeDB)?
2. **UMF extraction**: Should UMF's core mechanics be extracted to a standalone project, or is bridging ESF ↔ UMF sufficient?
3. **Schema versioning**: How are schema versions tracked? Monotonic integer per collection? Content hash?

---

## 6. Correctness Requirements

### Deterministic Simulation Testing

Following FoundationDB's approach (see [research](../00-discover/foundationdb-dst-research.md)):

- **Test suite written before implementation** — correctness properties defined as executable invariants
- **Deterministic replay** — any test failure reproducible with a seed
- **Fault injection (BUGGIFY)** — disk failures, network errors, clock skew, process crashes injected during simulation
- **Simulation framework**: MadSim-based or custom Rust DST framework with Net2/Sim2 runtime swapping
- **Cycle test equivalent**: Ring of entities with link traversals; transactional isolation verified by ring integrity after chaos

### Correctness Invariants

| Invariant | Verification |
|-----------|-------------|
| **No lost updates** | Concurrent writers to same entity: exactly one succeeds per version |
| **Snapshot Isolation** | Cycle test (ring integrity) under concurrent transactions; write skew prevention is P1. |
| **Audit completeness** | Every committed mutation has a corresponding audit entry |
| **Audit immutability** | No audit entry is ever modified or deleted |
| **Schema enforcement** | No entity in storage violates its collection schema |
| **Link integrity** | No link references a non-existent entity (unless force-deleted) |
| **Version monotonicity** | Entity versions strictly increase; no gaps, no reuse |
| **Transaction atomicity** | Multi-op transaction: all audit entries share a transaction ID; all or none are visible |

### HELIX Ratchets

Quality metrics that can only improve:

| Ratchet | Direction | Metric |
|---------|-----------|--------|
| Correctness properties verified | Increasing | Count of invariants passing in simulation |
| Simulation hours | Increasing | Total simulated hours of fault injection |
| Test coverage (line) | Increasing | % of lines exercised by tests |
| Performance benchmarks | Improving | p99 latency must not regress |
| Audit gap count | Decreasing (toward 0) | Mutations without audit entries |

---

## 7. Operational Acceptance Criteria

### Embedded Mode

| Criterion | Acceptance |
|-----------|-----------|
| Zero external dependencies | Axon embedded runs with no external processes, services, or network |
| Single-file database | All data (entities, links, audit, schemas) in one file or directory |
| In-process API | Same Rust types and trait interfaces as server mode |
| Test suite parity | Identical correctness test suite passes in embedded and server modes |

### Server Mode

| Criterion | Acceptance |
|-----------|-----------|
| Stateless server | Any server instance can handle any request; no leader election |
| Horizontal scaling | Adding server instances increases throughput linearly (backing store permitting) |
| Health check endpoint | `/health` returns 200 with backing store connectivity status |
| Graceful shutdown | In-flight transactions complete or abort cleanly on SIGTERM |
| Connection pooling | Server manages connection pool to backing store |
| Observability | OpenTelemetry traces and metrics for all operations |

### Backing Store Operations

| Criterion | Acceptance |
|-----------|-----------|
| Backup/restore | Documented procedure for backing up and restoring all Axon data |
| Schema migration | Axon manages its own internal schema migrations on backing store |
| Connection failure handling | Transient backing store failures produce retryable errors, not data corruption |
| Capacity monitoring | Metrics for backing store utilization (storage, connections, latency) |

---

## 8. API Requirements

### Protocol

- **Primary**: gRPC (tonic) with protobuf definitions
- **Secondary**: HTTP/JSON gateway (tonic-web or separate gateway)
- **Embedded**: Native Rust trait (no serialization overhead)

### Client SDKs

| Language | Priority | Notes |
|----------|----------|-------|
| Rust | P0 | Native, generated from trait definitions |
| TypeScript | P0 | For local-first UI, generated from protobuf |
| Go | P1 | For DDx integration, generated from protobuf |
| Python | P1 | For data pipeline integration, generated from protobuf |

### CLI

- `axon` binary wrapping the gRPC API
- Supports embedded mode (no server required) and remote mode
- Output formats: human-readable table (default), JSON, YAML

---

## 9. Control Plane (P2)

A lightweight control plane for multi-tenant Axon deployments:

- **Backing store**: PostgreSQL database for control plane metadata.
- **Tenant lifecycle**: One Axon instance per tenant, managed centrally. Provisioning, deprovisioning, configuration.
- **Tenant isolation**: Each tenant's data lives in its own storage. The control plane provides a single pane of glass without touching customer data.
- **BYOC support**: Customer runs Axon in their infrastructure. Control plane provides management, monitoring, and operational visibility.
- **Monitoring**: Health checks, capacity monitoring, latency dashboards across all managed instances.

---

## 10. Client-Side Validation

The schema is the single source of truth for what data is valid. To eliminate the pattern where validation is maintained in three places (database, API, UI):

- **TypeScript validator** generated from ESF schema, usable directly in browser UIs.
- **Same rules** enforced server-side and client-side — no divergence.
- **Schema queryable by agents** via MCP/GraphQL — agents can understand what's valid before attempting a write, reducing failed writes and retry loops.

---

## Traceability

| Requirement Area | PRD Section | Feature Specs | ADRs |
|-----------------|-------------|---------------|------|
| Stateless servers | Section 6 (Cloud-native) | FEAT-005 (API Surface) | ADR-003 |
| Multi-backend | Section 10 (Constraints) | FEAT-001 (Collections) | ADR-003 |
| Data shape limits | Section 4 (Data Model) | FEAT-007 (Entity-Graph Model) | — |
| Schema system (ESF) | Section 4, 8 | FEAT-002 (Schema Engine) | ADR-002, ADR-007, ADR-008 |
| Correctness | Section 5 (Transactions) | FEAT-008 (ACID Transactions) | ADR-004 |
| Performance targets | Section 6 (Success Metrics) | FEAT-004 (Entity Operations) | — |
| Physical storage | Section 8 P1 #11 | FEAT-013 (Indexes) | ADR-010 |
| Secondary indexes | Section 8 P1 #9 | FEAT-013 (Indexes) | ADR-010 |
| Multi-tenancy / namespaces | Section 8 P1 #10, P2 #4 | FEAT-014 (Multi-Tenancy) | ADR-011 |
| Authentication / authorization | Section 8 P1 #6 | FEAT-012 (Authorization) | ADR-005 |
| Admin web UI | Section 8 P1 #8 | FEAT-011 (Admin Web UI) | ADR-006 |
| Schema evolution | Section 8 P1 #1 | FEAT-017 (Schema Evolution) | ADR-007 |
| Change feeds | Section 8 P1 #2 | FEAT-015 (GraphQL subscriptions), FEAT-003 (Audit polling) | ADR-003, ADR-012 |
| Aggregation queries | Section 8 P1 #3 | FEAT-018 (Aggregation) | — |
| GraphQL API | Section 8 P1 #12 | FEAT-015 (GraphQL) | ADR-012 |
| MCP server | Section 8 P1 #13 | FEAT-016 (MCP) | ADR-013 |
| Agent guardrails | Section 8 P1 #16 | FEAT-022 (Agent Guardrails) | — |
| Rollback and recovery | Section 8 P1 #17 | FEAT-023 (Rollback/Recovery) | — |
| Application substrate | Section 8 P2 #8 | FEAT-024 (Application Substrate) | — |
| Control plane | Section 8 P2 #9, Section 9 | FEAT-025 (Control Plane) | — |
| EAV storage model | Section 2a | FEAT-013 (Indexes), ADR-010 | ADR-010 |
| Client-side validation | Section 10 | FEAT-002 (Schema Engine) | ADR-002 |

---

*This document is a living artifact. Updated as spikes produce results and design decisions are made.*
