---
ddx:
  id: ADR-029
  depends_on:
    - helix.prd
    - FEAT-013
    - ADR-028
    - ADR-004
  review:
    self_hash: 620099bb22be72c64d5d056c397b28222537069f171fa8cc23742e71f6ad121f
    deps:
      ADR-004: a58eda0c55e1ac9c4e8cd6fc69d213455354b62286d62be2579de9add3ad01d2
      ADR-028: 82a972d29f478ad094cd7911c9166afe617d40f8727d338be87a13fa163e6417
      FEAT-013: e218b7499012d56e569acc094cc40b47360b34fda601b473ac425af2cec09b27
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:22:34Z"
---
# ADR-029: Persisted Byte-Keyed Secondary Indexes — Backend Parity & Query Performance

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-29 | Accepted | Erik LaBianca | FEAT-013, ADR-028, ADR-004, axon-esf::index_key | High |

## Context

FEAT-013 defines secondary indexes for efficient lookup of entities by field
value. Until now **only the in-memory adapter maintained them** (a `BTreeMap`
keyed by the typed `IndexValue`/`CompoundKey`). The durable backends — SQLite
and PostgreSQL — overrode **none** of the index trait methods
(`update_indexes` / `index_lookup` / `index_range` / `index_unique_conflict` /
`drop_indexes` and the compound variants), so every indexed-field query on a
durable backend **fell through to a full collection scan**, and behavior
diverged from the in-memory adapter.

This is a real parity and performance gap: the production backend (PostgreSQL)
had the *worst* indexed-query behavior (O(n) scans), and identical queries could
return results in a different order — or with different cost — depending on the
backend.

**ADR-028 mis-scoped this.** It framed the `axon-esf::index_key` byte-key work as
a migration of the *in-memory* index representation and concluded it was "mostly
aesthetic," deferring it. That analysis optimized the one backend that already
had indexes and treated the actual deliverable — **giving the SQL backends real
indexes** — as out of scope. The genuine objective is backend parity + efficient
indexed queries, which ADR-028 did not weigh. This ADR supersedes that deferral.

## Decision

**Maintain persisted, byte-keyed secondary indexes in every storage backend,
keyed by the canonical `axon-esf::index_key` encoding**, so indexed lookups are
efficient and behave identically across memory, SQLite, and PostgreSQL.

### Design

- **EAV byte-key index tables** in each SQL backend, mirroring the entities
  namespacing (`database_name, schema_name, collection`):
  - `entity_index(database_name, schema_name, collection, field, key, entity_id)`
  - `entity_compound_index(database_name, schema_name, collection, index_ordinal, key, entity_id)`
  - `key` is `BLOB` (SQLite) / `BYTEA` (PostgreSQL); both compare **bytewise**,
    which matches the canonical order-preserving encoding, and a native B-tree
    index over the key column gives efficient equality and range scans.
- **Key encoding (the single source of truth, `axon-esf`):**
  - Single-field key = the **unframed** order-preserving bytes from
    `encode_index_value(value, index_type)` — `memcmp` reproduces the typed value
    order for every type (verified by `index_key_conformance`). The *framed*
    `index_key` output is NOT used for single-field keys (its length prefix
    breaks string ordering — see ADR-028's amendment).
  - Compound key = the **framed** composite from `encode_compound_index_key`
    (length-prefixed per field): a leading-fields encoding is a literal byte
    prefix of the full key, so `compound_index_prefix` is a byte-range scan
    (`key >= P AND key < successor(P)`).
  - Null/missing → not indexed (sparse); type mismatch / unencodable → not
    indexed (FEAT-013), never an error. Array (`field[]`) indexes produce one
    key per scalar item. The in-memory adapter keeps its typed `IndexValue`
    representation, whose ordering is test-verified equal to these bytes, so all
    three backends agree without the in-memory adapter itself switching to bytes.
- **Backfill (data migration):** persisted indexes must cover entities that
  predate an index. Each SQL adapter reindexes a collection from its entities on
  schema (re)registration (`put_schema`) and once on open for any indexed
  collection whose index tables are empty.
- **Write/query integration:** the handler already calls `update_indexes` &c. on
  mutations and `index_lookup`/`index_range` (via `try_index_lookup`) on queries,
  so implementing the trait methods flips both paths to real indexes with no
  handler changes.

### Consistency

Because queries now *use* the persisted index, a stale index returns wrong
results — so maintenance must stay consistent with entity writes:

- **Multi-op transactions** previously bypassed index maintenance entirely (a
  pre-existing bug). They now maintain single + compound indexes inside the
  existing storage transaction (`begin_tx`/`commit_tx`), so maintenance rolls
  back atomically with the entity writes. Updates read the authoritative
  pre-image from storage so stale keys are removed reliably.
- **Single-entity mutations** pre-validate unique constraints **before** the
  write, so a uniqueness conflict cannot leave an entity written-but-unindexed.
  They are deliberately **not** wrapped in a per-write `begin_tx`: the in-memory
  adapter's `begin_tx` snapshots the entire store, so per-write wrapping would
  regress the fast backend. The accepted residual is a rare transient storage
  error strictly between the entity `put` and `update_indexes`; full
  transactional wrapping was rejected to preserve in-memory performance.

## Alternatives Considered

### 1. Defer (status quo, ADR-028's stance)
Rejected: leaves SQL backends full-scanning indexed queries and diverging from
the in-memory adapter — the opposite of the FEAT-013 intent. ADR-028's "mostly
aesthetic" conclusion only held under its mis-scoping to the in-memory adapter.

### 2. Native per-type SQL expression / generated-column indexes
Rejected: abandons the canonical `index_key` encoding, is per-type and
backend-specific (SQLite vs Postgres differ), and would not give uniform
cross-backend ordering. The EAV byte-key approach uses one SSOT encoding for all
backends.

### 3. Per-write transactional wrapping of single mutations
Rejected for now (see Consistency): the in-memory `begin_tx` full-store snapshot
makes per-write wrapping a performance regression. Pre-validation covers the
realistic divergence; the rare transient-error residual is documented.

## Consequences

| Type | Impact |
|------|--------|
| Positive | SQLite & PostgreSQL gain efficient indexed equality/range/unique/compound lookups (no more full scans); all three backends behave identically (parity), proven by a shared conformance suite; one canonical key encoding (`axon-esf`) across backends; transaction-path index-maintenance bug fixed |
| Negative | Each SQL backend carries index tables + write-path maintenance cost; a schema (re)registration backfills the collection's index; an accepted rare transient-error consistency window on single-entity mutations (not transaction-wrapped, to protect in-memory performance) |
| Neutral | The in-memory adapter keeps its typed `IndexValue` representation (test-verified equal to the byte encoding) rather than switching to bytes — parity is by verified equivalence, which is sufficient |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Index/entity divergence from the residual single-mutation window | Low | Medium | Uniqueness pre-validation removes the realistic case; reindex on schema registration repairs; revisit if a stronger guarantee is needed |
| Backfill cost on large collections at schema (re)registration | Low | Medium | Scoped to the affected collection; schema changes are rare |
| Backend behavioral drift on index edge cases | Low | Medium | Shared `storage_conformance_tests!` index suite runs identical assertions on memory/SQLite/Postgres |

## Supersession

- **Supersedes**: ADR-028's *deferral* of the byte-key index work. The technical
  findings ADR-028 recorded (the `index_key` framed-string order-contract defect;
  secondary indexes were in-memory-only; the datetime fix needed no migration)
  remain valid; only its decision to defer the SQL-backend index work is
  reversed, because it was scoped to the wrong objective (in-memory aesthetics
  rather than backend parity + query performance).
- **Superseded by**: [ADR-030](./ADR-030-storage-owns-index-maintenance.md) — but
  only its **single-mutation consistency stance** (this ADR's caller-driven
  maintenance + uniqueness pre-validation + documented per-write residual). The
  persisted-index design here (EAV byte-key tables, canonical encoding, backfill)
  stands; ADR-030 changes *who maintains and how atomically* (the storage write
  primitives, atomically) — not the index format.

## Concern Impact

- **rust-cargo**: `axon-storage` gains index tables + trait-method implementations
  in the SQLite and PostgreSQL adapters and shared encoders in `axon-esf`;
  `axon-api` gains transaction-path index maintenance + uniqueness pre-validation.
- **security-owasp**: None new. Index maintenance is internal; no new external
  surface. Indexed queries return the same authorized results as scans.

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Identical index behavior across memory/SQLite/Postgres (shared conformance suite green) | Any new index type/op or a backend-specific index test failure |
| Indexed queries on SQL backends use the index, not a full scan | A reported indexed-query performance regression |
| No index/entity divergence in normal operation | A reported stale-index result → revisit single-mutation transactional wrapping |

## References

- [FEAT-013: Secondary Indexes](../../01-frame/features/FEAT-013-secondary-indexes.md)
- [ADR-028: Index-Key Store SSOT](./ADR-028-index-key-store-ssot.md) (deferral superseded here)
- [ADR-004: Transaction Model — OCC](./ADR-004-transaction-model.md)
- [Canonical index-key encoder (SSOT)](../../../crates/axon-esf/src/index_key.rs)
- [Storage adapters & index conformance](../../../crates/axon-storage/src/adapter.rs)
