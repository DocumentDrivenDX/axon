---
ddx:
  id: STP-077
  review:
    self_hash: 8f69bfe9f4fc935fd136a27e9eb8ea8f5ed0bef817a6e28e8b681ea5b6e28006
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Story Test Plan: STP-077-subscribe-to-a-named-query

## Story Reference

**User Story**: [[US-077-subscribe-to-a-named-query]] (FEAT-009, P0)
**Technical Design**: [[TD-077-named-query-subscriptions]] — not yet authored; CONTRACT-002/CONTRACT-007 (QRY-12) currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (API-surface semantics → L6 contract)

## Scope and Objective

**Goal**: prove named-query subscriptions deliver an initial snapshot then exactly the relevant updates, per-subscriber policy-filtered, with clean teardown.
**Blocking Gate**: `cargo test -p axon-graphql`

**In Scope**
- Subscription lifecycle and delivery semantics for named queries.

**Out of Scope**
- Generic entity subscriptions (FEAT-015), `ready_beads`-specific case (STP-074 AC5).

## Acceptance Criteria Test Mapping

The named-query subscription test block in `crates/axon-graphql/src/dynamic.rs`
(at ~12777) is explicitly labeled "AC1–AC5, US-077" — evidence exists for every
criterion but uses comment labels, not the canonical `@covers` syntax.

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-077-AC1 | New subscription delivers initial snapshot first | `named_query_subscription_fields_appear_in_sdl`; `named_query_subscription_delivers_initial_snapshot` | Initial result-set snapshot delivered on subscribe | `@covers US-077-AC1` present on the subscription field and initial-snapshot tests | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC2 | Result-set-affecting change delivers an update without polling | `named_query_subscription_updates_on_entity_add`; `named_query_subscription_updates_on_status_change`; `named_query_subscription_updates_on_link_add` | Update delivered on relevant entity/link change | `@covers US-077-AC2` present on the subscription update tests | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC3 | Irrelevant change delivers no spurious update | none | No update for non-affecting commits | deferred - current subscription implementation still re-evaluates on the relevant change path; no phase-0 guarantee yet | DEFERRED (phase-0) | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC4 | Each subscriber's stream policy-filtered for its own identity | `named_query_subscription_filters_by_policy` | Hidden rows never appear in that subscriber's stream | `@covers US-077-AC4` present on the policy-filter test | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC5 | Disconnect tears down cleanly — no leaked watchers or continued evaluation | `named_query_subscription_clean_teardown_on_disconnect` | Watcher cleanup on drop asserted | `@covers US-077-AC5` present on the teardown test | COVERED | L6 contract | `crates/axon-graphql/src/dynamic.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-graphql
```

### Planned Test Files

- `crates/axon-graphql/src/dynamic.rs` (subscription block is now cited in place)

### Coverage Focus

- P0: AC1/AC2/AC4/AC5 are covered; AC3 remains phase-0 deferred.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Activated named query + mutation driver | AC1–AC3 | dynamic.rs test harness |
| Two subscriber identities with different visibility | AC4 | Policy fixture subjects |

## Edge Cases and Failure Modes

- Rapid successive changes must coalesce or deliver in order — no out-of-order frames.
- Server-side subscription count must return to baseline after mass disconnect (AC5 at scale).

## Build Handoff

**Implementation Order**
1. Citation-only pass: keep the existing AC1–AC5 `@covers` syntax synchronized.
2. Keep the `ready_beads` case mirrored into STP-074 AC5.

**Constraints**
- QRY-12 delivery semantics; CONTRACT-002 subscription transport.

**Done When**
- [x] AC1/AC2/AC4/AC5 passing with canonical citations; AC3 recorded as a phase-0 deferral

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
