---
ddx:
  id: US-036
---
# US-036: Organize Collections with Schemas

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-14, TEN-15, TEN-16
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder organizing a complex application
**I want** to group collections into logical namespaces within a database
**So that** billing collections are separate from engineering collections

## Context

Extracted from FEAT-014. Exercises schema namespace lifecycle (TEN-14), the
always-present `default` schema (TEN-15), and schema-scoped collection
naming (TEN-16). CLI command shapes are owned by CONTRACT-008.

## Walkthrough

1. Wei creates a `billing` schema in the `prod` database.
2. `billing.invoices` and `engineering.invoices` coexist as distinct collections.
3. Listing collections in `billing` shows only billing collections.
4. Dropping `billing` (with confirmation) removes its collections.

## Acceptance Criteria

- [ ] **US-036-AC1** — Given a database, when a schema namespace is created in it, then the namespace exists and can hold collections.
- [ ] **US-036-AC2** — Given schemas `billing` and `engineering` in one database, when each contains a collection named `invoices`, then both collections coexist as distinct collections.
- [ ] **US-036-AC3** — Given collections in several schemas, when collections are listed scoped to one schema, then only that schema's collections are returned.
- [ ] **US-036-AC4** — Given a non-empty schema, when it is dropped without the confirmation/force option, then the operation fails listing the dependent collections; with confirmation, the schema and its collections are removed.

## Edge Cases

- **`default` schema**: cannot be dropped; operations that omit the schema component target it.
- **Dots in collection names**: rejected at creation (reserved as namespace separator).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create namespace | US-036-AC1 | Database `prod` | Create schema `billing` | Namespace exists |
| Homonym collections | US-036-AC2 | Schemas `billing`, `engineering` | Create `invoices` in each | Two distinct collections |
| Scoped listing | US-036-AC3 | Collections across schemas | List `billing` collections | Only billing collections |
| Guarded drop | US-036-AC4 | `billing` has collections | Drop without confirmation | Fails listing dependents; succeeds with confirmation |
| Default protected | edge | Any database | Drop `default` schema | Rejected |

## Dependencies

- **Stories**: US-035 (database creation).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-14, TEN-15, TEN-16
- **PRD Requirements**: FR-25
- **External**: CONTRACT-001 (routes), CONTRACT-008 (CLI command shapes)

## Out of Scope

- Schema-level access policies (FEAT-012/FEAT-029).
- Schema inheritance (excluded by design).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
