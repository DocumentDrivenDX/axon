---
dun:
  id: FEAT-014
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-012
    - ADR-010
    - ADR-011
---
# Feature Specification: FEAT-014 - Multi-Tenancy and Namespace Hierarchy

**Feature ID**: FEAT-014
**Status**: Draft
**Priority**: P1 (data model), P2 (node topology and migration)
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Axon adopts a four-level namespace hierarchy to support multi-tenant
deployments and geographic data placement:

```
node          (deployment / physical location — routing only)
  └── database    (tenant isolation boundary)
       └── schema     (logical namespace within a database)
            └── collection  (entity container)
                 └── entity
```

The **data path** is three levels: `database.schema.collection`. The node
level is a routing/placement concept only — it does not appear in entity
addresses or storage keys.

See [ADR-011](../../02-design/adr/ADR-011-multi-tenancy-and-namespace-hierarchy.md)
for the full design.

## Problem Statement

Axon currently operates as a single flat namespace: one set of collections,
one set of schemas. This works for single-user embedded mode but fails for:

- **SaaS deployment**: Multiple tenants sharing infrastructure need data
  isolation guarantees
- **Team organization**: Within a tenant, different teams need logical
  grouping (billing collections separate from engineering collections)
- **Access control scoping**: RBAC/ABAC grants need granularity beyond
  "all collections" — per-database and per-namespace
- **Geographic compliance**: Some data must reside in specific regions
  (GDPR, data sovereignty)
- **Operational mobility**: Databases must be movable between nodes
  without changing application code or entity addresses

## Requirements

### Functional Requirements

#### Databases (P1)

- **Create database**: Named, isolated data space. All collections, schemas,
  entities, links, audit log, and indexes within a database are independent
  of other databases
- **Drop database**: Remove a database and all its contents (with confirmation)
- **List databases**: Enumerate all databases with metadata
- **Default database**: Single-tenant deployments use a `default` database
  created automatically on first startup. All operations target `default`
  when no database is specified
- **No cross-database queries**: Databases are fully isolated in V1
- **Database as backup unit**: A database is the unit of backup and restore
- **Database as migration unit**: A database can be moved between nodes
  without changing its data path

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

#### Fully Qualified Names (P1)

- **Three-part names**: `database.schema.collection`
- **Resolution with defaults**: `invoices` → `{current_db}.default.invoices`;
  `billing.invoices` → `{current_db}.billing.invoices`
- **Connection-level database**: Clients specify target database via header
  (`X-Axon-Database` for HTTP, `x-axon-database` metadata for gRPC) or
  path prefix (`/db/{name}/...`). Defaults to `default`
- **Backward compatibility**: Existing single-tenant code works unchanged —
  `beads` resolves to `default.default.beads`

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

- **Database-scoped grants**: "Alice is admin on database `prod`"
- **Schema-scoped grants**: "Bob is viewer on `prod.billing`"
- **Collection-scoped grants**: Existing FEAT-012 behavior, now with
  fully qualified collection names
- **Global grants**: "tag:ci is viewer everywhere"
- **Resolution**: Most-specific grant wins. Narrower scope overrides
  broader scope

#### Physical Database Isolation (added by FEAT-028)

FEAT-014 defines "database" as a *logical* isolation boundary (namespace
hierarchy). Physical isolation maps each logical database to a separate
backing store, providing OS-level separation:

- **SQLite mode**: The master/control-plane database lives at
  `{data_dir}/axon.db`. Each tenant database gets its own file at
  `{data_dir}/tenants/{db_name}.db`. The server opens adapters lazily
  on first request to a database and caches them.
- **PostgreSQL mode**: When a superadmin DSN is provided via config, the
  server creates a master database (`axon_master`) on first startup.
  When a new database is created via API, the server issues
  `CREATE DATABASE axon_{db_name}` and opens a connection pool for it.
- **Routing**: A `TenantRouter` resolves the `X-Axon-Database` header
  or `/db/{name}/` path prefix, looks up the adapter for that database,
  and injects it into the request. The existing `ControlPlaneState` is
  the catalog — `TenantRouter` reads from it, not duplicates it.
- **Default database**: The `default` database is always available. It
  is the implicit target when no database header is provided.
- **Drop database**: Closes the adapter, deletes the SQLite file (or
  `DROP DATABASE` for PostgreSQL), and removes the catalog entry.

### Non-Functional Requirements

- **Name resolution latency**: < 1ms (cached). Collection name resolution
  adds negligible overhead
- **Zero-config single tenant**: `default` database and `default` schema
  are created automatically. No configuration needed for single-tenant use
- **Storage overhead**: Minimal. Database and schema are resolved at
  collection lookup time. Entity, link, and index tables are unchanged
  from ADR-010

## User Stories

### Story US-035: Create and Use a Database [FEAT-014]

**As an** operator deploying Axon for multiple teams
**I want** to create isolated databases for each team
**So that** team A's data is invisible and inaccessible to team B

**Acceptance Criteria:**
- [ ] `axon database create teamA` creates an isolated data space
- [ ] Collections created in `teamA` are not visible from `teamB`
- [ ] Dropping `teamA` removes all its collections, entities, and audit log
- [ ] Audit entries within `teamA` only reference `teamA` data
- [ ] Listing collections in database teamA returns zero results for teamB's collections
- [ ] Audit log entries for teamA are not visible from teamB

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

### Story US-037: Use Axon Without Multi-Tenancy Config [FEAT-014]

**As a** developer running Axon locally
**I want** multi-tenancy to be invisible when I don't need it
**So that** I can use Axon exactly as before with zero configuration

**Acceptance Criteria:**
- [ ] First startup creates `default` database with `default` schema
- [ ] `axon collection create beads` works (resolves to `default.default.beads`)
- [ ] `axon entity get beads bead-1` works (no database/schema prefix needed)
- [ ] No multi-tenancy configuration is required for single-tenant use
- [ ] Default database is named `default`; default schema within it is named `default`

### Story US-038: Scope Access Control to Databases [FEAT-014]

**As an** operator
**I want** to grant Alice admin access to the `prod` database only
**So that** she can manage production data without affecting staging

**Acceptance Criteria:**
- [ ] A grant scoped to database `prod` gives Alice full access within `prod`
- [ ] Alice has no access to database `staging` unless separately granted
- [ ] Grants at database scope apply to all schemas and collections within

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

- **FEAT-001** (Collections): Collections now live within schemas within
  databases. FEAT-001 is updated to reference the namespace hierarchy
- **FEAT-012** (Authorization): Access control gains database and schema
  scope levels
- **ADR-010**: Physical storage tables use integer collection IDs that
  implicitly encode database + schema via the collections lookup table
- **ADR-011**: Full design for namespace hierarchy and node topology

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
- **User Stories**: US-035, US-036, US-037, US-038, US-039
- **Architecture**: ADR-010, ADR-011
- **Implementation**: `crates/axon-core/` (namespace types),
  `crates/axon-storage/` (collection lookup), `crates/axon-api/` (name
  resolution)

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-012
- **Depended By**: FEAT-011 (Admin UI gains database/schema navigation),
  FEAT-028 (Unified Binary — physical isolation, TenantRouter)
