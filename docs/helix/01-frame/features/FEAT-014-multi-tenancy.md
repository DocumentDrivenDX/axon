---
dun:
  id: FEAT-014
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-012
    - ADR-010
    - ADR-011
    - ADR-018
---
# Feature Specification: FEAT-014 - Tenancy, Namespace Hierarchy, and Path-Based Addressing

**Feature ID**: FEAT-014
**Status**: Draft
**Priority**: P1 (tenant + database model), P2 (node topology and migration)
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-14

## Overview

Axon organizes data under a four-level conceptual hierarchy with
**tenant** as the top-level account boundary:

```
tenant  (global account boundary — owns users, credentials, and databases)
├── users            (M:N membership via tenant_users)
├── credentials      (tenant-scoped JWTs granting per-database access)
└── databases        (N per tenant — placed on nodes per ADR-011)
     └── schemas     (logical namespace within a database)
          └── collections  (entity containers with schemas)
               └── entities

node  (physical placement only — invisible from the data path)
```

**Wire addressing** is **pure path-based**:

```
/tenants/{tenant}/databases/{database}/{resource...}
```

Every data-plane route nests under this prefix. There is no
`X-Axon-Database` header, no `X-Axon-Tenant` header, and no un-prefixed
routes. Every entity has a canonical URL that simultaneously serves as its
identifier, its routing key, and its HTTP cache key. A request from any
client to any node can be routed to the correct database by parsing the
path alone — no body inspection, no header lookup.

See [ADR-018](../../02-design/adr/ADR-018-tenant-user-credential-model.md)
for the full decision record, including the walk-back of commit `efe4aa1`
and the amendment to ADR-011. See [ADR-011](../../02-design/adr/ADR-011-multi-tenancy-and-namespace-hierarchy.md)
for node topology and the database migration protocol, which this feature
inherits unchanged.

## Problem Statement

The pre-ADR-018 model collapsed tenant and database into a single concept
(commit `efe4aa1`: "one tenant, one database"). That was adequate for
single-user embedded dev mode but fails for:

- **SaaS deployments with multi-database customers**: A SaaS customer
  often runs `billing`, `analytics`, `events` as separate databases under
  one account boundary. A 1:1 tenant:database model cannot express this.
- **Users in multiple tenants**: A single human (or integration) is
  commonly a member of several organizations, each with a different role.
  A user-scoped-to-one-tenant model prevents the "switch workspace" flow
  that every SaaS tool offers.
- **Scoped machine credentials**: CI jobs and integrations need
  short-lived tokens with grants smaller than their issuer's role, so
  that a credential leak compromises one narrow scope rather than an
  entire user's access. Per-tenant JWTs with explicit `grants` claims
  make this model-level, not policy-level.
- **Federated identity**: Today users authenticate via Tailscale whois.
  Tomorrow we'll want OIDC, API keys external to the JWT system, and
  email+password. A first-class `users` table with a `user_identities`
  federation layer gives us those providers for free without coupling
  the rest of the stack to Tailscale.
- **Path-identifiable entities**: An edge gateway, an HTTP cache, and a
  webhook consumer all want to identify an entity by a stable URL. Header-
  based database routing (`X-Axon-Database`) breaks all three — URLs alone
  don't uniquely address an entity, cache keys don't include headers, and
  webhook consumers can't POST back to a canonical URL. Path-based
  addressing solves all three with one change.
- **Team organization within a tenant**: Within a single tenant,
  different teams still need logical grouping (billing collections
  separate from engineering collections). This is what schemas within a
  database provide and remains unchanged from ADR-011.
- **Geographic compliance**: Some data must reside in specific regions
  (GDPR, data sovereignty). Node placement per ADR-011 handles this; a
  tenant can have databases placed on multiple nodes in different regions.
- **Operational mobility**: Databases must be movable between nodes
  without changing application code or entity addresses. ADR-011's
  migration protocol handles this unchanged — because URLs address
  `(tenant, database)` not `(node, database)`, node migration is
  client-transparent.

## Requirements

### Functional Requirements

#### Tenants (P1)

Tenant ownership and lifecycle is defined here; tenant authentication,
users, and credentials are defined in FEAT-012 (Authorization) and
ADR-018.

- **Create tenant**: An explicit admin-only control-plane operation
  creates a tenant with a name, display name, and metadata. Tenants are
  global — not bound to any specific node. The control plane persists
  tenant rows in its SQL store.
- **Drop tenant**: Deleting a tenant cascades: all of its databases,
  memberships, and credentials are removed. Requires admin confirmation
  and blocks if any database is in an active migration.
- **List tenants**: Enumerate tenants visible to the caller. A caller
  sees only tenants they are a member of, unless they are a deployment
  admin (who sees all).
- **Default tenant bootstrap**: On a deployment with zero tenants, the
  first successful authenticated request auto-creates a `default` tenant
  with the authenticating user as its sole admin. Idempotent — runs only
  when `tenants` is empty. This replaces the old "auto-create default
  database" behavior; the default tenant is what now owns the default
  database. Bootstrap MUST be concurrency-safe: two simultaneous
  first-requests on a fresh deployment MUST converge on a single
  `tenants.name="default"` row via `UNIQUE(name)` + `INSERT ... ON
  CONFLICT DO NOTHING`. Check-then-insert is forbidden. See ADR-018
  Section 6 for the normative SQL pattern.
- **`--no-auth` mode and tenant URLs**: when the server is started with
  `--no-auth`, no persistent bootstrap runs and no `tenants`/`users` rows
  are written. The URL path `{tenant}/{database}` is still honored —
  the middleware synthesizes an anonymous admin claim scoped to
  whichever `(tenant, database)` the URL names, and the storage adapter
  materializes a per-URL in-memory namespace on first touch. This gives
  `--no-auth` a clean dev-mode semantic: any URL works, nothing is
  persisted past process lifetime unless the configured storage adapter
  says so. See ADR-018 Section 6.
- **Tenant owns databases**: A `tenant_databases(tenant_id, database_name)`
  join authoritatively declares which databases belong to which tenant.
  Database names are unique within a tenant but not globally — two
  tenants can both have a database named `orders`.
- **Tenant owns users and credentials**: See FEAT-012 for the user and
  credential model. At this spec's level, it suffices to say that every
  database operation is authorized against the `(user, tenant, database)`
  triple, not against the database alone.

#### Databases (P1)

- **Create database**: Named, isolated data space within a tenant. All
  collections, schemas, entities, links, audit log, and indexes within a
  database are independent of other databases, even within the same
  tenant.
- **Drop database**: Remove a database and all its contents (with
  confirmation). Tenant is required — a database only exists within a
  tenant's scope.
- **List databases**: Enumerate databases within a specified tenant.
- **Default database**: Within the default tenant, a `default` database
  is auto-created on tenant bootstrap. This is a convenience for
  single-tenant dev deployments — operators who explicitly create a
  tenant get no auto-database and must create one with `POST
  /tenants/{id}/databases`.
- **No cross-database queries**: Databases are fully isolated in V1,
  including across databases within the same tenant.
- **Database as backup unit**: A database is the unit of backup and
  restore.
- **Database as migration unit**: A database can be moved between nodes
  without changing its data path — the canonical URL
  `/tenants/{t}/databases/{d}/...` continues to resolve while the
  underlying placement table changes. See ADR-011 for the migration
  protocol.

#### Schemas / Namespaces (P1)

- **Create schema**: Logical namespace within a database. Groups related
  collections
- **Drop schema**: Remove a namespace and all its collections (with
  confirmation)
- **List schemas**: Enumerate schemas within a database
- **Default schema**: Every database has a `default` schema created
  automatically. Operations that omit the schema component target `default`
- **Collection uniqueness**: Collection names are unique within a schema,
  not globally. `billing.invoices` and `engineering.invoices` can coexist

#### Wire Addressing — Path-Based (P1)

All data-plane routes are nested under a fixed prefix:

```
/tenants/{tenant}/databases/{database}/{resource...}
```

- **`{tenant}` and `{database}` are required path segments**, not
  optional. A request without the prefix returns 404.
- **No `X-Axon-Database` header.** The header is fully removed. Same for
  any `x-axon-database` gRPC metadata.
- **No un-prefixed routes.** `POST /entities/tasks/t-001` returns 404,
  not a redirect. Clients, tests, and the UI all use the full path form.
- **No path-prefix `/db/{name}/...` legacy shape.** That form is removed.
- **No cross-tenant references in a single URL.** A URL addresses one
  tenant and one database. Cross-tenant operations go through the
  control plane (`/control/tenants/...`).
- **Canonical entity URL**:
  `/tenants/{tenant}/databases/{database}/entities/{collection}/{id}`.
  This is simultaneously the entity's identifier, its routing key, and
  its HTTP cache key.
- **Three-part internal collection names**: Internally, collections are
  still identified by `database.schema.collection` for link-type
  references and schema cross-references. This is an implementation
  detail; external clients always use the path-based URL form.

**Backward compatibility**: **none**. Pre-release clean break per
ADR-018. Existing SDKs, tests, CLI commands, and the admin UI are
rewritten in the same commits that change the routing. There is no
deprecation period.

#### Node Topology (P2)

- **Node registry**: Track running Axon nodes with name, region, zone,
  endpoint, status, and capabilities
- **Database placement**: Map databases to nodes. Each database has exactly
  one primary node. Optional read replicas
- **Request routing**: Any node can accept requests for any database. If
  the target database is remote, the node proxies or redirects
- **Database migration**: Move a database from one node to another via
  replication + routing table update. Data path is unchanged
- **Geographic metadata**: Nodes carry region and zone for placement
  decisions

#### Access Control Integration (P1)

Authentication and authorization are defined in FEAT-012. This section
summarizes the interaction with the tenant/database hierarchy:

- **Authorization is a two-layer check**, evaluated on every request:
  1. **Membership**: the caller's `user_id` must appear in
     `tenant_users(tenant_id=…)` for the tenant named in the URL path.
     The row's `role` (admin | write | read) sets the ceiling.
  2. **Grant**: if the caller is using a JWT credential, the credential's
     `grants.databases[]` claim must include an entry for the URL's
     database segment with an `ops` list that intersects the required
     op (read for GET, write for other methods).
- **Per-tenant roles** (admin | write | read): set at membership-creation
  time. Admin can do anything in the tenant; write can CRUD entities but
  cannot manage members, credentials, or schema; read can only query.
  Roles are per `(user, tenant)` — the same user can be admin in one
  tenant and read in another.
- **Grant ≤ role invariant**: at credential-issuance time, the control
  plane enforces that the requested grants are a subset of what the
  issuer's role in the tenant permits. A `read` member cannot issue a
  credential with `write` grants.
- **No cross-tenant grants**: grants live inside credentials, credentials
  are tenant-scoped, so cross-tenant access requires holding multiple
  credentials — one per tenant. Prevents a single compromised credential
  from affecting more than one tenant.
- **Tailscale auth** (see FEAT-012 and ADR-005): Tailscale-identified
  callers do not carry JWTs. The auth middleware resolves the tailnet
  identity to a `user_id` via `user_identities`, looks up
  `tenant_users(tenant_id=…)` for the URL tenant, synthesizes an
  in-memory grants struct (usually all databases within the tenant, at
  the membership's role level), and treats the rest of the request
  identically to a JWT-authenticated one.

#### Physical Database Isolation (added by FEAT-028)

FEAT-014 defines "database" as a *logical* isolation boundary (namespace
hierarchy). Physical isolation maps each logical database to a separate
backing store, providing OS-level separation:

- **SQLite mode**: The master/control-plane database lives at
  `{data_dir}/axon.db`. Each tenant's database gets its own file at
  `{data_dir}/tenants/{tenant}/databases/{database}.db`. The server
  opens adapters lazily on first request to a database and caches them.
  The per-tenant subdirectory also isolates disk-level access: a tenant
  admin with filesystem access sees only their own databases.
- **PostgreSQL mode**: When a superadmin DSN is provided via config, the
  server creates a master database (`axon_master`) on first startup.
  When a new database is created via API (`POST
  /control/tenants/{tenant}/databases`), the server issues
  `CREATE DATABASE axon_{tenant}_{database}` and opens a connection
  pool for it. The tenant-prefixed physical name prevents collisions
  across tenants with the same database name.
- **Routing**: A `DatabaseRouter` (renamed from `TenantRouter` in
  pre-evolution code) resolves the `(tenant, database)` pair parsed
  from the URL path `/tenants/{tenant}/databases/{database}/...`, looks
  up the adapter for that `(tenant, database)`, and injects it into
  the request. The router also enforces that the authenticated
  `(user, tenant)` membership and credential grants permit the
  requested database — see FEAT-012 for the auth middleware order.
- **Default database**: Within the auto-created `default` tenant, a
  `default` database is created alongside. This is a convenience for
  dev mode; explicitly-created tenants get no auto-database.
- **Drop database**: Closes the adapter, deletes the SQLite file (or
  `DROP DATABASE` for PostgreSQL), removes the `tenant_databases` row,
  and removes any per-database grants from outstanding JWT credentials
  by adding their jtis to the revocation list (optional safety step).

### Non-Functional Requirements

- **Name resolution latency**: < 1ms (cached). Collection name resolution
  adds negligible overhead
- **Zero-config single tenant**: `default` database and `default` schema
  are created automatically. No configuration needed for single-tenant use
- **Storage overhead**: Minimal. Database and schema are resolved at
  collection lookup time. Entity, link, and index tables are unchanged
  from ADR-010

## User Stories

### Story US-087: Create a Tenant with Multiple Databases [FEAT-014]

**As an** operator onboarding a SaaS customer
**I want** to create one tenant per customer and then N databases within it
**So that** I can group the customer's `billing`, `analytics`, and `events`
  databases under a single billing and access boundary

**Acceptance Criteria:**
- [ ] `POST /control/tenants` with a name creates a tenant; the response
  includes the tenant's stable id
- [ ] `POST /control/tenants/{id}/databases` creates a database inside
  the tenant; subsequent `GET /control/tenants/{id}/databases` lists it
- [ ] `GET /tenants/{tenant}/databases` (data plane) returns the same list
  for callers with membership in the tenant
- [ ] Two tenants can both have a database named `orders` without collision
- [ ] Dropping the tenant cascades to all its databases, memberships, and
  credentials
- [ ] Database operations under `/tenants/{tenant}/databases/{database}/...`
  only see data in that tenant — cross-tenant visibility is impossible
  even for admins

### Story US-088: Users Are Members of Multiple Tenants [FEAT-014]

**As a** developer working across two customer engagements
**I want** to be a member of two tenants with different roles in each
**So that** my identity follows me across both and I can switch workspace
  without creating a second user account

**Acceptance Criteria:**
- [ ] A single `users` row can be linked to `tenant_users` in multiple
  tenants with a different role per row
- [ ] The user's `display_name` and `email` are global (on the `users`
  table), not per-tenant
- [ ] Auth middleware on a request to `/tenants/acme/...` checks
  `tenant_users(tenant_id=acme, user_id=…)` and honors that membership's
  role, independently of any membership the user has in other tenants
- [ ] A user removed from one tenant's membership list still has full
  access to any other tenants they belong to
- [ ] Listing tenants visible to a non-admin caller returns only the
  tenants that caller is a member of

### Story US-035: Create and Use a Database (within a tenant) [FEAT-014]

**As an** operator organizing one tenant's data
**I want** to create isolated databases for different purposes within the
  same tenant
**So that** the tenant's billing data is isolated from its analytics data

**Acceptance Criteria:**
- [ ] `POST /control/tenants/{tenant}/databases` with a name creates an
  isolated data space within the tenant
- [ ] Collections created in `/tenants/{t}/databases/billing/collections/*`
  are not visible from `/tenants/{t}/databases/analytics/*`
- [ ] `DELETE /control/tenants/{tenant}/databases/{db}` removes all its
  collections, entities, and audit log
- [ ] Audit entries for database `billing` only reference `billing` data
- [ ] Listing collections in database `billing` returns zero results for
  other databases
- [ ] Audit log entries for `billing` are not visible from `analytics`

### Story US-036: Organize Collections with Schemas [FEAT-014]

**As a** developer organizing a complex application
**I want** to group collections into logical namespaces
**So that** billing collections are separate from engineering collections

**Acceptance Criteria:**
- [ ] `axon schema create billing --database prod` creates a namespace
- [ ] `prod.billing.invoices` and `prod.engineering.invoices` are
  distinct collections
- [ ] Listing collections in `prod.billing` shows only billing collections
- [ ] Dropping the `billing` schema removes all its collections
- [ ] Dropping a non-empty schema fails with error listing dependent collections unless force flag is set

### Story US-037: Zero-Config Default Tenant for Dev Mode [FEAT-014]

**As a** developer running Axon locally
**I want** a working default tenant and database without explicit
  provisioning
**So that** I can start building immediately after `axon serve`

**Acceptance Criteria:**
- [ ] First successful authenticated request on a fresh deployment
  auto-creates a `default` tenant with the authenticating user as its
  sole admin, plus a `default` database with a `default` schema
- [ ] Subsequent requests to `/tenants/default/databases/default/...`
  work without any explicit provisioning step
- [ ] The CLI's `axon entity create` defaults to the `default` tenant
  and `default` database when no tenant/database flags are provided
- [ ] Auto-bootstrap is idempotent — it runs only when `tenants` is
  empty; after any tenant exists, it does not re-run
- [ ] `--no-auth` mode does not require a tenant to be present in
  the database; it synthesizes an in-memory default tenant context
  on every request

### Story US-038: Scope Access to a Specific Database via Tenant Membership [FEAT-014]

**As an** operator
**I want** to grant Alice admin access to the `prod` tenant only
**So that** she can manage production data without affecting staging

**Acceptance Criteria:**
- [ ] Adding Alice to `tenant_users(tenant_id=prod, user_id=alice, role=admin)`
  grants her full access within `prod`
- [ ] Alice has no access to tenant `staging` unless separately added
- [ ] Within `prod`, admin role applies to all of the tenant's databases
  and schemas by default — fine-grained per-database grants live in
  credentials Alice can issue to herself or to integrations (see FEAT-012)
- [ ] Alice cannot issue a credential with grants exceeding her membership
  role

### Story US-039: Register Nodes and Track Placement [FEAT-014] (P2)

**As an** operator running Axon across regions
**I want** to register nodes and assign databases to specific nodes
**So that** data lives in the region closest to its users

**Acceptance Criteria:**
- [ ] `axon node register us-east --region us-east-1 --endpoint ...`
  adds a node to the registry
- [ ] `axon database create eu-data --node eu-west` places the database
  on the EU node
- [ ] Requests to `eu-data` from the US node are proxied to the EU node
- [ ] `axon database migrate eu-data --to us-east` moves the database

## Edge Cases

- **Database name collision**: Creating a database with an existing name
  returns a conflict error
- **Drop database with active connections**: Connections targeting the
  dropped database receive errors on next request. No connection hijacking
- **Schema name `default`**: The `default` schema cannot be dropped (it's
  always present)
- **Three-part name with dots in collection name**: Collection names cannot
  contain dots (reserved as namespace separator). Validated on creation
- **Node goes offline**: Database placement shows `offline` status.
  Requests to databases on that node fail with 503. No automatic failover
  in V1 (requires consensus, deferred)
- **Rename database**: Not supported in V1. Create new + migrate + drop old

## Dependencies

- **FEAT-001** (Collections): Collections live within schemas within
  databases within tenants. FEAT-001 is updated to reference the path-
  based addressing form.
- **FEAT-012** (Authorization): Defines the user + credential model plus
  the `tenant_users` M:N join and the grant enforcement middleware.
  FEAT-014 relies on FEAT-012 for every authorization decision.
- **FEAT-025** (Control Plane): Hosts the CRUD routes for tenants,
  users, memberships, and credentials.
- **ADR-010**: Physical storage tables use integer collection IDs that
  implicitly encode database + schema via the collections lookup table.
  Collection IDs are unique across the whole deployment — an entity's
  collection ID alone does not identify its tenant. Tenant comes from
  the URL path.
- **ADR-011**: Namespace hierarchy below database (schemas and
  collections), node topology, database placement, and the database
  migration protocol. Amended by ADR-018 for the tenant aspect.
- **ADR-018**: Governing decision record for tenant + user + credential
  model, path-based wire protocol, walk-back of `efe4aa1`.

## Out of Scope

- **Cross-database queries**: Databases are fully isolated. No joins or
  references across databases
- **Cross-database links**: Links cannot span databases
- **Automatic failover**: If a node goes down, its databases are
  unavailable until the node recovers or an operator migrates them
- **Database-level quotas**: Storage limits per database. Deferred
- **Schema inheritance**: Schemas within a database do not inherit from
  each other

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #9 (Storage architecture),
  P2 #4 (Multi-tenancy)
- **User Stories**: US-035, US-036, US-037, US-038, US-039, US-087, US-088
- **Architecture**: ADR-010, ADR-011, ADR-018
- **Implementation**: `crates/axon-core/` (tenant, user, credential
  types), `crates/axon-server/src/control_plane.rs` (control plane
  storage), `crates/axon-server/src/gateway.rs` (path-prefixed router),
  `crates/axon-server/src/auth.rs` (JWT verification, user resolution),
  `crates/axon-api/src/handler.rs` (request extension consumption)

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-012
- **Depended By**: FEAT-011 (Admin UI gains database/schema navigation),
  FEAT-028 (Unified Binary — physical isolation, TenantRouter)
