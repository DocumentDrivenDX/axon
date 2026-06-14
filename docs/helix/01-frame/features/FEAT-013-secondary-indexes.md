---
ddx:
  id: FEAT-013
  depends_on:
    - helix.prd
  review:
    self_hash: e218b7499012d56e569acc094cc40b47360b34fda601b473ac425af2cec09b27
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:25:45Z"
---
# Feature Specification: FEAT-013 — Secondary Indexes and Query Acceleration

**Feature ID**: FEAT-013
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Requirement Prefix**: IDX
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-4
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Secondary indexes accelerate entity queries by maintaining pre-computed lookup
structures for declared fields, implementing PRD FR-4. Without indexes, all
queries perform full collection scans with application-layer filtering. With
indexes, equality, range, and sort queries execute in sub-millisecond time.
Indexes are declared in the collection schema and maintained automatically on
every entity write. The physical index design (table layout, compound sort-key
encoding) is owned by ADR-010 (Physical Storage and Secondary Indexes).

## Ideal Future State

A developer declares which fields their queries filter and sort on, and those
queries become fast — on every supported backend, with no backend-specific
operators leaking into the API. Uniqueness constraints are enforced at the
storage level. Adding an index to a live collection never blocks normal
operations: the index builds in the background and the planner adopts it when
ready. Developers never think about index internals; they think in terms of
declared query patterns.

## Problem Statement

- **Current situation**: Entity queries perform a full range scan of every entity in the collection, deserializing and filtering in application code.
- **Pain points**: Acceptable for small collections but degrades linearly with entity count. Agents frequently query by status ("give me all pending beads") and sort by priority or creation time; these queries are O(n) in collection size. Collections with thousands of entities need sub-millisecond lookups on common filter fields.
- **Desired outcome**: Declared single-field and compound indexes make common filters, sorts, and uniqueness checks fast and portable across backends.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Index declaration | "How do I say which fields are indexed?" | Schema-declared single-field, compound, and unique indexes (grammar owned by CONTRACT-010) |
| Index maintenance | "Do indexes stay correct as data changes?" | Automatic maintenance on every write and delete; uniqueness enforcement |
| Query acceleration | "Are my queries actually faster?" | Planner uses indexes for equality, range, sort, and prefix matching; safe fallback |
| Index lifecycle | "Can I add or remove indexes on a live collection?" | Background build, ready/dropping states, rebuild |

## Requirements

### Functional Requirements by Area

#### Index Declaration

- **IDX-01**: A schema must be able to declare a single-field index (field path plus value type) for fast equality and range lookups.
- **IDX-02**: A schema must be able to declare a compound index — an ordered list of field+type pairs — for multi-field queries and multi-field sorts.
- **IDX-03**: Single-field and compound indexes must support a uniqueness option: no two entities in the same collection may share the same indexed value.
- **IDX-04**: Supported index value types are `string`, `integer`, `float`, `datetime`, and `boolean`.
- **IDX-05**: Index declarations are part of the collection schema, alongside the entity schema, link types, and lifecycle declarations. The normative declaration grammar is owned by CONTRACT-010 (ESF schema format).
- **IDX-06**: Null or missing field values are not indexed. Entities with null indexed fields are excluded from index lookups but included in full scans.

#### Index Maintenance

- **IDX-07**: Every entity create, update, and patch must maintain all declared indexes for that entity, so that index entries always reflect the latest committed entity state.
- **IDX-08**: When an entity is deleted, its index entries must be removed.
- **IDX-09**: If a write would violate a unique index, the operation must fail with a conflict error identifying the conflicting field and value.

#### Query Acceleration

- **IDX-10**: Equality queries on an indexed field (e.g., `status = "pending"`) must use the index rather than a full scan.
- **IDX-11**: Range queries on an indexed field (e.g., `priority > 3`) must use an index range scan.
- **IDX-12**: When the query's sort field matches an index, the index scan order must satisfy the sort with no application-layer sort.
- **IDX-13**: A compound index must accelerate queries filtering on a leftmost prefix of its fields (e.g., a `(status, priority)` index accelerates `status`-only filters).
- **IDX-14**: Queries on non-indexed fields must fall back to a full scan with application-layer filtering, producing the same results as an indexed path would.

#### Index Lifecycle

- **IDX-15**: A new index on an existing collection starts in a `building` state: a background process populates it from existing entities, and the query planner does not use it until built.
- **IDX-16**: Once the build completes, the index transitions to `ready` and the query planner begins using it. Writes during the build also populate the new index.
- **IDX-17**: When an index is removed from the schema, it enters a `dropping` state: queries stop using it immediately and its entries are cleaned up in the background.
- **IDX-18**: An admin operation must be able to trigger a full rebuild: the index returns to `building`, existing entries are discarded, and entities are rescanned.

### Non-Functional Requirements

- **Write amplification**: Each entity write may touch one maintenance structure per declared index; typical workloads (1–5 indexes per collection) must stay within the entity-write latency targets.
- **Query latency**: Indexed equality query on a 100K-entity collection < 5ms p99.
- **Index build**: Background build processes at least 10K entities/second.
- **Backend portability**: Index behavior must be identical on PostgreSQL, SQLite, and KV stores; no backend-specific query operators are exposed (see ADR-010 for the portable physical design).

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-031 | Declare a Secondary Index | [US-031](../user-stories/US-031-declare-a-secondary-index.md) |
| US-032 | Enforce Uniqueness via Index | [US-032](../user-stories/US-032-enforce-uniqueness-via-index.md) |
| US-033 | Compound Index for Multi-Field Queries | [US-033](../user-stories/US-033-compound-index-for-multi-field-queries.md) |
| US-034 | Background Index Build | [US-034](../user-stories/US-034-background-index-build.md) |

## Edge Cases and Error Handling

- **Schema change removes an index**: Index enters `dropping` state; queries stop using it immediately. Background cleanup follows.
- **Index type mismatch**: Entity has a string value for an integer-indexed field. The value is not indexed (skip, don't error). Query results for that entity come from the fallback scan path.
- **Concurrent unique violation**: Two transactions insert entities with the same unique-indexed value. Exactly one succeeds; the other gets a conflict error.
- **Empty collection**: Index on an empty collection is immediately `ready`.
- **All entities have null for indexed field**: Index is empty but valid. Queries on that field return no results from the index, but a full scan would find entities (with null values matching `IS NULL` predicates if supported).

## Success Metrics

- Indexed equality and range queries meet the < 5ms p99 NFR target on a 100K-entity collection, versus O(n) scan behavior without the index.
- Index results and fallback-scan results are identical for the same query (zero correctness divergence across the two paths and across backends).
- Adding an index to a live, busy collection causes no failed or blocked entity writes during the background build.

## Constraints and Assumptions

### Constraints

- Index behavior must be portable: no GIN, JSONB containment, or other backend-specific operators in the public surface (FR-4).
- Indexes are declarative schema artifacts; there is no imperative "create index" data path outside schema changes and the rebuild operation.

### Assumptions

- Typical collections declare 1–5 indexes.
- Rules-based index selection (no cost-based planner) is sufficient for V1 query shapes.

## Dependencies

- **Other features**: FEAT-001 (Collections — indexes belong to collections), FEAT-002 (Schema Engine — index declarations are part of the schema), FEAT-004 (Entity Operations — index maintenance hooks into the write path).
- **External services**: None. Normative interface surface: CONTRACT-010 (index declaration grammar). Physical design: ADR-010 (Physical Storage and Secondary Indexes — EAV index tables, compound sort-key encoding, index lifecycle states).
- **PRD requirements**: FR-4 (P1).

## Out of Scope

- **Array field indexing**: one row per array element — deferred.
- **Full-text search indexes**: separate feature (PRD P2 "Advanced indexes and search").
- **Cost-based query planning**: V1 uses simple rules-based index selection.
- **GIN / JSONB containment indexes**: deliberately omitted for portability.
- **Expression indexes**: indexes on computed values — deferred.

## Review Checklist

Use this checklist when reviewing this feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details — WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
