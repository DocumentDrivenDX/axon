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
`Float` / `Boolean`) with an `Ord` implementation — materialised as:

- a `BTreeMap` keyed by `IndexValue` / `CompoundKey` in the in-memory adapter;
- EAV index tables with typed value columns in the SQLite and PostgreSQL
  adapters.

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

1. **Datetime canonicalization (actionable).** Make the store index `Datetime`
   fields by their canonical instant rather than their raw string: parse the
   value to epoch nanoseconds (reusing the SSOT's RFC 3339 / epoch-nanos
   coercion) and store it in the ordered domain (an `i64`, i.e. the existing
   `IndexValue::Integer` lane or a dedicated datetime lane), so ordering is
   instant-correct across offsets. This requires **reindexing existing datetime
   indexes** (a data migration), so it is scoped as its own bead with a
   migration step, not folded into a sweep.
2. **Type-mismatch policy (no change).** The `None`-vs-`Err` difference is
   intentional (FEAT-013) and already documented on both sides; no
   reconciliation is required.
3. **Full byte-key store migration (deferred-with-conditions).** Re-scope
   `axon-302fe6be` to a deferred item: pursue a literal byte-key store only if a
   triggering condition fires (below). The conformance test already guarantees
   the *ordering* the SSOT promises, so the typed model is not incorrect — it is
   merely a second encoding of the same order.

### Why not the full byte-key migration now

The migration's cost is high and concentrated in the query path and schema:

- The EAV value columns (typed) become opaque `BYTEA` / `BLOB`; range-scan
  bound translation must encode query bounds to canonical bytes on every
  lookup; the in-memory `BTreeMap<IndexValue>` becomes `BTreeMap<Vec<u8>>`.
- A full data migration to re-encode every existing index row across all three
  backends.
- It changes the same datetime representation that the targeted fix changes —
  so it does not avoid the migration, it *enlarges* it.

…while the benefit is largely **SSOT purity**, not correctness: equivalence of
ordering for the scalar types is already proven by test. The single correctness
gain (datetime) is obtainable far more cheaply by the targeted fix. Spending the
larger migration now would be paying for aesthetics and pre-empting the cheap
fix's data migration with a larger one.

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

Rejected for now: high cost (query path + EAV schema + cross-backend data
migration), benefit dominated by SSOT aesthetics. Deferred with explicit
revisit conditions; the conformance test holds the line in the meantime.

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
| Positive | The only real correctness defect (datetime cross-offset ordering) gets a scoped, migration-aware fix; the typed model and its test-verified SSOT equivalence are preserved; the FEAT-013 type-mismatch policy is unchanged; the large byte-key migration is not paid for prematurely and is gated behind concrete conditions |
| Negative | Two encodings of the canonical order continue to coexist (typed `Ord` in the store, bytes in `axon-esf`), so `axon-esf::index_key` remains the SSOT *by verified equivalence* rather than by direct use; until the datetime fix lands, mixed-offset datetime indexes mis-sort |
| Neutral | No code change from this ADR itself; it records the decision, re-scopes `axon-302fe6be` to deferred-with-conditions, and spawns a focused datetime-canonicalization bead |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| A deployment already stores mixed-offset datetimes in an indexed field and relies on correct ordering before the targeted fix ships | Low | Medium | Prioritize the datetime-canonicalization bead; document the residual (done, in `IndexValue` docs + the esf Store-SSOT note); deployments emitting only `Z` datetimes are unaffected |
| The two encodings drift on a future type or edge case the conformance test misses | Low | Medium | The `index_key_conformance` test is the guard; extend it whenever an index type or coercion rule changes |
| The datetime fix's data migration is mishandled (stale string-form rows remain) | Medium | Medium | Treat reindexing as an explicit migration step in the fix's bead, with a conformance assertion over migrated data across all three backends |

## Supersession

- **Supersedes**: None.
- **Superseded by**: None. Re-scopes the implementation bead `axon-302fe6be`
  (full byte-key migration → deferred-with-conditions) and is the decision of
  record for the Store-SSOT follow-up from `axon-7fbe288e`.

## Concern Impact

- **rust-cargo**: None from this ADR (design-only). The targeted datetime fix it
  authorizes will touch `axon-storage` index extraction across the three
  adapters plus a data migration; that work is tracked separately.
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
