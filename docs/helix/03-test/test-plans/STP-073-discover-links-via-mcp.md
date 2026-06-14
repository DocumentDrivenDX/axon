---
ddx:
  id: STP-073
  review:
    self_hash: 427f05a898510e9e3cec69633eb48c4584bd9ffb21767644cbecf627086e8933
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-073-discover-links-via-mcp

## Story Reference

**User Story**: [[US-073-discover-links-via-mcp]] (FEAT-009, P0)
**Technical Design**: [[TD-073-mcp-graph-tools]] — not yet authored; CONTRACT-003/CONTRACT-007 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (agent-surface semantics → L6 contract + parity)

## Scope and Objective

**Goal**: prove activated named queries surface as MCP tools with declared parameters and descriptions, the ad-hoc query tool shares the GraphQL parser/planner/policy path, and MCP results match GraphQL exactly.
**Blocking Gate**: `cargo test -p axon-server --test mcp_contract`

**In Scope**
- MCP tool generation from named queries; MCP↔GraphQL query parity.

**Out of Scope**
- GraphQL exposure (STP-072), ad-hoc grammar/budget semantics (STP-076).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-073-AC1 | Activated named query → MCP tool with parameters from query declarations | `mcp_tools_list_includes_crud_after_collection_created` + suite header (item.link_candidates / item.neighbors tools) | Named-query-derived tools listed with parameters | missing — add `@covers US-073-AC1`; verify the parameter-schema assertion | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/mcp_contract.rs` |
| US-073-AC2 | Tool description includes the named query's description | none (descriptions are asserted for policy envelopes, not named-query description passthrough) | n/a | planned `@covers US-073-AC2` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/mcp_contract.rs` |
| US-073-AC3 | Generic ad-hoc query tool follows same parser/planner/policy path as GraphQL ad-hoc (QRY-10) | none (the existing `axon.query` bridge tests cover GraphQL bridging; the Cypher ad-hoc tool path is not asserted) | n/a | planned `@covers US-073-AC3` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/mcp_contract.rs` |
| US-073-AC4 | Same subject/query/data via MCP and GraphQL: identical decisions, redactions, limits, results | `mcp_axon_query_matches_graphql_policy_semantics`; `mcp_nexiq_reference_policy_queries_match_graphql` | MCP results match GraphQL for the same subject/query | missing — add `@covers US-073-AC4` | UNCITED_COVERAGE | L6 parity | `crates/axon-server/tests/mcp_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test mcp_contract
```

### Planned Test Files

- `crates/axon-server/tests/mcp_contract.rs` (extend: description passthrough, ad-hoc tool path identity)

### Coverage Focus

- P0: AC4 parity (PRD policy-parity metric) and AC3 single-path execution (no second query engine for agents).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Activated named query with description + parameters | AC1, AC2 | Schema fixture from STP-075 |
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
- [ ] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
