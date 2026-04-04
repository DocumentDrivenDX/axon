---
dun:
  id: SPIKE-001
  depends_on:
    - helix.prd
    - ADR-001
    - FEAT-007
    - FEAT-008
    - FEAT-003
---
# SPIKE-001: Backing Store Evaluation

**Version**: 0.1.0
**Date**: 2026-04-04
**Status**: Draft
**Author**: Erik LaBianca
**Time-box**: 2 weeks (implementation), 1 week (analysis and write-up)

---

## 1. Purpose

Axon servers are stateless -- they do not implement consensus (Raft/Paxos) themselves but rely on backing stores for durability and consistency. This spike evaluates candidate backing stores across the dimensions that matter for Axon's entity-graph-relational model, ACID transactions, and audit-first architecture.

The goal is not to pick a single winner. Axon's storage abstraction (PRD value proposition #5) means multiple backends will be supported. The goal is to determine:

1. Which store serves as the **embedded default** (dev, single-node, CLI)
2. Which store serves as the **server-mode default** (multi-client, production)
3. Whether any store offers unique advantages for the **audit log** subsystem
4. Which stores to defer or eliminate from further consideration

---

## 2. Evaluation Criteria

Each candidate is evaluated on seven axes derived from Axon's requirements:

| # | Criterion | Weight | Rationale |
|---|-----------|--------|-----------|
| 1 | Rust ecosystem support | High | ADR-001 commits to Rust. Poor bindings are a hard blocker |
| 2 | Transaction model | High | FEAT-008 requires serializable isolation, OCC, multi-key atomicity |
| 3 | Embeddability | High | PRD P0 #11 requires in-process embedded mode |
| 4 | Operational model | Medium | Stateless Axon servers need a backing store that handles its own durability |
| 5 | Performance characteristics | Medium | PRD targets <10ms p99 single-entity, <20ms p99 multi-entity transactions |
| 6 | Schema/structure support | Medium | Entity-graph-relational model (FEAT-007) must map naturally |
| 7 | Audit log suitability | Medium | FEAT-003 requires append-only, ordered, scannable log |

---

## 3. Candidate Analysis

### 3.1 PostgreSQL

**Role**: Server-mode primary store (confirmed)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `sqlx` v0.8.x -- pure-Rust async driver with compile-time query checking. Tokio-native. 54M+ downloads. Actively maintained by LaunchBadge. Alternative: `tokio-postgres` v0.7.x for lower-level control |
| **Transaction model** | Full ACID. Serializable isolation via SSI (Serializable Snapshot Isolation). Multi-statement transactions. Advisory locks for coordination. Row-level OCC possible via `xmin`/version column pattern |
| **Embeddability** | Not embeddable. Requires external process. `pgembedded` crate exists for testing but is not production-grade |
| **Operational model** | Client-server. Connection pooling via `sqlx::PgPool` or external PgBouncer. Managed options: AWS RDS, Supabase, Neon, CrunchyData. Stateless Axon servers connect as clients |
| **Performance** | Single-row read: ~0.3ms local, ~1-3ms network. Write + fsync: ~2-5ms. Throughput: 10K-50K TPS on modern hardware. JSONB operations add overhead vs flat columns |
| **Schema mapping** | JSONB for entity bodies, relational tables for metadata/indexes. Recursive CTEs for link traversal. GIN indexes on JSONB for field-level queries. `ltree` extension for hierarchical paths. Natural fit with some impedance mismatch on deep nesting |
| **Audit log** | Append-only table with `GENERATED ALWAYS AS IDENTITY`. Partitioning by time range for retention. `pg_partman` for automated partition management. BRIN indexes for time-range scans |

**Known issues**:
- `sqlx` compile-time checking requires a running database during `cargo check` (can be worked around with offline mode and `sqlx-data.json`)
- JSONB indexing has higher write amplification than flat columns
- Recursive CTEs for deep graph traversal (>5 hops) can be slow without careful query planning

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `sqlx` | 0.8.6 | Yes (tokio, async-std) | Compile-time checked queries, connection pooling, migrations |
| `tokio-postgres` | 0.7.x | Yes (tokio) | Lower-level, more control, no compile-time checks |
| `deadpool-postgres` | 0.14.x | Yes | Connection pooling for `tokio-postgres` |

**Verdict**: Confirmed for server mode. Mature, well-understood, excellent Rust support.

---

### 3.2 SQLite / libSQL

**Role**: Embedded-mode primary store (confirmed)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | **SQLite**: `rusqlite` v0.38.0 -- synchronous, bundled SQLite feature compiles SQLite into binary. Async wrappers: `async-sqlite`, `async-rusqlite` (spawn onto background thread). **libSQL**: `libsql` v0.9.x -- Turso's fork, native async API, builder pattern for local/replica/remote modes. Tokio-native |
| **Transaction model** | ACID with WAL mode. `SERIALIZABLE` is the default (and only) isolation level. Single-writer, multiple-reader. `BEGIN IMMEDIATE` for write transactions to avoid SQLITE_BUSY. libSQL adds server-side WAL for concurrent writes |
| **Embeddability** | Excellent. Single-file database, compiles into binary via `bundled` feature. Zero external dependencies. libSQL adds embedded replicas that sync to cloud |
| **Operational model** | Embedded by default. libSQL adds server mode (sqld) and Turso managed service. Embedded replicas provide local reads + remote writes. Axon embedded mode maps directly to SQLite in-process |
| **Performance** | Single-row read: ~5-50us in-process. Write + WAL sync: ~0.1-1ms. Throughput: 50K-200K reads/sec, 5K-50K writes/sec (WAL mode, single writer). libSQL improves concurrent write throughput |
| **Schema mapping** | Same as PostgreSQL (JSONB via `json()` / `json_extract()`), but JSON support is less mature. Recursive CTEs supported. No GIN indexes -- JSON field queries require generated columns or FTS5 |
| **Audit log** | Append-only table. No native partitioning (must be done at application level via table rotation). Adequate for dev/single-node; not for production-scale audit retention |

**Known issues**:
- `rusqlite` is synchronous -- async wrappers add a thread-hop per operation
- `libsql` crate API is still evolving (pre-1.0)
- Single-writer limitation means audit log writes contend with entity writes
- JSON support is functional but less optimized than PostgreSQL JSONB
- No advisory locks or `SKIP LOCKED` for queue patterns

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `rusqlite` | 0.38.0 | No (sync only) | Bundled SQLite, feature-rich, very stable |
| `async-sqlite` | 0.3.x | Yes (tokio, async-std) | Wraps rusqlite on background thread |
| `libsql` | 0.9.29 | Yes (tokio-native) | Turso fork, embedded replicas, encryption |
| `sqlx` | 0.8.6 | Yes | Also supports SQLite backend |

**Verdict**: Confirmed for embedded mode. `libsql` is preferred over raw `rusqlite` for async support and future sync capabilities. Fall back to `rusqlite` + `async-sqlite` if `libsql` stability is insufficient.

---

### 3.3 FoundationDB

**Role**: Server-mode alternative / distributed store (strong candidate)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `foundationdb` v0.10.0 -- wraps the FDB C client library via FFI. Futures-based (runtime-agnostic). Alternative: `fdb` v0.3.1 -- Tokio-native API, supports FDB client API 710+. Neither crate is as mature as `sqlx` |
| **Transaction model** | Strictly serializable. Multi-key ACID transactions across the entire keyspace. OCC with automatic conflict detection. **Hard constraints**: transactions limited to 10MB affected data and 5-second duration. These are fundamental to FDB's architecture and cannot be raised |
| **Embeddability** | Not embeddable. Requires external `fdbserver` cluster (minimum 1 node for dev, 3+ for production). The C client library (~15MB) must be linked |
| **Operational model** | Stateless client, external cluster. Axon connects as a stateless client -- perfect match for the architecture. Apple runs it at massive scale. Snowflake uses it for metadata. No major managed offering (self-hosted or FoundationDB on Kubernetes via `fdb-kubernetes-operator`) |
| **Performance** | Single-key read: ~0.5-1ms (network). Write + commit: ~2-5ms. Throughput: millions of operations/sec at cluster scale. Latency is bounded and predictable. The 5-second transaction limit forces efficient transaction design |
| **Schema mapping** | Ordered key-value only. **No native schema support** -- everything is bytes. The Record Layer (Java only) provides structured storage, indexes, and query. A Rust Record Layer equivalent does not exist (there is an incomplete WIP). Axon would need to build its own structured layer: key encoding (entity/link/audit key prefixes), secondary index maintenance, query planning. This is significant engineering effort but provides maximum control |
| **Audit log** | Excellent fit. Ordered keys with versionstamp prefixes give globally ordered, append-only semantics for free. Atomic batch writes ensure audit entries are committed with their corresponding mutations. Range scans over audit key prefixes are efficient |

**Known issues**:
- 5-second transaction limit requires careful design for bulk operations and schema migrations
- 10MB transaction size limit requires batching strategies for large entities
- No Rust Record Layer -- building structured storage on raw KV is weeks of work
- C client library FFI adds build complexity and a non-Rust dependency
- No managed cloud offering -- operational burden is on the team
- `foundationdb` crate maintenance velocity is moderate (community-driven, not backed by Apple)

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `foundationdb` | 0.10.0 | Yes (runtime-agnostic futures) | FFI to C client, supports FDB API 510-740 |
| `fdb` | 0.3.1 | Yes (Tokio-native) | Alternative bindings, Tokio-first design |
| `foundationdb-simulation` | 0.2.2 | Yes | DST support for testing |

**Verdict**: Strong candidate for distributed server mode. The ordered KV + serializable transactions + deterministic simulation testing story aligns perfectly with Axon's correctness-first philosophy. However, the lack of a Rust Record Layer means significant upfront investment in structured storage. Evaluate in spike but defer commitment until the structured layer cost is understood.

---

### 3.4 TiKV

**Role**: Distributed server-mode alternative (worth evaluating)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `tikv-client` -- official Rust client. Async (Tokio). Provides `RawClient` and `TransactionClient`. **Caveat**: the README states "not suitable for production use -- APIs are not yet stable and the crate has not been thoroughly tested in real-life use." SurrealDB maintains a fork (`surrealdb/tikv-client`) with production patches |
| **Transaction model** | ACID with MVCC. Percolator-based distributed transactions. Optimistic and pessimistic locking modes. Snapshot isolation by default; serializable via `for_update` locks. Timestamp acquisition requires an RTT to the Placement Driver (PD) |
| **Embeddability** | Not embeddable. Requires a TiKV cluster (minimum 3 TiKV nodes + 3 PD nodes for production). Heavyweight deployment |
| **Operational model** | Distributed cluster. PD (Placement Driver) manages scheduling, region splitting, load balancing. Auto-sharding via Raft groups. TiKV is written in Rust but the cluster is complex to operate. TiDB Cloud offers managed hosting (but includes full TiDB SQL layer) |
| **Performance** | Single-key read: ~1-3ms (network + Raft). Write + Raft commit: ~5-15ms. Higher latency than FDB due to Raft consensus per write. Throughput scales horizontally. Timestamp acquisition adds ~1 RTT per transaction |
| **Schema mapping** | Raw KV or transactional KV. Same situation as FDB -- no structured layer. TiDB provides SQL on top, but using full TiDB defeats the purpose of a lightweight backing store |
| **Audit log** | Ordered KV scans work. MVCC provides historical versions. But the operational overhead of running a TiKV cluster for audit storage is hard to justify vs PostgreSQL or FDB |

**Known issues**:
- Official Rust client is explicitly marked as not production-ready
- Minimum 6-node cluster (3 PD + 3 TiKV) for production is heavy
- Higher write latency than FDB due to Raft consensus model
- PD dependency adds a single point of coordination
- SurrealDB fork diverges from upstream -- uncertain long-term alignment

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `tikv-client` | 0.3.x | Yes (tokio) | Official, explicitly not production-ready |
| `surrealdb/tikv-client` | fork | Yes (tokio) | SurrealDB's production fork |

**Verdict**: Defer. The non-production-ready client, heavy operational requirements, and higher latency make TiKV a poor fit compared to FDB for the distributed KV role. If FDB proves unsuitable, TiKV is the next distributed candidate to revisit.

---

### 3.5 DuckDB

**Role**: Query/aggregation backend (worth evaluating)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `duckdb` v1.10500.0 -- official bindings, actively maintained. FFI to C++ DuckDB library. Features: vtab, Arrow integration, appender for bulk insert. Async wrapper: `async-duckdb` (background thread, like async-sqlite) |
| **Transaction model** | ACID with MVCC and OCC. Supports `BEGIN TRANSACTION` / `COMMIT` / `ROLLBACK`. Designed for analytical (OLAP) workloads -- transactions are correct but optimized for large reads, not high-concurrency small writes |
| **Embeddability** | Excellent. In-process, single-file or in-memory. Compiles into binary via bundled C++ library. Similar deployment model to SQLite |
| **Operational model** | Embedded only. No server mode, no replication, no clustering. Purely in-process analytical engine |
| **Performance** | Analytical reads: 10-100x faster than SQLite for scans and aggregations. Columnar storage excels at `GROUP BY`, `COUNT`, `SUM` over large datasets. Single-row point lookups: slower than SQLite (columnar overhead). Write throughput: optimized for batch appends, not individual inserts |
| **Schema mapping** | Full SQL with rich types. `STRUCT`, `LIST`, `MAP` types map well to nested entities. But: designed for analytics, not OLTP. No row-level locking, no advisory locks, no concurrent writer optimization |
| **Audit log** | Excellent for **querying** audit logs (analytical scans, time-range aggregations, actor/operation breakdowns). Poor for **writing** audit logs (not optimized for high-frequency individual appends). Best used as a read-only analytical projection of audit data written elsewhere |

**Known issues**:
- C++ library is large (~50MB compiled) -- increases binary size significantly
- Not designed for OLTP workloads; concurrent small writes will perform poorly
- `async-duckdb` is a community wrapper, not official
- In-process only -- cannot share a DuckDB instance across Axon server instances

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `duckdb` | 1.10500.0 | No (sync) | Official, Arrow integration, vtab support |
| `async-duckdb` | 0.2.x | Yes (tokio, async-std) | Community wrapper, background thread |

**Verdict**: Not suitable as a primary backing store. Potentially valuable as a P2 analytical query engine for audit log analysis and collection aggregations (the "niflheim bridge" scenario). Defer to post-V1 evaluation.

---

### 3.6 RocksDB / Fjall

**Role**: Embedded write-optimized store, audit log backend (worth evaluating)

#### 3.6.1 RocksDB

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `rust-rocksdb` v0.24.x (primary, zaidoon1 fork) -- FFI to C++ RocksDB. Features: transactions, column families, compression, backups. Active maintenance. Alternative: `rocksdb` crate (original, less actively maintained) |
| **Transaction model** | OptimisticTransactionDB and TransactionDB (pessimistic). Column families provide keyspace isolation. WriteBatch for atomic multi-key writes. Snapshot reads for consistent views |
| **Embeddability** | Excellent. In-process, directory-based storage. C++ library linked via FFI. Well-proven in production (used by TiKV, CockroachDB, many others) |
| **Operational model** | Embedded only. No built-in replication or clustering. Applications must build their own replication (as TiKV does with Raft on top of RocksDB) |
| **Performance** | Optimized for write-heavy workloads (LSM-tree). Point reads: ~1-10us in-process. Sequential writes: extremely fast (append to memtable). Range scans: good with bloom filters. Compaction can cause latency spikes |
| **Schema mapping** | Raw bytes KV. Same as FDB -- no structured layer. Column families can separate entities, links, audit entries. Prefix-based key encoding for organized scans |
| **Audit log** | Excellent. LSM-tree is inherently append-optimized. Sequential key writes (timestamp-prefixed) are the best case for LSM. Column family isolation keeps audit I/O separate from entity I/O. Compaction handles space reclamation |

**Known issues**:
- C++ dependency adds build complexity
- Compaction storms can cause latency spikes (tunable but requires expertise)
- No built-in transactions across column families in the optimistic mode (WriteBatch covers this for most cases)
- Memory usage requires careful tuning (block cache, memtable sizes)

#### 3.6.2 Fjall

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `fjall` v3.1.x -- **pure Rust**, no FFI. LSM-tree based. Cross-keyspace atomic writes. Serializable transactions (single-writer and multi-writer OCC modes). Actively maintained through 2025; feature development winding down in 2026 |
| **Transaction model** | Serializable via single-writer (trivially serializable) or multi-writer OCC. Cross-keyspace atomic semantics. This maps directly to Axon's OCC model from FEAT-008 |
| **Embeddability** | Excellent. Pure Rust, no external dependencies. Compiles cleanly, small binary footprint. Simpler build than RocksDB |
| **Operational model** | Embedded only. No replication or clustering |
| **Performance** | Comparable to RocksDB for common workloads. Pure Rust means no FFI overhead. Less battle-tested at extreme scale but adequate for Axon's target range (100K-10M entities) |
| **Schema mapping** | Raw bytes KV. Keyspace per logical concern (entities, links, audit). Same encoding approach as RocksDB/FDB |
| **Audit log** | Same LSM advantages as RocksDB. Pure Rust simplifies the build story. Compaction filters (v3.1) enable retention policies at the storage level |

**Known issues**:
- Less battle-tested than RocksDB (no production use at the scale of TiKV/CockroachDB)
- Active feature development winding down -- maintenance-mode risk
- Smaller community than RocksDB
- No equivalent of RocksDB's extensive tuning knobs for extreme workloads

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `rust-rocksdb` | 0.24.x | No (sync) | FFI to C++, feature-rich, zaidoon1 fork |
| `rocksdb` | 0.22.x | No (sync) | Original crate, less active |
| `fjall` | 3.1.x | No (sync) | Pure Rust, serializable transactions, compaction filters |

**Verdict**: Fjall is the more interesting option for Axon -- pure Rust, serializable OCC, cross-keyspace atomicity, and no FFI. It could serve as the embedded-mode storage engine (replacing SQLite for the entity/link/audit storage layer) or as a dedicated audit log backend alongside SQLite/Postgres for entity storage. RocksDB is the proven fallback if Fjall's maturity proves insufficient. Include both in spike benchmarks.

---

### 3.7 Sled

**Role**: Embedded KV (worth evaluating, with caveats)

| Dimension | Assessment |
|-----------|------------|
| **Rust crate** | `sled` v0.34.x -- pure Rust, B+ tree based. API: `Tree`, `Batch`, `CompareAndSwap`. Merge operators for atomic read-modify-write |
| **Transaction model** | Single-tree transactions with serializable isolation. Multi-tree transactions via `sled::transaction::Transactional` trait. CAS (compare-and-swap) for OCC patterns |
| **Embeddability** | Excellent. Pure Rust, in-process, zero dependencies |
| **Operational model** | Embedded only |
| **Performance** | Competitive with RocksDB for some workloads. High memory usage. Space amplification issues |
| **Schema mapping** | Raw bytes KV. Same approach as other KV stores |
| **Audit log** | Sequential writes are reasonable. Space amplification is a concern for large audit logs |

**Known issues**:
- **Maintenance status is the critical concern**. Sled is pre-1.0 with an unstable on-disk format. The storage subsystem is being rewritten as part of the `komora`/`marble` project, but this has been in progress for years with no release timeline
- A November 2024 assessment noted that "version maintenance has stalled"
- High space amplification and write amplification compared to LSM-tree designs
- On-disk format will require manual migration before 1.0
- The `marble` rewrite may never ship, leaving sled in permanent beta

**Crate versions**:

| Crate | Version | Async | Notes |
|-------|---------|-------|-------|
| `sled` | 0.34.7 | No (sync) | Pre-1.0, unstable on-disk format, uncertain future |

**Verdict**: Eliminate. The maintenance uncertainty, unstable on-disk format, and availability of Fjall as a superior pure-Rust alternative make sled too risky. Do not include in spike benchmarks.

---

## 4. Comparison Matrix

### 4.1 Feature Comparison

| Capability | PostgreSQL | SQLite/libSQL | FoundationDB | TiKV | DuckDB | RocksDB | Fjall | Sled |
|------------|:----------:|:-------------:|:------------:|:----:|:------:|:-------:|:-----:|:----:|
| Rust async driver | sqlx (excellent) | libsql (good) | foundationdb (adequate) | tikv-client (poor) | async-duckdb (adequate) | None (sync) | None (sync) | None (sync) |
| Pure Rust driver | Yes (sqlx) | Partial (libsql FFI) | No (C FFI) | Yes | No (C++ FFI) | No (C++ FFI) | **Yes** | **Yes** |
| Serializable isolation | SSI | Yes (default) | Yes (strict) | Via for_update | Yes (MVCC) | OptimisticTxn | Yes (OCC) | Yes (single-tree) |
| Multi-key transactions | Yes | Yes | Yes (10MB/5s limit) | Yes | Yes | WriteBatch | Yes (cross-keyspace) | Yes (multi-tree) |
| Embeddable | No | **Yes** | No | No | **Yes** | **Yes** | **Yes** | **Yes** |
| Distributed | Via replicas | Via libSQL/Turso | **Yes (native)** | **Yes (native)** | No | No | No | No |
| Managed offerings | Many (RDS, Neon, etc.) | Turso | None (self-host) | TiDB Cloud | None | None | None | None |
| Schema/SQL support | **Full SQL** | **Full SQL** | None (raw KV) | None (raw KV) | **Full SQL** | None (raw KV) | None (raw KV) | None (raw KV) |
| JSON/nested data | JSONB (excellent) | json() (adequate) | Bytes (manual) | Bytes (manual) | STRUCT/LIST (good) | Bytes (manual) | Bytes (manual) | Bytes (manual) |
| Append-optimized | No (B-tree) | No (B-tree) | Yes (LSM) | Yes (RocksDB) | No (columnar) | **Yes (LSM)** | **Yes (LSM)** | No (B+ tree) |

### 4.2 Suitability by Axon Role

| Role | Best Fit | Runner-up | Notes |
|------|----------|-----------|-------|
| **Embedded primary store** | SQLite/libSQL | Fjall | SQL support reduces entity-graph mapping effort |
| **Server-mode primary store** | PostgreSQL | FoundationDB | Postgres is simpler; FDB is more aligned with correctness-first |
| **Distributed store** | FoundationDB | TiKV (deferred) | FDB has better latency and proven track record |
| **Audit log (embedded)** | Fjall | SQLite | LSM-tree is append-optimized; SQLite is simpler |
| **Audit log (server)** | PostgreSQL | FoundationDB | Partition-based retention in Postgres; ordered KV in FDB |
| **Analytical queries** | DuckDB (P2) | PostgreSQL | DuckDB for aggregation workloads; Postgres adequate for V1 |

---

## 5. Recommended Architecture

### 5.1 V1 Architecture (Minimum Viable)

```
Embedded mode:  Axon -> SQLite/libSQL (entities + links + audit, single file)
Server mode:    Axon -> PostgreSQL (entities + links + audit, connection pool)
```

This is the simplest path to V1. Both backends support SQL, ACID transactions, and are well-understood. The storage abstraction trait isolates Axon from backend specifics.

### 5.2 V1+ Architecture (Post-spike, if FDB proves viable)

```
Embedded mode:  Axon -> libSQL (entities + links) + Fjall (audit log)
Server mode:    Axon -> FoundationDB (entities + links + audit)
                   or -> PostgreSQL (entities + links + audit)
```

FDB provides stronger consistency guarantees and aligns with the DST story (see `foundationdb-dst-research.md`). However, it requires building a structured storage layer on raw KV. This architecture is only justified if the spike demonstrates that:
1. The structured layer can be built in <4 weeks
2. FDB's 5s/10MB transaction limits do not constrain Axon's use cases
3. Operational complexity is manageable

### 5.3 Storage Abstraction Trait

The spike must validate that a common trait can abstract over SQL (Postgres/SQLite) and KV (FDB/Fjall) backends. Sketch:

```rust
#[async_trait]
pub trait BackingStore: Send + Sync + 'static {
    type Transaction: StoreTransaction;

    async fn begin(&self) -> Result<Self::Transaction>;
    async fn get_entity(&self, collection: &str, id: &EntityId) -> Result<Option<Entity>>;
    async fn scan_entities(&self, collection: &str, filter: &Filter) -> Result<Vec<Entity>>;
    async fn get_links(&self, source: &EntityRef, link_type: &str) -> Result<Vec<Link>>;
    async fn traverse_links(
        &self,
        source: &EntityRef,
        link_type: &str,
        depth: u32,
    ) -> Result<TraversalResult>;
    async fn append_audit(&self, entries: &[AuditEntry]) -> Result<()>;
    async fn scan_audit(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>>;
}

#[async_trait]
pub trait StoreTransaction: Send {
    async fn put_entity(&mut self, collection: &str, entity: &Entity) -> Result<()>;
    async fn delete_entity(&mut self, collection: &str, id: &EntityId) -> Result<()>;
    async fn put_link(&mut self, link: &Link) -> Result<()>;
    async fn delete_link(&mut self, link_id: &LinkId) -> Result<()>;
    async fn append_audit(&mut self, entries: &[AuditEntry]) -> Result<()>;
    async fn commit(self) -> Result<()>;
    async fn rollback(self) -> Result<()>;
}
```

The spike should implement this trait for at least SQLite and PostgreSQL, and prototype the FDB implementation to assess structured-layer complexity.

---

## 6. Spike Metrics Framework

### 6.1 Benchmark Environment

All benchmarks run on a single machine unless noted. Configuration:

| Parameter | Value |
|-----------|-------|
| CPU | 8 cores (or equivalent cloud instance) |
| Memory | 32 GB |
| Storage | NVMe SSD |
| Entity size | Small (~500 bytes), Medium (~5KB), Large (~50KB) |
| Collection sizes | 10K, 100K, 1M, 10M entities |
| Link density | 5 links per entity average (power-law distribution) |
| Rust toolchain | Stable, release mode, LTO |
| Async runtime | Tokio multi-threaded |

### 6.2 Benchmark Definitions

Each benchmark measures the specified operation in isolation, with a warm cache, after initial data loading.

#### BM-01: Single Entity Read Latency

| Parameter | Value |
|-----------|-------|
| **Operation** | Read a single entity by ID from a collection |
| **Setup** | Pre-load N entities into collection |
| **Measurement** | p50, p95, p99, p999 latency over 100K operations |
| **Variants** | N = 10K, 100K, 1M; entity size = small, medium, large |
| **Target** | p99 < 5ms (embedded), p99 < 10ms (server) |
| **Report** | Latency histogram per backend per variant |

#### BM-02: Single Entity Write Latency

| Parameter | Value |
|-----------|-------|
| **Operation** | Create or update a single entity (including version check, schema validation, audit entry) |
| **Setup** | Pre-load N entities; updates target random existing entities with version check |
| **Measurement** | p50, p95, p99, p999 latency over 50K operations |
| **Variants** | N = 10K, 100K, 1M; entity size = small, medium, large; create vs update |
| **Target** | p99 < 10ms (embedded), p99 < 15ms (server) |
| **Report** | Latency histogram, includes audit write time |

#### BM-03: Multi-Entity Transaction Commit Latency

| Parameter | Value |
|-----------|-------|
| **Operation** | Atomic transaction: read M entities, update M entities, create 1 audit batch |
| **Setup** | Pre-load 100K entities. Transactions update disjoint entity sets (no contention) |
| **Measurement** | p50, p95, p99 latency over 10K transactions |
| **Variants** | M = 2, 5, 10, 20 entities per transaction |
| **Target** | p99 < 20ms for M=5 |
| **Report** | Latency vs transaction size curve per backend |

#### BM-04: Multi-Entity Transaction Under Contention

| Parameter | Value |
|-----------|-------|
| **Operation** | Same as BM-03 but with contention: C concurrent tasks targeting overlapping entity sets |
| **Setup** | 100K entities. Hot set of 100 entities. Each transaction touches 2-5 entities from hot set |
| **Measurement** | p50, p99 latency; conflict rate; retry count; successful commits/sec |
| **Variants** | C = 10, 50, 100, 500 concurrent tasks |
| **Target** | Conflict rate < 5% at C=10; graceful degradation at C=100+ |
| **Report** | Throughput vs concurrency curve; conflict rate vs concurrency curve |

#### BM-05: Audit Log Append Throughput

| Parameter | Value |
|-----------|-------|
| **Operation** | Append audit entries (simulating entity mutation audit records) |
| **Setup** | Empty audit log. Each entry ~1KB (operation, actor, timestamp, before/after diff) |
| **Measurement** | Sustained appends/sec over 60 seconds; p99 append latency |
| **Variants** | Single-threaded append; batched (10, 100 entries per batch); concurrent (10, 100 writers) |
| **Target** | > 10K appends/sec sustained; p99 < 5ms per append |
| **Report** | Throughput over time (watch for compaction stalls in LSM stores) |

#### BM-06: Audit Log Scan Performance

| Parameter | Value |
|-----------|-------|
| **Operation** | Scan audit entries by: (a) entity ID, (b) time range, (c) actor, (d) full scan |
| **Setup** | Pre-load 1M audit entries spanning 30 simulated days |
| **Measurement** | Time to return matching entries; throughput (entries/sec) |
| **Variants** | Result set sizes: 10, 100, 1K, 10K entries |
| **Target** | < 100ms for typical queries (single entity, recent time range) per FEAT-003 |
| **Report** | Query time vs result set size per backend |

#### BM-07: Entity Collection Scan and Filter

| Parameter | Value |
|-----------|-------|
| **Operation** | Query entities in a collection with field-level filter (`status = "pending" AND priority > 3`) |
| **Setup** | Pre-load N entities with varied field values. Indexes on filtered fields where supported |
| **Measurement** | Time to return matching entities; throughput |
| **Variants** | N = 10K, 100K, 1M; selectivity = 1%, 10%, 50% |
| **Target** | < 50ms for 1% selectivity on 100K entities |
| **Report** | Query time vs collection size vs selectivity |

#### BM-08: Link Traversal Performance

| Parameter | Value |
|-----------|-------|
| **Operation** | Traverse typed links from a source entity to depth D |
| **Setup** | Pre-load 100K entities with 500K links (avg 5 links/entity, power-law). Build a realistic dependency graph |
| **Measurement** | Traversal time; number of entities visited; memory usage |
| **Variants** | D = 1, 3, 5; fan-out = low (2 avg), medium (5 avg), high (20 avg) |
| **Target** | D=1: < 5ms; D=3: < 50ms; D=5: < 200ms (per FEAT-007 constraints) |
| **Report** | Traversal time vs depth; visited nodes vs depth (exponential growth characterization) |

#### BM-09: Storage Efficiency

| Parameter | Value |
|-----------|-------|
| **Operation** | Measure on-disk storage after loading a known dataset |
| **Setup** | Load exactly 100K entities (500 bytes each = 50MB raw) + 500K links + 200K audit entries |
| **Measurement** | Total bytes on disk; bytes per entity; overhead ratio (disk / raw) |
| **Variants** | After initial load; after 10x update churn (200K updates generating audit entries) |
| **Target** | Overhead ratio < 3x for SQL stores; < 5x for KV stores (due to key encoding) |
| **Report** | Storage breakdown by component (entities, links, audit, indexes, overhead) |

#### BM-10: Concurrent Writer Throughput

| Parameter | Value |
|-----------|-------|
| **Operation** | Sustained mixed workload: 80% reads, 15% single-entity writes, 5% multi-entity transactions |
| **Setup** | Pre-load 100K entities. C concurrent agent tasks |
| **Measurement** | Operations/sec sustained over 60 seconds; p50, p99 latency; error rate |
| **Variants** | C = 1, 10, 100, 1000 concurrent tasks |
| **Target** | Linear scaling to C=10; graceful degradation to C=100; no crashes at C=1000 |
| **Report** | Throughput vs concurrency curve; latency vs concurrency curve |

### 6.3 Benchmark Implementation

```
axon-spike-001/
  Cargo.toml
  src/
    main.rs                    # CLI: select backend + benchmark + parameters
    backends/
      mod.rs                   # BackingStore trait definition
      sqlite.rs                # SQLite/libSQL implementation
      postgres.rs              # PostgreSQL implementation
      foundationdb.rs          # FoundationDB implementation (prototype)
      fjall.rs                 # Fjall implementation
    benchmarks/
      mod.rs                   # Benchmark runner framework
      bm01_read_latency.rs
      bm02_write_latency.rs
      bm03_txn_commit.rs
      bm04_txn_contention.rs
      bm05_audit_append.rs
      bm06_audit_scan.rs
      bm07_collection_scan.rs
      bm08_link_traversal.rs
      bm09_storage_efficiency.rs
      bm10_concurrent_writers.rs
    data_gen/
      mod.rs                   # Entity, link, and audit entry generators
    reporting/
      mod.rs                   # HDR histogram collection, CSV/JSON output
```

### 6.4 Reporting Format

Each benchmark produces:
- **HDR histogram** (via `hdrhistogram` crate) for latency distributions
- **CSV** with columns: `backend, benchmark, variant, p50_us, p95_us, p99_us, p999_us, throughput_ops_sec, error_rate`
- **Summary table** comparing backends side-by-side for each benchmark

---

## 7. Decision Framework

After spike execution, score each backend on a 1-5 scale for each criterion:

| Criterion | Weight | PostgreSQL | SQLite/libSQL | FoundationDB | Fjall |
|-----------|--------|------------|---------------|--------------|-------|
| Rust ecosystem quality | 3x | | | | |
| Transaction model fit | 3x | | | | |
| Embeddability | 2x | | | | |
| Operational simplicity | 2x | | | | |
| Read latency (BM-01) | 2x | | | | |
| Write latency (BM-02) | 2x | | | | |
| Transaction latency (BM-03/04) | 3x | | | | |
| Audit throughput (BM-05/06) | 2x | | | | |
| Link traversal (BM-08) | 2x | | | | |
| Storage efficiency (BM-09) | 1x | | | | |
| Concurrent throughput (BM-10) | 2x | | | | |
| **Weighted total** | | | | | |

TiKV, DuckDB, and Sled are excluded from scoring: TiKV and DuckDB are deferred to post-V1; Sled is eliminated.

---

## 8. Risks and Open Questions

### Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| FDB structured layer takes >4 weeks to build | Medium | High | Time-box to 2 weeks. If insufficient, fall back to PostgreSQL for server mode |
| Fjall maintenance stalls completely | Low | Medium | RocksDB is a drop-in replacement for the KV layer. Fjall's pure-Rust advantage is nice-to-have, not critical |
| SQLite single-writer bottleneck limits embedded throughput | Medium | Medium | libSQL improves this. Alternatively, use Fjall for writes + SQLite for reads |
| Storage abstraction trait is too leaky | Medium | High | Design the trait around Axon's operations (not storage primitives). Accept that SQL and KV impls will differ significantly behind the trait |
| Benchmark results are misleading due to synthetic workloads | Low | Medium | Include a realistic "bead lifecycle" benchmark that exercises the full create-update-link-query-audit cycle |

### Open Questions

1. **Should the audit log use a different backend than entities?** Separation (e.g., Fjall for audit, SQLite for entities in embedded mode) adds complexity but optimizes for append-heavy audit writes vs read-heavy entity access.

2. **How much does FDB's 5-second transaction limit constrain schema migrations?** Schema changes that touch many entities may need to be broken into batches. Is this acceptable?

3. **Is libSQL mature enough to replace rusqlite?** The async-native API is appealing but the crate is pre-1.0. What is the fallback plan if we hit stability issues?

4. **Should the spike test cross-backend scenarios?** e.g., entities in PostgreSQL + audit log in FoundationDB. This adds spike scope but may reveal architectural insights.

5. **What is the upgrade path from SQLite (embedded) to PostgreSQL (server)?** The storage abstraction must support data export/import between backends. Should this be validated in the spike?

---

## 9. Success Criteria

The spike is complete when:

- [ ] `BackingStore` trait is defined and implemented for SQLite and PostgreSQL
- [ ] `BackingStore` trait is prototyped for FoundationDB (entity CRUD + transactions, not necessarily full feature parity)
- [ ] `BackingStore` trait is prototyped for Fjall (entity CRUD + audit log)
- [ ] All 10 benchmarks (BM-01 through BM-10) are implemented and produce results for at least SQLite and PostgreSQL
- [ ] At least BM-01, BM-02, BM-05, and BM-08 produce results for FDB and Fjall
- [ ] Decision matrix is scored based on measured data
- [ ] Recommendation document identifies the V1 embedded and server-mode defaults
- [ ] FDB structured-layer complexity is estimated with a confidence interval

---

## 10. Timeline

| Week | Activities |
|------|-----------|
| 1 | Define `BackingStore` trait. Implement SQLite and PostgreSQL backends. Implement data generators. Set up benchmark harness |
| 2 | Implement all 10 benchmarks for SQLite and PostgreSQL. Prototype FDB and Fjall backends (entity CRUD + audit). Run BM-01, BM-02, BM-05, BM-08 for FDB and Fjall |
| 3 | Run full benchmark suite. Analyze results. Score decision matrix. Write recommendation. Present findings |

---

## References

- [sqlx - Async Rust SQL toolkit](https://github.com/launchbadge/sqlx)
- [rusqlite - Ergonomic SQLite bindings](https://github.com/rusqlite/rusqlite)
- [libsql - Turso's SQLite fork](https://docs.turso.tech/sdk/rust/reference)
- [foundationdb-rs - FoundationDB Rust client](https://github.com/foundationdb-rs/foundationdb-rs)
- [fdb-rs - Tokio-native FDB bindings](https://fdb-rs.github.io/blog/introducing-fdb-crate/)
- [FoundationDB Known Limitations](https://apple.github.io/foundationdb/known-limitations.html)
- [FoundationDB Record Layer paper](https://www.foundationdb.org/files/record-layer-paper.pdf)
- [FDB Record Layer Rust WIP](https://forums.foundationdb.org/t/rust-fdb-record-layer-work-in-progress-repository/3765)
- [tikv/client-rust](https://github.com/tikv/client-rust)
- [TiKV Architecture Overview](https://tikv.org/docs/7.1/reference/architecture/overview/)
- [duckdb-rs - DuckDB Rust bindings](https://github.com/duckdb/duckdb-rs)
- [rust-rocksdb](https://github.com/zaidoon1/rust-rocksdb)
- [fjall - Pure Rust LSM storage](https://github.com/fjall-rs/fjall)
- [sled - Embedded database](https://github.com/spacejam/sled)
- [Axon FoundationDB DST Research](../../../00-discover/foundationdb-dst-research.md)
