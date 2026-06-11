---
ddx:
  id: US-017
---

# US-017: Model Entities with Nested Structure

**Feature**: FEAT-007 — Entity-Graph Data Model
**Feature Requirements**: GRF-01, GRF-03, GRF-04
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As** Wei, a business workflow builder modeling real-world records
**I want** to store entities with deeply nested fields
**So that** I can represent business objects faithfully without flattening them into rows

## Context

Business records have depth — addresses inside customers, line items inside
invoices, children inside tree nodes. This story exercises FEAT-007's entity
model (GRF-01 nesting, GRF-03 schema binding into nested objects, GRF-04
recursive types). Nested-field query semantics are owned by FEAT-009's read
model.

## Walkthrough

1. Wei declares a collection schema with nested objects, arrays, and a recursive tree-node type (grammar per CONTRACT-010).
2. Wei writes an entity with five levels of nesting; the system validates the whole structure, including required fields inside nested objects.
3. Wei reads the entity back and receives the identical nested structure.
4. Wei queries by a nested dot-path predicate through the unified read model (FEAT-009) and finds the entity.

## Acceptance Criteria

- [ ] **US-017-AC1** — Given a schema permitting deep nesting, when Wei stores an entity with five levels of nesting, then reading it back returns the structure unchanged.
- [ ] **US-017-AC2** — Given a schema with required fields inside nested objects, when an entity missing a required nested field is written, then the write fails with a field-level validation error identifying the nested path.
- [ ] **US-017-AC3** — Given a recursive schema type (tree node with children of the same type), when Wei stores a multi-level tree entity, then validation and storage succeed.
- [ ] **US-017-AC4** — Given stored entities with nested fields, when Wei queries by a nested dot-path predicate (read model per FEAT-009 / CONTRACT-007), then matching entities are returned.

## Edge Cases

- **Nesting beyond schema-declared shape**: undeclared nested fields are rejected per schema strictness rules (CONTRACT-010).
- **Very deep recursive instance**: validated structurally; entities exceeding the size limit (1 MB, FEAT-004) are rejected with a structured error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Round-trip | US-017-AC1 | `customer.address.geo.lat`-style 5-level body | Create, then read | Identical structure returned |
| Nested required | US-017-AC2 | Schema requires `address.city` | Write without `city` | Validation error with nested field path |
| Recursive tree | US-017-AC3 | Tree node with 3 generations of children | Create | Stored and retrievable |
| Dot-path query | US-017-AC4 | Entities with `address.city` values | Query `address.city = "Seattle"` | Matching entities returned |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-007
- **Feature Requirements**: GRF-01, GRF-03, GRF-04
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010 (ESF schema grammar), CONTRACT-007 (nested-path query semantics, owned by FEAT-009), FEAT-002 (schema engine)

## Out of Scope

- Query language semantics beyond exercising one nested predicate (FEAT-009).
- Cross-entity relationships — that is the link model (US-018).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
