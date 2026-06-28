---
ddx:
  id: ADR-026
  depends_on:
    - helix.prd
    - ADR-004
    - FEAT-008
---
# ADR-026: Predicate-Read Serializability — Per-Collection Structural-Version Phantom Guard

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-27 | Accepted | Erik LaBianca | FEAT-008 (TXN-05), ADR-004 | Medium-High |

## Context

ADR-004 selected OCC with entity-level version vectors. B-104 (axon-api
`transaction.rs`) shipped an opt-in `IsolationLevel::Serializable` that validates
the **key-addressed read set**: every entity recorded via
`Transaction::record_read` is re-checked at commit (Phase-1b) and the transaction
aborts first-committer-wins if its observed version changed. This prevents write
skew whose invariant is expressed over **specific entities read by id**.

It does **not** catch **predicate / phantom** write skew: invariants expressed
over the *result set* of a query, secondary-index scan, link traversal, or
aggregation. Example: invariant "at most one on-call engineer". Two transactions
each `MATCH (e:Engineer {on_call:true})`, both observe one on-call engineer
(themselves about to go off-call is fine), each inserts/flips a *different* row so
that the predicate now matches two rows. No previously-read entity *version*
changed — a concurrently inserted or removed row changed a **predicate result**.
Key-addressed Serializable cannot see this because the anomalous row was never
read by id (it is a phantom).

This ADR decides how Axon catches predicate/phantom write skew for Serializable
transactions, what is delivered now, and what remains future work.

### Constraints inherited from ADR-004 / FEAT-008

- OCC only; no pessimistic locks, no SELECT FOR UPDATE, deadlock-free by
  construction.
- Must be expressible across all three `StorageAdapter` backends
  (memory / SQLite / PostgreSQL). `get` / `range_scan` / `count` are `&self`;
  writes are `&mut self`.
- Snapshot remains the default and must stay allocation-free / zero-overhead.
- No surface may claim unqualified "serializable".

## Where predicate-read validation lives

`Transaction` (axon-api) **cannot re-run a query** — it has no Cypher planner, no
filter evaluator, and no schema/index context. Re-running arbitrary predicates at
commit would also be unsound under OCC (the re-run sees post-commit state of
*other* committed txns, and re-running a Cypher `MATCH` from inside the storage
commit path is a layering inversion).

Two layers are therefore involved:

1. **The handler / query layer** (which *does* run queries) is responsible for
   **recording** what a Serializable transaction observed via a scan/predicate
   read, onto the `Transaction`. This is symmetric with how callers already call
   `record_read` for key-addressed reads.
2. **`Transaction::commit` Phase-1b** (which already holds the storage-level
   transaction) is responsible for **validating** the recorded scan reads against
   storage at commit, reusing `AxonError::ConflictingVersion` exactly like the
   key-addressed path.

Validation must use only `&self` storage primitives, because Phase-1b runs while
the staged writes have not yet been applied and must not mutate.

## Decision

Adopt a **per-collection structural-version counter** as the sound, bounded
phantom guard, recorded and validated alongside the existing key-addressed read
set.

### Mechanism

1. Add `StorageAdapter::structural_version(&self, collection) -> Result<u64>`.
   The contract: the returned value is a **monotonic counter that strictly
   increases whenever the membership of the collection changes** — i.e. on any
   create or delete of an entity (or link) in that collection. Pure in-place
   updates (which change an entity's `version` but not the *set* of ids) need not
   bump it; those are already covered by the key-addressed read set when the
   updated row was read by id, and a predicate whose truth flips on an in-place
   field change without membership change is a key-addressed concern, not a
   phantom (see "Soundness scope" below).

2. A Serializable transaction that performs a scan/predicate read records, via
   the new `Transaction::record_scan_read(collection, observed_structural_version)`,
   the structural version of each scanned collection **at read time**.

3. At commit, Phase-1b (extended) re-reads `structural_version(collection)` for
   each recorded scan read. If it differs from the observed value, a concurrent
   create/delete touched the scanned collection between the read and the commit:
   abort first-committer-wins with `ConflictingVersion` (409, retryable).

This is deliberately **conservative**: it aborts on *any* concurrent
insert/delete to a scanned collection, even an insert that does not match the
transaction's predicate. That over-aborts (higher abort rate) but is **sound** —
it can never miss a phantom, because every phantom is, by definition, a
membership change in some scanned collection. It requires no predicate
re-evaluation and no per-row read-set materialisation, so its commit-time cost is
O(number of distinct scanned collections), not O(rows scanned).

### Default-implementation strategy (the key tractability decision)

`structural_version` is added to the trait **without** a silently-passing
default. A default that returns a constant would make every Serializable scan
read *appear* valid forever — a **silent soundness hole**. Instead the trait
default is **fail-closed**: it returns
`Err(AxonError::InvalidOperation("structural_version not supported by this
adapter"))`. Phase-1b surfaces that error, so a Serializable transaction that
recorded a scan read against an adapter without structural-version support
**aborts loudly** rather than committing an unvalidated phantom. Snapshot
transactions never call it, so unmigrated adapters are unaffected for the default
isolation level.

`MemoryStorageAdapter` implements it concretely (a per-`CatalogKey` monotonic
counter bumped on membership-changing mutations). SQLite and PostgreSQL keep the
fail-closed default for now (see "Future work"), which is honest: Serializable +
scan-read is only *sound and available* on the memory adapter in this increment;
on SQL backends a scan-read-recording Serializable txn fails closed instead of
giving a false guarantee.

## Soundness vs. abort-rate vs. implementation cost, across adapters

| Option | Soundness | Abort rate | Impl cost (mem / sqlite / pg) |
|--------|-----------|-----------|-------------------------------|
| **(A) Re-run predicate at commit** | Unsound under OCC + layering inversion (Transaction has no planner) | low | very high everywhere |
| **(B) Materialise full (id,version) scan read-set, re-scan at commit** | Catches *changed/removed* observed rows, but **misses pure phantoms** (newly inserted matching rows were never in the set) | low | medium; bounded by `MAX_READS`; needs re-scan + diff |
| **(C) Per-collection structural-version counter (CHOSEN)** | **Sound** for phantoms (every insert/delete bumps the counter) | higher (aborts on any concurrent insert/delete to a scanned collection) | mem: low (one counter map + bumps); sqlite/pg: medium (needs a durable monotonic per-collection counter or a `MAX(seq)`-style query) |
| **(D) Full Cahill SSI (SIREAD locks, rw-antidependency pivot detection)** | Sound and precise (true serializable) | lowest | very high; needs predicate-lock / SIREAD infrastructure in every adapter |

(B) is *not* sound on its own — it is exactly the gap B-104 already has, restated
over scans. (C) is the smallest sound design. (B) **plus** a phantom guard
collapses into (C) anyway (the phantom guard is the structural counter), so (C)
subsumes the useful part of (B) at lower cost.

**SQLite cost note**: a sound structural counter needs to survive process
restart and bump inside the same `BEGIN IMMEDIATE` as the mutations. A natural
implementation is a `collection_structural_version(collection_id INTEGER PRIMARY
KEY, version INTEGER)` row bumped on every entity/link insert/delete, or deriving
it from an existing monotonic audit/row sequence filtered by collection. Either
is a schema migration — out of scope for this increment.

**PostgreSQL cost note**: same shape; the storage transaction already runs at
`SERIALIZABLE`, so Postgres' own engine already rejects many of these anomalies
when the read happens *inside* the same DB transaction — but Axon's reads happen
in a *separate* read request before the commit transaction opens, so the engine's
guarantee does not transitively apply. An explicit counter (or `xmin`-range /
sequence check) is still required to bridge read-request → commit-request.

## What is delivered in this increment (honesty)

- Trait method `StorageAdapter::structural_version` (fail-closed default).
- `MemoryStorageAdapter` sound implementation (monotonic per-collection counter).
- `Transaction::record_scan_read` + Phase-1b validation reusing
  `ConflictingVersion`.
- Tests proving predicate/phantom write skew is **allowed under Snapshot** and
  **prevented under Serializable** on the memory adapter.

**Delivered guarantee**: "Serializable for key-addressed read sets **and**
collection-granular predicate/phantom reads, on the memory adapter, via a
conservative structural-version guard." SQLite and PostgreSQL **fail closed** on
scan-read-recording Serializable transactions (no false guarantee, but the
feature is not yet available there).

## What remains future work

- **SQLite + PostgreSQL structural-version persistence** — schema migration +
  bump-in-transaction wiring so the predicate guard is available on durable
  backends. Until then they fail closed.
- **Wiring the handler read paths** (`query_entities`, `aggregate`, `traverse`,
  Cypher executor) to *automatically* call `record_scan_read` when a transaction
  is threaded through a read. This increment provides the transaction-level API
  and validation; automatic capture from the query layer is a follow-up because
  reads and the transaction are not currently threaded together through the
  handler (`query_*` take `&self`; the `Transaction` is built by the caller).
- **Full Cahill SSI** (SIREAD locks, rw-antidependency / dangerous-structure
  pivot detection) for *precise* serializability with minimal aborts. The
  structural-version guard is intentionally coarser (collection-granular,
  over-aborts) and remains the V1 stance; SSI is the long-term path if abort
  rates under real predicate-heavy workloads prove unacceptable.

## Consequences

| Type | Impact |
|------|--------|
| Positive | Sound phantom prevention on the memory adapter at O(scanned-collections) commit cost; reuses existing `ConflictingVersion` retry contract; Snapshot path unchanged and zero-overhead; fail-closed default prevents silent soundness holes |
| Negative | Conservative: over-aborts on non-matching concurrent inserts/deletes to a scanned collection (acceptable for low-contention agentic workloads, ADR-004); only the memory adapter is wired now; handler auto-capture is a follow-up |
| Neutral | Collection-granular guard is coarser than true SSI; the honest claim grows to "key-addressed reads + collection-granular predicate reads (memory adapter)" |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Over-abort under high-contention predicate workloads | Low (agentic = low contention) | Medium | Documented; SSI is the escalation path |
| A backend forgets to bump the counter on a membership path | Low | High (silent miss) | Bumps are co-located with the membership mutations; conformance test asserts bump-on-insert/delete |
| Callers assume SQL backends provide the guard | Medium | Medium | Fail-closed default + explicit "memory-only" wording in FEAT-008 / ADR-004 |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Predicate/phantom write skew allowed under Snapshot, prevented under Serializable on memory (`phantom_write_skew_allowed_under_snapshot`, `phantom_write_skew_prevented_under_serializable`) | Any phantom committed under Serializable on a supported adapter |
| Serializable scan-read on an unsupported adapter fails closed | A scan-read Serializable txn committing without validation on SQLite/Postgres |

## References

- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md) (TXN-05)
- [ADR-004: Transaction Model — OCC](./ADR-004-transaction-model.md)
- [Transaction implementation](../../../crates/axon-api/src/transaction.rs)
- Cahill, Röhm, Fekete, "Serializable Isolation for Snapshot Databases" (SIGMOD 2008) — the SSI design deferred as future work.
