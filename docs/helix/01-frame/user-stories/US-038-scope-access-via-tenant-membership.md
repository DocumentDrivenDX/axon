---
ddx:
  id: US-038
---
# US-038: Scope Access to a Specific Database via Tenant Membership

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-03, TEN-07
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder administering a production deployment
**I want** to grant a teammate admin access to the `prod` tenant only
**So that** she can manage production data without affecting staging

## Context

Extracted from FEAT-014. Exercises tenancy scoping of every authorization
decision (TEN-07) and member-scoped visibility (TEN-03). The membership
roles, credentials, and the grant ≤ role invariant are owned by FEAT-012;
this story verifies that tenancy boundaries hold.

## Walkthrough

1. Wei adds Alice to the `prod` tenant with the admin role.
2. Alice has full access within `prod` — all of its databases and schemas.
3. Alice has no access to the `staging` tenant.
4. Alice cannot issue a credential whose grants exceed her membership role.

## Acceptance Criteria

- [ ] **US-038-AC1** — Given Alice is added to tenant `prod` with the admin role, when she operates within `prod`, then she has full admin access to the tenant's databases and schemas.
- [ ] **US-038-AC2** — Given Alice has no membership in tenant `staging`, when she addresses `staging` resources, then access is denied.
- [ ] **US-038-AC3** — Given Alice's admin membership in `prod`, when she works across `prod`'s databases, then her role applies tenant-wide by default; narrower per-database scoping is done via credentials (FEAT-012).
- [ ] **US-038-AC4** — Given Alice's role in a tenant, when she requests a credential with grants exceeding that role, then issuance is refused (invariant owned by FEAT-012; verified here at the tenancy boundary).

## Edge Cases

- **Membership revocation**: removing Alice from `prod` immediately stops new requests from succeeding; her other tenant memberships are unaffected.
- **Deployment admin vs tenant admin**: a tenant admin sees only their tenants; only a deployment admin sees all tenants.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Tenant-wide admin | US-038-AC1 | Alice admin in `prod` | Manage data in any `prod` database | Allowed |
| Cross-tenant denial | US-038-AC2 | No `staging` membership | Read `staging` data | Denied |
| Tenant-wide default | US-038-AC3 | Same membership | Operate on two `prod` databases | Both allowed at admin level |
| Grant ceiling | US-038-AC4 | Alice has read role in `dev` tenant | Request write-grant credential in `dev` | Refused |

## Dependencies

- **Stories**: US-088 (multi-tenant membership), FEAT-012 stories (roles, credentials, grants).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-03, TEN-07
- **PRD Requirements**: FR-25
- **External**: CONTRACT-001 (routes); FEAT-012/ADR-018 (membership roles and grant model)

## Out of Scope

- Role and grant semantics themselves (FEAT-012).
- Field-level and row-level policy (FEAT-029).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
