---
ddx:
  id: STP-074
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
- Generic named-query declaration ([[STP-075]]), subscription mechanics ([[STP-077]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-074-AC1 | `ready_beads` returns exactly the open beads with no non-closed deps, one round-trip | `ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `sqlite_ddx_ready_query_returns_open_beads_whose_deps_are_all_closed`; `scn_006_issue_dependency_dag_and_ready_queue` | Exact ready set on both backends | missing — add `@covers US-074-AC1` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-cypher/tests/sqlite_parity.rs`, `crates/axon-api/tests/business_scenarios.rs` |
| US-074-AC2 | `blocked_beads` returns exactly the open beads excluded from ready | `ddx_blocked_query_returns_open_beads_with_at_least_one_non_closed_dep`; `sqlite_…` twin | Exact complement asserted | missing — add `@covers US-074-AC2` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `sqlite_parity.rs` |
| US-074-AC3 | 1K beads (~500 open): ready under 100 ms p99 | none | n/a | planned `@covers US-074-AC3` | UNTESTED | L5 benchmark | planned `criterion` bench |
| US-074-AC4 | 10K beads: ready under 500 ms p99 | none | n/a | planned `@covers US-074-AC4` | UNTESTED | L5 benchmark | planned `criterion` bench |
| US-074-AC5 | Active subscription on `ready_beads` delivers updates on result-set change (QRY-12) | named-query subscription machinery is tested generically in [[STP-077]] (`dynamic.rs` US-077 block); a `ready_beads`-shaped case is absent | n/a | planned `@covers US-074-AC5` | UNTESTED | L6 contract | planned in `crates/axon-graphql/src/dynamic.rs` tests |

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
1. Citation pass on AC1/AC2 (both backends).
2. Benchmarks AC3/AC4; subscription case AC5 after [[STP-077]] citations land.

**Constraints**
- CONTRACT-007 named-query semantics; identical results across backends.

**Done When**
- [ ] AC1/AC2/AC5 passing with citations; AC3/AC4 recorded in the ratchet file

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
