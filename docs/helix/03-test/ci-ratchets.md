---
ddx:
  id: TP-001-ratchets
  depends_on:
    - TP-001
  review:
    self_hash: e6524f2597a069006e2eb7bc0fb452878a8480da90f7832bf284a72f235d0962
    deps:
      TP-001: b2fd65f5c9fee74cac32a456a2eb53e5f492374e51469bbfdfce158ade121821
    reviewed_at: "2026-06-15T00:35:16Z"
---
# CI Ratchet Enforcement Schedule

Derived from TP-001 §2 (Ratchets) and §11 (Test Execution Schedule).

This document records which quality gates are enforced at each trigger point and
how to run each gate locally.

---

## Ratchet Summary

| Ratchet | Direction | Trigger | Command |
|---------|-----------|---------|---------|
| `@covers` scanner (AC citation format) | Malformed → 0 | Every commit (CI) | `python3 scripts/check_covers_traceability.py --format text` |
| Correctness seeds (L1 invariants, 10 seeds) | Pass-count ↑ | Every commit (CI) | `scripts/run-sim-seeds.sh` (default 10 seeds) |
| Correctness seeds (L1 invariants, 1 000 seeds) | Pass-count ↑ | Nightly | `AXON_SIM_SEEDS=1000 scripts/run-sim-seeds.sh` |
| Performance p99 (BM-001, 002, 003, 004, 006, 007, 009, 010, 011, 012) | Latency < TP-001 §9 target, enforced automatically | Nightly (`cargo bench` panics on trip) | `scripts/run-benchmarks.sh` |
| Slow-test wall-clock (axon-storage L4 conformance) | Wall-clock ≤ 90s | Every commit (CI) | `scripts/check-slow-tests.sh 90 /tmp/storage-conformance.log -- cargo test -p axon-storage` |
| Line coverage (axon-core + axon-api ≥ 90%) | % ↑ | Per-release review | `cargo llvm-cov --package axon-core --package axon-api` |
| Workspace line coverage (≥ 80%) | % ↑ | Per-release review | `cargo llvm-cov --workspace` |
| Audit gap count | Count → 0 | Every commit (CI, via `cargo test`) | `cargo test -p axon-sim -- audit` |

---

## Per-Commit CI Gates

These run in `.github/workflows/ci.yml` on every push and pull request.

### `@covers` citation scanner

Scans `crates/`, `ui/`, and `sdk/typescript/` for `@covers US-<n>-AC<m>` citations
and fails on malformed citations.

```bash
python3 scripts/check_covers_traceability.py --format text
```

The scanner does **not** fail on zero coverage — it only fails on malformed
`@covers` tokens. The coverage report is informational. Once the first
remediation pass adds citations, a stricter "fail if P0 ACs are uncited"
mode will be added.

### Bounded simulation seed sweep (L1, 10 seeds)

Runs five correctness invariants (INV-001/002/003/004/008) across 10 seeds.
Seed count is controlled by `AXON_SIM_SEEDS` (default: 10 for CI).

```bash
scripts/run-sim-seeds.sh          # 10 seeds
cargo test -p axon-sim            # includes seed_sweep + all unit tests
```

Any seed that fails must be added to `scripts/regression-seeds.txt` and
replayed on every future CI build.

### Cargo test (all crates)

```bash
cargo test                        # runs L1–L4 tests wired into cargo
cargo clippy -- -D warnings       # lint gate
cargo fmt --check                 # format gate
```

---

## Nightly / Manual Gates

These run in `.github/workflows/nightly.yml` (scheduled 02:00 UTC) or on
`workflow_dispatch`. They are **not** enforced on every commit because they
are too expensive (benchmark wall-clock, extended seed sweep).

### Extended simulation seed sweep (L1, 1 000 seeds)

```bash
AXON_SIM_SEEDS=1000 scripts/run-sim-seeds.sh
# or via workflow_dispatch: set sim_seeds input
```

### L5 Criterion benchmarks (BM-001..BM-012)

Benchmarks are defined in `crates/axon-api/benches/benchmarks.rs` and
`crates/axon-cypher/benches/ddx_benchmark.rs`; together they measure the
targets from TP-001 §9 plus the graph and DDx named-query workloads now
captured in the story test plans.

Each benchmark asserts its own p99 gate in-process (`assert_p99_gate` in
`benchmarks.rs`, mirroring the pre-existing `assert_p99_gate` in
`ddx_benchmark.rs`): 101 timed samples are taken after a 10-iteration warmup,
and `cargo bench` panics (nonzero exit) if the measured p99 does not stay
under the TP-001 §9 target. This makes the p99 ratchet self-enforcing —
no separate baseline file or comparison step is needed, and thresholds can
only be tightened by editing the target constants alongside a TP-001 update,
never loosened silently. BM-005 (audit overhead, measured as a delta) and
BM-008 (concurrent-writer throughput scaling) have no fixed single-sample
budget in TP-001 and are not gated; their Criterion output is still reviewed
manually per the guidance below.

```bash
scripts/run-benchmarks.sh         # all benchmarks (fails if any p99 gate trips)
scripts/run-benchmarks.sh BM-001  # single benchmark by name filter
cargo bench -p axon-api -- --test # gate-only smoke run (single sample pass, no Criterion report)
```

### Benchmark evidence bundle

`scripts/run-benchmarks.sh` writes `target/benchmarks/<run-id>/benchmark-metadata.json`
so each run carries the exact commit and host/environment that produced it.
The manifest records:

- commit SHA
- host/environment snapshot
- backend inventory
- dataset sizes
- p99 artifact paths

Nightly uploads the benchmark evidence roots:

- `target/benchmarks/`
- `target/criterion/`

Criterion p99 evidence is retained under:

- `target/criterion/**/new/sample.json`
- `target/criterion/**/new/estimates.json`
- `target/criterion/**/report/index.html`

| Suite | Backend | Dataset inventory |
|-------|---------|-------------------|
| `axon-api` | `memory` | BM-001..BM-012 ranges from 10,000 point lookups to 99-link neighbor queries |
| `axon-cypher` | `fixture` | 1,000-bead and 10,000-bead ready/blocked queue fixtures |

Automatic p99 threshold enforcement (resolved 2026-07-08): each gated
benchmark now fails `cargo bench` — and therefore the nightly `benchmarks` job
in `.github/workflows/nightly.yml` — if its own in-process p99 sample exceeds
the TP-001 §9 target. Criterion's own statistical output (mean, throughput,
regression-vs-previous-run plots) remains informational and should still be
reviewed manually after any change to hot paths; the automated gate only
covers the fixed p99 ceiling, not relative regression detection.

The L5 criterion suite above only exercises the `memory` and `fixture`
backends; it does not benchmark `PostgresStorageAdapter`. Postgres storage-layer
latency is not currently ratcheted by L5 — see the classification below for
why the *conformance test suite* was slow, which is a separate concern from
storage-engine latency.

### PostgreSQL L4 conformance test performance — classification (2026-07-08)

A readiness review flagged the PostgreSQL L4 backend-conformance test run
(`cargo test -p axon-storage`) as slow. Investigation found this was a
**test-environment cost, not a storage-layer performance issue**:

- Three external integration test files
  (`crates/axon-storage/tests/{auth_schema,tenant_users_test,postgres_tenant_isolation}.rs`)
  each started a **brand-new Docker `testcontainers` PostgreSQL container per
  `#[test]` function** (19 container startups total across the three files),
  instead of sharing one container per test binary the way the L4 conformance
  macro suite already did (`pg_conformance_superadmin_dsn` in
  `crates/axon-storage/src/postgres.rs`).
- The in-crate native PostgreSQL test module (`postgres::tests`, ~39 tests)
  additionally started its own fresh container per test *and* serialized all
  39 tests behind a single global `Mutex`, because every test connected to the
  same fixed `axon_test` database and needed exclusive access to avoid
  cross-test data races.
- None of this exercised `PostgresStorageAdapter`'s actual read/write/index
  hot paths any differently than the (already fast) L4 conformance macro
  tests — the cost was entirely container-startup and forced serialization
  overhead.

**Fix**: every PostgreSQL-backed test file now starts a single container once
per test binary (cached in a process-wide `OnceLock`, mirroring the existing
`pg_conformance_superadmin_dsn` pattern) and provisions a fresh, uniquely
named database per test via `provision_postgres_database` / `tenant_dsn`
(the same isolation mechanism the conformance suite already used). This
removed the need for the global `Mutex` entirely — each test now owns an
independent database and can run fully in parallel.

**Evidence** (`cargo test -p axon-storage --release`, same host, same
dataset — full local historical p99 run vs. the version at this bead's
`base-rev`):

| Test binary | Before | After |
|---|---|---|
| `axon-storage` lib (333 tests incl. L4 conformance ×3 backends) | 129.59s | 10.78s |
| `tests/auth_schema.rs` (12 tests) | 9.88s | 2.36s |
| `tests/postgres_tenant_isolation.rs` (4 tests) | 3.83s | 3.23s |
| `tests/tenant_users_test.rs` (15 tests) | 4.93s | 2.37s |
| **Total test execution** | **148.23s** | **18.74s (~7.9× faster)** |

**Classification**: resolved as a test-environment cost. No
`PostgresStorageAdapter` production code path changed; the storage engine's
own per-operation performance is unaffected and remains outside the L5
criterion suite's current backend coverage (memory/fixture only, per the
table above).

**Slow-test ratchet**: the `storage-conformance` job in
`.github/workflows/ci.yml` first runs `cargo test -p axon-storage --no-run`
(untimed build), then wraps the real test run with
`scripts/check-slow-tests.sh 90 /tmp/storage-conformance.log -- cargo test -p
axon-storage`. Building first means the timed step measures test execution,
not compilation — compile time varies with CI cache state and is not what
regressed in the incident above. The script times the wrapped command, tees
its output to the given log path (preserving the existing "PostgreSQL
conformance tests ran" grep check below it), and fails the job if wall-clock
time exceeds 90s. 90s gives headroom over the 18.74s measured above for
slower CI runners while remaining far below the 148.23s pre-fix baseline, so
a regression back to per-test container startup trips the gate on every
commit rather than waiting for a readiness review to notice it again.

---

## Coverage

Line-coverage measurement requires `cargo-llvm-cov` and is not run in CI yet
(compile-time cost). The ratchet file at `ratchets/coverage.json` (to be
created on first measurement) will track the current minimum.

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

Per TP-001 §4:
- `axon-core` + `axon-api`: target ≥ 90%, minimum 80%
- Workspace: target ≥ 80%, minimum 70%

---

## Regression Seed File

`scripts/regression-seeds.txt` (not yet created) will hold seeds that have
previously caused invariant violations. Once a seed appears there it is never
removed. The seed sweep test does not read this file yet — that integration is
planned as a follow-up.
