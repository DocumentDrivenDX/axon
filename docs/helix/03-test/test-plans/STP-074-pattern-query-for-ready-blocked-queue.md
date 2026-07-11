---
ddx:
  id: STP-074
  review:
    self_hash: e29bcd0a5817e2018b406491c76cf1c4267860f7dfb8fc0743ef8ab6a548a928
    deps: {}
    reviewed_at: "2026-07-11T03:28:00Z"
---

# Story Test Plan: STP-074-pattern-query-for-ready-blocked-queue

## Story Reference

**User Story**: [[US-074-pattern-query-for-ready-blocked-queue]] (FEAT-009, P0)
**Technical Design**: [[TD-074-ready-blocked-queries]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow → L2; latency → L5; subscription → L6)

## Scope and Objective

**Goal**: prove `ready_beads`/`blocked_beads` return exact complementary partitions of open beads in one round-trip, meet latency targets at 1K/10K scale, and drive subscriptions.
**Blocking Gate**: `cargo test -p axon-cypher`

**In Scope**
- Ready/blocked queue correctness on memory and SQLite backends.

**Out of Scope**
- Generic named-query declaration (STP-075), subscription mechanics (STP-077).

## Benchmark Contract

This story freezes the release-blocking graph benchmark for `TARGET_RELEASE`.
The correctness smoke fixture stays at 10 beads / 15 links. The release gate
uses synthetic DDX graphs at `1,000` and `10,000` beads on the dedicated
reference host, with `10` warmup iterations, `101` measured samples, the
nearest-rank `p99` method, and `100 ms` / `500 ms` thresholds for the ready
and blocked queries.

GitHub-hosted functional runs are still useful for exercising the code path,
but they are not authoritative and must never clear `release.block`.

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-074-AC1 | `ready_beads` returns exactly the open beads with no non-closed deps, one round-trip | `ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `sqlite_ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `scn_006_issue_dependency_dag_and_ready_queue` | Exact ready set on both backends | missing — add `@covers US-074-AC1` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-cypher/tests/sqlite_parity.rs`, `crates/axon-api/tests/business_scenarios.rs` |
| US-074-AC2 | `blocked_beads` returns exactly the open beads excluded from ready | `ddx_blocked_query_returns_open_beads_with_at_least_one_non_closed_dep`; `sqlite_…` twin | Exact complement asserted | missing — add `@covers US-074-AC2` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `sqlite_parity.rs` |
| US-074-AC3 | 1K beads (~500 open): ready under 100 ms p99 | none | n/a | planned `@covers US-074-AC3` | UNTESTED | L5 benchmark | planned `criterion` bench |
| US-074-AC4 | 10K beads: ready under 500 ms p99 | none | n/a | planned `@covers US-074-AC4` | UNTESTED | L5 benchmark | planned `criterion` bench |
| US-074-AC5 | Active subscription on `ready_beads` delivers updates on result-set change (QRY-12) | named-query subscription machinery is tested generically in STP-077 (`dynamic.rs` US-077 block); a `ready_beads`-shaped case is absent | n/a | planned `@covers US-074-AC5` | UNTESTED | L6 contract | planned in `crates/axon-graphql/src/dynamic.rs` tests |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
cargo test -p axon-api --test business_scenarios
```

### Planned Test Files

- `criterion` benchmarks at 1K/10K bead scale (AC3/AC4)
- `ready_beads` subscription case (AC5)

### Coverage Focus

- P0: AC1/AC2 exactness (agents schedule work off this answer); AC3/AC4 ratcheted.
- P0: AC3/AC4 are release-blocking only on the dedicated reference host; GitHub-hosted functional runs are non-authoritative.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| 10-bead/15-link DDx dataset | AC1, AC2 | Correctness smoke fixture for the queue partition tests |
| Generated 1K/10K bead graphs | AC3, AC4 | Seeded generator in benchmark harness; release gate uses the dedicated reference host |
| Benchmark metadata bundle | AC3, AC4 | `commit`, `environment`, `artifact_paths`, `metadata`, `p99_ms`, `threshold_ms`, `runner_class`, and `backend_configuration` |

## Edge Cases and Failure Modes

- Bead with a dependency on a *deleted* bead: defined as blocked or ready per spec — assert explicitly.
- Cycle between open beads must not hang the partitioning.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2 (both backends).
2. Benchmarks AC3/AC4; subscription case AC5 after STP-077 citations land.

**Constraints**
- CONTRACT-007 named-query semantics; identical results across backends.

**Done When**
- [ ] AC1/AC2/AC5 passing with citations; AC3/AC4 recorded in the ratchet file and qualified only on the dedicated reference host

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
