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
| Requirements | Snapshot Isolation in V1 (Serializable is P1), deadlock-free, sub-20ms p99 for 2–5 entity transactions, embedded and server mode |
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
  - begin_tx() on StorageAdapter
  - For each write op, verify stored version == expected_version
  - If any mismatch: abort_tx(), return ConflictingVersion with
    the conflicting entity's current state
  - If all versions match:
      - Apply all entity writes (version + 1)
      - commit_tx() (makes entity writes durable)
      - Flush buffered audit entries (shared transaction_id)
  (See Audit Integration below for rationale on post-commit audit writes.)
```

### Version Semantics

| Expected Version | Meaning |
|-----------------|---------|
| `0` | Entity must not exist (create guard) |
| `n > 0` | Entity must be at exactly version `n` |

### Conflict Response

On version conflict, Axon returns `AxonError::ConflictingVersion` containing:
- `expected`: the version the caller passed
- `actual`: the current version in storage
- `current_entity`: the entity's current state (so the caller can merge and retry)

Version conflicts are always retryable. Schema violations return
`AxonError::SchemaValidation` with a human-readable message listing all
field-path violations.

### Isolation Guarantees

OCC as implemented provides **Snapshot Isolation** for write transactions:

- Writes are buffered locally; no storage reads occur at stage time
- Version checks at commit detect any interleaving write to the same entities,
  preventing lost updates
- Dirty reads and non-repeatable reads are prevented; phantom reads are
  prevented for entity lookups by version
- The commit phase runs under a storage-level transaction (SQLite `BEGIN
  IMMEDIATE`, PostgreSQL `SERIALIZABLE`), preventing concurrent commits from
  racing at the storage layer

**V1 known gap — write skew is not prevented**: OCC tracks only the write set.
Two transactions that each read disjoint entities and write to each other's
read set can both commit, violating serializability. Preventing write skew
requires read-set tracking, which is deferred to P1.

### Audit Integration

Every committed transaction assigns a single `transaction_id` to all audit
entries it produces.

**Post-commit audit strategy**: Audit entries are buffered during `execute()` and
flushed to the `AuditLog` only after `commit_tx()` succeeds. This two-phase
approach is a deliberate trade-off:

| Property | Guarantee |
|----------|-----------|
| No orphan entries | Audit entries for rolled-back or failed transactions are never written |
| No phantom storage | Entity mutations that are rolled back leave no audit trace |
| Crash-safety window | If the process dies between `commit_tx()` and the last `audit.append()`, committed mutations have no audit trail until recovery |

The crash-safety window is acceptable for V1 (in-memory) because both entity state
and audit log are volatile; a crash loses both equally. For durable backends
(SQLite, PostgreSQL), the audit log should be integrated into the same backing
store transaction so that both entity and audit writes commit atomically. This
is a P1 follow-on tracked by the durable storage adapter implementation tasks.

**Recovery invariant**: Any implementation targeting durable storage must ensure
INV-003 holds after restart — either by writing audit entries inside the same
database transaction as entity mutations, or by implementing a write-ahead intent
log that is replayed on startup to close any gap left by a crash between commit
and audit flush.

### Limits and Timeouts (Not Yet Implemented)

Per FEAT-008, the following limits are planned but not yet enforced:
- **Maximum 100 operations per transaction** — the 101st `stage_*` call should
  return `InvalidArgument`. This prevents unbounded write buffers and forces
  callers to batch appropriately.
- **30-second timeout** (configurable) — transactions open beyond the timeout
  should be aborted. This requires a creation timestamp on the `Transaction`
  struct and a timeout check in the commit path.

Implementation tracked by: `hx-b189dfa9`.

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
| Neutral | Snapshot Isolation is the V1 default (write-set OCC prevents lost updates and dirty reads but not write skew); Serializable isolation requires read-set tracking and is P1 per FEAT-008 |

## Implementation Notes

- `Transaction` struct in `axon-api/src/transaction.rs` buffers `WriteOp` entries
- Commit validates versions, then delegates to `StorageAdapter::begin_tx` /
  `commit_tx` for storage-level atomicity
- Audit entries produced within the transaction share `transaction_id = tx.id`
- Version increment (`entity.version += 1`) happens inside the commit loop,
  not at stage time
- The 100-op limit and 30s timeout are planned per FEAT-008 but not yet
  implemented (see Limits and Timeouts section; tracked by `hx-b189dfa9`)

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| PROP-004: snapshot isolation verified — no lost updates, no dirty reads (write skew detection deferred to P1 serializable work) | Any lost-update report |
| No deadlocks observed in load tests | Deadlock report (should be impossible by construction) |
| Transaction commit p99 < 20ms for 2–5 entity transactions (BM-005/BM-006) | Benchmark regression |
| INV-003 (audit completeness) confirms all committed transactions have full audit trails | Any audit gap detected |

## References

- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md)
- [ADR-003: Backing Store Architecture](./ADR-003-backing-store-architecture.md)
- [FEAT-003: Audit Log](../../01-frame/features/FEAT-003-audit-log.md)
- [Transaction implementation](../../../crates/axon-api/src/transaction.rs)
