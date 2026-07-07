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
**Blocking Gate**: `cargo test -p axon-graphql && cargo test -p axon-schema`

**In Scope**
- GraphQL exposure of named graph queries.

**Out of Scope**
- Named-query declaration/compilation (STP-075), subscriptions (STP-077).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-072-AC1 | Activated named query queryable as typed connection (edges, pageInfo, totalCount) | `named_query_ready_beads_executes_with_connection_policy_and_redaction` | Named-query field executes end-to-end (not just SDL presence): `totalCount`, `pageInfo`, and `edges { node }` are populated from a live multi-hop `NOT EXISTS` traversal, with per-row policy redaction and owner scoping applied in the same pass | `@covers US-072-AC1` | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-072-AC2 | Multi-hop named query runs as one planned execution — no N+1 | `named_query_ready_beads_connection_is_one_planned_execution` | A `CountingStorageAdapter` records identical storage-call counts whether the connection query selects only `edges { node { id } }` or additionally selects `totalCount`, `pageInfo`, and every scalar on every row — proving the whole page is computed once per request, not once per requested field or row | `@covers US-072-AC2` | COVERED | Unit (storage-call instrumentation) | `crates/axon-graphql/src/dynamic.rs` |
| US-072-AC3 | Nested traversal beyond depth limit (default 10) rejected with documented error | `variable_length_path_beyond_depth_cap_reports_unsupported_query_plan` | A named query declaring a variable-length pattern (`*1..11`) beyond CONTRACT-007's depth cap (10) fails schema compile with `NamedQueryStatus::UnsupportedQueryPlan` and a message naming "depth cap 10" — the same planner enforcement path every named query compiles through, so an over-depth named-query connection can never be activated | `@covers US-072-AC3` | COVERED | Unit (schema compile) | `crates/axon-schema/src/named_queries.rs` |
| US-072-AC4 | Forward and backward pagination per CONTRACT-002 on named-query connections | `named_query_ready_beads_connection_paginates_forward` | CONTRACT-002 defines cursor-forward pagination only (`first`/`after`; no `last`/`before` on any connection); a 3-page traversal of a 5-row `ready_beads` connection holds `totalCount` stable across pages and flips `pageInfo.hasPreviousPage`/`startCursor` correctly once paging has advanced past page 1 | `@covers US-072-AC4` | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-graphql
cargo test -p axon-schema
```

### Test Files

- `crates/axon-graphql/src/dynamic.rs` (AC1, AC2, AC4 covered)
- `crates/axon-schema/src/named_queries.rs` (AC3 covered)

### Coverage Focus

- P0: AC2 (N+1 silently destroys the performance contract) and AC3 (unbounded execution) — both covered.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Activated multi-hop named query | All ACs | `ddx_beads_named_query_schema()`'s `ready_beads` fixture in `crates/axon-graphql/src/dynamic.rs` |
| Storage-call counter | AC2 | `CountingStorageAdapter` test-only `StorageAdapter` wrapper |

## Edge Cases and Failure Modes

- CONTRACT-002 pagination is forward-only (`first`/`after`); `pageInfo.hasPreviousPage`/`startCursor` — not a `before` argument — are how a client detects it is past the first page.
- Depth-limit error must be the documented CONTRACT-007 message ("depth cap 10"), not a timeout or generic parse failure.

## Build Handoff

**Implementation Order**
1. Citation pass + execution assertion for AC1. — done
2. Red tests AC3 → AC4 → AC2 (instrumentation last). — done

**Constraints**
- CONTRACT-002 connection semantics (forward-only pagination); CONTRACT-007 10-hop default depth cap.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
