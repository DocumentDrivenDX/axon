---
ddx:
  id: STP-072
  review:
    self_hash: 9fefe0a8bac78aff398f91f6df15432607cb7d2c5d0d08364c4a17e9da9a7994
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Story Test Plan: STP-072-explore-graph-via-graphql

## Story Reference

**User Story**: [[US-072-explore-graph-via-graphql]] (FEAT-009, P0)
**Technical Design**: [[TD-072-graphql-graph-exposure]] — not yet authored; CONTRACT-002/CONTRACT-007 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (API-surface semantics → L6 contract)

## Scope and Objective

**Goal**: prove activated named queries surface as typed GraphQL connections with single-plan execution, depth-limit rejection, and CONTRACT-002 pagination.
**Blocking Gate**: `cargo test -p axon-graphql && cargo test -p axon-server --test graphql_contract`

**In Scope**
- GraphQL exposure of named graph queries.

**Out of Scope**
- Named-query declaration/compilation (STP-075), subscriptions (STP-077).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-072-AC1 | Activated named query queryable as typed connection (edges, pageInfo, totalCount) | named-query SDL/dynamic tests (US-072 block in `graph.rs`; `named_query_subscription_fields_appear_in_sdl` proves SDL presence) | Named-query field exists with connection typing | missing — add `@covers US-072-AC1`; add an execution (not just SDL) assertion if absent | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/graph.rs`, `crates/axon-graphql/src/dynamic.rs` |
| US-072-AC2 | Multi-hop named query runs as one planned execution — no N+1 | none (needs plan/query-count instrumentation assertion) | n/a | planned `@covers US-072-AC2` | UNTESTED | L6 + unit (planner) | planned in `crates/axon-graphql/` with storage-call counter |
| US-072-AC3 | Nested traversal beyond depth limit (default 10) rejected with documented error | none | n/a | planned `@covers US-072-AC3` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_contract.rs` |
| US-072-AC4 | Forward and backward pagination per CONTRACT-002 on named-query connections | none (generic connection pagination is covered for entity lists in `graphql_consumer_parity.rs`, not for named-query connections) | n/a | planned `@covers US-072-AC4` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-graphql
cargo test -p axon-server --test graphql_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_contract.rs` (extend: depth-limit rejection, named-query pagination)
- `crates/axon-graphql/` instrumented N+1 guard (AC2)

### Coverage Focus

- P0: AC2 (N+1 silently destroys the performance contract) and AC3 (unbounded execution).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Activated multi-hop named query | All ACs | Schema fixture from STP-075 |
| Storage-call counter / plan metadata hook | AC2 | Test-only instrumentation |

## Edge Cases and Failure Modes

- Backward pagination from the last page must mirror forward semantics.
- Depth-limit error must be the documented CONTRACT-002 error, not a timeout.

## Build Handoff

**Implementation Order**
1. Citation pass + execution assertion for AC1.
2. Red tests AC3 → AC4 → AC2 (instrumentation last).

**Constraints**
- CONTRACT-002 connection semantics; CONTRACT-007 10-hop default depth cap.

**Done When**
- [ ] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
