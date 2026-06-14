---
ddx:
  id: US-088
  review:
    self_hash: 2a48880eb058fd7b22faa676616ff3f949f387c1428457a675dcd346e8fcc640
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---
# US-088: Users Are Members of Multiple Tenants

**Feature**: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing
**Feature Requirements**: TEN-03, TEN-07
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Draft

## Story

**As a** Ava, an agent application developer working across two customer engagements
**I want** to be a member of two tenants with a different role in each
**So that** my identity follows me across both and I can switch workspace without a second user account

## Context

Extracted from FEAT-014. Exercises per-tenant membership roles and the
`(user, tenant, database)` authorization triple (TEN-07) and member-scoped
tenant listing (TEN-03). The user/membership/credential data model is owned
by FEAT-012 and ADR-018; this story covers the tenancy-scoping behavior.

## Walkthrough

1. Ava's single user identity is added to tenant `acme` as admin and tenant `globex` as read.
2. A request from Ava to an `acme` path is authorized against her `acme` membership only.
3. Ava lists tenants and sees exactly `acme` and `globex`.
4. Ava is removed from `globex`; her `acme` access is unaffected.

## Acceptance Criteria

- [ ] **US-088-AC1** — Given a single user identity, when it is granted membership in two tenants with different roles, then both memberships coexist with their distinct roles.
- [ ] **US-088-AC2** — Given a user with global profile attributes (display name, email), when read from either tenant's context, then the profile is the same — identity is global, role is per-tenant.
- [ ] **US-088-AC3** — Given a request addressing one tenant, when authorization runs, then only the caller's membership in that tenant is consulted, independent of memberships elsewhere.
- [ ] **US-088-AC4** — Given a user removed from one tenant, when they access another tenant they still belong to, then that access is unaffected.
- [ ] **US-088-AC5** — Given a non-admin caller, when they list tenants, then only tenants they are a member of are returned.

## Edge Cases

- **No membership in the addressed tenant**: a request to a tenant the caller does not belong to is denied as if scoped resources do not exist for them.
- **Role asymmetry**: an admin of tenant A with read in tenant B cannot perform admin operations in B.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Dual membership | US-088-AC1 | User ava | Add to `acme` (admin) and `globex` (read) | Both memberships exist with distinct roles |
| Per-tenant authz | US-088-AC3 | Same memberships | Write to `globex` data as ava | Denied (read role), despite admin in `acme` |
| Removal isolation | US-088-AC4 | Remove ava from `globex` | Access `acme` data | Full admin access unaffected |
| Member-scoped listing | US-088-AC5 | Third tenant `initech` exists | Ava lists tenants | Only `acme`, `globex` returned |

## Dependencies

- **Stories**: US-038 (membership-scoped access), FEAT-012 user stories (identity model).
- **Feature Spec**: [FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing](../features/FEAT-014-multi-tenancy.md)
- **Feature Requirements**: TEN-03, TEN-07
- **PRD Requirements**: FR-25
- **External**: CONTRACT-001 (routes); FEAT-012/ADR-018 (user, membership, and credential model)

## Out of Scope

- Credential issuance, grants, and the grant ≤ role invariant (FEAT-012).
- Workspace-switching UI (FEAT-011).

## Review Checklist

- [ ] Persona comes from the PRD
- [ ] Every AC is independently testable with one Given/When/Then
- [ ] No exact API/CLI/event/schema surface inline; Contract IDs referenced
- [ ] Test scenarios cover the happy path and at least one edge case
