# Backend Conformance — Deferred Backends

**Bead**: axon-dfe3cc58 — "test: ratchet backend and embedded/server conformance"

## Completed (this bead)

| Backend | Status | Invocation |
|---------|--------|------------|
| Memory (`MemoryStorageAdapter`) | **Done** — `cargo test -p axon-storage` | `crates/axon-storage/src/memory.rs` |
| SQLite (`SqliteStorageAdapter`) | **Done** — `cargo test -p axon-storage` | `crates/axon-storage/src/sqlite.rs` |
| PostgreSQL (`PostgresStorageAdapter`) | **Done** — `cargo test -p axon-storage` (skips gracefully when no cluster reachable) | `crates/axon-storage/src/postgres.rs` |

CI: the `storage-conformance` job in `.github/workflows/ci.yml` runs the full
suite against SQLite (always) and PostgreSQL (via a `postgres:16` service
container), satisfying TP-001 L4's "CI: SQLite. Pre-merge: all backends" requirement
for the currently-implemented adapters.

## Deferred — FoundationDB

The test plan (TP-001 §8 L4 table) lists FoundationDB as a required backend for
conformance. FoundationDB support is **not yet implemented** in `axon-storage`
(no `FoundationDbStorageAdapter` type exists). It is deferred until:

1. The `axon-storage` crate gains a FoundationDB adapter implementation.
2. A CI environment with a FoundationDB cluster (or `foundationdb` Docker image)
   is available.

**Follow-up work**: file a bead titled
`"feat: FoundationDB StorageAdapter + L4 conformance"` with:
- Implement `FoundationDbStorageAdapter: StorageAdapter` in `crates/axon-storage/src/foundationdb.rs`
- Invoke `storage_conformance_tests!(maybe: {...}, fdb_conformance)` at the end of that file
- Wire a CI job with a FDB service container (similar to the PostgreSQL conformance job)
- Add to the L4 table in TP-001 once passing

Until that bead is completed, the FoundationDB row in TP-001 §8 remains `Required`
but untested; this bead documents the gap explicitly rather than implying
completeness.
