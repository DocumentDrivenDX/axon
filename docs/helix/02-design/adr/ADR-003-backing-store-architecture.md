---
dun:
  id: ADR-003
  depends_on:
    - helix.prd
    - ADR-001
    - FEAT-003
    - FEAT-008
    - SPIKE-001
---
# ADR-003: Backing Store Architecture — SQLite + PostgreSQL with Application-Layer Audit

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-04 | Accepted | Erik LaBianca | FEAT-003, FEAT-008, SPIKE-001 | High |

## Context

Axon servers are stateless. They delegate durability, replication, and crash recovery to backing stores. The StorageAdapter trait abstracts the backing store so Axon can run embedded (development, edge, single-user) or as a server (production, multi-user).

The audit log and change data capture (CDC) are foundational to Axon — principle P2 says "audit is not optional" and the PRD lists audit-first architecture as value proposition #2. The question is whether audit/CDC happens at the storage layer (triggers, WAL tailing, CDC connectors) or the application layer (Axon produces audit entries as part of every write path).

| Aspect | Description |
|--------|-------------|
| Problem | Choose V1 backing stores and decide where audit/CDC lives in the architecture |
| Current State | SPIKE-001 evaluated 7 candidates. 83 tests pass against memory + SQLite backends |
| Requirements | Embedded mode (zero-config), server mode (multi-client), ACID transactions, audit on every mutation, CDC for downstream consumers |

## Decision

### V1 Backing Stores

| Backend | Mode | Crate | Role |
|---------|------|-------|------|
| **SQLite** | Embedded | `rusqlite` v0.32 (bundled) | Development, testing, single-user, edge, CLI |
| **PostgreSQL** | Server | `postgres` v0.19 (sync, with `with-serde_json-1`) | Production, multi-user, existing infrastructure |

> **Implementation note (2026-04-05):** The original ADR specified `libsql` v0.9.x
> and `sqlx` v0.8.x. During implementation, `rusqlite` v0.32 was chosen for SQLite
> (bundled build, simpler API, no async overhead for embedded mode) and the synchronous
> `postgres` v0.19 crate was chosen for PostgreSQL (simpler integration with the
> synchronous `StorageAdapter` trait using `RefCell<Client>`). Both choices pass the
> full L4 conformance suite and L2 backend parity tests.

Both backends implement the same `StorageAdapter` trait. Both pass the identical test suite (L4 backend conformance).

### Future Backends (Spike Required)

| Backend | When | Gating Question |
|---------|------|-----------------|
| **FoundationDB** | After V1, if scale-out is needed | Can we build a usable structured layer on raw KV in 2 weeks? (SPIKE-001 time-box) |
| **Fjall** | After V1, if audit write throughput is a bottleneck | Does LSM-tree append performance justify a split-backend architecture? |

### Application-Layer Audit and CDC

**Audit and CDC happen at the Axon application layer, not the storage layer.** This is a deliberate architectural choice, not a compromise.

#### Why application-layer, not storage-layer

| Storage-layer approach | Problem |
|----------------------|---------|
| PostgreSQL triggers | Capture SQL-level mutations, not semantic operations. A trigger sees `UPDATE jsonb SET data = '...'`, not "agent-X updated invoice status from draft to approved with reason 'budget review passed'". No actor attribution, no operation semantics, no structured diff, no metadata |
| PostgreSQL logical replication / WAL | Same problem — captures physical row changes, not application intent. Also couples Axon to Postgres internals |
| SQLite triggers | Same semantic gap. Also, no built-in replication or CDC export |
| FoundationDB watch API | Notifies that a key changed, but doesn't provide old value, actor, or semantics |
| External CDC (Debezium) | Adds operational complexity, eventual consistency, and still only captures physical changes |

**Application-layer audit captures what matters:**
- **Actor**: which agent, user, or API key performed the operation
- **Operation semantics**: `entity.create`, `entity.update`, `entity.delete`, `collection.create` — not `INSERT INTO entities`
- **Before/after state**: full entity state, not a SQL diff
- **Structured diff**: which fields changed, with old and new values
- **Metadata**: reason, correlation ID, agent session, transaction ID
- **No-orphan guarantee**: audit entries are flushed post-commit — rolled-back transactions never produce audit entries, and audit entries are never written for uncommitted mutations

#### Audit write path

```
Agent/Client
    │
    ▼
Transaction (application layer)
    │
    ├── 1. Validate entity against schema
    ├── 2. Build AuditEntry (actor, operation, before, after, tx_id) — buffered
    │
    ▼
StorageAdapter.begin_tx()
    │
    ├── 3. Check version (OCC — Phase 1 of commit)
    ├── 4. Apply entity writes (Phase 2 of commit)
    └── 5. commit_tx()  ← entity write is durable here
    │
    ▼  (post-commit)
AuditLog.append()  ← audit entries flushed after storage commit
    └── 6. Write audit entry
```

The audit entry is constructed at the application layer where all semantic context
is available. Entity mutations commit via `StorageAdapter` first; audit entries
are flushed to the `AuditLog` immediately after `commit_tx()` succeeds.

This **post-commit audit strategy** guarantees:
- **No orphans**: rolled-back transactions never produce audit entries
- **Full semantics**: audit entries carry actor, operation type, before/after state, metadata
- **Backend independence**: same audit behavior on memory, SQLite, PostgreSQL

**Trade-off and recovery**: There is a narrow crash-safety window between
`commit_tx()` and the completion of `AuditLog.append()`. If the process dies in
that window, the committed mutation has no audit trail. For V1 (in-memory), this
is acceptable — both entity state and audit log are volatile.

For durable backends (SQLite, PostgreSQL), implementations must close this gap by
one of:
- Writing audit entries to the same database transaction as entity mutations (requires
  audit storage to be integrated into `StorageAdapter`)
- Writing a pre-commit intent record inside the storage transaction; on startup,
  scanning for committed intents without matching audit entries and replaying them

Until a durable backend is implemented, INV-003 (audit completeness) holds for
all mutations where the process remains alive — which covers all non-crash scenarios.

#### CDC as audit log projection

Change data capture for downstream consumers is a **read projection of the audit log**, not a separate mechanism:

```
Audit Log (append-only, ordered)
    │
    ├── CDC Consumer A (polls: give me all entries since cursor X)
    ├── CDC Consumer B (subscribes: stream new entries as they arrive)
    └── CDC Consumer C (filtered: only entity.create on "invoices" collection)
```

This means:
- CDC is free once audit works — it's the same data, different read pattern
- CDC consumers get the same rich semantics as the audit log (actor, diff, metadata)
- No separate CDC infrastructure needed in V1 (consumers poll the audit query API)
- P1 change feeds (FEAT for real-time subscriptions) are a push optimization over the same data

### StorageAdapter Trait

The trait provides transactional primitives. Axon builds audit, OCC, and schema validation on top.

```rust
pub trait StorageAdapter: Send + Sync {
    // Point operations
    fn get(&self, collection: &CollectionId, id: &EntityId) -> Result<Option<Entity>>;
    fn put(&self, collection: &CollectionId, entity: &Entity) -> Result<()>;
    fn delete(&self, collection: &CollectionId, id: &EntityId) -> Result<()>;
    fn count(&self, collection: &CollectionId) -> Result<u64>;

    // Range operations
    fn range_scan(&self, collection: &CollectionId, prefix: &str,
                  start: Option<&str>, end: Option<&str>, limit: Option<usize>)
                  -> Result<Vec<Entity>>;

    // Transactions
    fn begin_tx(&self) -> Result<TxHandle>;
    fn put_in_tx(&self, tx: &TxHandle, collection: &CollectionId, entity: &Entity) -> Result<()>;
    fn delete_in_tx(&self, tx: &TxHandle, collection: &CollectionId, id: &EntityId) -> Result<()>;
    fn commit_tx(&self, tx: TxHandle) -> Result<()>;
    fn abort_tx(&self, tx: TxHandle) -> Result<()>;

    // Optimistic concurrency
    fn compare_and_swap(&self, collection: &CollectionId, entity: &Entity,
                        expected_version: u64) -> Result<()>;
}
```

Key design points:
- `begin_tx` / `commit_tx` / `abort_tx` provide storage-level atomicity
- SQLite: maps to `BEGIN IMMEDIATE` / `COMMIT` / `ROLLBACK`
- PostgreSQL: maps to `BEGIN` / `COMMIT` / `ROLLBACK` with serializable isolation
- Memory: maps to mutex-guarded batch apply
- Entity writes commit via `StorageAdapter`; audit entries are appended post-commit (post-commit audit strategy, see ADR-004)

### Data Layout

Both SQLite and PostgreSQL use the same logical schema:

```sql
-- Entities (one table per collection, or one table with collection column)
CREATE TABLE entities (
    collection  TEXT NOT NULL,
    id          TEXT NOT NULL,
    version     BIGINT NOT NULL,
    data        JSONB NOT NULL,  -- entity body
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL,
    created_by  TEXT,
    updated_by  TEXT,
    PRIMARY KEY (collection, id)
);

-- Links
CREATE TABLE links (
    id              TEXT NOT NULL PRIMARY KEY,
    source_collection TEXT NOT NULL,
    source_id       TEXT NOT NULL,
    target_collection TEXT NOT NULL,
    target_id       TEXT NOT NULL,
    link_type       TEXT NOT NULL,
    metadata        JSONB,
    version         BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL,
    created_by      TEXT,
    UNIQUE (source_collection, source_id, link_type, target_collection, target_id)
);

-- Audit log (append-only)
CREATE TABLE audit_log (
    id              BIGSERIAL PRIMARY KEY,  -- monotonically increasing
    timestamp       TIMESTAMPTZ NOT NULL,
    actor           TEXT NOT NULL DEFAULT 'anonymous',
    operation       TEXT NOT NULL,  -- entity.create, entity.update, etc.
    collection      TEXT NOT NULL,
    entity_id       TEXT,
    transaction_id  TEXT,
    data_before     JSONB,
    data_after      JSONB,
    diff            JSONB,          -- structured field-level diff
    metadata        JSONB,          -- key-value: reason, correlation_id, etc.
);
CREATE INDEX idx_audit_entity ON audit_log (collection, entity_id, id);
CREATE INDEX idx_audit_time ON audit_log (timestamp);
CREATE INDEX idx_audit_actor ON audit_log (actor, id);
CREATE INDEX idx_audit_tx ON audit_log (transaction_id);

-- Collection metadata
CREATE TABLE collections (
    name            TEXT NOT NULL PRIMARY KEY,
    schema_version  BIGINT NOT NULL DEFAULT 1,
    entity_schema   JSONB NOT NULL,  -- JSON Schema document
    link_types      JSONB,           -- Axon link-type definitions
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL,
    entity_count    BIGINT NOT NULL DEFAULT 0
);
```

For SQLite: `JSONB` becomes `TEXT` (JSON stored as text), `BIGSERIAL` becomes `INTEGER PRIMARY KEY AUTOINCREMENT`, `TIMESTAMPTZ` becomes `TEXT` (ISO 8601).

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Storage-layer audit (triggers) | No application code needed for audit | Captures physical changes, not semantic operations. No actor, no diff, no metadata. Different per backend | Rejected: semantic gap is fatal for an audit-first system |
| External CDC (Debezium + Kafka) | Proven at scale, decoupled | Massive operational complexity for V1. Eventually consistent. Still no application semantics | Rejected for V1: revisit as a P2 CDC export target |
| Single backend (Postgres only) | Simpler, one code path | No embedded mode. Developers must run Postgres for `axon init` | Rejected: embedded mode is P0 |
| Single backend (SQLite only) | Embeddable, simple | Single-writer, no concurrent multi-client server mode | Rejected: production needs multi-client |
| **SQLite + PostgreSQL with app-layer audit** | Embedded + server. Audit captures full semantics. CDC is a read projection | Two backends to maintain. App-layer audit adds write-path complexity | **Selected** |

## Consequences

| Type | Impact |
|------|--------|
| Positive | Audit entries carry full semantic context (actor, operation, diff, metadata) regardless of backend. CDC comes free as audit log reads. Embedded mode works with zero external dependencies. Same test suite validates both backends |
| Negative | Two storage backends to maintain and test. Application-layer audit adds ~2ms overhead per write (post-commit append). Must ensure audit write never silently fails; narrow crash window between commit and audit flush |
| Neutral | FoundationDB and fjall remain viable future backends behind the same trait |

## Implementation Impact

| Aspect | Assessment |
|--------|------------|
| Effort | Medium — SQLite adapter exists (needs tx methods). PostgreSQL adapter is new but `sqlx` makes it straightforward |
| Performance | SQLite: <5ms single-entity writes including audit. PostgreSQL: <10ms over network including audit. Both within targets |
| Security | Audit log is append-only at the SQL level (no UPDATE/DELETE granted to the Axon connection role on Postgres) |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Audit write fails silently, leaving gap | Low | Critical | Post-commit audit strategy: audit entries are flushed only after `commit_tx()` succeeds, so no audit entry is written for a rolled-back mutation. For durable backends, INV-003 (audit completeness) recovery invariant must be implemented (intent record or same-transaction audit). INV-003 runs in CI |
| SQLite single-writer bottleneck in embedded mode | Medium | Low | Embedded mode is single-user by design. WAL mode enables concurrent reads |
| PostgreSQL connection pool exhaustion under load | Medium | Medium | `sqlx` pool with configurable max connections. Health check endpoint monitors pool utilization |
| Backend behavior divergence | Medium | Medium | L4 backend conformance: identical parameterized test suite. Any divergence is a bug |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| INV-003 (audit completeness) passes on both backends | Any audit gap detected |
| L4 conformance: 100% tests pass on both SQLite and Postgres | Any backend-specific failure |
| Write latency p99 within targets (SQLite <5ms, Postgres <10ms including audit) | If audit overhead exceeds 2ms |

## References

- [SPIKE-001: Backing Store Evaluation](../spikes/SPIKE-001-backing-store-evaluation.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md)
- [Technical Requirements](../../01-frame/technical-requirements.md)
- [FoundationDB DST Research](../../00-discover/foundationdb-dst-research.md)
