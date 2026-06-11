---
ddx:
  id: STP-047
---

# Story Test Plan: STP-047-attribute-based-write-control

## Story Reference

**User Story**: [[US-047-attribute-based-write-control]] (FEAT-029 — moved from FEAT-012, P0)
**Technical Design**: [[TD-047-abac-write-control]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (API-surface decision semantics → L6 contract)

## Scope and Objective

**Goal**: prove per-collection write grants and field-level write-deny rules apply symmetrically per subject: allowed collection writes succeed, others fail with the stable forbidden envelope, protected fields are never silently dropped.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract --test feat_029_contract_parent`

**In Scope**
- Collection-scoped write allow/deny per subject; field write-deny naming the path.

**Out of Scope**
- Transactional abort and idempotent replay ([[STP-103]]), approval envelopes ([[STP-106]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-047-AC1 | Subject with write on collection A updates entity in A → success | `run_procurement_graphql_suite` (via `feat_029_contract_parent_keeps_reference_policy_contracts_in_sync`) | Allowed write commits for the granted subject/collection | missing — add `@covers US-047-AC1`; verify the suite asserts the success leg explicitly | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/feat_029_contract_parent.rs` |
| US-047-AC2 | Same subject updates in read-only collection B → stable forbidden envelope | `graphql_nexiq_reference_policy_set_returns_stable_write_denials` | `forbidden` envelope; stored entity unchanged (`status` stays `active`) | missing — add `@covers US-047-AC2` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-047-AC3 | Complementary policy for second subject applies symmetrically (write B / denied A) | none (fixtures are single-direction; mirror-subject case absent) | n/a | planned `@covers US-047-AC3` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-047-AC4 | Write including a write-deny protected field fails naming the field path (never silently dropped) | `graphql_nexiq_reference_policy_set_returns_stable_write_denials` | `field_write_denied` with `field_path` naming the protected field | missing — add `@covers US-047-AC4` | UNCITED_COVERAGE | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
cargo test -p axon-server --test feat_029_contract_parent
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: mirror-subject symmetry case for AC3)

### Coverage Focus

- P0: AC4 — silent field-drop is the dangerous failure mode; the test must assert the write *fails*, not that the field is stripped.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Two subjects with complementary collection grants | AC1–AC3 | Extend procurement/nexiq fixture subjects |
| Write-deny rule on a protected field (e.g. `approved_by`) | AC4 | Existing nexiq field rules |

## Edge Cases and Failure Modes

- Patch vs full update must behave identically for protected fields.
- Grant evaluated against the *URL-scoped* tenant/database, never a body field (ADR-018).

## Build Handoff

**Implementation Order**
1. Citation pass on AC1/AC2/AC4 (verify AC1's success-leg assertion while citing).
2. Red test for AC3 mirror subject.

**Constraints**
- CONTRACT-004 forbidden envelope; symmetric evaluation must come from the same policy engine path on all surfaces.

**Done When**
- [ ] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
