---
ddx:
  id: STP-046
---

# Story Test Plan: STP-046-field-level-masking

## Story Reference

**User Story**: [[US-046-field-level-masking]] (FEAT-029 — moved from FEAT-012, P0)
**Technical Design**: [[TD-046-field-masking]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (API-surface decision semantics → L6 contract)

## Scope and Objective

**Goal**: prove role/subject-conditional field masking: the same entity returns the sensitive field to authorized subjects and a CONTRACT-004 null redaction to masked subjects, on every read surface including audit after-state.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract`

**In Scope**
- Subject-conditional read-deny field rules; redaction shape stability; audit-read masking.

**Out of Scope**
- Generated-type nullability mechanics ([[STP-102]]), write-side field denial ([[STP-047]], [[STP-103]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-046-AC1 | Low-privilege subject reads entity → sensitive field redacted | `graphql_nexiq_reference_policy_set_applies_visibility_and_redaction` | Contractor-class subject receives null for masked field | missing — add `@covers US-046-AC1` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-046-AC2 | Allowed subject reads same entity → full entity returned | `graphql_nexiq_reference_policy_set_applies_visibility_and_redaction` | Authorized subject sees the sensitive field value | missing — add `@covers US-046-AC2` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-046-AC3 | Redaction shape follows CONTRACT-004 (null, nullable field), never original value | `graphql_nexiq_reference_policy_set_applies_visibility_and_redaction` | Redacted field is explicit null, not omitted/masked-string | missing — add `@covers US-046-AC3` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-046-AC4 | Query results, entity detail, and audit after-state all apply the same redaction | none at API level for the audit-read leg (UI evidence exists for list/detail/audit in `ui/tests/e2e/policy-enforcement.spec.ts`, see [[STP-115]]) | n/a | planned `@covers US-046-AC4` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` (audit after-state redaction) |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: audit after-state redaction for masked subject)

### Coverage Focus

- P0: AC4 audit leg — audit payloads are the highest-risk leak path.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Read-deny rule on a sensitive field scoped to a role (e.g. `salary` for `read` role) | All ACs | Variant in shared policy-fixture suite (nexiq rate-card rules are the existing analogue) |
| Mutated entity with audit history | AC4 | Mutate then query audit as masked subject |

## Edge Cases and Failure Modes

- Masking must apply to historical audit before/after images, not just current reads.
- Subject role change must take effect on next read (no cached unmasked payloads).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1–AC3 against the nexiq redaction suite.
2. Red test for AC4 audit after-state masking.

**Constraints**
- CONTRACT-004 null-redaction; CONTRACT-005 audit payload shape.

**Done When**
- [ ] AC1–AC4 passing with citations, including the audit leg

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
