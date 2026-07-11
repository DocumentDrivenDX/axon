---
ddx:
  id: TP-001-ratchets
  depends_on:
    - TP-001
  review:
    self_hash: 82e29fcd752662a16d10afecdddb1622ba0b0a31ac95ec763c6306f0a7a532d8
    deps:
      TP-001: 058932393e672c4c5c89acf600d9d45b3f712fe114e7caa139f0e5ac11dc7967
    reviewed_at: "2026-07-11T05:09:15Z"
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
| Performance p99 (BM-001..BM-010) | Latency ↓ or stable | Nightly / manual | `scripts/run-benchmarks.sh` |
| Line coverage (axon-core + axon-api ≥ 90%) | % ↑ | Per-release review | `cargo llvm-cov --package axon-core --package axon-api` |
| Workspace line coverage (≥ 80%) | % ↑ | Per-release review | `cargo llvm-cov --workspace` |
| Audit gap count | Count → 0 | Every commit (CI, via `cargo test`) | `cargo test -p axon-sim -- audit` |

---

## Graph Benchmark Qualification

The release-blocking graph benchmark is the `ready_beads` / `blocked_beads`
gate from `STP-074`. It is frozen for `TARGET_RELEASE`, and only the dedicated
reference host/runner may decide pass/hold. GitHub-hosted functional runs are
useful for smoke checking the code path, but they are not authoritative and
must not clear or set `release.block`.

| Field | Frozen value |
|-------|--------------|
| Dataset | Synthetic DDX bead graphs at `1,000` and `10,000` beads; the 10-bead/15-link smoke fixture stays correctness-only |
| Hardware class | Dedicated reference host, not a GitHub-hosted runner |
| Backend / configuration | `cargo bench -p axon-cypher` on the `ddx_ready_blocked_queue_benchmark` path, using the in-memory named-query fixture |
| Warmup | `10` warmup iterations before measurement |
| Sample count | `101` measured samples |
| Percentile method | nearest-rank `p99` |
| Pass threshold | `ready_beads` and `blocked_beads` must both stay below `100 ms` at `1,000` beads and below `500 ms` at `10,000` beads |
| Artifact / metadata | Each release run records `commit`, `environment`, `artifact_paths`, `metadata`, `p99_ms`, `threshold_ms`, and `runner_class` in the execution bundle |
| Hold rule | Any GitHub-hosted functional run is informational only; only the dedicated reference host may clear `release.block` for `TARGET_RELEASE` |

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

### L5 Criterion benchmarks (BM-001..BM-010)

Benchmarks are defined in `crates/axon-api/benches/benchmarks.rs` and measure
the targets from TP-001 §9. They are not ratcheted automatically yet — a
failing seed or regression in benchmark output should be investigated before
merging the offending change.

```bash
scripts/run-benchmarks.sh         # all benchmarks
scripts/run-benchmarks.sh BM-001  # single benchmark by name filter
```

Benchmark blocker note: automatic threshold enforcement (fail CI if p99 exceeds
target) requires a baseline measurement file and a comparison step. This is
planned but not yet implemented. For now, criterion output should be reviewed
manually after any change to hot paths.

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
