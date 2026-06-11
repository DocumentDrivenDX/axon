---
ddx:
  id: US-040
---

# US-040: Navigate the Tenant and Database Model

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-28, UI-29, UI-30, UI-31
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As an** operator managing Axon (Wei, Business Workflow Builder persona)
**I want** the UI to make tenant and database scope explicit
**So that** data operations cannot accidentally cross tenant boundaries

## Context

The admin console represents the ADR-018 control-plane hierarchy: tenants
contain databases, and all data tools are database-scoped. Without explicit
scope in navigation, an operator could inspect or mutate data in the wrong
tenant. This story exercises FEAT-011's navigation requirements (UI-28
through UI-31).

## Walkthrough

1. Operator opens the UI root.
2. System redirects to the tenant list; top navigation exposes only tenant
   and user control-plane roots.
3. Operator creates a tenant.
4. System routes into the tenant workspace, exposing Databases, Members, and
   Credentials sub-navigation.
5. Operator creates a database under the tenant.
6. System routes into the database workspace, immediately usable for
   collection and entity creation, with Collections, Schemas, Audit Log, and
   GraphQL sub-navigation.

## Acceptance Criteria

- [ ] **US-040-AC1** — Given a running Axon server with the UI enabled, when
  the operator opens the UI root, then the UI redirects to the tenant list
  and top navigation exposes only tenant and user control-plane roots.
- [ ] **US-040-AC2** — Given the tenant list, when the operator creates a
  tenant, then the UI routes to that tenant's workspace and tenant
  sub-navigation exposes Databases, Members, and Credentials.
- [ ] **US-040-AC3** — Given a tenant workspace, when the operator creates a
  database, then the UI routes to the database workspace, the database is
  immediately usable for collection/entity creation, and sub-navigation
  exposes Collections, Schemas, Audit Log, and GraphQL.
- [ ] **US-040-AC4** — Given a URL referencing an unknown tenant, when the
  operator opens it, then the UI renders a not-found state instead of a
  misleading empty console.
- [ ] **US-040-AC5** — Given two tenants containing identical database,
  collection, and entity IDs, when the operator navigates each tenant, then
  the contents remain fully isolated in the UI.

## Edge Cases

- **Unknown database under a known tenant**: renders the not-found state, not
  an empty database workspace.
- **Deep link into a database tool**: opening a database-scoped tool URL
  directly resolves scope from the URL and renders the correct context.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Root redirect | US-040-AC1 | Fresh server, UI enabled | Open UI root | Tenant list shown; nav exposes tenant and user roots only |
| Tenant creation flow | US-040-AC2 | Tenant list open | Create tenant `acme` | Routed to `acme` workspace with Databases/Members/Credentials |
| Database creation flow | US-040-AC3 | Tenant `acme` workspace | Create database `app` | Database workspace with Collections/Schemas/Audit Log/GraphQL; collection creation works |
| Unknown tenant | US-040-AC4 | No tenant `ghost` | Open URL for tenant `ghost` | Not-found state rendered |
| Tenant isolation | US-040-AC5 | Tenants `a` and `b` each with database `app`, collection `c`, entity `e1` | Browse both tenants | Each tenant shows only its own data |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-28, UI-29, UI-30, UI-31
- **PRD Requirements**: FR-24
- **External**: CONTRACT-001 (control-plane routes), CONTRACT-002
  (control-plane GraphQL), ADR-018 (tenant/database hierarchy)

## Out of Scope

- Tenant member and credential administration flows (US-041).
- Policy and intent navigation (FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
