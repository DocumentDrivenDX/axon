---
ddx:
  id: STP-109
  review:
    self_hash: ae76f71bf135900f633da6076ee6729250bd140787bc8c68871676054d85d797
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Story Test Plan: STP-109-author-and-test-policy-before-activation

## Story Reference

**User Story**: [[US-109-author-and-test-policy-before-activation]] (FEAT-029, P0)
**Technical Design**: [[TD-109-policy-compile-pipeline]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (compile diagnostics → unit/L6; parity → L6 parity fixtures)

## Scope and Objective

**Goal**: prove candidate policies can be compiled, fixture-evaluated, and activated safely: dry-runs never touch the active version, invalid policies are rejected at write time, and activation is audited.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract`

**In Scope**
- Dry-run compile reports, write-time rejection, nullability reporting, fixture evaluation, parity of fixture results, audited activation.

**Out of Scope**
- UI authoring workflow (STP-114), runtime enforcement of the activated policy (STP-101/STP-103).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-109-AC1 | Dry-run schema update returns compile report; active policy version unchanged | `graphql_put_schema_exposes_policy_compile_reports_and_errors` | Compile report returned on dry-run path; active schema version asserted unchanged at v1 | `@covers US-109-AC1` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC2 | Invalid field paths / subject refs / relationship cycles rejected at write time with stable invalid-expression reason | `graphql_put_schema_blocks_activation_on_policy_compile_errors` | Activation blocked; persisted policy unchanged | `@covers US-109-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC3 | Compile report names GraphQL fields made nullable by redaction | `graphql_put_schema_exposes_policy_compile_reports_and_errors` (asserts `nullable_fields[0].field == "secret"` and `required_by_schema == true`) | Compile report names affected nullable fields with schema-required flag | `@covers US-109-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC4 | Fixture subjects + sample mutations evaluated against candidate policy without touching live data | `graphql_put_schema_dry_run_evaluates_candidate_policy_without_live_mutation` | explainInputs evaluated against proposed v2 policy; active schema remains v1; entity version unchanged; no mutation audit entries | `@covers US-109-AC4` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC5 | Fixture evaluations match across GraphQL/MCP/SDK/CLI for same tuple | `feat_029_dry_run_explain_matches_activated_policy_graphql_only` (GraphQL only; MCP/SDK/CLI legs absent — `putSchema(explainInputs)` is GraphQL-only; MCP does not expose `putSchema`; SDK/CLI do not thread `explainInputs`) | Dry-run explanation decision and policy version match the activated-policy `explainPolicy` result for the same (actor, operation, entity) tuple | `@covers US-109-AC5` in test body | PARTIALLY COVERED (GraphQL leg only; MCP/SDK/CLI absent) | L6 parity | `crates/axon-server/tests/feat_029_contract_parent.rs` |
| US-109-AC6 | Activation audited with old and new policy versions | `graphql_put_schema_exposes_policy_compile_reports_and_errors` (asserts `old_policy_version: "none"`, `new_policy_version: "2"` in `schema.update` audit entry) | Audit entry records old and new policy versions on activation | `@covers US-109-AC6` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: AC3 nullability naming, AC4 fixture evaluation, AC6 activation audit entry v(n)→v(n+1))
- `crates/axon-server/tests/feat_029_contract_parent.rs` (extend: AC5 candidate-fixture parity)

### Coverage Focus

- P0: AC2 (write-time rejection) and AC6 (audited activation) protect production policy integrity.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Candidate policy v(n+1) with redaction additions | AC1, AC3 | Variant of `seed_policy_fixture` schema |
| Deliberately invalid policy (bad field path, subject ref, cycle) | AC2 | Inline fixtures per failure class |
| Named fixture subjects + sample mutations | AC4, AC5 | Shared policy-fixture suite |

## Edge Cases and Failure Modes

- Concurrent activation attempts must serialize (no torn policy version).
- A dry-run must not leave staged state observable to other requests.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2 (plus active-version-unchanged assertion).
2. Verify-or-add AC3 nullability naming.
3. Red tests AC4 → AC5 → AC6.

**Constraints**
- CONTRACT-004 compile-report and invalid-expression vocabulary; CONTRACT-005 audit record shape for activation entries.

**Done When**
- [x] AC1–AC6 passing with citations; activation audit proves v(n) → v(n+1)
- Note: AC5 is partially covered (GraphQL leg only); MCP/SDK/CLI legs are absent because `putSchema(explainInputs)` is not exposed via those interfaces.

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses (uncertain evidence marked UNTESTED with verify-and-cite notes)
- [x] Scope bounded; commands runnable
