---
ddx:
  id: STP-104
  review:
    self_hash: 760b38a6415c09c60b911a6f7dc5955787f288d01a976bbaf4cde279534b7396
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-104-explain-effective-policy

## Story Reference

**User Story**: [[US-104-explain-effective-policy]] (FEAT-029, P0)
**Technical Design**: [[TD-104-policy-introspection]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (decision semantics → L6; surface parity → L6 parity fixtures)

## Scope and Objective

**Goal**: prove effective-policy metadata and dry-run explanations are returned, are identical across surfaces, and are advisory only (enforcement re-evaluates at execution).
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract --test mcp_contract`

**In Scope**
- `effectivePolicy` / `explainPolicy` content, cross-surface metadata parity, advisory-only semantics.

**Out of Scope**
- Policy authoring/dry-run of *candidate* policies (STP-109), UI explain panels (STP-113).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-104-AC1 | Effective collection policy returns allowed ops, redacted/denied fields, policy version | `graphql_effective_policy_reports_subject_capabilities` | Capabilities + `redactedFields` + version returned per subject/collection | `@covers US-104-AC1` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-104-AC2 | Dry-run explanation returns decision, reason, matching policy, field paths; no execution | `graphql_explain_policy_reports_rules_denials_and_approval_envelopes` | Explain reports rules/denials/approval envelopes without mutating | `@covers US-104-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-104-AC3 | Same tuple via MCP/SDK/CLI/operator preserves GraphQL decision metadata | `graphql_mcp_policy_parity_matrix_matches_expected_decisions`; `mcp_tools_list_exposes_policy_metadata_matching_graphql_effective_policy` | MCP policy metadata equals GraphQL effective policy for the same subject | `@covers US-104-AC3` in test bodies | COVERED | L6 parity | `crates/axon-server/tests/mcp_contract.rs` — SDK/CLI legs still absent |
| US-104-AC4 | Execution re-evaluates enforcement regardless of stale advisory answer | `graphql_execution_reevaluates_policy_regardless_of_stale_explain` | `explainPolicy` returns `allow` for finance-agent; `putSchema` narrows policy to deny that subject; subsequent `updateTask` returns `forbidden` with no mutation to the entity | `@covers US-104-AC4` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
cargo test -p axon-server --test mcp_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: AC4 advisory-only case)
- SDK/CLI parity legs for AC3 once those surfaces expose policy metadata

### Coverage Focus

- P0: AC3 parity (PRD policy-parity metric) and AC4 advisory-only (security-relevant).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Procurement + nexiq fixture subjects | AC1–AC3 | `seed_policy_fixture`, `seed_nexiq_fixture` |
| Policy-version bump helper | AC4 | Schema update path used by `mcp_tools_list_refreshes_policy_metadata_after_schema_update` |

## Edge Cases and Failure Modes

- Explanation for a subject with zero grants must not leak collection existence beyond CONTRACT-004 rules.
- Policy-version change between explain and execute (AC4) is the canonical TOCTOU case.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1–AC3.
2. Red test for AC4 (allow-explain → narrow policy → execute → deny).

**Constraints**
- CONTRACT-004 explanation vocabulary; parity is decision-for-decision, not text-for-text.

**Done When**
- [x] AC1–AC4 passing with citations; SDK/CLI parity gap explicitly tracked

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
