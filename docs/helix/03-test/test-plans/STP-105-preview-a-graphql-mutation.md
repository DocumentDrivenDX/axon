---
ddx:
  id: STP-105
---

# Story Test Plan: STP-105-preview-a-graphql-mutation

## Story Reference

**User Story**: [[US-105-preview-a-graphql-mutation]] (FEAT-030, P0)
**Technical Design**: [[TD-105-mutation-preview]] — not yet authored; ADR-023 and CONTRACT-002/CONTRACT-005 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (intent decision semantics → L6 contract)

## Scope and Objective

**Goal**: prove mutation preview returns diff + decision + bindings, never mutates state, and behaves identically to commit-time evaluation and across surfaces.
**Blocking Gate**: `cargo test -p axon-server --test graphql_intents_contract --test graphql_policy_contract`

**In Scope**
- Preview response content, denial previews, no-side-effect guarantee, intent record bindings, surface-stable decision vocabulary.

**Out of Scope**
- Approval routing ([[STP-106]]), staleness on commit ([[STP-107]]), UI preview modal ([[STP-116]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-105-AC1 | Preview returns entity ID, pre-image version, field diff, policy decision | `graphql_preview_mutation_records_policy_diff_and_never_writes_entity_state` | Preview payload carries diff + decision + pre-image version | missing — add `@covers US-105-AC1` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-105-AC2 | Denied mutation preview → `deny` with matching rule, no executable token | `denied_preview_has_no_executable_token` | Deny decision and absent commit token | missing — add `@covers US-105-AC2` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-105-AC3 | Preview creates no entity/link mutation audit entry and changes no state | `graphql_preview_mutation_records_policy_diff_and_never_writes_entity_state` | Pre/post entity state and mutation-audit counts unchanged by preview | missing — add `@covers US-105-AC3` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-105-AC4 | Given identical state, preview applies the same validation/transition/policy rules as commit | none (planned: preview-vs-commit determinism case comparing decisions and validation outcomes on fixed state) | n/a | planned `@covers US-105-AC4` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_intents_contract.rs` |
| US-105-AC5 | Executable token's intent record stores schema/policy versions, operation hash, all pre-image versions | `graphql_preview_mutation_binds_versions_for_all_operation_shapes`; lineage asserts in `graphql_intents_contract.rs` (`schema_version`, `policy_version`, `operation_hash`) | Intent record carries all four binding dimensions | missing — add `@covers US-105-AC5` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs`, `graphql_intents_contract.rs` |
| US-105-AC6 | Preview decision fields stable and machine-readable across SDK/CLI/MCP/UI | `axon_query_intent_mutations_match_graphql_review_flow` | MCP intent flow returns the same decision vocabulary as GraphQL | missing — add `@covers US-105-AC6`; SDK/CLI legs absent | UNCITED_COVERAGE | L6 parity | `crates/axon-server/tests/mcp_intents_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_intents_contract
cargo test -p axon-server --test graphql_policy_contract
cargo test -p axon-server --test mcp_intents_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_intents_contract.rs` (extend: AC4 preview/commit determinism)

### Coverage Focus

- P0: AC3 (no side effects) and AC5 (binding completeness — feeds [[STP-107]] staleness).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Invoice/task fixture with approval-threshold policy | All ACs | `seed_intent_fixture` / `seed_policy_fixture` |
| Mutation-audit snapshot helper | AC3 | `audit_by_intent` / `audit_entity` helpers in the suite |

## Edge Cases and Failure Modes

- Preview of a multi-op transaction must bind every pre-image (`graphql_preview_mutation_binds_versions_for_all_operation_shapes`).
- Preview against an entity the subject cannot read must follow [[STP-101]] hidden semantics, not leak via diff.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2/AC3/AC5/AC6.
2. Red test for AC4 determinism.

**Constraints**
- ADR-023 preview-audit semantics; CONTRACT-002/009 decision vocabulary.

**Done When**
- [ ] AC1–AC6 passing with citations; SDK/CLI parity gap explicitly tracked

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
