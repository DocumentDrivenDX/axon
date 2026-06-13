---
ddx:
  id: STP-102
---

# Story Test Plan: STP-102-redact-sensitive-fields

## Story Reference

**User Story**: [[US-102-redact-sensitive-fields]] (FEAT-029, P0)
**Technical Design**: [[TD-102-field-redaction]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (API-surface decision semantics → L6 contract; cross-surface parity → L6 parity fixtures)

## Scope and Objective

**Goal**: prove read-deny field rules redact as null on every read surface, and that generated GraphQL types make redactable fields nullable.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract --test feat_029_contract_parent`

**In Scope**
- Field redaction shape, nullability generation, cross-surface redaction parity.

**Out of Scope**
- Row hiding ([[STP-101]]), masking story-level role scenarios ([[STP-046]]), UI DOM-leak checks ([[STP-115]]).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-102-AC1 | Redactable field generated nullable even if JSON Schema requires it | none (planned: introspection assertion on generated type nullability) | n/a | planned `@covers US-102-AC1` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-102-AC2 | Read-deny field rule returns null for matching subject | `graphql_nexiq_reference_policy_set_applies_visibility_and_redaction`, `graphql_policy_read_semantics_are_safe` | Contractor read returns null for redacted commercial fields | `@covers US-102-AC2` in test bodies | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-102-AC3 | Generic JSON, REST compat, and audit reads apply identical redaction | `feat_029_contract_parent_keeps_reference_policy_contracts_in_sync` (procurement/nexiq GraphQL+MCP suites) | Same subject/row redacted identically across surfaces | missing — add `@covers US-102-AC3` | UNCITED_COVERAGE | L6 parity | `crates/axon-server/tests/feat_029_contract_parent.rs` — REST-compat and audit-read legs need explicit assertions |
| US-102-AC4 | JSON-Schema-required field with read-deny still redacted on read | none | n/a | planned `@covers US-102-AC4` | UNTESTED | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
cargo test -p axon-server --test feat_029_contract_parent
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (extend: AC1 nullability introspection, AC4 required-field redaction)

### Coverage Focus

- P0: AC2/AC3 (redaction correctness and parity); AC1/AC4 close the schema-generation gap.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Nexiq reference policy set (contractor redaction) | AC2, AC3 | `seed_nexiq_fixture` / `seed_nexiq` |
| Schema with required field carrying a `redact_as: null` rule | AC1, AC4 | New fixture variant in the shared policy-fixture suite |

## Edge Cases and Failure Modes

- Redacted field must never round-trip its original value through audit after-state payloads.
- Redaction must hold when the field is also selected via relationship traversal.

## Build Handoff

**Implementation Order**
1. Citation pass on AC2/AC3.
2. Add introspection nullability test (AC1), then required-field redaction test (AC4) — both should fail red before any schema-generation change if behavior is absent.

**Constraints**
- CONTRACT-004 null-redaction shape; CONTRACT-002 nullability of generated fields.

**Done When**
- [ ] AC1–AC4 each have a passing, citing test
- [ ] Parity leg explicitly covers REST compatibility and audit reads

## Review Checklist

- [x] Stable AC IDs key every row; asserted behavior named
- [x] UNTESTED rows name the planned test shape
- [x] Scope bounded; commands runnable
