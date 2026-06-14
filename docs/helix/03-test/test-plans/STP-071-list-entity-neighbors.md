---
ddx:
  id: STP-071
  review:
    self_hash: 9f475bb611092c1f4c4e69fd7039faba28cbdc69536568d24a900865a294bfaf
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-071-list-entity-neighbors

## Story Reference

**User Story**: [[US-071-list-entity-neighbors]] (FEAT-009, P0)
**Technical Design**: [[TD-071-neighbor-query]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow → L2; latency → L5 benchmarks)

## Scope and Objective

**Goal**: prove single-hop neighbor queries return all neighbors with relationship types, honoring direction and link-type filters.
**Blocking Gate**: `cargo test -p axon-api`

**In Scope**
- Undirected/directed single-hop matching with type filters.

**Out of Scope**
- Multi-hop traversal (STP-023), candidates (STP-070).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-071-AC1 | Undirected single-hop match returns all neighbors with relationship types | list-neighbors tests (US-071 block, `setup_neighbor_graph`); `neighbors` leg in `graphql_consumer_parity.rs` | Inbound + outbound neighbors returned with types | missing — add `@covers US-071-AC1` | UNCITED_COVERAGE | L2/unit | `crates/axon-api/src/handler.rs` (block at ~21197), `crates/axon-server/tests/graphql_consumer_parity.rs` |
| US-071-AC2 | Direction constraint returns only that direction | verify the US-071 block's direction cases — cite if present, add if not | n/a until verified | planned `@covers US-071-AC2` | UNTESTED | Unit | `crates/axon-api/src/handler.rs` |
| US-071-AC3 | Link-type filter returns only neighbors via that type | verify the US-071 block's type-filter cases — cite if present, add if not | n/a until verified | planned `@covers US-071-AC3` | UNTESTED | Unit | `crates/axon-api/src/handler.rs` |
| US-071-AC4 | <100 links: results under 20 ms p99 | none | n/a | planned `@covers US-071-AC4` | UNTESTED | L5 benchmark | planned `criterion` bench |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-api
```

### Planned Test Files

- `crates/axon-api/src/handler.rs` US-071 block (verify/extend AC2, AC3)
- `criterion` neighbor benchmark (AC4)

### Coverage Focus

- P0: AC1–AC3 correctness; AC4 ratcheted.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| `setup_neighbor_graph` fixture (mixed directions/types) | AC1–AC3 | Existing handler test helper |

## Edge Cases and Failure Modes

- Entity with zero links returns an empty list, not an error.
- Duplicate links between the same pair must not duplicate neighbors unless semantics say so.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1; verify-and-cite or extend AC2/AC3; benchmark last.

**Constraints**
- CONTRACT-007 single-hop match semantics.

**Done When**
- [ ] AC1–AC3 passing with citations; AC4 benchmark in the ratchet file

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
