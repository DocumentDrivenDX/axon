---
ddx:
  id: US-031
  review:
    self_hash: bed482dda62dbde031f4449344e3eb439e109eb2ec66dd7061bdbe4468bb66ba
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---
# US-031: Declare a Secondary Index

**Feature**: FEAT-013 — Secondary Indexes and Query Acceleration
**Feature Requirements**: IDX-01, IDX-04, IDX-05, IDX-10, IDX-14
**PRD Requirements**: FR-4
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer defining a collection schema
**I want** to declare which fields should be indexed
**So that** queries on those fields are fast without backend-specific tuning

## Context

Extracted from FEAT-013. Exercises single-field index declaration (IDX-01,
IDX-04, IDX-05), index-accelerated equality queries (IDX-10), and the
full-scan fallback for non-indexed fields (IDX-14). The declaration grammar
is owned by CONTRACT-010.

## Walkthrough

1. Ava adds a string index on `status` to the collection schema.
2. Queries filtering on `status` now use the index instead of a full scan.
3. Queries on non-indexed fields keep working via the fallback scan.
4. A schema with an invalid index declaration is rejected at validation time.

## Acceptance Criteria

- [ ] **US-031-AC1** — Given a collection schema, when an index declaration for field `status` with type `string` is added, then an index on `status` exists for the collection.
- [ ] **US-031-AC2** — Given a `ready` index on `status`, when a query filters on `status`, then the index is used instead of a full scan and the results are correct.
- [ ] **US-031-AC3** — Given a non-indexed field, when a query filters on it, then the query still returns correct results via the fallback scan path.
- [ ] **US-031-AC4** — Given an invalid index declaration (unknown value type or missing field), when the schema is saved, then validation rejects it with an error naming the invalid declaration.

## Edge Cases

- **Type mismatch on data**: an entity holding a non-string value in a string-indexed field is skipped by the index, not an error; the entity remains reachable via fallback scan.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Declare index | US-031-AC1 | Schema without indexes | Add `status:string` index declaration | Index exists for the collection |
| Indexed query | US-031-AC2 | 10K entities, index `ready` | Query `status = "pending"` | Index path used; correct matches returned |
| Fallback | US-031-AC3 | No index on `owner` | Query `owner = "ava"` | Full-scan path; correct matches returned |
| Invalid declaration | US-031-AC4 | Index with type `decimal128` | Save schema | Validation error naming the declaration |

## Dependencies

- **Stories**: US-034 (background build when the collection already has data).
- **Feature Spec**: [FEAT-013 — Secondary Indexes and Query Acceleration](../features/FEAT-013-secondary-indexes.md)
- **Feature Requirements**: IDX-01, IDX-04, IDX-05, IDX-10, IDX-14
- **PRD Requirements**: FR-4
- **External**: CONTRACT-010 (index declaration grammar); ADR-010 (physical design)

## Out of Scope

- Uniqueness enforcement (US-032) and compound indexes (US-033).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
