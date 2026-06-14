---
ddx:
  id: STP-024
  review:
    self_hash: afd15353ec32b1430078e90e3b62ccfc91eca3fec985c5880a2fc8c105a0f641
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# Story Test Plan: STP-024-explode-a-bill-of-materials

## Story Reference

**User Story**: [[US-024-explode-a-bill-of-materials]] (FEAT-009, P0)
**Technical Design**: [[TD-024-bom-traversal]] — not yet authored; CONTRACT-007 currently serves as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow over live store → L2 scenario)

## Scope and Objective

**Goal**: prove BOM explosion returns the full component tree with relationship metadata, handles diamond-shaped sharing, and skips dangling links without error.
**Blocking Gate**: `cargo test -p axon-api --test business_scenarios scn_004`

**In Scope**
- Multi-level `contains` traversal with quantity metadata.

**Out of Scope**
- Generic dependency traversal (STP-023), GraphQL connections (STP-072).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-024-AC1 | Bounded traversal returns full BOM tree with relationship metadata per hop | `scn_004_bom_explosion_recursive_traversal` | Widget-A explosion returns Sub-Assembly-B, Component-C, Component-D with per-hop metadata | missing — add `@covers US-024-AC1` | UNCITED_COVERAGE | L2 scenario | `crates/axon-api/tests/business_scenarios.rs` |
| US-024-AC2 | Relationship properties (`quantity`) accessible in result rows | `scn_004_bom_explosion_recursive_traversal` | Total Component-C = 4 direct + 2×1 via B = 6 | missing — add `@covers US-024-AC2` | UNCITED_COVERAGE | L2 scenario | `crates/axon-api/tests/business_scenarios.rs` |
| US-024-AC3 | Diamond-shared component appears once with all paths recoverable | `scn_004_bom_explosion_recursive_traversal` | Component-C reached via both direct and via-B paths | missing — add `@covers US-024-AC3`; verify the once-with-paths projection is asserted, not just both-path reachability | UNCITED_COVERAGE | L2 scenario | `crates/axon-api/tests/business_scenarios.rs` |
| US-024-AC4 | Dangling link targets skipped without error | `scn_024_ac4_dangling_link_targets_skipped_without_error` | Force-delete a component; traverse from root succeeds without error; deleted entity absent, remaining entities present | `@covers US-024-AC4` | COVERED | L2 scenario | `crates/axon-api/tests/business_scenarios.rs` |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-api --test business_scenarios
```

### Planned Test Files

- `crates/axon-api/tests/business_scenarios.rs` (extend scn_004 with a dangling-target leg)

### Coverage Focus

- P0: AC2 quantity math (drives procurement decisions) and AC4 dangling-link resilience.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Widget-A / Sub-Assembly-B / Component-C/D tree with quantities | AC1–AC3 | scn_004 setup |
| Force-deleted component leaving a dangling `contains` link | AC4 | New setup step |

## Edge Cases and Failure Modes

- Zero-quantity links must round-trip, not be dropped.
- Depth cap reached mid-tree must return the bounded subtree.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1–AC3 (verify AC3's projection assertion while citing).
2. Red test for AC4 dangling-link skip.

**Constraints**
- CONTRACT-007 collection projection semantics for path recovery.

**Done When**
- [ ] AC1–AC4 passing with citations

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
