---
ddx:
  id: STP-074
  review:
    self_hash: 8c2611ec8e15ed0e0e17fd9ce7dea2c323be157df6901743011eac74b3c26f50
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
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

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-074-AC1 | `ready_beads` returns exactly the open beads with no non-closed deps, one round-trip | `ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `sqlite_ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `scn_006_issue_dependency_dag_and_ready_queue` | Exact ready set on both backends | `@covers US-074-AC1` present on the DDx integration, SQLite parity, and business scenario tests | COVERED | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-cypher/tests/sqlite_parity.rs`, `crates/axon-api/tests/business_scenarios.rs` |
| US-074-AC2 | `blocked_beads` returns exactly the open beads excluded from ready | `ddx_blocked_query_returns_open_beads_with_at_least_one_non_closed_dep`; `sqlite_…` twin | Exact complement asserted | `@covers US-074-AC2` present on the DDx integration and SQLite parity tests | COVERED | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-cypher/tests/sqlite_parity.rs` |
| US-074-AC3 | 1K beads (~500 open): ready under 100 ms p99 | `ddx_ready_blocked_queue_benchmark` | Ready/blocked partition on a 1K seeded fixture | `@covers US-074-AC3` present on `crates/axon-cypher/benches/ddx_benchmark.rs` | COVERED | L5 benchmark | `crates/axon-cypher/benches/ddx_benchmark.rs` |
| US-074-AC4 | 10K beads: ready under 500 ms p99 | `ddx_ready_blocked_queue_benchmark` | Ready/blocked partition on a 10K seeded fixture | `@covers US-074-AC4` present on `crates/axon-cypher/benches/ddx_benchmark.rs` | COVERED | L5 benchmark | `crates/axon-cypher/benches/ddx_benchmark.rs` |
| US-074-AC5 | Active subscription on `ready_beads` delivers updates on result-set change (QRY-12) | `named_query_subscription_updates_on_entity_add` | Ready-beads subscription emits on result-set change | `@covers US-074-AC5` present on the ready-beads subscription update test | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
cargo test -p axon-api --test business_scenarios
```

### Benchmark Files

- `crates/axon-cypher/benches/ddx_benchmark.rs` `ddx_ready_blocked_queue_benchmark` at 1K/10K bead scale

### Coverage Focus

- P0: AC1/AC2 exactness, AC3/AC4 latency, and AC5 subscription semantics are covered.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| 10-bead/15-link DDx dataset | AC1, AC2 | `ddx_integration.rs` builders |
| Generated 1K/10K bead graphs | AC3, AC4 | Seeded generator in benchmark harness |

## Edge Cases and Failure Modes

- Bead with a dependency on a *deleted* bead: defined as blocked or ready per spec — assert explicitly.
- Cycle between open beads must not hang the partitioning.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2/AC5 (both backends and subscription).
2. Benchmarks AC3/AC4. — done via `ddx_ready_blocked_queue_benchmark`

**Constraints**
- CONTRACT-007 named-query semantics; identical results across backends.

**Done When**
- [x] AC1/AC2/AC5 passing with citations; AC3/AC4 backed by benchmark evidence

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
