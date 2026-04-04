---
dun:
  id: helix.technical-requirements
  depends_on:
    - helix.prd
    - helix.principles
---
# Technical Requirements

**Version**: 0.1.0
**Date**: 2026-04-04
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
- **Transactions**: begin, commit, abort with serializable isolation
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
| **Serializable isolation** | Cycle test (ring integrity) under concurrent transactions |
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

## Traceability

| Requirement Area | PRD Section | Feature Specs |
|-----------------|-------------|---------------|
| Stateless servers | Section 6 (Cloud-native) | FEAT-005 (API Surface) |
| Multi-backend | Section 10 (Constraints) | FEAT-001 (Collections) |
| Data shape limits | Section 4 (Data Model) | FEAT-007 (Entity-Graph Model) |
| Schema system (ESF) | Section 4, 8 | FEAT-002 (Schema Engine) |
| Correctness | Section 5 (Transactions) | FEAT-008 (ACID Transactions) |
| Performance targets | Section 6 (Success Metrics) | FEAT-004 (Document Operations) |

---

*This document is a living artifact. Updated as spikes produce results and design decisions are made.*
