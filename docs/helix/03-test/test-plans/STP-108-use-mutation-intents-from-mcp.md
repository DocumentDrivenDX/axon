---
ddx:
  id: STP-108
  review:
    self_hash: 9fa6304fb065a3a7bf70cb349bad7f1433910c576495a8b815f1620c0cf3910b
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# Story Test Plan: STP-108-use-mutation-intents-from-mcp

## Story Reference

**User Story**: [[US-108-use-mutation-intents-from-mcp]] (FEAT-030, P0)
**Technical Design**: [[TD-108-mcp-intents]] — not yet authored; ADR-023 and CONTRACT-003 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (agent-surface decision semantics → L6 contract + parity fixtures)

## Scope and Objective

**Goal**: prove MCP tools expose policy envelopes and follow the same intent semantics and decision vocabulary as GraphQL for preview/approve/commit, denial, staleness, and conflict.
**Blocking Gate**: `cargo test -p axon-server --test mcp_intents_contract --test mcp_contract`

**In Scope**
- MCP tool envelope summaries, structured `needs_approval`/denied outputs, MCP↔GraphQL intent parity.

**Out of Scope**
- GraphQL-only intent flows (STP-105–STP-107), UI inspection of MCP intents (STP-119).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-108-AC1 | Generated MCP tool descriptions include policy envelope summaries | `mcp_tool_descriptions_summarize_autonomous_and_approval_envelopes` | Tool descriptions carry autonomous/approval envelope summaries | `@covers US-108-AC1` in test body | COVERED | L6 contract | `crates/axon-server/tests/mcp_contract.rs` |
| US-108-AC2 | Tool call routed for approval → structured `needs_approval` with intent token + approval summary | `generated_mcp_tools_preview_commit_and_block_approval_bypass` | Approval-routed tool output is structured, bypass blocked | `@covers US-108-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/mcp_intents_contract.rs` |
| US-108-AC3 | Denied tool call → structured policy explanation | `graphql_mcp_policy_parity_matrix_matches_expected_decisions`; nexiq MCP suites via `feat_029_contract_parent` | Denied MCP outcomes are structured and match expected decisions | `@covers US-108-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/mcp_contract.rs`, `feat_029_contract_parent.rs` |
| US-108-AC4 | MCP query/mutation path: preview/approve/commit follows GraphQL intent semantics | `axon_query_intent_mutations_match_graphql_review_flow` | Full intent workflow via MCP matches the GraphQL review flow | `@covers US-108-AC4` in test body | COVERED | L6 parity | `crates/axon-server/tests/mcp_intents_contract.rs` |
| US-108-AC5 | needs_approval/denied/stale/conflict outcomes preserve machine-readable fields identically vs GraphQL | `axon_query_intent_commit_conflict_matches_graphql_error_extensions`; `generated_mcp_tools_report_stale_commit_conflict` | Conflict/stale payload fields match GraphQL error extensions | `@covers US-108-AC5` in test bodies | COVERED | L6 parity | `crates/axon-server/tests/mcp_intents_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test mcp_intents_contract
cargo test -p axon-server --test mcp_contract
```

### Planned Test Files

- All evidence files exist; this story needs a citation-only pass.

### Coverage Focus

- P0: AC4/AC5 parity — agents must never see a weaker envelope than GraphQL clients (PRD policy-parity metric).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Seeded intent collection + approval policy | AC2, AC4, AC5 | `seed_intent_collection` in `mcp_intents_contract.rs` |
| Query-policy fixture for tool metadata | AC1, AC3 | `seed_query_policy_fixture` in `mcp_contract.rs` |

## Edge Cases and Failure Modes

- Tool metadata must refresh after schema/policy change (`mcp_tools_list_refreshes_policy_metadata_after_schema_update`).
- Stdio transport parity with HTTP MCP remains a FEAT-016 gap — out of scope here but noted.

## Build Handoff

**Implementation Order**
1. Citation-only pass across AC1–AC5 (no new tests needed unless citing reveals an unasserted leg).

**Constraints**
- CONTRACT-003 tool envelope shape; decision vocabulary must be byte-stable across MCP and GraphQL.

**Done When**
- [ ] AC1–AC5 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
