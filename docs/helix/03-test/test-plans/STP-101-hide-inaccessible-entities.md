---
ddx:
  id: STP-101
  review:
    self_hash: e7599d73084cf6e694a3c52162604a292817627481eab37c37f7305ec3758ee5
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-101-hide-inaccessible-entities

## Story Reference

**User Story**: [[US-101-hide-inaccessible-entities]] (FEAT-029, P0)
**Technical Design**: [[TD-101-access-control-read-path]] — not yet authored; ADR-019 and CONTRACT-004 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (AC class: API-surface decision semantics → L6 contract)

## Scope and Objective

**Goal**: prove hidden entities are indistinguishable from missing entities on every read path — point read, lists/connections, pagination/counts, and relationship traversal.
**Blocking Gate**: `cargo test -p axon-server --test graphql_policy_contract`

**In Scope**
- Read-denial semantics (not-found/null, never forbidden) across GraphQL read shapes.

**Out of Scope**
- Field redaction (STP-102), write denial (STP-103), UI rendering of hidden rows (STP-115).

## Acceptance Criteria Test Mapping

No test currently cites an AC ID; statuses are honest as of 2026-06-10.

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-101-AC1 | Hidden entity point read ≡ missing entity, never forbidden | `graphql_policy_read_semantics_are_safe` | Point read of policy-hidden row returns the missing-entity shape with no policy error | `@covers US-101-AC1` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-101-AC2 | Lists/connections omit hidden rows without error | `graphql_policy_read_semantics_are_safe` | List returns only visible rows, `errors` is null | `@covers US-101-AC2` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-101-AC3 | Pagination windows and totals computed after policy filtering | `graphql_policy_read_semantics_are_safe` | `totalCount == 2` (visible only), `pageInfo` paged over filtered set | `@covers US-101-AC3` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |
| US-101-AC4 | Hidden traversal/relationship targets not materialized | `graphql_policy_read_semantics_are_safe` | Relationship resolution omits hidden link target | `@covers US-101-AC4` in test body | COVERED | L6 contract | `crates/axon-server/tests/graphql_policy_contract.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-server --test graphql_policy_contract
```

### Planned Test Files

- `crates/axon-server/tests/graphql_policy_contract.rs` (exists — needs `@covers` citations)

### Coverage Focus

- P0: AC1–AC4 (all read shapes deny by omission, never by error).

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Policy fixture with hidden + visible rows | All ACs | `seed_policy_fixture` / `seed_nexiq_fixture` in `graphql_policy_contract.rs` |
| Link from visible to hidden entity | AC4 | Seeded via `/tenants/default/databases/default/links` in the same suite |

## Edge Cases and Failure Modes

- Empty result set after filtering must not leak hidden-row counts.
- Live insertion of a hidden row must not surface via subscriptions (UI angle covered in STP-115).

## Build Handoff

**Implementation Order**
1. Citation-only pass: add `@covers US-101-AC1..AC4` to the assertions in `graphql_policy_read_semantics_are_safe`.
2. Extend the shared policy-fixture suite if any read shape (e.g. backward pagination) is unasserted.

**Constraints**
- CONTRACT-004 read-denial semantics: not-found/null, never 403, for hidden rows.

**Done When**
- [ ] Every AC row's test cites its AC ID and passes
- [ ] `cargo test -p axon-server --test graphql_policy_contract` green

## Review Checklist

- [x] References the governing story; TD absence is recorded honestly
- [x] Every active AC keyed by stable `US-101-AC<m>` ID
- [x] Every row names the asserted behavior, not just a test name
- [x] Citation status recorded per row (all UNCITED_COVERAGE — fix is citation, not new tests)
- [x] Commands are runnable; scope bounded to one story
