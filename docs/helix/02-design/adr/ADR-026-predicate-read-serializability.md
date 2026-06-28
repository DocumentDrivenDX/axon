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

`structural_version` is added to the trait with a **sound, backend-agnostic
default** rather than a fail-closed stub. An earlier iteration failed closed
(returning `InvalidOperation`) so that unmigrated adapters aborted loudly instead
of committing an unvalidated phantom — correct, but it left the guard working
only on the memory adapter. The default is now
`structural_version_by_scan`: it reads the collection's entity ids via
`range_scan` and returns `hash_id_set(ids)` — a stable, order-independent hash of
the **id-set**. The id-set hash changes on any insert/delete (a phantom changes
the set) and is stable across in-place updates and reads, so the default is
**sound for the phantom guard on every adapter**, at O(n) read cost per scanned
collection and **zero write-path cost**. There is no silent-soundness-hole risk
(the default is a real membership signature, not a constant) and nothing fails
closed.

Adapters override `structural_version` when they can do better than a full
range_scan:

- `MemoryStorageAdapter` — a per-`CatalogKey` monotonic counter bumped on
  membership-changing mutations (O(1)).
- `SqliteStorageAdapter` — `SELECT id … ORDER BY id` hashed with the shared
  `hash_id_set` (ids only, not full rows; runs inside the active `BEGIN
  IMMEDIATE`).
- `PostgresStorageAdapter` — `md5(string_agg(id, ',' ORDER BY id))` pushed fully
  into the database (transfers 32 hex chars regardless of collection size; folded
  to `u64`).

All four paths are **read-derived signatures** (or, for memory, an equivalent
maintained counter) — none requires a schema migration or a write-time counter
on the SQL backends, so the common write path is never taxed and there is no
counter-row contention.

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

**Read-derived signature avoids the write-path cost.** Rather than maintaining a
write-time counter on the SQL backends (a `collection_structural_version` row
bumped on every insert/delete — a schema migration that also serializes
concurrent inserts on the counter row), the SQL adapters compute the signature
**read-only at scan-record and commit time**. The id-set hash is sound for
phantoms because a new/removed id changes the set. This is the right trade for an
**opt-in, rare** guard: it taxes only the Serializable-scan transactions that use
it (O(n) read over ids), never the common write path. SQLite hashes ordered ids
client-side; PostgreSQL pushes `md5(string_agg(...))` into the engine so only 32
hex chars cross the wire. Reads run inside the active storage transaction
(`BEGIN IMMEDIATE` / `SERIALIZABLE`) during commit validation, so they observe a
consistent snapshot; the read-request → commit-request gap is bridged because the
signature is recaptured at commit and compared to the value observed at scan time.

## What is delivered in this increment (honesty)

- Trait method `StorageAdapter::structural_version` with a **sound,
  backend-agnostic default** (`structural_version_by_scan` + `hash_id_set`).
- Native overrides on **all three** shipped adapters: memory (O(1) counter),
  SQLite (ordered-id hash), PostgreSQL (`md5(string_agg)` push-down).
- `Transaction::record_scan_read` + Phase-1b validation reusing
  `ConflictingVersion`.
- Tests proving predicate/phantom write skew is **allowed under Snapshot** and
  **prevented under Serializable** (axon-api transaction tests on memory), plus a
  cross-adapter conformance test (`structural_version_tracks_membership_not_updates`)
  run on memory, SQLite, and — when a cluster is reachable — PostgreSQL, and a
  generic-default unit test exercising the scan-based path.

**Delivered guarantee**: "Serializable for key-addressed read sets **and**
collection-granular predicate/phantom reads, on **every** storage backend, via a
conservative membership-signature guard." No backend fails closed.

**Scope limit (still honest):** the guard is **membership-only** — it catches
insert/delete phantoms, not **update-driven** predicate changes (e.g. a
concurrent `status: open → closed` that flips a `WHERE status = open` count
without changing the id-set). Catching those soundly requires either a
version-inclusive signature (which over-aborts on every concurrent update to a
scanned collection) or full SSI; both remain future work. Invariants over
mutable predicates must still be guarded at the application level.

## What remains future work

- **Update-driven predicate serializability** — extend the signature to
  `(id, version)` or adopt SSI so predicate changes via in-place updates are
  caught. Deferred deliberately: a version-inclusive signature over-aborts on any
  concurrent update to a scanned collection.
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
| Positive | Sound phantom prevention on **every** backend (generic scan default + native overrides); reuses existing `ConflictingVersion` retry contract; Snapshot path unchanged and zero-overhead; read-derived signatures add no write-path cost and need no schema migration |
| Negative | Conservative: over-aborts on non-matching concurrent inserts/deletes to a scanned collection (acceptable for low-contention agentic workloads, ADR-004); the generic default and SQLite paths are O(n)-ids per scanned collection at commit (Postgres push-down and memory are O(1)); membership-only (update-driven predicate skew not caught); handler auto-capture is a follow-up |
| Neutral | Collection-granular guard is coarser than true SSI; the honest claim grows to "key-addressed reads + collection-granular phantom reads, all backends" |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Over-abort under high-contention predicate workloads | Low (agentic = low contention) | Medium | Documented; SSI is the escalation path |
| Memory's native counter misses a membership path (silent miss vs the read-derived default) | Low | High | Counter bumps are co-located with membership mutations; the cross-adapter conformance test asserts create/delete change the signature and updates do not, on every backend |
| Hash collision masks a phantom (different id-set, same signature) | Negligible (~2⁻⁶⁴) | Medium | 64-bit fixed-seed hash / md5-folded; far better than the prior memory-only/fail-closed state; SSI removes it entirely if ever needed |

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
