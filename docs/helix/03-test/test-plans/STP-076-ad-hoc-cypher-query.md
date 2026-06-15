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
| US-076-AC1 | Valid ad-hoc query returns rows with column type metadata and plan/index/policy metadata | ad-hoc execution tests across `ddx_integration.rs` (e.g. `count_star_counts_all_open_beads`, `order_by_priority_asc_returns_open_beads_in_ascending_order`) | Rows and orderings correct — column/plan metadata legs need verification while citing | missing — add `@covers US-076-AC1` | UNCITED_COVERAGE | L2/unit | `crates/axon-cypher/tests/ddx_integration.rs` |
| US-076-AC2 | Unknown label/property/relationship rejected at parse with documented stable code | error-path tests in the US-076 block of `crates/axon-cypher/src/error.rs` | Unknown-reference rejection with stable code | missing — add `@covers US-076-AC2` | UNCITED_COVERAGE | Unit | `crates/axon-cypher/src/error.rs` |
| US-076-AC3 | Ad-hoc vs equivalent named query: identical policy enforcement (rows, redaction, counts) | none (cypher × policy integration absent; see STP-025 AC4) | n/a | planned `@covers US-076-AC3` | UNTESTED | L3 property + L6 | planned property test generating query pairs |
| US-076-AC4 | Planned cardinality over ad-hoc budget → rejected before execution with documented code | none | n/a | planned `@covers US-076-AC4` | UNTESTED | Unit (planner) | planned in `crates/axon-cypher/` |
| US-076-AC5 | Every ad-hoc failure class carries its stable CONTRACT-007 error code | partial — error.rs covers unknown-reference; unsupported clause/plan, policy bypass, budget, timeout classes unasserted | n/a as a complete matrix | planned `@covers US-076-AC5` | UNTESTED | Unit | planned table-driven error-code matrix in `crates/axon-cypher/src/error.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
```

### Planned Test Files

- `crates/axon-cypher/src/error.rs` table-driven failure-class matrix (AC5)
- planner budget tests (AC4); policy-parity property test (AC3)

### Coverage Focus

- P0: AC5 stable error vocabulary (agents branch on these codes) and AC3 policy parity.

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
2. AC5 matrix → AC4 budget → AC3 parity property.

**Constraints**
- CONTRACT-007 §Stable error codes is the authoritative vocabulary; read-only (no Cypher writes per PRD non-goal).

**Done When**
- [ ] AC1–AC5 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
