---
ddx:
  id: ADR-027
  depends_on:
    - helix.prd
    - ADR-004
    - ADR-026
    - FEAT-008
---
# ADR-027: Serializable Snapshot Isolation (SSI) — Precise, Minimal-Abort Serializability

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-28 | Proposed | Erik LaBianca | ADR-026, ADR-004, FEAT-008 (TXN-05) | Medium |

## Context

ADR-004 selected OCC with entity-level version vectors. ADR-026 layered
predicate/phantom serializability onto it with a **per-collection signature**
guard, validated at commit (Phase-1b) and surfaced as
`AxonError::ConflictingVersion` (first-committer-wins, retryable). Two tiers ship
today:

- **`Serializable`** — key-addressed read-set validation (every entity recorded
  via `record_read` must still be at its observed version) **plus** a
  *membership* phantom guard: each scanned collection's `structural_version`
  (a hash of its id-set) is re-checked at commit. A concurrent **insert/delete**
  to a scanned collection aborts. It catches insert/delete phantoms but **not**
  a concurrent in-place **update** that flips a predicate without changing
  membership (e.g. `status: open → closed`).
- **`SerializableStrict`** — identical, but scan reads validate `content_version`
  (a hash of `(id, version)` pairs) instead of membership. *Any* concurrent
  create, delete, **or update** to a scanned collection aborts. It closes the
  update-driven gap, but at **table granularity**: it aborts on a concurrent
  update to *any* row of a scanned collection, even one the transaction's
  predicate never matched.

Both signatures are auto-captured across every read shape (`tx_get_entity`,
`tx_query_entities`, `tx_aggregate`, `tx_traverse`, Cypher footprints) and
`Serializable` is reachable over GraphQL. This is single-instance OCC, validated
at commit, uniform across the three `StorageAdapter` backends (memory, SQLite,
PostgreSQL).

The remaining gap is **precision**. `SerializableStrict` is sound and complete
for serializability over the collections it tracks, but it is *coarse*: its abort
predicate is "did the collection's content signature move", not "did a concurrent
write actually conflict with what I read". Under predicate-heavy contention this
over-aborts — every writer to a hot collection forces every concurrent reader of
that collection to retry, regardless of whether their predicates intersect. ADR-026
named **full Cahill-style SSI** (Cahill/Röhm/Fekete, SIGMOD 2008) as the path
from this conservative table-granular guard to **precise, minimal-abort**
serializability, and deferred it. This ADR analyses that path: what SSI is, how
it would map onto Axon's commit-time OCC across three adapters, what it costs, and
**whether Axon should pursue it**.

This is a **design-only** ADR. It records the decision about *whether and under
what conditions* to adopt SSI; it does not schedule implementation.

### Constraints inherited from ADR-004 / ADR-026 / FEAT-008

- OCC only; no pessimistic locks, no `SELECT FOR UPDATE`, deadlock-free by
  construction.
- Must be expressible across all three `StorageAdapter` backends. Read primitives
  (`get` / `range_scan` / `count` / `structural_version` / `content_version`) are
  `&self`; writes are `&mut self`.
- Snapshot remains the default and must stay allocation-free / zero-overhead.
- The read set is captured at the **API/handler layer**, not the SQL layer.
- No surface may claim unqualified "serializable" until the claim is precise.

## What full Cahill SSI is

SSI augments snapshot isolation with just enough conflict tracking to detect the
*specific* cycle structure that lets snapshot anomalies (write skew, phantoms)
occur, and aborts one transaction in that structure — rather than aborting on any
overlap.

1. **SIREAD locks.** When a transaction reads data (a row, or a predicate /
   index range), it leaves a lightweight, non-blocking **SIREAD** marker on what
   it read. SIREAD locks never block anyone; they are read-tracking metadata, not
   mutual-exclusion locks. They outlive the reading transaction (held until no
   concurrent transaction could still conflict with them).

2. **rw-antidependency edges.** When transaction `Tw` writes data that
   transaction `Tr` had SIREAD-marked (and the two are concurrent — overlapping
   in time, neither seeing the other's writes), there is an
   **rw-antidependency** `Tr → Tw` ("`Tr` read a version that `Tw` made stale").
   Each transaction tracks whether it has an **inbound** edge (someone read what
   *it* will overwrite — `inConflict`) and an **outbound** edge (it read what
   someone else overwrote — `outConflict`).

3. **The dangerous pivot.** Adya/Cahill's theorem: every snapshot-isolation
   serialization anomaly contains a transaction `Tpivot` with **both** an inbound
   and an outbound rw-antidependency edge, where the transaction on the outbound
   end commits first. A cycle in the dependency graph cannot form without such a
   pivot. So the abort rule is: **abort a transaction that has both an inbound and
   an outbound rw-edge** (with the standard "commits first / is concurrent"
   refinements). This is precise: a transaction is aborted only when it sits on a
   *genuine* dangerous structure, not merely because it touched a contended
   collection.

The payoff over the conservative guard: where `SerializableStrict` aborts a reader
because *some* row of a scanned collection changed, SSI aborts only when the
reader is the pivot of a real rw/rw fan that could close a cycle — far fewer
false positives under contention.

### What maps onto Axon today, and what is missing

Axon already captures most of the *read* side of SSI:

| SSI ingredient | Axon analogue today | Gap |
|----------------|---------------------|-----|
| SIREAD on rows read by id | `ReadRef` (the key-addressed read set via `record_read`) — collection + id + observed version | None for the marker itself; it is per-key and precise. Missing: it is consumed only at *this* txn's commit, not published for *other* txns to find a rw-edge against. |
| SIREAD on a predicate / scan | `ScanReadRef` (collection + observed signature via `record_scan_read`) | Coarse: marks the *whole collection*, not the predicate or index range. No true predicate locks / index-range locks → no per-predicate phantom precision. |
| Write side of an rw-edge | The staged write set (`WriteOp` / link ops), known at commit | Writes are known, but Axon never *matches* a committing write against another live transaction's SIREAD markers. |
| rw-antidependency detection | — | **Entirely missing.** There is no shared, cross-transaction conflict graph; each transaction validates only its own read set against current storage. |
| Pivot detection + abort rule | — | **Entirely missing.** Today's abort rule is "my read signature moved", not "I am a pivot". |

So Axon has precise SIREAD markers for key-addressed reads and *coarse* SIREAD
markers for predicate reads, but **no rw-antidependency tracking and no pivot
detection** — the two pieces that make SSI precise. True phantom precision further
needs **predicate locks or index-range locks** (SIREAD on the *range scanned*, so
a concurrent insert into that range registers an rw-edge), which Axon does not have
— its scan marker is collection-granular by construction.

## Mapping SSI onto Axon's architecture

Axon is **single-instance OCC validated at commit**. SSI is normally implemented
*inside* an MVCC engine that sees every read and write at the row/index level. Two
honest structural mismatches must be confronted:

1. **Reads are captured at the handler layer, not the storage/SQL layer.** Axon's
   read set is what the handler chose to record (`record_read` / `record_scan_read`,
   auto-captured by the `tx_*` methods). SSI's SIREAD locks assume the engine sees
   *all* reads. Any axon-level SSI is therefore only as precise as the handler's
   capture — and capture is collection-granular for predicates. Axon cannot get
   index-range SIREAD precision without pushing read tracking into the adapters'
   scan paths (per-adapter, and impossible to express uniformly: the memory
   adapter has no index ranges in the SQL sense, SQLite and Postgres differ).

2. **Validation is at commit, not continuous.** Cahill SSI maintains the conflict
   graph *continuously* as transactions read and write, and can abort a still-running
   transaction the moment it becomes a pivot. Axon has no long-lived transaction
   object spanning the read phase — reads happen against committed state through the
   handler, and the `Transaction` only materialises staged writes + recorded reads
   at commit. A commit-time approximation of SSI is possible (build the rw-edges at
   commit by matching the committing write set and read set against a registry of
   recently-committed and in-flight transactions' SIREAD markers), but it requires a
   **shared, process-global conflict registry** — new shared mutable state that
   ADR-004's stateless commit path deliberately avoids.

### Tracking rw-antidependencies across the three adapters

To detect rw-edges at axon level we would need, process-globally (single
instance), a registry of:

- **Live SIREAD markers**: for each not-yet-finalised transaction, the keys and
  collection-predicates it read (already captured as `ReadRef` / `ScanReadRef`).
- **Recently committed writes**: a window of `(collection, id)` (and
  collection-predicate footprints) written by transactions that may still be
  concurrent with a live reader, retained until no live transaction predates them.

At commit, a transaction would (a) match its **write set** against live readers'
SIREAD markers: a committing writer `Tw` that overwrites a key in live reader
`Tr`'s SIREAD set creates the edge `Tr → Tw`, giving `Tr` an *outbound* edge and
`Tw` an *inbound* edge; and (b) match its **read set** against the recent-write
window to discover its own *outbound* edges (it read a key a still-concurrent
committed writer overwrote). A transaction that ends up holding **both** an
inbound and an outbound edge is the pivot and aborts.

This registry, not the per-adapter primitives, is the hard part. The adapter
surface needed is modest and already mostly present:

- **In-memory** — the registry lives naturally beside the adapter's in-process
  state; SIREAD markers and the recent-write window are plain maps. Lowest
  friction. Could even approach true row-granular precision because the adapter
  sees every key.
- **SQLite** — single-writer; the registry is still axon-level (in-process), since
  axon owns the only writer. SQLite gives no native SSI to delegate to. Predicate
  precision is still bounded by handler-layer capture; SQLite has no SIREAD locks
  of its own to borrow.
- **PostgreSQL** — **already implements full SSI natively** (`SERIALIZABLE`
  isolation *is* Cahill SSI; the 2008 paper is the basis of Postgres's
  implementation). This is the decisive asymmetry: for the Postgres backend, the
  precise serializability this ADR is about **already exists in the engine**, and
  could be obtained by running the storage-level transaction at `SERIALIZABLE` and
  mapping Postgres's `40001 serialization_failure` to `ConflictingVersion`.

### Delegating to the backend vs. uniform axon-level SSI — the central tension

| Approach | Precision | Uniformity | Cost |
|----------|-----------|------------|------|
| **Delegate to Postgres native SSI** | True, index-range-precise serializability (Postgres tracks SIREAD at the page/tuple/index level) | **Postgres only** — memory and SQLite get nothing | Low: run the commit txn at `SERIALIZABLE`, retry on `40001`. But: only sound if *all* reads that matter happen **inside** that SQL transaction — and axon's reads happen at the handler layer, often *before* the commit transaction opens. The read set axon captures would have to be **replayed as SQL reads inside the serializable commit transaction** for Postgres's SSI to see them, which is the same "re-run the predicate at commit" layering inversion ADR-026 rejected. |
| **Uniform axon-level SSI** (shared conflict registry + handler-captured SIREAD) | Row-precise for key-addressed reads; **still collection-granular for predicates** (no index-range locks) | All three backends behave identically | High: new process-global mutable conflict-graph state, GC of the SIREAD/recent-write window, and it contradicts ADR-004's deadlock-free, shared-nothing commit path. And it *still* does not reach index-range phantom precision — the very thing Postgres gives for free. |

The tension is real and unflattering to a uniform axon-level build: the backend
that would benefit most from precise serializability (Postgres, the production
durable store) is the one that **already has it**, and axon's handler-layer read
capture is exactly the wrong shape to feed it without a layering inversion. A
uniform axon-level SSI would deliver its *least* precision (collection-granular
predicates) on the backend where it is the *only* option (memory/SQLite), and
duplicate — more coarsely — what Postgres already does well.

## Cost / trade-offs

| Dimension | Conservative guard (today) | Axon-level SSI | Postgres-native delegation |
|-----------|---------------------------|----------------|----------------------------|
| Soundness | Sound (membership or content) | Sound | Sound |
| Abort precision | Coarse (collection-granular) | Better for key reads; still collection-granular for predicates | Best (index-range precise) |
| Memory | O(reads) per txn, freed at commit | **Process-global** SIREAD + recent-write window, GC'd by concurrency horizon — unbounded-ish under long readers | Borne by Postgres |
| Write-path cost | Zero (read-derived signatures) | Writers must publish to the registry and scan live readers' markers | Native (already paid by SERIALIZABLE) |
| Complexity | Low — already shipped | **High** — conflict graph, pivot rule, GC, shared-state concurrency; contradicts shared-nothing commit | Low-medium — isolation level + error mapping, *plus* the read-replay problem |
| Deadlock-free | Yes | Yes (SSI aborts, never blocks) | Yes (SSI aborts) |
| Granularity ceiling | Collection | Row (keys) / collection (predicates) | Row + index range |

The headline trade-off: **SSI buys lower abort rate under contention at the price
of standing, shared, GC'd conflict-graph state** — state ADR-004 was explicitly
designed to avoid, and whose benefit is concentrated in exactly the workload
(high predicate contention) that FEAT-008 assumes is *rare* for agentic clients.

## Decision

**Do not build a uniform axon-level Cahill SSI at this time.** Keep the ADR-026
tiering (`Snapshot` / `Serializable` / `SerializableStrict`) as the standing
stance, and treat precise serializability as obtainable, *when needed*, by
**delegating to PostgreSQL's native SSI** for the durable production backend
rather than reimplementing SSI in axon.

This decision rests on three findings:

1. **The conservative tier is sound and sufficient for the assumed workload.**
   FEAT-008's stated assumption is low contention (agents work on disjoint
   records). Over-aborting under contention only bites when contention is high and
   sustained; the conservative guard's cost is paid by retries, which are cheap and
   already part of the OCC contract. There is no evidence (no benchmark, no
   reported workload) that abort rates are a problem yet — ADR-026 itself names SSI
   the *escalation path "if abort rates under real predicate-heavy workloads prove
   unacceptable."* That trigger has not fired.

2. **Uniform axon-level SSI is high-cost and structurally mismatched.** It
   introduces process-global mutable conflict-graph state against ADR-004's
   shared-nothing commit path, and — because reads are captured at the handler
   layer with collection granularity — it would **not** even reach index-range
   phantom precision. It delivers its weakest precision on the backends where it
   is the only option, and re-implements, more coarsely, what Postgres already does.

3. **Postgres already provides precise SSI.** For the durable production backend,
   true serializability is an isolation-level setting plus error mapping — *if* the
   read set is made visible to the engine. The right future investment is therefore
   **not** an axon SSI engine but a focused design for **feeding axon's captured
   read set into a Postgres `SERIALIZABLE` commit transaction** (replaying the
   recorded reads as SQL reads inside that transaction so Postgres's SIREAD
   tracking sees them), accepting that this precise tier would be **Postgres-only**
   and that memory/SQLite retain the conservative guard.

### Conditions under which to revisit

Adopt a precise tier (Postgres-native delegation first; axon-level SSI only if a
backend-uniform precise guarantee becomes a hard product requirement) when **all**
of these hold:

- A real, measured workload shows `SerializableStrict` abort rates high enough to
  harm throughput or latency SLOs (BM-005/BM-006-style evidence), **and**
- The invariant cannot be expressed as a key-addressed read (which `Serializable`
  already handles precisely) or narrowed by predicate-scoped signatures (the
  cheaper Phase-1 below), **and**
- The deployment uses PostgreSQL (so delegation is available) — or a
  backend-uniform precise guarantee is contractually required, which is the only
  case that justifies the axon-level build.

### Incremental narrowing short of full SSI

If abort rates become a problem before the full conditions are met, the abort
*scope* can be narrowed without any conflict graph, by making the existing
signature finer-grained rather than building rw-edge tracking. In rough order of
cost:

1. **Predicate-scoped content signatures.** Instead of one `content_version` over
   a whole collection, compute the signature over the **subset matching the
   transaction's predicate** (e.g. hash only `(id, version)` of rows where
   `status = open`). A concurrent update to a non-matching row no longer moves the
   signature, eliminating the largest class of `SerializableStrict` false aborts —
   while staying a commit-time read-derived signature with no shared state. This is
   the highest-value, lowest-risk step and is fully within the ADR-026 mechanism.
2. **Index-range / value-range signatures.** For range predicates, sign the
   `(id, version)` set over the scanned key/value range so only writes into that
   range abort. Approaches index-range-lock precision for phantoms without a
   conflict graph; cost is per-adapter range support.
3. **Full rw-antidependency tracking + pivot detection (true SSI).** Only this
   step adds the shared conflict registry. It is the last resort, justified only by
   the revisit conditions above, and even then Postgres-native delegation is
   preferred over the axon-level build.

Steps 1–2 are refinements of ADR-026's signature mechanism and preserve its
properties (sound, read-derived, no write-path cost, no shared state). Step 3 is
the architectural break.

## Consequences

| Type | Impact |
|------|--------|
| Positive | Avoids introducing process-global conflict-graph state and the shared-mutable-commit-path complexity ADR-004 was designed to exclude; preserves the deadlock-free, shared-nothing OCC commit; keeps the honest tiered claim; identifies a concrete, lower-cost narrowing path (predicate-scoped signatures) and the correct precise-tier strategy (Postgres-native delegation) if/when needed |
| Negative | `SerializableStrict` remains table-granular and over-aborts under sustained predicate contention; precise serializability is not available uniformly across backends; the Postgres-native path, when pursued, will be Postgres-only and still must solve read-set replay inside the serializable commit transaction |
| Neutral | No code changes from this ADR; SSI is analysed and deferred with explicit revisit conditions rather than scheduled; the precise tier, if built, is expected to be backend-delegated (Postgres) rather than a uniform axon engine |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Sustained predicate contention makes `SerializableStrict` abort rate unacceptable before any narrowing ships | Low (agentic = low contention per FEAT-008) | Medium | Predicate-scoped content signatures (narrowing step 1) are a low-risk first response within the ADR-026 mechanism; Postgres-native delegation for the durable backend |
| A future product requirement demands *uniform* precise serializability across memory/SQLite/Postgres | Low | High | Revisit conditions documented; axon-level SSI is the design of record for that case, with the cost (shared conflict registry) understood up front |
| Postgres-native delegation re-introduces the "re-run predicate at commit" layering inversion ADR-026 rejected | Medium (if pursued naively) | Medium | Treat read-set replay into the serializable commit transaction as an explicit design problem in its own ADR before adopting; do not couple the handler's planner into the storage commit path |

## Alternatives Considered

### 1. Build uniform axon-level Cahill SSI now

Rejected for V1: high complexity (shared conflict graph, GC, pivot rule against a
shared-nothing commit path), and it still cannot reach index-range phantom
precision because reads are captured at the handler layer at collection
granularity. It would deliver its weakest guarantee on the backends where it is
the only option and duplicate Postgres's native SSI more coarsely.

### 2. Delegate entirely to Postgres `SERIALIZABLE` and drop axon-level guards

Rejected: would abandon serializability on the memory and SQLite backends
entirely, and is unsound unless axon's handler-captured reads are replayed inside
the serializable SQL transaction (the layering inversion ADR-026 rejected). It is
the right *target* for the durable backend's precise tier, but not a wholesale
replacement for the cross-backend conservative guard.

### 3. Keep the conservative tier; narrow via predicate-scoped signatures only when needed (selected direction)

Selected. Retains ADR-026's sound, read-derived, shared-nothing guards; offers a
low-cost precision improvement (predicate-scoped / range signatures) that stays
within the existing mechanism; and reserves true SSI — preferentially as
Postgres-native delegation — for a measured, demonstrated need.

## Supersession

- **Supersedes**: None.
- **Superseded by**: None. Extends ADR-026 (analyses the SSI future it deferred).

## Concern Impact

- **rust-cargo**: None — design-only; no code, dependency, or workspace change.
- **security-owasp**: None beyond ADR-004/ADR-026. Serializability is a
  correctness/isolation concern; the conflict-graph state a future axon-level SSI
  would add is process-local and carries no new external surface.

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Measured `SerializableStrict` abort rate stays within throughput/latency SLOs (BM-005/BM-006) under representative workloads | Sustained abort rate that harms SLOs → adopt narrowing step 1 (predicate-scoped signatures) |
| No backend silently claims unqualified "serializable" | Any surface advertising precise serializability without the precise tier in place |
| Precise-tier need, if it arises, is met by Postgres-native delegation before any axon-level SSI build | A uniform-precise-serializability product requirement → open an SSI implementation ADR |

## References

- [FEAT-008: ACID Transactions](../../01-frame/features/FEAT-008-acid-transactions.md) (TXN-05)
- [ADR-004: Transaction Model — OCC](./ADR-004-transaction-model.md)
- [ADR-026: Predicate-Read Serializability — Per-Collection Structural-Version Phantom Guard](./ADR-026-predicate-read-serializability.md)
- [Transaction implementation](../../../crates/axon-api/src/transaction.rs)
- [Storage adapter trait — `structural_version` / `content_version`](../../../crates/axon-storage/src/adapter.rs)
- Cahill, Röhm, Fekete, "Serializable Isolation for Snapshot Databases" (SIGMOD 2008) — the SSI design analysed here; the basis of PostgreSQL's native `SERIALIZABLE`.
