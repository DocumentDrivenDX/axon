---
ddx:
  id: FEAT-014
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-012
    - FEAT-025
    - ADR-010
    - ADR-011
    - ADR-018
---
# Feature Specification: FEAT-014 — Tenancy, Namespace Hierarchy, and Path-Based Addressing

**Feature ID**: FEAT-014
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Requirement Prefix**: TEN
**Covered PRD Subsystem(s)**: Identity, Tenancy, and Storage Portability
**Covered PRD Requirements**: FR-25 (the tenant/database scope model and namespace hierarchy; the user, credential, and grant model is owned by FEAT-012), supporting FR-26 (per-tenant physical isolation through the storage adapter)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Axon organizes data under a four-level conceptual hierarchy with **tenant** as
the top-level account boundary, implementing the tenant/database scope aspect
of PRD FR-25:

```
tenant  (global account boundary — owns users, credentials, and databases)
├── users            (M:N membership)
├── credentials      (tenant-scoped, granting per-database access)
└── databases        (N per tenant)
     └── schemas     (logical namespace within a database)
          └── collections  (entity containers with schemas)
               └── entities
```

Wire addressing is pure path-based: every data-plane resource is addressed by
a canonical URL that names its tenant and database, and that URL is
simultaneously the resource's identifier, its routing key, and its HTTP cache
key. The governing decision record is
[ADR-018](../../02-design/adr/ADR-018-tenant-user-credential-model.md);
the namespace hierarchy below the database comes from
[ADR-011](../../02-design/adr/ADR-011-multi-tenancy-and-namespace-hierarchy.md).

## Ideal Future State

An operator onboards a SaaS customer as one tenant, creates as many isolated
databases under it as the customer needs, and never worries about cross-tenant
leakage — isolation is structural, down to separate physical backing stores. A
developer running locally gets a working default tenant and database with zero
provisioning. Every entity has one canonical URL that any gateway, cache, or
webhook consumer can use to identify and route to it by parsing the path
alone. Users belong to multiple tenants with independent roles, and nothing in
the data path depends on headers, body inspection, or out-of-band context.

## Problem Statement

- **Current situation**: The pre-ADR-018 model collapsed tenant and database into a single concept ("one tenant, one database"), adequate only for single-user embedded dev mode.
- **Pain points**: SaaS customers run multiple databases (`billing`, `analytics`, `events`) under one account boundary, which a 1:1 model cannot express. Users belong to multiple organizations with different roles, which a user-scoped-to-one-tenant model prevents. Header-based database routing (`X-Axon-Database`) breaks edge gateways, HTTP caches, and webhook consumers, because URLs alone do not identify an entity. Teams within a tenant need logical grouping of collections.
- **Desired outcome**: A tenant → database → schema → collection hierarchy with path-based canonical addressing, structural tenant isolation, and zero-config defaults for development.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Tenant model | "What is the account boundary, and how do I manage it?" | Tenant lifecycle, ownership of databases, default-tenant bootstrap |
| Database model | "How do I isolate one customer's billing data from their analytics data?" | Database lifecycle and isolation semantics within a tenant |
| Schema namespaces | "How do teams organize collections inside one database?" | Logical namespaces, default schema, collection-name scoping |
| Path-based addressing | "How is a resource identified and routed?" | Canonical tenant/database-scoped URLs as the only addressing mechanism |
| Physical isolation | "How strong is the isolation, really?" | One physical backing database per (tenant, database) pair |

## Requirements

### Functional Requirements by Area

#### Tenant Model

- **TEN-01**: An admin-only control-plane operation must create a tenant with a name, display name, and metadata. Tenants are global — not bound to any node.
- **TEN-02**: Dropping a tenant must cascade: all of its databases, memberships, and credentials are removed. The operation requires admin confirmation.
- **TEN-03**: Listing tenants must return only tenants the caller is a member of, unless the caller is a deployment admin (who sees all).
- **TEN-04**: A tenant authoritatively owns its databases. Database names are unique within a tenant but not globally — two tenants can both have a database named `orders`.
- **TEN-05**: On a deployment with zero tenants, the first successful authenticated request must auto-create a `default` tenant with the authenticating user as its sole admin, plus a `default` database and `default` schema. Bootstrap is idempotent (runs only when no tenant exists) and concurrency-safe: two simultaneous first-requests converge on a single default tenant. The normative concurrency pattern is ADR-018 Section 6.
- **TEN-06**: In no-auth development mode, no persistent tenant or user records are written; any tenant/database named in a request path is honored with a synthesized anonymous admin context and a per-path namespace materialized on first touch, with no persistence beyond process lifetime unless the configured storage adapter persists it. Behavior per ADR-018 Section 6; the mode flag is owned by CONTRACT-008.
- **TEN-07**: Every database operation must be authorized against the `(user, tenant, database)` triple, never against the database alone. The user, membership, credential, and grant model is owned by FEAT-012 (with ADR-018); FEAT-014 requires only that tenancy scope participates in every decision.

#### Database Model

- **TEN-08**: A database must be creatable within a tenant as a named, isolated data space: all collections, schemas, entities, links, audit log, and indexes within it are independent of every other database, including others in the same tenant.
- **TEN-09**: Dropping a database must remove the database and all its contents, with confirmation. A database exists only within a tenant's scope.
- **TEN-10**: Databases must be listable within a tenant for authorized callers.
- **TEN-11**: A `default` database is auto-created only in the auto-bootstrapped `default` tenant. Explicitly created tenants get no auto-database; operators create databases through the control plane.
- **TEN-12**: There are no cross-database queries: databases are fully isolated in V1, including within the same tenant.
- **TEN-13**: A database is the unit of backup and restore.

#### Schema Namespaces

- **TEN-14**: A schema (logical namespace) must be creatable, listable, and droppable within a database; dropping a non-empty schema requires confirmation and removes its collections.
- **TEN-15**: Every database has a `default` schema created automatically; operations that omit the schema component target `default`; the `default` schema cannot be dropped.
- **TEN-16**: Collection names are unique within a schema, not globally — `billing.invoices` and `engineering.invoices` can coexist in one database.

#### Path-Based Addressing

Path-based addressing is a capability boundary of this feature: the canonical
URL is the resource's identity, routing key, and cache key. The exact route
grammar, required path segments, status codes for unprefixed or malformed
paths, and error envelope are owned by CONTRACT-001 (HTTP API surface,
routing and tenancy addressing).

- **TEN-17**: Every data-plane resource must be addressable by a canonical URL that names its tenant and database. A request must be routable to the correct database by parsing the path alone — no header lookup, no body inspection.
- **TEN-18**: There must be no header-based or out-of-band database routing on the data plane (no database/tenant headers or equivalent metadata) and no un-prefixed or legacy route shapes.
- **TEN-19**: A single URL addresses exactly one tenant and one database. Cross-tenant operations go through the control plane only.
- **TEN-20**: Addressing must be placement-independent: a resource's canonical URL identifies `(tenant, database)`, never a node, so physical placement can change without changing addresses.

#### Physical Isolation

- **TEN-21**: Each `(tenant, database)` pair must be physically isolated in its own backing store — one database file per pair in embedded mode, one backend database per pair in server mode — so that tenant separation is enforced at the storage level, not only by query filtering. Physical naming, file layout, and DDL are owned by ADR-010 and ADR-018.
- **TEN-22**: Dropping a database must remove its physical backing store and its tenant-database association, and must invalidate outstanding credential grants scoped to it.

### Non-Functional Requirements

- **Name resolution latency**: < 1ms (cached); collection name resolution adds negligible overhead to the data path.
- **Zero-config single tenant**: a fresh deployment is usable with no tenancy configuration — default tenant, database, and schema exist after the first authenticated request.
- **Isolation**: zero cross-tenant reads or writes are possible through any data-plane request, verified by contract tests addressing one tenant while data exists in another.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-087 | Create a Tenant with Multiple Databases | [US-087](../user-stories/US-087-create-a-tenant-with-multiple-databases.md) |
| US-088 | Users Are Members of Multiple Tenants | [US-088](../user-stories/US-088-users-are-members-of-multiple-tenants.md) |
| US-035 | Create and Use a Database (within a tenant) | [US-035](../user-stories/US-035-create-and-use-a-database.md) |
| US-036 | Organize Collections with Schemas | [US-036](../user-stories/US-036-organize-collections-with-schemas.md) |
| US-037 | Zero-Config Default Tenant for Dev Mode | [US-037](../user-stories/US-037-zero-config-default-tenant-for-dev-mode.md) |
| US-038 | Scope Access to a Specific Database via Tenant Membership | [US-038](../user-stories/US-038-scope-access-via-tenant-membership.md) |
| US-039 | Register Nodes and Track Placement | [US-039](../user-stories/US-039-register-nodes-and-track-placement.md) (deferred — see Out of Scope) |

## Edge Cases and Error Handling

- **Database name collision**: Creating a database with an existing name in the same tenant returns a conflict error.
- **Drop database with active connections**: Connections targeting the dropped database receive errors on their next request. No connection hijacking.
- **Schema named `default`**: The `default` schema cannot be dropped (always present).
- **Dots in collection names**: Collection names cannot contain dots (reserved as namespace separator). Validated on creation.
- **Concurrent bootstrap**: Two simultaneous first-requests on a fresh deployment converge on one default tenant; neither caller observes a duplicate or an error caused by the race.
- **Rename database**: Not supported in V1 — create new + copy + drop old.

## Success Metrics

- 100% of data-plane requests are routable by path parsing alone (no header or body inspection) in contract tests.
- Zero cross-tenant visibility in the isolation test suite, including for tenant admins.
- A developer reaches a working default tenant/database/schema on a fresh deployment with zero provisioning steps.

## Constraints and Assumptions

### Constraints

- **No backward compatibility**: pre-release clean break per ADR-018 — no deprecated header routing, no legacy route shapes, no migration period.
- Tenant and database participate in every policy and audit decision (FR-25); FEAT-014 cannot be bypassed by any surface.
- Physical isolation layout follows ADR-010/ADR-018; this spec owns only the behavioral guarantee.

### Assumptions

- Deployments host tens to hundreds of tenants, each with a handful of databases — not millions.
- The control plane (FEAT-025) hosts tenant/database CRUD; the data plane only resolves and enforces the hierarchy.

## Dependencies

- **Other features**: FEAT-001 (Collections — collections live within schemas within databases within tenants), FEAT-012 (Authorization — user, membership, credential, and grant model; every authorization decision), FEAT-025 (Control Plane — hosts tenant/database/membership CRUD routes).
- **External services**: None. Normative interface surface: CONTRACT-001 (route grammar, tenancy addressing, status codes), CONTRACT-008 (CLI flags and dev-mode config). Design records: ADR-010 (physical storage layout), ADR-011 (namespace hierarchy below database), ADR-018 (tenant/user/credential model, path-based wire protocol, bootstrap concurrency pattern).
- **PRD requirements**: FR-25 (P1); supports FR-26 (P1).

## Out of Scope

- **Node topology, distributed placement, and database migration**: node registry, database-to-node placement, request proxying/redirects between nodes, placement metadata, and the migration protocol are deferred — see the "Distributed Placement and Migration" entry in [docs/helix/parking-lot.md](../../parking-lot.md) (PRD P2 #3, FR-27 deferral). US-039 is parked with it.
- **Cross-database queries and links**: databases are fully isolated; no joins, references, or links across databases.
- **Automatic failover**: out of scope with node topology.
- **Database-level quotas**: storage limits per database — deferred.
- **Schema inheritance**: schemas within a database do not inherit from each other.
- **User, credential, and grant mechanics**: owned by FEAT-012.

## Review Checklist

Use this checklist when reviewing this feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details — WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
