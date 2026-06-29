---
ddx:
  id: ADR-028
  depends_on:
    - helix.prd
    - FEAT-013
    - ADR-026
---
# ADR-028: Index-Key Store SSOT — Typed `IndexValue` vs. Canonical `index_key` Bytes

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-28 | Accepted | Erik LaBianca | FEAT-013, axon-esf::index_key, axon-7fbe288e | High |

## Context

`axon-esf::index_key` is declared the **single source of truth** for Axon's
index-key encoding: it turns a JSON record plus an index declaration
(`IndexDef` / `CompoundIndexDef`) into an order-preserving byte key whose
`memcmp` reproduces the natural ordering of the underlying typed value
(negatives-first integers, total-order floats, byte-ordered strings,
`false < true`, datetimes as epoch nanoseconds). `axon-schema` already
re-exports the index *type* definitions from `axon-esf`, so the two crates
share the type system.

`axon-storage`, however, does **not** use those bytes. It maintains its own
ordered representation — the typed `IndexValue` enum (`String` / `Integer` /
`Float` / `Boolean`) with an `Ord` implementation — materialised as a `BTreeMap`
keyed by `IndexValue` / `CompoundKey`.

Crucially, **only the in-memory adapter maintains secondary indexes.** The
`update_indexes` / `index_lookup` / `index_range` `StorageAdapter` methods are
no-op / `Err` defaults that **only `MemoryStorageAdapter` overrides**; SQLite and
PostgreSQL do not implement them and the query planner falls through to a full
scan for them. So the typed `IndexValue` ordering is an **in-memory, ephemeral**
structure — rebuilt from entity data each process lifetime — not a persisted
on-disk index. (There are no EAV index tables in the SQL backends today.)

`axon-7fbe288e` added a cross-crate conformance test (`index_key_conformance`
in `axon-storage`'s `adapter.rs`) that proves the typed `IndexValue` `Ord` and
the canonical `index_key` byte ordering agree for `String`, `Integer`, `Float`,
and `Boolean`, so the two encodings cannot silently drift on those types. It
also pins two **residual divergences** with executable evidence:

1. **Datetime representation.** The store indexes `Datetime` as the *raw JSON
   string* (lexicographic order). The SSOT encoder canonicalizes to epoch
   nanoseconds (instant-correct). They agree for `Z`-normalized RFC 3339 but
   **diverge across timezone offsets**: `2026-06-28T12:00:00+05:00` is the
   instant `07:00:00Z` — chronologically *before* `2026-06-28T10:00:00Z`, but
   lexicographically *after* it.
2. **Type mismatch.** A present-but-wrong-typed value is "not indexed" (`None`)
   in the store per FEAT-013, whereas the SSOT encoder returns
   `Err(IndexKeyError::TypeMismatch)` (fail-closed).

The open question — tracked by `axon-302fe6be` — is whether `axon-storage`
should be migrated to call `index_key` directly (a literal byte-key store), or
whether the verified-equivalence model is sufficient. This ADR decides that.

### Is the datetime divergence a real bug or only theoretical?

It is **real but narrow**. Entity validation is JSON-Schema-based (the
`jsonschema` crate) and performs *no* datetime normalization; there is no
`chrono` dependency and nothing on the write path canonicalizes a datetime
field to UTC. A client may therefore store `2026-06-28T12:00:00+05:00`
verbatim. For any deployment that (a) declares a `Datetime` index and (b)
stores datetimes with mixed UTC offsets, the store's string-ordered index will
mis-sort those records relative to one another and to `Z`-form values — wrong
range-scan boundaries and wrong ordering. Deployments that only ever store
`Z`-normalized datetimes (the common case Axon emits) are unaffected, because
fixed-width UTC ISO strings sort chronologically.

The type-mismatch divergence (#2), by contrast, is **not a bug**: FEAT-013
specifies that mismatched-type values are not indexed (not errors). `None` is
the intended store behavior; the encoder's `Err` is for callers that *choose*
to fail closed. The two policies coexist by design.

## Decision

**Do not migrate `axon-storage` to a literal `index_key` byte-key store at this
time.** Keep the typed `IndexValue` model with its test-verified ordering
equivalence to the SSOT. Instead, **fix the one genuine correctness defect — the
datetime ordering residual — within the typed model**, and treat the full
byte-key migration as deferred-with-conditions.

Concretely:

1. **Datetime canonicalization (actionable).** Make the index extraction
   (`extract_index_value` / `extract_index_values`, consumed by the in-memory
   index and by the handler's index-bound planning) encode `Datetime` fields by
   their canonical instant rather than their raw string: parse the value to epoch
   nanoseconds (reusing the SSOT's RFC 3339 / epoch-nanos coercion) and store it
   in the ordered domain (an `i64` — a dedicated `IndexValue::Datetime(i64)` lane
   is cleaner than overloading `Integer`), so ordering is instant-correct across
   offsets. Because secondary indexes are **in-memory and ephemeral** (rebuilt
   from entity data each process), there is **no persistent data migration** — the
   query bound and the stored key both flow through the same extraction, so they
   stay consistent automatically. (If the SQL backends later gain *persisted*
   secondary indexes, a reindex migration becomes part of that work, not this.)
2. **Type-mismatch policy (no change).** The `None`-vs-`Err` difference is
   intentional (FEAT-013) and already documented on both sides; no
   reconciliation is required.
3. **Full byte-key store migration (deferred-with-conditions).** Re-scope
   `axon-302fe6be` to a deferred item: pursue a literal byte-key store only if a
   triggering condition fires (below). The conformance test already guarantees
   the *ordering* the SSOT promises, so the typed model is not incorrect — it is
   merely a second encoding of the same order.

### Why not the full byte-key migration now

The migration's cost is concentrated in the index/query path:

- The in-memory `BTreeMap<IndexValue>` / `BTreeMap<CompoundKey>` becomes
  `BTreeMap<Vec<u8>>`, and every index-bound query must encode its bounds to
  canonical bytes; the typed `IndexValue` lattice (and its `Display`, equality
  lookups, unique-conflict checks) is replaced by opaque bytes, losing the type
  information the typed model carries for free.
- It would only become genuinely large if/when the SQL backends grow *persisted*
  byte-keyed index tables (a new on-disk format + a data migration) — work that
  does not exist today and that the targeted datetime fix does not require.
- It changes the same datetime representation that the targeted fix changes —
  so doing it instead of the targeted fix does not avoid that change, it bundles
  it into a much larger one.

…while the benefit is largely **SSOT purity**, not correctness: equivalence of
ordering for the scalar types is already proven by test. The single correctness
gain (datetime) is obtainable far more cheaply by the targeted fix.

### Conditions under which to revisit the byte-key migration

Adopt the literal byte-key store when **any** of these holds:

- An external consumer requires **byte-identical** keys (e.g. an ordered
  KV/`pqueue` backend, or a shared on-disk index format), so verified-equivalent
  orderings are insufficient and the literal bytes must match.
- A new storage backend is **natively byte-keyed** (an ordered KV store), making
  the typed model the impedance mismatch rather than the byte model.
- The typed `Ord` and the SSOT bytes are found to diverge on a type the
  conformance test does not cover (a real drift, not a representation choice).

## Alternatives Considered

### 1. Full byte-key migration now (the original `axon-302fe6be` scope)

Rejected for now: high cost (re-encoding the in-memory index/query path to opaque
bytes, and a new persisted byte-keyed index format + migration if/when the SQL
backends gain on-disk indexes), benefit dominated by SSOT aesthetics. Deferred
with explicit revisit conditions; the conformance test holds the line in the
meantime.

### 2. Status quo — verified equivalence + documented residuals only

Rejected as the *final* state: it leaves the datetime cross-offset mis-sorting
unfixed, which is a genuine (if narrow) correctness gap, not merely a
representation difference. Acceptable only as the interim state until the
targeted datetime fix lands.

### 3. Targeted datetime canonicalization within the typed model (selected)

Selected. Fixes the real bug at the lowest cost, keeps the proven-equivalent
typed model, and preserves the FEAT-013 type-mismatch policy. The full
migration remains available behind documented conditions.

## Consequences

| Type | Impact |
|------|--------|
| Positive | The only real correctness defect (datetime cross-offset ordering) gets a scoped fix with no persistent migration (secondary indexes are in-memory/ephemeral); the typed model and its test-verified SSOT equivalence are preserved; the FEAT-013 type-mismatch policy is unchanged; the large byte-key migration is not paid for prematurely and is gated behind concrete conditions |
| Negative | Two encodings of the canonical order continue to coexist (typed `Ord` in the store, bytes in `axon-esf`), so `axon-esf::index_key` remains the SSOT *by verified equivalence* rather than by direct use; until the datetime fix lands, mixed-offset datetime indexes mis-sort |
| Neutral | No code change from this ADR itself; it records the decision, re-scopes `axon-302fe6be` to deferred-with-conditions, and spawns a focused datetime-canonicalization bead |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| A deployment already stores mixed-offset datetimes in an indexed field and relies on correct ordering before the targeted fix ships | Low | Medium | Prioritize the datetime-canonicalization bead; document the residual (done, in `IndexValue` docs + the esf Store-SSOT note); deployments emitting only `Z` datetimes are unaffected |
| The two encodings drift on a future type or edge case the conformance test misses | Low | Medium | The `index_key_conformance` test is the guard; extend it whenever an index type or coercion rule changes |
| A future persisted-index backend re-introduces the datetime residual on disk if it copies the string-form extraction | Low | Medium | Persisted-index work must use the canonical-instant extraction from the targeted fix, with a conformance assertion over its stored keys |

## Supersession

- **Supersedes**: None.
- **Superseded by**: None. Re-scopes the implementation bead `axon-302fe6be`
  (full byte-key migration → deferred-with-conditions) and is the decision of
  record for the Store-SSOT follow-up from `axon-7fbe288e`.

## Concern Impact

- **rust-cargo**: None from this ADR (design-only). The targeted datetime fix it
  authorizes will touch `axon-storage` index extraction (and the in-memory index
  it feeds) plus a small public coercion in `axon-esf`; that work is tracked
  separately.
- **security-owasp**: None. Index ordering is a correctness concern; no new
  external surface.

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| `index_key_conformance` keeps the typed `Ord` and SSOT bytes in agreement for all covered scalar types | Any new index type or coercion-rule change → extend the conformance test |
| Datetime indexes order by true instant across UTC offsets (after the targeted fix) | A reported mis-ordering on a datetime index, or mixed-offset datetimes observed in an indexed field |
| The byte-key migration is opened only when a revisit condition fires | An external byte-identical-key consumer or a natively byte-keyed backend is introduced |

## References

- [FEAT-013: Secondary Indexes](../../01-frame/features/FEAT-013-secondary-indexes.md)
- [ADR-026: Predicate-Read Serializability](./ADR-026-predicate-read-serializability.md)
- [Canonical index-key encoder (SSOT)](../../../crates/axon-esf/src/index_key.rs)
- [Storage `IndexValue` + `index_key_conformance` tests](../../../crates/axon-storage/src/adapter.rs)
