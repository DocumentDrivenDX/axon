---
ddx:
  id: STP-025
  review:
    self_hash: 87f09bfc0d7d7aaab25b0fb0da7ad6fed897696724cd896a70b14e6e772ce43f
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-025-check-reachability

## Story Reference

**User Story**: [[US-025-check-reachability]] (FEAT-009, P0)
**Technical Design**: [[TD-025-reachability]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow → L2 scenario; policy interaction → L6 contract)

## Scope and Objective

**Goal**: prove existence checks answer transitive reachability without materializing paths, short-circuit, support alternating link types, and never leak policy-hidden records.
**Blocking Gate**: `cargo test -p axon-cypher`

**In Scope**
- Boolean reachability semantics over typed links.

**Out of Scope**
- Full traversal result shapes (STP-023), ad-hoc query budgets (STP-076).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-025-AC1 | Transitive chain A→B existence check returns true without materializing the path | `reachability_bead_02_transitive_deps_via_variable_length_path`; `exists_true_finds_beads_that_have_at_least_one_non_closed_dep` | Existence semantics return true over transitive chains | missing — add `@covers US-025-AC1`; the no-materialization claim itself is unasserted | UNCITED_COVERAGE | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs` |
| US-025-AC2 | Multiple paths → evaluation short-circuits on first found | `us_025_ac2_reachable_short_circuits_on_first_path_found`; `us_025_ac2_reachable_returns_false_when_unreachable` | Diamond graph: reachable() returns true at depth=2 (shortest path) without exhausting all paths; negative case returns false with depth=None | `@covers US-025-AC2` | COVERED | L2 scenario | `crates/axon-api/tests/business_scenarios.rs` |
| US-025-AC3 | Alternating two link types: paths through either type satisfy the pattern | `alternating_link_types_both_satisfy_variable_length_pattern`; `type_specific_traversal_does_not_cross_link_type_boundary` | `[:CONNECTS\|LINKS*1..2]` reaches nodes via both link types; CONNECTS-only stops at the type boundary | `@covers US-025-AC3` | COVERED | L2 scenario | `crates/axon-cypher/tests/ddx_integration.rs` |
| US-025-AC4 | Path through policy-hidden record: answer must not reveal the hidden record's existence (QRY-16) | none | n/a — blocked on query-layer policy integration; cypher executor has no policy context; asserting non-disclosure requires CONTRACT-004/QRY-16 semantics wired into the query path | MANUAL_EXCEPTION: will be covered in `crates/axon-server/tests/graphql_policy_contract.rs` once query-layer policy integration lands (tracked in STP-101) | MANUAL_EXCEPTION | L6 contract | planned in `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-cypher
```

### Planned Test Files

- `crates/axon-cypher/tests/ddx_integration.rs` (extend: alternating link types)
- planner unit tests for short-circuit (AC2)
- policy-hidden reachability contract test (AC4)

### Coverage Focus

- P0: AC4 — reachability is an existence oracle; it must obey STP-101 hidden-row semantics.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| DDx bead dataset | AC1 | `ddx_integration.rs` builders |
| Graph with two alternating link types | AC3 | New fixture |
| Policy hiding an intermediate node | AC4 | Shared policy-fixture suite |

## Edge Cases and Failure Modes

- A→A (self) reachability semantics must be defined and tested.
- Hidden-intermediate answer (AC4) must be decided per CONTRACT-004/QRY-16 and asserted exactly — both "true" and "false" can leak depending on the rule.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1.
2. Red tests AC3 → AC2 → AC4 (AC4 blocked on query-layer policy integration; record a manual exception if the story must close before that lands).

**Constraints**
- CONTRACT-007 existence-check grammar; QRY-16 policy interaction.

**Done When**
- [ ] AC1–AC4 passing with citations (or AC4 exception recorded)

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
