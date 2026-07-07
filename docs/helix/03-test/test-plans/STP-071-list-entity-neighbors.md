---
ddx:
  id: STP-071
  review:
    self_hash: 9f475bb611092c1f4c4e69fd7039faba28cbdc69536568d24a900865a294bfaf
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
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
| US-071-AC1 | Undirected single-hop match returns all neighbors with relationship types | `list_neighbors_returns_outbound_and_inbound` (US-071 block, `setup_neighbor_graph`); `neighbors` leg in `graphql_consumer_parity.rs` | Inbound + outbound neighbors returned with types | `@covers US-071-AC1` present on both the handler test and the parity leg | COVERED | L2/unit | `crates/axon-api/src/handler.rs` (`#[cfg(test)]` block, `list_neighbors_returns_outbound_and_inbound`), `crates/axon-server/tests/graphql_consumer_parity.rs` (`@covers US-071-AC1 (neighbors leg)`, ~line 293) |
| US-071-AC2 | Direction constraint returns only that direction | `list_neighbors_filter_by_direction` | Requesting `Forward` direction returns only outbound groups (`total_count == 2`, all groups `direction == "outbound"`) | `@covers US-071-AC2` | COVERED | Unit | `crates/axon-api/src/handler.rs` |
| US-071-AC3 | Link-type filter returns only neighbors via that type | `list_neighbors_filter_by_link_type` | Filtering by `"assigned-to"` returns only that link type's group (`groups.len() == 1`, `link_type == "assigned-to"`) | `@covers US-071-AC3` | COVERED | Unit | `crates/axon-api/src/handler.rs` |
| US-071-AC4 | <100 links: results under 20 ms p99 | none | n/a | deferred — L5 criterion benchmark out of scope for axon-36f3a756 (broad benchmark ratchets excluded); not required by the not-yet-issued readiness verdict (axon-5744d96b, still open) | DEFERRED (ratchet) | L5 benchmark | planned `criterion` bench |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-api
```

### Test Files

- `crates/axon-api/src/handler.rs` US-071 block (AC1–AC3 covered)
- `criterion` neighbor benchmark (AC4, deferred, not yet written)

### Coverage Focus

- P0: AC1–AC3 correctness — covered; AC4 ratcheted, not commit-blocking.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| `setup_neighbor_graph` fixture (mixed directions/types) | AC1–AC3 | Existing handler test helper |

## Edge Cases and Failure Modes

- Entity with zero links returns an empty list, not an error.
- Duplicate links between the same pair must not duplicate neighbors unless semantics say so.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1; verify-and-cite or extend AC2/AC3; benchmark last. — done; AC4 benchmark deferred (ratchet, out of scope for axon-36f3a756)

**Constraints**
- CONTRACT-007 single-hop match semantics.

**Done When**
- [x] AC1–AC3 passing with citations; AC4 benchmark deferred (ratchet) — see AC4 row for rationale

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
