---
ddx:
  id: STP-070
  review:
    self_hash: 36dccf0f3f037f60d339ceedbac42a06f5123310ecb1d887b082a254681ecc48
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
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
| US-070-AC1 | Invoking with source ID + link type returns target-collection entities | `find_link_candidates_returns_target_entities` (US-070 test block); GraphQL `linkCandidates` leg in `graphql_consumer_parity.rs` | Candidates drawn from the declared target collection | `@covers US-070-AC1` present on both the handler test and the parity leg | COVERED | L2/unit | `crates/axon-api/src/handler.rs` (`#[cfg(test)]` block, `find_link_candidates_returns_target_entities`), `crates/axon-server/tests/graphql_consumer_parity.rs` (`@covers US-070-AC1 (linkCandidates leg)`, ~line 292) |
| US-070-AC2 | Search text + filters combine in one index-backed match (no client-side filtering) | `find_link_candidates_filter_is_index_backed` | `FindLinkCandidatesRequest` has only a `filter: Option<FilterNode>` field (no separate search-text field — confirmed against `request.rs`), so AC2 collapses to: the filter is resolved via the FEAT-013 `try_index_lookup` planner (asserted `Some(..)`) when the target collection declares a matching index, contrasted against the same filter on an unindexed collection (asserted `None`, i.e. full-scan fallback); end-to-end `find_link_candidates` response matches the index-selected rows | `@covers US-070-AC2` | COVERED | Unit | `crates/axon-api/src/handler.rs` |
| US-070-AC3 | Rows carry already-linked indicator via optional matching | `find_link_candidates_marks_already_linked` | Already-linked target is flagged `already_linked: true`; unlinked target is `false` | `@covers US-070-AC3` | COVERED | Unit | `crates/axon-api/src/handler.rs` |
| US-070-AC4 | Cardinality available as schema metadata (CONTRACT-010), not computed per query | `find_link_candidates_cardinality_sourced_from_schema` | With an explicit `link_types["depends-on"]` schema entry (`Cardinality::ManyToMany`), the response's `cardinality` field reflects that declared value (`"many-to-many"`) rather than the "unknown" default seen when no `link_types` entry exists | `@covers US-070-AC4` | COVERED | Unit | `crates/axon-api/src/handler.rs` |
| US-070-AC5 | 10K-entity target collection, indexed predicate: <50 ms p99 | none | n/a | deferred — L5 criterion benchmark out of scope for axon-36f3a756 (broad benchmark ratchets excluded); not required by the not-yet-issued readiness verdict (axon-5744d96b, still open) | DEFERRED (ratchet) | L5 benchmark | planned `criterion` bench alongside BM-006 |

## Executable Proof

### Primary Commands

```bash
cargo test -p axon-api
cargo test -p axon-server --test graphql_consumer_parity
```

### Test Files

- `crates/axon-api/src/handler.rs` test block (AC1–AC4 covered)
- `criterion` benchmark for AC5 (deferred, not yet written)

### Coverage Focus

- P0: AC1–AC4 (correct candidates with linked indicators, index-backed filtering, schema-sourced cardinality) are covered; AC5 is ratcheted, not commit-blocking.

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
1. Citation pass on AC1; verify-and-cite or extend AC2/AC3. — done
2. Schema-metadata test (AC4); benchmark (AC5) last. — AC4 done; AC5 deferred (ratchet, out of scope for axon-36f3a756)

**Constraints**
- CONTRACT-010 cardinality metadata location; no client-side filtering.

**Done When**
- [x] AC1–AC4 passing with citations; AC5 benchmark deferred (ratchet) — see AC5 row for rationale

## Review Checklist

- [x] Stable AC IDs; asserted behaviors named; honest statuses
- [x] Scope bounded; commands runnable
