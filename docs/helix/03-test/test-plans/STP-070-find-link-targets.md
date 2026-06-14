---
ddx:
  id: STP-070
  review:
    self_hash: 36dccf0f3f037f60d339ceedbac42a06f5123310ecb1d887b082a254681ecc48
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# Story Test Plan: STP-070-find-link-targets

## Story Reference

**User Story**: [[US-070-find-link-targets]] (FEAT-009, P0)
**Technical Design**: [[TD-070-link-candidates]] — not yet authored; CONTRACT-007/CONTRACT-010 currently serve as the design surface
**Related Solution Design**: N/A
**Project Test Plan**: [[test-plan]] §3 (business workflow → L2; latency → L5 benchmarks)

## Scope and Objective

**Goal**: prove the link-candidates named query returns target-collection entities with already-linked indicators, index-backed search/filter, and schema-sourced cardinality metadata.
**Blocking Gate**: `cargo test -p axon-api`

**In Scope**
- Link-candidate query semantics and metadata sourcing.

**Out of Scope**
- Neighbor listing (STP-071), GraphQL/MCP exposure (STP-072, STP-073).

## Acceptance Criteria Test Mapping

| AC ID | Criterion (condensed) | Test(s) | Asserted Behavior | Citation | Status | Level | File or Command |
|-------|----------------------|---------|-------------------|----------|--------|-------|-----------------|
| US-070-AC1 | Invoking with source ID + link type returns target-collection entities | `find_link_candidates_returns_target_entities` (US-070 test block); GraphQL `linkCandidates` leg in `graphql_consumer_parity.rs` | Candidates drawn from the declared target collection | missing — add `@covers US-070-AC1` | UNCITED_COVERAGE | L2/unit | `crates/axon-api/src/handler.rs` (`#[cfg(test)]` block at ~21396), `crates/axon-server/tests/graphql_consumer_parity.rs` |
| US-070-AC2 | Search text + filters combine in one index-backed match (no client-side filtering) | handler US-070 block may exercise search/filter — verify and cite; the index-backed (plan-level) claim is unasserted | n/a until verified | planned `@covers US-070-AC2` | UNTESTED | Unit + planner | `crates/axon-api/src/handler.rs`; planner assertion planned |
| US-070-AC3 | Rows carry already-linked indicator via optional matching | verify handler block for an already-linked assertion; otherwise add | n/a until verified | planned `@covers US-070-AC3` | UNTESTED | Unit | `crates/axon-api/src/handler.rs` |
| US-070-AC4 | Cardinality available as schema metadata (CONTRACT-010), not computed per query | none | n/a | planned `@covers US-070-AC4` | UNTESTED | Unit | planned in `crates/axon-schema/` tests |
| US-070-AC5 | 10K-entity target collection, indexed predicate: <50 ms p99 | none | n/a | planned `@covers US-070-AC5` | UNTESTED | L5 benchmark | planned `criterion` bench alongside BM-006 |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-api
cargo test -p axon-server --test graphql_consumer_parity
```

### Planned Test Files

- `crates/axon-api/src/handler.rs` test block (verify/extend AC2, AC3)
- `criterion` benchmark for AC5

### Coverage Focus

- P0: AC1/AC3 (correct candidates with linked indicators); AC5 is ratcheted, not commit-blocking.

## Data and Setup

| Need | Required For | Source / Strategy |
|------|--------------|-------------------|
| Source entity with declared link type + target collection | AC1–AC3 | Handler test fixtures |
| 10K-entity seeded collection with index | AC5 | Benchmark seeding harness |

## Edge Cases and Failure Modes

- Candidate set when *all* targets are already linked (empty-but-flagged vs omitted).
- Unknown link type must return the documented error, not an empty list.

## Build Handoff

**Implementation Order**
1. Citation pass on AC1; verify-and-cite or extend AC2/AC3.
2. Schema-metadata test (AC4); benchmark (AC5) last.

**Constraints**
- CONTRACT-010 cardinality metadata location; no client-side filtering.

**Done When**
- [ ] AC1–AC4 passing with citations; AC5 benchmark recorded in the ratchet file

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
