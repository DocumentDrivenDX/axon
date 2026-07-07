---
ddx:
  id: STP-073
  review:
    self_hash: 427f05a898510e9e3cec69633eb48c4584bd9ffb21767644cbecf627086e8933
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Story Test Plan: STP-073-discover-links-via-mcp

## Story Reference

**User Story**: [[US-073-discover-links-via-mcp]] (FEAT-009, P0)
**Technical Design**: [[TD-073-mcp-graph-tools]] — not yet authored; CONTRACT-003/CONTRACT-007 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (agent-surface semantics → L6 contract + parity)

## Scope and Objective

**Goal**: prove activated named queries surface as MCP tools with declared parameters and descriptions, the ad-hoc query tool shares the GraphQL parser/planner/policy path, and MCP results match GraphQL exactly.
**Blocking Gate**: `cargo test -p axon-server --test mcp_contract` (AC4 parity) plus `cargo test -p axon-mcp` (AC1–AC3 tool-generation and shared-path unit coverage; `axon-mcp` is a library dependency of `axon-server` and its unit tests run under plain `cargo test`)

**In Scope**
- MCP tool generation from named queries; MCP↔GraphQL query parity.

**Out of Scope**
- GraphQL exposure (STP-072), ad-hoc grammar/budget semantics (STP-076).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-073-AC1 | Activated named query → MCP tool with parameters from query declarations | `named_query_tools_surface_descriptions_and_execute_graphql_path` | Calls `build_named_query_tools` against a schema with a `ready_tasks` named query; asserts the generated `tasks.ready_tasks` tool's `input_schema` marks `status` required and typed `string`, and exposes `first`/`after` connection params | `@covers US-073-AC1` | COVERED | Unit (`cargo test -p axon-mcp`) | `crates/axon-mcp/src/handlers.rs` |
| US-073-AC2 | Tool description includes the named query's description | `named_query_tools_surface_descriptions_and_execute_graphql_path` | Asserts the generated tool's `description` contains the schema's named-query description string ("Ready tasks ordered by points") | `@covers US-073-AC2` | COVERED | Unit (`cargo test -p axon-mcp`) | `crates/axon-mcp/src/handlers.rs` |
| US-073-AC3 | Generic ad-hoc query tool follows same parser/planner/policy path as GraphQL ad-hoc (QRY-10) | `query_tool_executes_live_handler_queries`; `query_tool_rejects_invalid_cypher_syntax`; `query_tool_rejects_unknown_schema_references` | The `axon.query` tool handler calls `axon_graphql::dynamic::execute_axon_query_json`, which delegates directly into the same `execute_axon_query` function backing GraphQL's `axonQuery` resolver (verified in `crates/axon-graphql/src/dynamic.rs`); tests assert the response is keyed `data.axonQuery` and that invalid syntax / unknown schema references are rejected through the shared parser/validator | `@covers US-073-AC3` | COVERED | Unit (`cargo test -p axon-mcp`) | `crates/axon-mcp/src/handlers.rs` |
| US-073-AC4 | Same subject/query/data via MCP and GraphQL: identical decisions, redactions, limits, results | `mcp_axon_query_matches_graphql_policy_semantics`; `mcp_nexiq_reference_policy_queries_match_graphql` | MCP results match GraphQL byte-for-byte for the same subject/query, including pagination, redaction, and cross-collection policy fixtures | `@covers US-073-AC4` | COVERED | L6 parity | `crates/axon-server/tests/mcp_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-mcp
cargo test -p axon-server --test mcp_contract
```

### Test Files

- `crates/axon-mcp/src/handlers.rs` (unit tests: named-query tool generation/description passthrough, ad-hoc `axon.query` shared-path assertions)
- `crates/axon-server/tests/mcp_contract.rs` (L6 parity: MCP vs GraphQL results over shared policy fixtures)

### Coverage Focus

- P0: AC4 parity (PRD policy-parity metric) and AC3 single-path execution (no second query engine for agents).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Activated named query with description + parameters | AC1, AC2 | `make_graph_handler` fixture's `ready_tasks` named query in `crates/axon-mcp/src/handlers.rs` |
| Shared policy fixture subjects | AC4 | `seed_query_policy_fixture` |

## Edge Cases and Failure Modes

- Tool list must refresh after schema change (asserted for policy metadata; extend to named-query tools).
- Ad-hoc tool failure codes must match CONTRACT-007 stable codes (STP-076).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC4 (verify parameter-schema assertion while citing AC1).
2. Red tests AC2 → AC3.

**Constraints**
- CONTRACT-003 tool generation; QRY-10 single parser/planner/policy path.

**Done When**
- [x] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
