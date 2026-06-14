---
ddx:
  id: STP-023
  review:
    self_hash: 907ead453db6034b1c2f5d48e148905ff56eb7921e4d56c5087a4c92cdee9f41
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-023-traverse-a-dependency-graph

## Story Reference

**User Story**: [[US-023-traverse-a-dependency-graph]] (FEAT-009, P0)
**Technical Design**: [[TD-023-graph-traversal]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow over live store → L2 scenario)

## Scope and Objective

**Goal**: prove bounded variable-length dependency traversal returns all transitive dependencies, supports path projection and per-hop predicates, and terminates safely on cycles.
**Blocking Gate**: `cargo test -p axon-cypher`

**In Scope**
- Depth-bounded traversal correctness over the DDx bead dataset.

**Out of Scope**
- Ready/blocked named queries (STP-074), GraphQL exposure (STP-072), perf ([[test-plan]] L5).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-023-AC1 | Bounded variable-length traversal (1..10) returns all transitive dependencies | `reachability_bead_02_transitive_deps_via_variable_length_path`; `dependency_dag_direct_deps_of_bead_01_ordered_by_id`; `scn_006_issue_dependency_dag_and_ready_queue` | Variable-length path returns the full transitive dep set | missing — add `@covers US-023-AC1` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-api/tests/business_scenarios.rs` |
| US-023-AC2 | Path projection: each row includes root→dependency path | `path_projection_each_row_carries_root_and_dep_bindings` | Every row in variable-length MATCH carries both the root anchor (b.id) and the endpoint (d.id) | `@covers US-023-AC2` | COVERED | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs` |
| US-023-AC3 | Cycle in graph → traversal terminates safely with correct results | `cycle_traversal_terminates_and_returns_correct_nodes`; `self_loop_terminates_with_depth_cap` | 3-node cycle + self-loop both terminate within depth cap; DISTINCT collapses duplicates to correct unique set | `@covers US-023-AC3` | COVERED | L2 + L3 property | `crates/axon-cypher/tests/ddx_integration.rs` |
| US-023-AC4 | Per-hop predicate filters returned dependencies | `exists_true_finds_beads_that_have_at_least_one_non_closed_dep`; `not_exists_finds_beads_with_no_non_closed_deps` | Status predicates applied during traversal | missing — add `@covers US-023-AC4` | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
cargo test -p axon-api --test business_scenarios
```

### Planned Test Files

- `crates/axon-cypher/tests/ddx_integration.rs` (extend: path projection, cyclic fixture)

### Coverage Focus

- P0: AC3 cycle safety — unbounded traversal is a denial-of-service class bug.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| DDx bead dataset (10 beads, 15 links) | AC1, AC2, AC4 | `ddx_schema`/`ddx_store` builders in `ddx_integration.rs` |
| Cyclic dependency fixture | AC3 | New fixture variant |

## Edge Cases and Failure Modes

- Depth cap (default 10 hops per CONTRACT-007 §Limits) must truncate, not error, within bounds.
- Self-referencing link is the minimal cycle case.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC4.
2. Red tests: path projection (AC2), then cycle termination (AC3) including the SQLite parity twin.

**Constraints**
- CONTRACT-007 traversal grammar and limits; results must match across memory and SQLite backends (`sqlite_parity.rs`).

**Done When**
- [ ] AC1–AC4 passing with citations on both backends

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
