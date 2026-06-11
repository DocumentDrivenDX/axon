---
ddx:
  id: STP-109
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
- UI authoring workflow ([[STP-114]]), runtime enforcement of the activated policy ([[STP-101]]/[[STP-103]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-109-AC1 | Dry-run schema update returns compile report; active policy version unchanged | `graphql_put_schema_exposes_policy_compile_reports_and_errors` | Compile report returned on dry-run path | missing — add `@covers US-109-AC1`; add explicit active-version-unchanged assertion if absent | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC2 | Invalid field paths / subject refs / relationship cycles rejected at write time with stable invalid-expression reason | `graphql_put_schema_blocks_activation_on_policy_compile_errors` | Activation blocked; persisted policy unchanged | missing — add `@covers US-109-AC2` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC3 | Compile report names GraphQL fields made nullable by redaction | possibly exercised by the compile-report test — verify the assertion names affected fields, then cite; otherwise add it | n/a until verified | planned `@covers US-109-AC3` | UNTESTED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC4 | Fixture subjects + sample mutations evaluated against candidate policy without touching live data | none at API level (UI dry-run exists in [[STP-114]]; backend fixture-evaluation contract test absent) | n/a | planned `@covers US-109-AC4` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-109-AC5 | Fixture evaluations match across GraphQL/MCP/SDK/CLI for same tuple | none (parity matrix covers *active* policy, not candidate-fixture evaluation) | n/a | planned `@covers US-109-AC5` | UNTESTED | L6 parity | planned in `crates/axon-server/tests/feat_029_contract_parent.rs` |
| US-109-AC6 | Activation audited with old and new policy versions | none (UI asserts version increments; audit-entry contract test absent) | n/a | planned `@covers US-109-AC6` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs`; related UI evidence: `ui/tests/e2e/intent-audit-lineage.spec.ts` ("records administrative audit evidence for policy activation") |

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
- [ ] AC1–AC6 passing with citations; activation audit proves v(n) → v(n+1)

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses (uncertain evidence marked UNTESTED with verify-and-cite notes)
- [x] Scope bounded; commands runnable
