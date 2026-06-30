---
ddx:
  id: ADR-030
  depends_on:
    - helix.prd
    - FEAT-013
    - ADR-029
    - ADR-004
---
# ADR-030: Storage Owns Index Maintenance — Atomic by Construction

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-06-29 | Accepted | Erik LaBianca | FEAT-013, ADR-029, ADR-004 | High |

## Context

ADR-029 made the SQL backends maintain persisted secondary indexes, but **the
caller drove maintenance**: the handler (and the multi-op transaction) wrote an
entity via a storage primitive (`put` / `compare_and_swap` / `delete` /
`create_if_absent`) and then *separately* called `update_indexes` /
`update_compound_indexes` / `remove_*`. That left two problems:

1. **A consistency residual.** Entity write and index maintenance were separate,
   non-atomic operations on single-entity mutations, so a failure between them
   could leave an entity written-but-unindexed (or vice-versa) — and since
   queries now use the index, that divergence yields wrong results. ADR-029
   closed this only partially (uniqueness pre-validation + a documented residual),
   deliberately *not* wrapping single mutations in a transaction because the
   in-memory `begin_tx` snapshots the whole store.
2. **A "caller forgot to maintain" bug class.** Maintenance was scattered across
   every write path; one had already shipped wrong — the multi-op transaction
   `execute()` never maintained indexes at all (fixed reactively). Requiring every
   caller to know when a write touches indexes does not scale.

## Decision

**Move secondary-index maintenance INTO the storage write primitives**, so an
entity write and its index changes are atomic by construction, on every backend,
and no caller can forget. The four write primitives — `put`, `compare_and_swap`,
`delete`, `create_if_absent` — maintain single + compound indexes themselves; all
callers simply write entities.

### Design

- **Index defs from the entity's stamped `schema_version`** (the adapter resolves
  them via `StorageAdapter::index_defs_for_entity`, preferring the exact version,
  falling back to latest). No registered schema / no indexes → maintenance is
  skipped and the write keeps its single-statement fast path (so schemaless and
  system-collection writes pay nothing).
- **Atomicity, begin-if-not-already.** On SQL, a primitive that needs to maintain
  wraps entity write + index DELETE/INSERTs + unique check in one transaction —
  but only opens its own `BEGIN` when not already inside one. A primitive invoked
  inside the multi-op transaction's `begin_tx` *joins* it: it never commits or
  rolls back its parent's transaction (owned-vs-joined ownership). In-memory
  mutations are in-process; the existing snapshot `abort_tx` already covers the
  index maps.
- **Uniqueness is enforced atomically** by the primitive (validate-then-mutate),
  returning `AxonError::UniqueViolation` and rolling back the entity write. The
  former caller-side pre-validation is removed.
- **Restore/rollback/revert paths** now maintain indexes via the primitives (they
  previously did so inconsistently or not at all) — fixing latent stale-index bugs.
- **The maintenance logic is shared** (the public maintenance trait methods and
  the primitives route through one set of private helpers).

## Alternatives Considered

### K — capability-gated `begin_tx` wrapping of single mutations (rejected)
Wrap entity+index (+audit) in `begin_tx` on backends that need it; in-memory skips
it. Smaller, and it would also have closed the single-entity **audit** gap. Rejected
because it leaves maintenance caller-driven — the "caller forgot" class persists and
does not scale (the directing concern). C eliminates that class for index
maintenance.

### D — cheap in-memory `begin_tx` (undo log) + uniform wrapping (rejected)
Same caller-driven shortfall as K, plus a substantial in-memory undo-log rewrite.

### Keep ADR-029's caller-driven maintenance + pre-validation (rejected)
The status quo this ADR supersedes: scattered maintenance, a real "forgot" bug
already shipped, and only a partial residual closure.

## Consequences

| Type | Impact |
|------|--------|
| Positive | Entity↔index writes are atomic on every backend; the single-entity index-maintenance residual is closed; the "caller forgot to maintain indexes" bug class is eliminated for the write path (every write maintains, or cheaply no-ops when index-free); restore/rollback/revert latent stale-index bugs fixed; maintenance logic centralized |
| Negative | A maintaining write resolves the collection's index defs (a schema read) and, on SQL, runs a short transaction — paid only when the collection has indexes; `put` is now schema-aware (no longer a policy-free byte upsert) for the index dimension |
| Neutral | The in-memory adapter keeps its typed `IndexValue` representation; the public maintenance trait methods remain for now as a tested low-level primitive (their production callers are removed) — their removal is tracked separately |

## Out of scope here — audit atomicity (resolved by the follow-up below)

C itself makes entity↔index atomic but not the **audit** write. That gap — the
pre-existing **ADR-004 INV-003** — was closed for **single-entity mutations** by a
follow-up (`axon-06459077`): the durable audit append (`storage.append_audit_entry`)
now runs INSIDE the write transaction, co-located with the entity+index write,
mirroring the multi-op transaction path. It is capability-gated via
`StorageAdapter::supports_durable_audit()` (SQLite/Postgres `true`; in-memory
`false`, whose `append_audit_entry` is a no-op and whose `begin_tx` snapshots the
whole store). The in-memory `self.audit` (`MemoryAuditLog`) append still runs
post-commit as the queryable view; the API-visible audit id remains that view's.
All 8 single-entity write paths (create / update / patch / delete / revert /
rollback_entity / rollback_single_entity_to_timestamp /
rollback_single_entity_from_transaction) route through this co-location.

Still open (separate, tracked): (a) the SAME co-location for **non-entity** audit
writes — collection/template/schema/link operations still append audit out of band;
(b) rehydrating the in-memory `MemoryAuditLog` from the durable `audit_log` table on
restart so audit *queries* reflect the durable record. Neither regresses anything
(both pre-existed and affect the transaction path identically).

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| A joined primitive mishandles the parent transaction (commits/rolls back) | Low | High | Owned-vs-joined ownership keyed on `in_tx`; joined-path abort tests on SQLite + Postgres assert the parent rolls back both entity and index rows |
| Schema-version skew (maintain per a different version than validated) | Low | Medium | Resolve index defs by the entity's stamped `schema_version`; single-instance `&mut self` serializes schema changes against writes |
| A future caller calls the still-public maintenance method redundantly | Low | Low | Maintenance is idempotent (set / delete-then-insert); methods scheduled for removal |

## Supersession

- **Supersedes**: ADR-029's **single-mutation consistency stance** (caller-driven
  maintenance + uniqueness pre-validation + the documented per-write residual).
  ADR-029's persisted-index design (EAV byte-key tables, canonical encoding,
  backfill) stands; only *who maintains and how atomically* changes here.
- **Superseded by**: None.

## Concern Impact

- **rust-cargo**: `axon-storage` write primitives maintain indexes (memory/sqlite/
  postgres); `axon-api` handler + transaction write paths simplified (maintenance
  + pre-validation removed).
- **security-owasp**: None new; indexed reads return the same authorized results.

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Every entity-write path (incl. transactions, rollback, revert) reflects in indexed queries across all backends | A stale-index or missing-index query result |
| Joined-path abort rolls back entity + index together (SQL) | A partial-write divergence report |
| Single-entity audit atomicity (INV-003) closed via `axon-06459077` (durable audit co-located in the write tx); non-entity audit + query rehydration remain tracked follow-ups | A committed-but-unaudited mutation report on a non-entity operation |

## References

- [FEAT-013: Secondary Indexes](../../01-frame/features/FEAT-013-secondary-indexes.md)
- [ADR-029: Persisted Byte-Keyed Secondary Indexes](./ADR-029-persisted-sql-secondary-indexes.md) (single-mutation consistency stance superseded here)
- [ADR-004: Transaction Model — OCC](./ADR-004-transaction-model.md) (INV-003 audit atomicity — closed for single-entity mutations via `axon-06459077`; non-entity audit paths still open)
- [Storage write primitives + index maintenance](../../../crates/axon-storage/src/adapter.rs)
