---
dun:
  id: ADR-004
  depends_on:
    - helix.prd
    - ADR-003
    - FEAT-008
---
# ADR-004: Transaction Model — Optimistic Concurrency Control

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-008, ADR-003, FEAT-003 | High |

## Context

Axon guarantees ACID transactions for agentic workloads. The core question is which
concurrency control mechanism provides the correctness guarantees required by FEAT-008
while matching the workload characteristics of agentic applications.

| Aspect | Description |
|--------|-------------|
| Problem | Choose a concurrency control mechanism for multi-entity atomic transactions |
| Workload | Agentic applications — typically low contention, short-lived operations on bounded entity sets |
| Requirements | Serializable isolation, deadlock-free, sub-20ms p99 for 2–5 entity transactions, embedded and server mode |
| Prior context | ADR-003 selected SQLite + PostgreSQL with application-layer audit. The transaction model must work across both backends behind the StorageAdapter trait |

## Decision

**Optimistic Concurrency Control (OCC) with version vectors at the entity level.**

Every entity carries a monotonically increasing `version` field. Writes include the
caller's expected version. At commit time, Axon compares the stored version against
the expected version for every entity in the transaction. Any mismatch aborts the
entire transaction — no partial commits.

### Protocol

```
Begin:
  - Allocate transaction ID (monotonic counter)
  - Create write buffer (Vec<WriteOp>)

Stage (create / update / delete):
  - Record (entity, expected_version, data_before, mutation_type) in write buffer
  - No storage access at stage time

Commit:
  - For each write op, verify stored version == expected_version
  - If any mismatch: abort entire transaction, return ConflictError with
    the conflicting entity's current state
  - If all versions match:
      - Apply all entity writes (version + 1)
      - Write all audit entries (shared transaction_id)
      - Commit storage transaction (atomic via StorageAdapter)
```

### Version Semantics

| Expected Version | Meaning |
|-----------------|---------|
| `0` | Entity must not exist (create guard) |
| `n > 0` | Entity must be at exactly version `n` |

### Conflict Response

On version conflict, Axon returns `AxonError::Conflict` containing:
- The entity ID and collection that caused the conflict
- The current version in storage
- The current entity state (so the caller can merge and retry)
- A `retryable: true` flag (version conflict is always retryable)

Schema violations return `AxonError::SchemaViolation` with `retryable: false`.

### Isolation Guarantees

OCC as implemented provides **serializable isolation** for write transactions:

- Writes are buffered locally; no storage reads occur at stage time
- Version checks at commit detect any interleaving write that could produce
  an inconsistent result
- Because conflicting transactions are aborted (not queued), there are no
  dirty reads, non-repeatable reads, phantom reads, or write skew
- The commit phase runs under a storage-level transaction (SQLite `BEGIN
  IMMEDIATE`, PostgreSQL `SERIALIZABLE`), preventing concurrent commits from
  racing at the storage layer

### Audit Integration

Every committed transaction assigns a single `transaction_id` to all audit
entries it produces. Audit entries are written in the same storage transaction
as the entity mutations (see ADR-003 audit write path). If the storage commit
fails, both entity and audit writes roll back atomically.

### Limits and Timeouts

Per FEAT-008:
- **Maximum 100 operations per transaction** — the 101st `stage_*` call returns
  `InvalidArgument`. This prevents unbounded write buffers and forces callers to
  batch appropriately.
- **30-second timeout** (configurable) — transactions open beyond the timeout are
  aborted. The `Transaction` struct carries a creation timestamp; the commit path
  checks elapsed time before entering the version-check loop.

## Alternatives Considered

### 1. Pessimistic Locking (SELECT FOR UPDATE)

Acquire exclusive locks on entities at read time; hold until commit.

| Pros | Cons |
|------|------|
| No retries required — conflicts are prevented, not detected | Deadlock risk: T1 locks A then B; T2 locks B then A → deadlock |
| Predictable commit latency when contention is high | Locks held for transaction duration increase latency for other writers |
| Natural fit for PostgreSQL (`SELECT FOR UPDATE`) | Requires lock manager or database-level locking; memory adapter cannot support it without significant complexity |
| | Mismatched with SQLite (single-writer WAL mode has no row-level locking) |

**Rejected.** Deadlock risk and poor fit with SQLite. Agentic workloads have low
contention — OCC retry cost is negligible in practice. The no-deadlock property
of OCC is architecturally simpler than a deadlock-detection cycle.

### 2. Multi-Version Concurrency Control (MVCC)

Maintain multiple versions of each entity. Readers get a snapshot; writers check
only their written entities at commit.

| Pros | Cons |
|------|------|
| Non-blocking reads — readers never contend with writers | Complex garbage collection of old versions |
| Natural snapshot isolation | Higher storage overhead per entity |
| Used by PostgreSQL internally | Application-layer MVCC duplicates what PostgreSQL already does |
| | Memory and SQLite adapters would require substantial new infrastructure |

**Rejected for V1.** Application-layer MVCC adds significant complexity without
proportional benefit. PostgreSQL's own MVCC is leveraged implicitly when the
storage transaction uses `SERIALIZABLE` isolation. A future P1 snapshot-isolation
opt-in (per FEAT-008) can expose this via the `StorageAdapter` without a full
application-layer MVCC implementation.

### 3. Two-Phase Locking (2PL)

Acquire shared locks on reads, exclusive locks on writes; release all at commit.

| Pros | Cons |
|------|------|
| Provides serializable isolation without retry | Deadlock-prone (same as pessimistic locking) |
| Standard relational database approach | Lock manager required at application layer |
| | Incompatible with the StorageAdapter abstraction across three backends |

**Rejected.** Same deadlock concern as pessimistic locking, plus higher
implementation complexity. The StorageAdapter trait is intentionally simple;
embedding a lock manager would couple the transaction protocol tightly to adapter
internals.

### 4. Timestamp Ordering (TO)

Assign each transaction a timestamp at start; abort any transaction whose
timestamp would violate the serial order implied by committed timestamps.

| Pros | Cons |
|------|------|
| Deadlock-free | Requires a global timestamp oracle or logical clock |
| Well-studied in distributed databases | More complex abort logic than OCC version checks |
| | Distributed clock synchronization is out of scope for V1 (single-instance) |

**Rejected for V1.** Timestamp ordering is valuable for distributed transactions
(P2 scope). For single-instance V1, OCC version checks accomplish the same
correctness goal more simply.

### 5. OCC with Version Vectors (Selected)

| Pros | Cons |
|------|------|
| Deadlock-free by construction — no locks held during execution | Writers must retry on conflict |
| Simple mental model: "expected version must match" | Retry logic is the caller's responsibility |
| Uniform across SQLite, PostgreSQL, and memory adapters | High-contention workloads degrade to a spin-retry pattern |
| No lock manager, no clock synchronization | |
| Conflict response includes current state — callers can merge intelligently | |
| Transaction ID links audit entries; easy to audit transaction boundaries | |

**Selected.** OCC matches the expected agentic workload (low contention, short
transactions, bounded entity sets). It is deadlock-free, simple to reason about,
and maps cleanly onto all three StorageAdapter backends.

## Consequences

| Type | Impact |
|------|--------|
| Positive | Deadlock-free. Simple implementation — version check is a comparison, not a lock acquisition. Works uniformly across SQLite, PostgreSQL, and memory. Conflict response carries current state, enabling intelligent client-side merging |
| Negative | Callers must implement retry logic on conflict. High-contention workloads (unlikely for agentic use cases) will retry frequently |
| Neutral | Serializable isolation is the default; relaxed isolation levels (snapshot, read-committed) are a P1 opt-in per FEAT-008, not required for V1 |

## Implementation Notes

- `Transaction` struct in `axon-api/src/transaction.rs` buffers `WriteOp` entries
- Commit validates versions, then delegates to `StorageAdapter::begin_tx` /
  `commit_tx` for storage-level atomicity
- Audit entries produced within the transaction share `transaction_id = tx.id`
- Version increment (`entity.version += 1`) happens inside the commit loop,
  not at stage time
- The 100-op limit and 30s timeout are checked in the commit path per FEAT-008

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| PROP-004: serializability simulation passes (concurrent transactions produce serial-equivalent result) | Any lost-update or write-skew report |
| No deadlocks observed in load tests | Deadlock report (should be impossible by construction) |
| Transaction commit p99 < 20ms for 2–5 entity transactions (BM-005/BM-006) | Benchmark regression |
| INV-003 (audit completeness) confirms all committed transactions have full audit trails | Any audit gap detected |

## References

- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md)
- [ADR-003: Backing Store Architecture](./ADR-003-backing-store-architecture.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [Transaction implementation](../../../crates/axon-api/src/transaction.rs)
