---
ddx:
  id: US-035
---
# US-035: Create and Use a Database (within a tenant)

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-08, TEN-09, TEN-12, TEN-21, TEN-22
**PRD Requirements**: FR-25, FR-26
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder organizing one tenant's data
**I want** to create isolated databases for different purposes within the same tenant
**So that** the tenant's billing data is isolated from its analytics data

## Context

Extracted from FEAT-014. Exercises database creation and deletion (TEN-08,
TEN-09), intra-tenant isolation (TEN-12), and the physical-isolation
guarantee (TEN-21, TEN-22). Exact routes are owned by CONTRACT-001; physical
layout by ADR-010/ADR-018.

## Walkthrough

1. Wei creates a `billing` database inside the tenant via the control plane.
2. Collections and entities created in `billing` are invisible from the tenant's `analytics` database.
3. Audit history is database-scoped: `billing` audit entries reference only `billing` data.
4. Dropping `billing` removes all of its collections, entities, and audit log, and its physical backing store.

## Acceptance Criteria

- [ ] **US-035-AC1** — Given a tenant, when a database is created in it with a name, then an isolated data space exists within the tenant.
- [ ] **US-035-AC2** — Given collections created in the tenant's `billing` database, when collections are listed in the same tenant's `analytics` database, then none of the `billing` collections appear.
- [ ] **US-035-AC3** — Given a database with collections, entities, and audit history, when the database is dropped with confirmation, then all of its contents are removed.
- [ ] **US-035-AC4** — Given mutations in `billing`, when the audit log is queried from `analytics`, then no `billing` audit entries are visible.
- [ ] **US-035-AC5** — Given a `(tenant, database)` pair, when it is created, then it is backed by its own physical store (one file or backend database per pair), and dropping it removes that store.

## Edge Cases

- **Duplicate name**: creating a database with an existing name in the tenant returns a conflict error.
- **Requests after drop**: requests addressing a dropped database fail cleanly on next request; no connection hijacking.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create database | US-035-AC1 | Tenant `acme` | Create database `billing` | Isolated data space exists |
| Collection isolation | US-035-AC2 | Collections in `billing` | List collections in `analytics` | Zero `billing` collections |
| Cascading drop | US-035-AC3 | `billing` with data and audit | Drop with confirmation | All contents removed |
| Audit isolation | US-035-AC4 | Mutations in `billing` | Query audit from `analytics` | No `billing` entries |
| Physical store | US-035-AC5 | Create then drop `billing` | Inspect backing stores | Dedicated store created, then removed |

## Dependencies

- **Stories**: US-087 (tenant creation), US-036 (schemas within a database).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-08, TEN-09, TEN-12, TEN-21, TEN-22
- **PRD Requirements**: FR-25, FR-26
- **External**: CONTRACT-001 (routes); ADR-010/ADR-018 (physical layout and naming)

## Out of Scope

- Backup/restore mechanics (TEN-13 is feature-level; tooling is separate).
- Cross-database queries or links (excluded by design).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
