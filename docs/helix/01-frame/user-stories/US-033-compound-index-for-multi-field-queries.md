---
ddx:
  id: US-033
---
# US-033: Compound Index for Multi-Field Queries

**Feature**: FEAT-013 — Secondary Indexes and Query Acceleration
**Feature Requirements**: IDX-02, IDX-12, IDX-13
**PRD Requirements**: FR-4
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer querying entities by multiple fields
**I want** a compound index that accelerates multi-field lookups
**So that** queries like "status=pending AND priority>3" are fast

## Context

Extracted from FEAT-013. Exercises compound index declaration (IDX-02),
leftmost-prefix acceleration (IDX-13), and index-order sorting (IDX-12).

## Walkthrough

1. Ava declares a compound index on `(status, priority)`.
2. Queries filtering on both fields use the compound index.
3. Queries filtering on `status` alone also use it via prefix matching.
4. Sorting by `(status, priority)` rides the index scan order.

## Acceptance Criteria

- [ ] **US-033-AC1** — Given a compound index on `(status, priority)`, when a query filters on both fields, then the compound index is used and results are correct.
- [ ] **US-033-AC2** — Given the same index, when a query filters on `status` alone, then the index is used via leftmost-prefix matching.
- [ ] **US-033-AC3** — Given the same index, when a query sorts by `(status, priority)`, then the index scan order satisfies the sort with no application-layer sort.

## Edge Cases

- **Non-prefix filter**: a query filtering on `priority` alone does not match the `(status, priority)` prefix and falls back to scan (or another index).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Both fields | US-033-AC1 | Compound index `ready` | Query `status="pending" AND priority>3` | Compound index used; correct results |
| Prefix match | US-033-AC2 | Same index | Query `status="pending"` | Index used via prefix |
| Index-order sort | US-033-AC3 | Same index | Query sorted by status, priority | Pre-sorted results from index scan |
| Non-prefix | edge | Same index | Query `priority>3` only | Fallback path; correct results |

## Dependencies

- **Stories**: US-031 (single-field declaration baseline).
- **Feature Spec**: [FEAT-013 — Secondary Indexes and Query Acceleration](../features/FEAT-013-secondary-indexes.md)
- **Feature Requirements**: IDX-02, IDX-12, IDX-13
- **PRD Requirements**: FR-4
- **External**: CONTRACT-010 (declaration grammar); ADR-010 (compound sort-key design)

## Out of Scope

- Cost-based selection among multiple candidate indexes (V1 is rules-based).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
