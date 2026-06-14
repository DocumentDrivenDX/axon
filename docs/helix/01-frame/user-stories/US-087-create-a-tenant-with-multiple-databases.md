---
ddx:
  id: US-087
  review:
    self_hash: 511661a33c55f9e75d2387035374af219e06c4bb69234b1e2cfeae34aaa16d73
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---
# US-087: Create a Tenant with Multiple Databases

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-01, TEN-02, TEN-04, TEN-08, TEN-10, TEN-12
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder onboarding a SaaS customer onto an Axon deployment
**I want** to create one tenant per customer and then N databases within it
**So that** the customer's `billing`, `analytics`, and `events` databases sit under a single account and access boundary

## Context

Extracted from FEAT-014. Exercises tenant creation and cascade deletion
(TEN-01, TEN-02), tenant-scoped database naming (TEN-04), database creation
and listing (TEN-08, TEN-10), and tenant isolation on the data plane
(TEN-12). Exact control-plane and data-plane routes are owned by
CONTRACT-001.

## Walkthrough

1. Wei creates a tenant for the customer through the control-plane API and receives its stable id.
2. Wei creates `billing`, `analytics`, and `events` databases inside the tenant and lists them back.
3. A member of the tenant lists the same databases through the data plane.
4. Another tenant independently creates a database with one of the same names — no collision.
5. When the engagement ends, dropping the tenant cascades to its databases, memberships, and credentials.

## Acceptance Criteria

- [ ] **US-087-AC1** — Given an admin caller, when a tenant is created with a name via the control plane, then the response includes the tenant's stable id.
- [ ] **US-087-AC2** — Given a tenant, when a database is created inside it via the control plane, then a subsequent control-plane listing of the tenant's databases includes it.
- [ ] **US-087-AC3** — Given a caller with membership in the tenant, when they list the tenant's databases through the data plane, then they see the same list as the control plane.
- [ ] **US-087-AC4** — Given two tenants, when both create a database named `orders`, then both creations succeed without collision.
- [ ] **US-087-AC5** — Given a tenant with databases, memberships, and credentials, when the tenant is dropped with confirmation, then all of its databases, memberships, and credentials are removed.
- [ ] **US-087-AC6** — Given data in two tenants, when any data-plane request addresses one tenant's database, then it can never observe the other tenant's data — even for tenant admins.

## Edge Cases

- **Duplicate database name within one tenant**: returns a conflict error (tenant-scoped uniqueness).
- **Drop without confirmation**: tenant drop without the confirmation step is rejected.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create tenant | US-087-AC1 | Admin caller | Create tenant `acme` | Stable tenant id returned |
| Create + list databases | US-087-AC2 | Tenant `acme` | Create `billing`; list databases | `billing` listed |
| Same name, two tenants | US-087-AC4 | Tenants `acme`, `globex` | Both create `orders` | Both succeed |
| Cascade drop | US-087-AC5 | `acme` with 3 dbs, 2 members, 1 credential | Drop tenant with confirmation | All dependent records removed |
| Isolation | US-087-AC6 | Entities in `acme/billing` and `globex/billing` | Read via `acme` paths as `acme` admin | Only `acme` data visible |

## Dependencies

- **Stories**: US-088 (membership model), US-035 (database usage within a tenant).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-01, TEN-02, TEN-04, TEN-08, TEN-10, TEN-12
- **PRD Requirements**: FR-25
- **External**: CONTRACT-001 (control-plane and data-plane routes); FEAT-025 (control plane hosts the CRUD routes)

## Out of Scope

- Credential issuance and grant mechanics (FEAT-012).
- Audit retention policy on tenant drop (audit/compliance design).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
