---
ddx:
  id: STP-075
  review:
    self_hash: 905a57318336f4ea12c32041f638a825425de05613535e7370f8292b7df356d9
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
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
- Execution semantics of activated queries (STP-072, STP-074), ad-hoc queries (STP-076).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-075-AC1 | Declaration accepted per CONTRACT-007 grammar on schema save | `esf_parses_named_queries_block` | Valid declaration round-trips through schema save | `@covers US-075-AC1` present on the schema parser test | COVERED | Unit | `crates/axon-schema/src/schema.rs` |
| US-075-AC2 | Unknown label/property/relationship → save fails with type-check diagnostic identifying the reference | `unknown_label_reports_unknown_identifier`; `unknown_property_reports_unknown_identifier`; `unknown_relationship_reports_unknown_identifier` | Save-time compile report surfaces the offending reference class | `@covers US-075-AC2` present on the named-query compiler diagnostics tests | COVERED | Unit | `crates/axon-schema/src/named_queries.rs` |
| US-075-AC3 | Unindexed scan above threshold → save fails suggesting an index (QRY-06) | `unindexed_plan_on_large_collection_reports_unsupported_query_plan` | Compile report suggests the missing index via the documented planner status | `@covers US-075-AC3` present on the named-query compiler diagnostics test | COVERED | Unit | `crates/axon-schema/src/named_queries.rs` |
| US-075-AC4 | Policy-bypass-requiring query → save fails with documented policy-compatibility error (QRY-07) | `policy_bypass_reports_policy_required_bypass` | Compile report records the policy bypass diagnostic | `@covers US-075-AC4` present on the named-query compiler diagnostics test | COVERED | Unit + L6 | `crates/axon-schema/src/named_queries.rs` |
| US-075-AC5 | Activation exposes typed GraphQL field and MCP tool | `named_query_subscription_fields_appear_in_sdl`; `named_query_tools_surface_descriptions_and_execute_graphql_path` | Activated query visible on both surfaces | `@covers US-075-AC5` present on the GraphQL field test and the MCP named-query tool test | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs`, `crates/axon-mcp/src/handlers.rs` |
| US-075-AC6 | Schema dry-run returns compile report incl. named-query diagnostics; nothing activated | `handle_put_schema_dry_run_reports_named_query_errors_without_activation` | Dry-run reports named-query diagnostics and leaves the schema inactive | `@covers US-075-AC6` present on the handler dry-run test | COVERED | L6 contract | `crates/axon-api/src/handler.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-schema
cargo test -p axon-graphql
cargo test -p axon-server --test mcp_contract
```

### Planned Test Files

- `crates/axon-schema/src/named_queries.rs` save-time diagnostic tests (AC2–AC4)
- `crates/axon-api/src/handler.rs` dry-run report coverage (AC6)

### Coverage Focus

- P0: AC1–AC6 are covered; the save-time diagnostics remain the primary readiness signal.

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
2. Diagnostics pass on AC2 → AC3 → AC4 → AC6.

**Constraints**
- CONTRACT-007 grammar + QRY-06/07 diagnostics; CONTRACT-002/003 activation surfaces.

**Done When**
- [x] AC1–AC6 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
