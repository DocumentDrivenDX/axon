---
ddx:
  id: US-032
  review:
    self_hash: 3ab4e78b28a4284634a74b1e25bf16b3971837bb339ee28cdec4622424d3b8cd
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---
# US-032: Enforce Uniqueness via Index

**Feature**: FEAT-013 — Secondary Indexes and Query Acceleration
**Feature Requirements**: IDX-03, IDX-09
**PRD Requirements**: FR-4
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder modeling entities with unique business keys
**I want** to declare a unique index on a field
**So that** the system prevents duplicate values at the storage level

## Context

Extracted from FEAT-013. Exercises the uniqueness option on index
declarations (IDX-03) and the conflict behavior on violation (IDX-09).

## Walkthrough

1. Wei declares a unique index on `invoice_number`.
2. Creating a second entity with an existing `invoice_number` fails with a conflict error naming the field and value.
3. Concurrent inserts of the same value resolve to exactly one winner.

## Acceptance Criteria

- [ ] **US-032-AC1** — Given an index declaration with the uniqueness option, when the schema is saved, then uniqueness is enforced for that field across the collection.
- [ ] **US-032-AC2** — Given an entity with `invoice_number = "INV-100"`, when a second entity with the same value is created, then the operation fails with a conflict error.
- [ ] **US-032-AC3** — Given a uniqueness conflict, when the error is returned, then it identifies the conflicting field and value.
- [ ] **US-032-AC4** — Given two concurrent transactions inserting the same unique value, when both commit, then exactly one succeeds and the other receives a conflict error (storage-level enforcement, not application-level).

## Edge Cases

- **Null values**: entities with null in the unique-indexed field are not indexed and do not conflict with each other (IDX-06).
- **Update into conflict**: updating an existing entity's field to a value held by another entity fails with the same conflict error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Duplicate create | US-032-AC2 | Entity with `INV-100` exists | Create second entity with `INV-100` | Conflict error |
| Error detail | US-032-AC3 | Same as above | Inspect error | Names field `invoice_number` and value `INV-100` |
| Concurrent insert | US-032-AC4 | Empty collection | Two transactions insert `INV-200` concurrently | Exactly one succeeds; other gets conflict |
| Nulls don't conflict | edge | Two entities with null `invoice_number` | Create both | Both succeed |

## Dependencies

- **Stories**: US-031 (index declaration).
- **Feature Spec**: [FEAT-013 — Secondary Indexes and Query Acceleration](../features/FEAT-013-secondary-indexes.md)
- **Feature Requirements**: IDX-03, IDX-09
- **PRD Requirements**: FR-4
- **External**: CONTRACT-010 (declaration grammar); ADR-010 (physical design)

## Out of Scope

- Compound unique indexes beyond the declaration mechanics (US-033 covers compound behavior).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
