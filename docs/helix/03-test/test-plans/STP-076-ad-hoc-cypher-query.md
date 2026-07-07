---
ddx:
  id: STP-076
  review:
    self_hash: aafb96e6f6c1722a76d64752eaf01339faf41593aa2cd9fbce9a5b9741c01fbe
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Story Test Plan: STP-076-ad-hoc-cypher-query

## Story Reference

**User Story**: [[US-076-ad-hoc-cypher-query]] (FEAT-009, P0)
**Technical Design**: [[TD-076-adhoc-query-execution]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (parsing/validation → unit; policy parity → L3 property + L6)

## Scope and Objective

**Goal**: prove ad-hoc Cypher queries return typed rows with plan/policy metadata, reject unknown schema references and over-budget plans with stable error codes, and enforce policy identically to named queries.
**Blocking Gate**: `cargo test -p axon-cypher`

**In Scope**
- Ad-hoc execution, validation, budget, and error vocabulary.

**Out of Scope**
- Named-query lifecycle (STP-075), MCP exposure (STP-073).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-076-AC1 | Valid ad-hoc query returns rows with column type metadata and plan/index/policy metadata | `count_star_counts_all_open_beads`; `order_by_priority_asc_returns_open_beads_in_ascending_order`; `axon_query_valid_query_returns_rows_schema_and_metadata` | Rows and orderings correct, with the documented metadata legs asserted in the query tests | `@covers US-076-AC1` present on the DDx integration tests and the GraphQL metadata test | COVERED | L2/unit | `crates/axon-cypher/tests/ddx_integration.rs`, `crates/axon-graphql/src/dynamic.rs` |
| US-076-AC2 | Unknown label/property/relationship rejected at parse with documented stable code | `unknown_label_rejected`; `unknown_property_in_inline_predicate_rejected`; `unknown_relationship_rejected` | Unknown-reference rejection with stable code | `@covers US-076-AC2` present on the validator tests | COVERED | Unit | `crates/axon-cypher/src/validator.rs` |
| US-076-AC3 | Ad-hoc vs equivalent named query: identical policy enforcement (rows, redaction, counts) | none (cypher × policy integration absent; see STP-025 AC4) | n/a | deferred - parity property test is outside the current readiness verdict | DEFERRED (phase-0) | L3 property + L6 | planned property test generating query pairs |
| US-076-AC4 | Planned cardinality over ad-hoc budget → rejected before execution with documented code | `axon_query_query_too_large_error_code` | Query rejected before execution with the documented `query_too_large` code | `@covers US-076-AC4` present on the GraphQL ad-hoc budget test | COVERED | Unit (planner) | `crates/axon-graphql/src/dynamic.rs` |
| US-076-AC5 | Every ad-hoc failure class carries its stable CONTRACT-007 error code | `rejects_create_clause`; `rejects_merge_clause`; `unknown_label_rejected`; `axon_query_errors_use_stable_cypher_codes`; `axon_query_unsupported_query_plan_error_code`; `axon_query_query_too_large_error_code`; `timeout_is_checked_while_rows_are_pulled`; `storage_scan_error_propagates_as_cypher_error_storage`; `storage_traverse_error_propagates_through_expand` | Stable error vocabulary is asserted across parser, validator, GraphQL, and executor failure classes | `@covers US-076-AC5` present across the parser, validator, GraphQL, and executor tests | COVERED | Unit | `crates/axon-cypher/src/parser.rs`, `crates/axon-cypher/src/validator.rs`, `crates/axon-graphql/src/dynamic.rs`, `crates/axon-cypher/src/executor.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
```

### Planned Test Files

- planner budget tests (AC4)
- policy-parity property test (AC3, deferred)

### Coverage Focus

- P0: AC1/AC2/AC4/AC5 are covered; AC3 policy parity is deferred.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| DDx dataset (memory + SQLite) | AC1 | `ddx_integration.rs` / `sqlite_parity.rs` |
| One fixture per CONTRACT-007 failure class | AC5 | Table-driven cases |
| Configured ad-hoc budget threshold | AC4 | Test config knob |

## Edge Cases and Failure Modes

- Timeout class must return its stable code, not a transport error (30 s wall clock per FEAT-009).
- Parameterized queries with mismatched parameter types are a parse-or-plan failure, decided and asserted.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2 (verify metadata legs of AC1 while citing).
2. AC5 matrix and AC4 budget are covered; AC3 parity remains deferred.

**Constraints**
- CONTRACT-007 §Stable error codes is the authoritative vocabulary; read-only (no Cypher writes per PRD non-goal).

**Done When**
- [x] AC1–AC5 passing with citations, with AC3 recorded as a phase-0 deferral

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
