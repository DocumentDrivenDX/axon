---
ddx:
  id: STP-075
---

# Story Test Plan: STP-075-schema-declared-named-query

## Story Reference

**User Story**: [[US-075-schema-declared-named-query]] (FEAT-009, P0)
**Technical Design**: [[TD-075-named-query-compilation]] — not yet authored; CONTRACT-007/CONTRACT-010 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (compile diagnostics → unit; surface activation → L6 contract)

## Scope and Objective

**Goal**: prove named-query declarations are validated at schema-save time (grammar, type-check, index threshold, policy compatibility), activate onto GraphQL and MCP, and report through dry-run.
**Blocking Gate**: `cargo test -p axon-schema && cargo test -p axon-graphql`

**In Scope**
- Declaration validation and activation lifecycle.

**Out of Scope**
- Execution semantics of activated queries ([[STP-072]], [[STP-074]]), ad-hoc queries ([[STP-076]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-075-AC1 | Declaration accepted per CONTRACT-007 grammar on schema save | named-query schema tests (US-075 block in `crates/axon-schema/src/schema.rs:187` area) | Valid declaration round-trips through schema save | missing — add `@covers US-075-AC1` | UNCITED_COVERAGE | Unit | `crates/axon-schema/src/schema.rs` |
| US-075-AC2 | Unknown label/property/relationship → save fails with type-check diagnostic identifying the reference | none verified (cypher schema validation exists in `crates/axon-cypher/src/schema.rs` — verify a save-time diagnostic test and cite, else add) | n/a until verified | planned `@covers US-075-AC2` | UNTESTED | Unit | `crates/axon-cypher/src/schema.rs`, `crates/axon-schema/` |
| US-075-AC3 | Unindexed scan above threshold → save fails suggesting an index (QRY-06) | none | n/a | planned `@covers US-075-AC3` | UNTESTED | Unit | planned in `crates/axon-cypher/` planner diagnostics |
| US-075-AC4 | Policy-bypass-requiring query → save fails with documented policy-compatibility error (QRY-07) | none | n/a | planned `@covers US-075-AC4` | UNTESTED | Unit + L6 | planned alongside policy compile pipeline ([[STP-109]]) |
| US-075-AC5 | Activation exposes typed GraphQL field and MCP tool | `named_query_subscription_fields_appear_in_sdl` (GraphQL leg); MCP named-query tools in `mcp_contract.rs` ([[STP-073]] AC1) | Activated query visible on both surfaces | missing — add `@covers US-075-AC5` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs`, `crates/axon-server/tests/mcp_contract.rs` |
| US-075-AC6 | Schema dry-run returns compile report incl. named-query diagnostics; nothing activated | none (schema dry-run exists — `grpc_put_schema_dry_run` — but named-query diagnostics in the report are unasserted) | n/a | planned `@covers US-075-AC6` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/api_contract.rs` / `graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-schema
cargo test -p axon-graphql
cargo test -p axon-server --test mcp_contract
```

### Planned Test Files

- `crates/axon-cypher/` save-time diagnostic tests (AC2–AC4)
- dry-run report extension test (AC6)

### Coverage Focus

- P0: AC2–AC4 — bad declarations must die at save time, never at agent runtime.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Valid named-query declaration fixture | AC1, AC5 | Existing schema fixtures |
| Deliberately invalid declarations (unknown label, unindexed scan, policy bypass) | AC2–AC4 | One fixture per failure class |

## Edge Cases and Failure Modes

- Re-saving a schema with an unchanged named query must be idempotent (no duplicate tools/fields).
- Deactivating a named query must remove the GraphQL field and MCP tool atomically.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC5.
2. Red tests AC2 → AC3 → AC4 → AC6.

**Constraints**
- CONTRACT-007 grammar + QRY-06/07 diagnostics; CONTRACT-002/003 activation surfaces.

**Done When**
- [ ] AC1–AC6 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
