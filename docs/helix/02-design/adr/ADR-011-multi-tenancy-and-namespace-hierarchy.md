---
dun:
  id: ADR-011
  depends_on:
    - ADR-003
    - ADR-010
    - FEAT-012
    - FEAT-014
---
# ADR-011: Multi-Tenancy, Namespace Hierarchy, and Node Topology

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | ADR-003, ADR-010, FEAT-012 | High |

## Context

Axon currently operates as a single flat namespace: one set of collections,
one set of schemas, one set of entities. There is no concept of tenant
isolation, logical grouping of collections, or physical data placement.

Production deployments need:
- **Tenant isolation**: separate data spaces for different teams, customers,
  or applications sharing the same Axon infrastructure
- **Logical namespacing**: grouping related collections within a tenant
  (like Postgres schemas within a database)
- **Access control scoping**: RBAC/ABAC grants at different granularities
  (whole tenant, namespace, individual collection)
- **Geographic placement**: the ability to pin data to a region for
  latency, compliance, or sovereignty requirements
- **Mobility**: the ability to move a tenant's data between physical nodes
  without changing the logical data path

| Aspect | Description |
|--------|-------------|
| Problem | Flat namespace, no tenant isolation, no geographic placement model |
| Current State | Single implicit database, collections identified by name only |
| Requirements | Multi-tenant isolation, namespace hierarchy, geographic-aware node topology, location-independent data addressing |

## Decision

### 1. Namespace Hierarchy

Axon adopts a four-level namespace hierarchy:

```
node          (deployment / physical location — routing only)
  └── database    (tenant isolation boundary — the unit of data)
       └── schema     (logical namespace within a database)
            └── collection  (entity container with schema)
                 └── entity     (data record)
```

The **data path** is three levels: `{database}.{schema}.{collection}`.
The node level is not part of the data path — it is a routing/placement
concept only.

#### Database

A **database** is the fundamental unit of tenant isolation:

- A complete, self-contained data space
- No cross-database queries (V1) — databases are fully isolated
- The unit of backup and restore
- The unit of migration between nodes
- The coarsest scope for RBAC/ABAC grants
- Has its own set of schemas, collections, entities, links, audit log,
  and secondary indexes
- Users/identities are global (not per-database), but grants are scoped
  to databases

Every Axon deployment has at least one database. Single-tenant
deployments use a single `default` database transparently.

**Data model:**
```
databases:
    PK: id (int, auto-increment)
    name: text (unique within the node cluster)
    owner: text (identity that created it)
    created_at: timestamp
    metadata: bytes (arbitrary config — quotas, settings)
```

#### Schema (Namespace)

A **schema** is a logical namespace within a database, grouping related
collections:

- Analogous to PostgreSQL schemas (`public`, `billing`, `audit`)
- The second scope for RBAC/ABAC grants ("user X has read-only access
  to the `billing` schema in database `prod`")
- Every database has a `default` schema created automatically
- Collections within a schema have unique names; the same collection
  name can exist in different schemas

**Data model:**
```
schemas:
    PK: id (int, auto-increment)
    database_id: int → databases
    name: text (unique within database)
    created_at: timestamp
    metadata: bytes
    UNIQUE: (database_id, name)
```

#### Fully Qualified Names

Every collection is addressed by its fully qualified name:

```
database.schema.collection
```

When `database` is omitted, the connection's current database is used.
When `schema` is omitted, `default` is used. This means existing
single-tenant code continues to work — `beads` resolves to
`default.default.beads`.

**Resolution order:**
1. `beads` → `{current_db}.default.beads`
2. `billing.invoices` → `{current_db}.billing.invoices`
3. `prod.billing.invoices` → `prod.billing.invoices`

### 2. Node Topology

A **node** is a running Axon process (or cluster of processes) at a
specific deployment location. Nodes are a routing and placement concept
— they do not appear in the data path.

#### Design Principle

The data model is **location-independent**. An entity's identity is
`{database}.{schema}.{collection}/{entity_id}` regardless of which node
currently hosts it. When a database moves between nodes, only the routing
table changes — no data rewrite, no key-space migration, no client-visible
ID change.

#### Node Registry

```
node_registry:
    PK: node_id (int)
    name: text (unique, human-readable — "us-east-1a", "eu-west-prod")
    region: text ("us-east-1", "eu-west-1", "ap-southeast-1")
    zone: text (availability zone within region, optional)
    endpoint: text ("axon-1.us-east.internal:50051")
    status: text ("active", "draining", "offline")
    capabilities: bytes (storage backends available, capacity, etc.)
    last_heartbeat: timestamp
```

#### Database Placement

```
database_placement:
    PK: (database_id, node_id)
    database_id: int → databases
    node_id: int → node_registry
    role: text ("primary", "read-replica", "migrating-source", "migrating-target")
    assigned_at: timestamp
```

A database has exactly one `primary` node at any time. It may have
zero or more `read-replica` nodes.

#### Request Routing

When a client sends a request to any node:

1. **Local**: If the target database's primary is this node, handle
   locally.
2. **Proxy**: If the target database's primary is another node, proxy
   the request to that node transparently. The client doesn't need to
   know which node owns which database.
3. **Redirect** (optional, for latency-sensitive clients): Return a
   redirect response with the primary node's endpoint. The client
   reconnects directly. This is opt-in via a client capability flag.

For V1, all databases live on a single node. The routing table exists
but always points to the local node. No proxy or redirect logic is
needed — but the data model supports it from day one.

#### Database Migration

Moving a database from node A to node B:

1. Create a `migrating-target` placement entry on node B
2. Stream data from A to B (background replication)
3. Quiesce writes to the database (brief pause)
4. Finalize replication, verify consistency
5. Update placement: B becomes `primary`, A becomes `draining`
6. Resume writes on B
7. After drain period, remove A's placement entry

The client sees a brief write pause during step 3-6. Reads can continue
against A during migration (stale reads) or be redirected to B after
step 6.

This is the same model used by CockroachDB range moves and Vitess
tablet migrations. The key property is that the data path
(`database.schema.collection/entity`) never changes — only the routing
table updates.

### 3. Storage Layer Impact

The entity storage key gains database and schema prefixes:

**ADR-010 key (current):**
```
entities/{collection_id}/{entity_id}
```

**ADR-011 key (new):**
```
entities/{database_id}/{schema_id}/{collection_id}/{entity_id}
```

In practice, `database_id` and `schema_id` are folded into the
`collection_id` lookup — the `collections` table gains:

```
collections:
    PK: id (int, auto-increment)
    database_id: int → databases
    schema_id: int → schemas
    name: text
    UNIQUE: (schema_id, name)
```

The integer `collection_id` from ADR-010 remains the key used in entity
rows, link rows, and index rows. It already implicitly encodes the
database and schema because the collection ID is globally unique.

This means **no changes to the physical entity/link/index tables from
ADR-010** — they continue to use `collection_id` as their partition key.
The database and schema hierarchy is resolved during collection lookup,
not during every entity access.

#### Per-Database Isolation on PostgreSQL

On PostgreSQL, databases can optionally map to separate Postgres schemas
(the Postgres concept, not Axon schemas):

```sql
CREATE SCHEMA axon_db_1;   -- Axon database "prod"
CREATE SCHEMA axon_db_2;   -- Axon database "staging"
```

Each Axon database's tables live in their own Postgres schema. This
provides:
- Stronger isolation (separate table namespaces)
- Independent `GRANT` management
- Potential for separate tablespaces (different disks per tenant)

This is optional — a simpler approach uses a single Postgres schema with
`database_id` columns on every table (the approach ADR-010 already
supports). The per-schema approach is a deployment-time configuration
choice, not a data model change.

#### Per-Database Isolation on KV Stores

On KV stores, each Axon database is a key prefix:

```
/{database_id}/entities/{collection_id}/{entity_id}
/{database_id}/links/...
/{database_id}/idx/...
```

FoundationDB directory layer can map database names to prefixes
automatically. This provides clean isolation and enables per-database
rate limiting at the FDB layer.

### 4. Access Control Integration

FEAT-012 (Authorization) gains multi-level grant scoping:

```
grants:
    PK: id (int)
    identity: text (user, service account, tag)
    scope_type: text ("global", "database", "schema", "collection")
    scope_database_id: int (null for global)
    scope_schema_id: int (null for global/database)
    scope_collection_id: int (null for global/database/schema)
    role: text ("admin", "editor", "viewer", "custom-role-name")
    conditions: bytes (ABAC conditions, optional)
```

**Resolution**: When checking access, collect all matching grants from
most-specific to least-specific. A grant at a broader scope applies to
all contained objects unless overridden by a narrower grant.

**Examples:**
- "Alice is admin on database `prod`" → full access to everything in
  `prod`
- "Bob is viewer on schema `prod.billing`" → read-only access to all
  collections in `prod.billing`
- "Agent-X is editor on collection `prod.default.tasks`" → read/write
  on `tasks` only
- "tag:ci is viewer globally" → read-only access to everything on every
  database

### 5. API Surface

#### Connection-Level Database

Clients specify their target database at connection time:

**gRPC**: Metadata header `x-axon-database: prod`

**HTTP**: Header `X-Axon-Database: prod` or path prefix
`/db/prod/entities/...`

If omitted, the `default` database is used.

#### New Admin RPCs

```
CreateDatabase(name, metadata?) → database
DropDatabase(name, force?) → ()
ListDatabases() → [database]

CreateSchema(database, name, metadata?) → schema
DropSchema(database, name, force?) → ()
ListSchemas(database) → [schema]

-- Node management (superadmin only)
RegisterNode(name, region, zone, endpoint) → node
DeregisterNode(node_id) → ()
ListNodes() → [node]
MigrateDatabase(database, target_node_id) → migration_status
```

#### Fully Qualified Collection Names in Existing RPCs

Existing RPCs accept fully qualified collection names:

```
CreateEntity(collection: "prod.billing.invoices", ...)
```

The handler resolves the three-part name to a `collection_id` using the
namespace hierarchy. If the database or schema component is omitted, the
connection's defaults apply.

### 6. Default Behavior

For single-tenant deployments (the common case in V1):

- A `default` database is created on first startup
- A `default` schema is created within it
- All collection operations target `default.default` implicitly
- No database or schema headers are needed
- The node registry has one entry (self)
- The routing table maps `default` to the local node

This means **zero configuration change for existing single-tenant
deployments**. Multi-tenancy is opt-in.

## Consequences

**Positive**:
- Tenant isolation at the database level — clean data boundaries
- Logical namespacing within tenants via schemas
- Geographic placement modeled from day one — no retrofitting
- Location-independent data paths — databases can move between nodes
  without data rewrites or client ID changes
- RBAC/ABAC grants at four granularity levels (global, database, schema,
  collection)
- Single-tenant deployments are unaffected — defaults make it transparent
- Physical storage tables from ADR-010 are unchanged — the namespace
  hierarchy is resolved at collection lookup time

**Negative**:
- Adds two new concepts (database, schema) to the mental model and API
- Name resolution on every request (mitigated by caching)
- Database migration protocol is complex (but deferred to post-V1)
- Cross-database queries are not supported — intentional for isolation,
  but limits some use cases
- Node registry and placement tables add operational surface area

**V1 Scope**:
- Database and schema data model: implemented
- Default database/schema creation on startup: implemented
- Fully qualified name resolution: implemented
- Node registry: data model exists, single self-entry only
- Database migration: not implemented (data model supports it)
- Proxy/redirect routing: not implemented (single-node only)
- Per-database Postgres schema isolation: deferred (use column-based
  isolation)

## References

- [ADR-003: Backing Store Architecture](ADR-003-backing-store-architecture.md)
- [ADR-010: Physical Storage and Secondary Indexes](ADR-010-physical-storage-and-secondary-indexes.md)
- [FEAT-012: Authorization](../../01-frame/features/FEAT-012-authorization.md)
- CockroachDB: Range placement and leaseholder migration
- Vitess: Tablet migration and topology service
- Snowflake: Account → Database → Schema hierarchy
