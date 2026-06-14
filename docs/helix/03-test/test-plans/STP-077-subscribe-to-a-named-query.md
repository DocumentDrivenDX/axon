---
ddx:
  id: STP-077
  review:
    self_hash: 8f69bfe9f4fc935fd136a27e9eb8ea8f5ed0bef817a6e28e8b681ea5b6e28006
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
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
| US-077-AC1 | New subscription delivers initial snapshot first | US-077 AC1 case in the dynamic.rs block (incl. `named_query_subscription_fields_appear_in_sdl`) | Initial result-set snapshot delivered on subscribe | convert label to `@covers US-077-AC1` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC2 | Result-set-affecting change delivers an update without polling | US-077 AC2 case in the dynamic.rs block | Update delivered on relevant entity/link change | convert label to `@covers US-077-AC2` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC3 | Irrelevant change delivers no spurious update | US-077 AC3 case in the dynamic.rs block | No update for non-affecting commits | convert label to `@covers US-077-AC3` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC4 | Each subscriber's stream policy-filtered for its own identity | US-077 AC4 case in the dynamic.rs block | Hidden rows never appear in that subscriber's stream | convert label to `@covers US-077-AC4` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs` |
| US-077-AC5 | Disconnect tears down cleanly — no leaked watchers or continued evaluation | US-077 AC5 case in the dynamic.rs block | Watcher cleanup on drop asserted | convert label to `@covers US-077-AC5` | UNCITED_COVERAGE | L6 contract | `crates/axon-graphql/src/dynamic.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-graphql
```

### Planned Test Files

- `crates/axon-graphql/src/dynamic.rs` (exists — convert AC comment labels to `@covers US-077-ACm` citations)

### Coverage Focus

- P0: AC4 policy filtering (a subscription is a standing read; it must obey STP-101) and AC5 resource cleanup.

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
1. Citation-only pass: convert the existing AC1–AC5 labels to canonical `@covers` syntax.
2. Mirror a `ready_beads` case into STP-074 AC5.

**Constraints**
- QRY-12 delivery semantics; CONTRACT-002 subscription transport.

**Done When**
- [ ] AC1–AC5 passing with canonical citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
