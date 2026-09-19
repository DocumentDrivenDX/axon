# Governed System Backend Parity Report

Bead: `axon-gap-closure-94348231`

## Verification Matrix Plan

Required configuration:

- PostgreSQL qualification DSN from `AXON_TEST_POSTGRES`, redacted in this report.
- Serialized test execution for backend-sensitive gates via `RUST_TEST_THREADS=1`.
- Evidence path: `.ddx/executions/20260711T194728-29cf4671/governed_system_backend_parity_report.md`.

Required gates:

- `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test -p axon-api --test governed_system_backend_parity`
- `cargo test -p axon-api governed_system_`
- `cargo test -p axon-api bead_`
- `cargo test -p axon-api --test governed_system_compile_fail`
- `cargo test -p axon-api --test raw_access_compile_fail`
- `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test --workspace`
- `cargo clippy -- -D warnings`
- `cargo fmt --check`

Completion criteria:

- Every final required gate exits 0 after documented retries.
- The governed-system backend parity target reports seven named tests, with each test running memory, SQLite, and PostgreSQL fixture backends.
- Focused `governed_system_` and `bead_` filters report nonzero named test counts.
- Compile-fail suites pass.
- Workspace, clippy, and format gates pass without skipped governed-system, bead, raw-boundary, or PostgreSQL conformance assertions.

## Implementation Evidence

- Added `crates/axon-api/tests/governed_system_backend_parity.rs`.
- The target defines seven named vectors: bootstrap, schema, entity, link, lifecycle, OCC, and audit.
- Each vector is run through the same generic function body on:
  - `MemoryStorageAdapter`
  - `SqliteStorageAdapter`
  - `PostgresStorageAdapter`
- PostgreSQL cases require `AXON_TEST_POSTGRES`; missing PostgreSQL configuration fails the test instead of skipping.

## PostgreSQL Qualification Environment

- Environment variable: `AXON_TEST_POSTGRES` set.
- Redacted DSN: `postgres://postgres:***@192.168.215.10:5432/postgres`.
- `psql "$AXON_TEST_POSTGRES" -Atc "SHOW server_version;"`: exit 0, `16.14`.
- `psql "$AXON_TEST_POSTGRES" -Atc "SHOW server_version; SHOW server_version_num;"`: exit 0, `160014`.
- Qualification: PostgreSQL 16 (`server_version_num=160014`).

## Command Results

| Command | Exit | Evidence |
|---|---:|---|
| `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test -p axon-api --test governed_system_backend_parity` | 101 | Initial red run: 5 passed, 2 failed (`audit_vector_runs_on_memory_sqlite_and_postgres`, `schema_vector_runs_on_memory_sqlite_and_postgres`). |
| `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test -p axon-api --test governed_system_backend_parity` | 0 | Final run: 7 passed, 0 failed, 0 ignored. Seven named tests x three backends = 21 backend cases. |
| `cargo test -p axon-api governed_system_` | 0 | 10 named tests executed: 9 unit tests plus `governed_system_compile_fail_cases`; 0 failed. |
| `cargo test -p axon-api bead_` | 0 | 21 named tests executed: 20 unit tests plus `scn_007_bead_lifecycle_concurrent_agents`; 0 failed. |
| `cargo test -p axon-api --test governed_system_compile_fail` | 0 | 1 compile-fail test passed. |
| `cargo test -p axon-api --test raw_access_compile_fail` | 0 | 1 raw-access compile-fail test passed. |
| `AXON_TEST_POSTGRES=$AXON_TEST_POSTGRES RUST_TEST_THREADS=1 cargo test --workspace` | 0 | Workspace passed. Relevant non-doc gates included axon-api unit tests (381), existing backend parity (12), business scenarios (19), governed-system backend parity (7), governed compile-fail (1), raw-access compile-fail (1), serializable autocapture (10), axon-storage unit/conformance (333), PostgreSQL auth schema (12), PostgreSQL routine privileges (3), PostgreSQL tenant isolation (4), and tenant users (15). |
| `cargo clippy -- -D warnings` | 0 | Initial clippy pass before rustfmt. |
| `cargo fmt --check` | 1 | Initial format gate found rustfmt wrapping diffs in the new integration test. |
| `cargo fmt` | 0 | Applied rustfmt wrapping only. |
| `cargo fmt --check` | 0 | Final format gate passed. |
| `cargo clippy -- -D warnings` | 0 | Final clippy pass after rustfmt. |

## Named Test And Backend Counts

- Governed-system backend parity: 7 named tests.
- Backends per parity test: 3.
- Backend cases covered: 21.
- Vectors per backend: bootstrap, schema, entity, link, lifecycle, OCC, audit.
- Focused governed-system filter: 10 named tests.
- Focused bead filter: 21 named tests.
- Compile-fail focused targets: 2 named tests total.

## Skip Assessment

- `governed_system_backend_parity`: 0 ignored; PostgreSQL is mandatory and was exercised in each vector.
- `governed_system_`: relevant governed-system tests executed with nonzero counts.
- `bead_`: relevant bead tests executed with nonzero counts.
- `raw_access_compile_fail`: 0 ignored.
- Workspace PostgreSQL storage conformance and PostgreSQL integration suites executed under `AXON_TEST_POSTGRES`; no PostgreSQL conformance assertion was skipped in the observed command output.
- Workspace doc-test ignores were limited to documentation examples and are not governed-system, bead, raw-boundary, or PostgreSQL conformance assertions.

## Retry Rationale

- The first parity run failed because the new schema vector asserted total audit length stability, but idempotent bootstrap can append non-entity/link audit. The assertion was narrowed to the intended invariant: invalid import must not append entity or link audit.
- The first parity run also failed because the link audit assertion used a bead collection filter. Link audit entries are asserted through the full handler audit stream, matching existing audit semantics.
- The first format check failed only on rustfmt wrapping in the new test file. `cargo fmt` was applied and the final `cargo fmt --check` passed.
