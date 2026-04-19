---
dun:
  id: ADR-012
  depends_on:
    - ADR-002
    - ADR-003
    - ADR-008
    - ADR-010
    - FEAT-002
    - FEAT-004
    - FEAT-009
    - FEAT-013
---
# ADR-012: GraphQL Query Layer (Auto-Generated from ESF)

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | ADR-002, ADR-010, FEAT-002, FEAT-004, FEAT-009, FEAT-013 | High |

## Context

Axon's current query interface is a structured API: `QueryEntitiesRequest`
with `FilterNode`, `SortField`, limit/cursor. This works for programmatic
use (agents, SDKs) but is:

- Verbose for complex queries involving related entities
- Unable to express "give me this entity and its dependencies and their
  statuses" in a single request
- Missing a subscription/change-feed protocol
- Not self-documenting for API consumers

The Entity Schema Format (ESF) already contains everything needed to
describe Axon's data model to external consumers: entity field types,
link types with target collections, lifecycle states, and cardinality
constraints. This is the same information a GraphQL schema encodes.

| Aspect | Description |
|--------|-------------|
| Problem | No declarative query language; no way to fetch related entities in one request; no subscription protocol for change feeds |
| Current State | Structured API with FilterNode/SortField. Single-collection queries only. No subscriptions |
| Requirements | Declarative read queries with relationship traversal, field selection, filtering, pagination, and real-time subscriptions |

## Decision

Add a **read-only GraphQL API** that is **auto-generated from ESF schemas
at runtime**. No hand-written `.graphql` files. The GraphQL schema is
derived from the active collection schemas and regenerated when schemas
change.

### 1. Schema Generation

When a collection schema is created or updated, the GraphQL schema is
regenerated. The mapping from ESF to GraphQL:

#### Entity Types

Each collection produces a GraphQL object type. Field names are converted
from snake_case/kebab-case to camelCase. JSON Schema types map to GraphQL
scalar types.

| JSON Schema type | GraphQL type |
|---|---|
| `string` | `String` |
| `string` with `enum` | Generated enum type |
| `string` with `format: date-time` | `DateTime` scalar |
| `integer` | `Int` |
| `number` | `Float` |
| `boolean` | `Boolean` |
| `array` of scalars | `[ScalarType!]` |
| `array` of objects | `[NestedType!]` |
| `object` (nested) | Generated nested type |

JSON Schema `required` fields map to non-nullable GraphQL fields (`!`).
Optional fields are nullable.

#### System Fields

Every entity type includes system fields:

```graphql
type Bead {
  id: ID!
  version: Int!
  createdAt: DateTime!
  updatedAt: DateTime!
  createdBy: String
  updatedBy: String
  # ... entity fields ...
}
```

#### Link Types as Relationship Fields

Each link type declared in the schema produces a relationship field on
the entity type. The field resolves by querying the links table.

```yaml
# ESF
link_types:
  depends-on:
    target_collection: beads
    cardinality: many-to-many
  parent-of:
    target_collection: beads
    cardinality: one-to-many
```

```graphql
# Generated GraphQL
type Bead {
  # Forward links
  dependsOn(limit: Int, after: String, filter: BeadFilter): BeadConnection!
  parentOf(limit: Int, after: String, filter: BeadFilter): BeadConnection!

  # Reverse links (auto-generated)
  dependsOnInbound(limit: Int, after: String, filter: BeadFilter): BeadConnection!
  parentOfInbound(limit: Int, after: String, filter: BeadFilter): BeadConnection!
}
```

Forward link field names are the link type name in camelCase. Reverse
link field names append `Inbound`. Cardinality informs the return type:
`one-to-one` returns a nullable single entity; `one-to-many` and
`many-to-many` return connections.

#### Lifecycle Fields

Each lifecycle declared in the schema produces a `validTransitions` field:

```graphql
type Bead {
  # From lifecycle declaration
  validTransitions: [BeadStatus!]!
}
```

This resolves by looking up the current `status` value in the lifecycle's
transition map.

#### Filter Input Types

Each entity type gets a generated filter input matching its indexed fields:

```graphql
input BeadFilter {
  # Fields with declared indexes (FEAT-013)
  status: StringFilter
  priority: IntFilter
  beadType: StringFilter
  claimedAt: DateTimeFilter

  # Compound
  and: [BeadFilter!]
  or: [BeadFilter!]
}

input StringFilter {
  eq: String
  ne: String
  in: [String!]
  contains: String
}

input IntFilter {
  eq: Int
  ne: Int
  gt: Int
  gte: Int
  lt: Int
  lte: Int
}

input DateTimeFilter {
  eq: DateTime
  gt: DateTime
  gte: DateTime
  lt: DateTime
  lte: DateTime
}
```

Non-indexed fields are also included in the filter input — the query
planner falls back to a scan for those. But indexed fields are marked
in the schema introspection so clients know which filters are fast.

#### Sort Input Types

```graphql
enum BeadSortField {
  CREATED_AT
  UPDATED_AT
  STATUS
  PRIORITY
  TITLE
}

input BeadSort {
  field: BeadSortField!
  direction: SortDirection = ASC
}

enum SortDirection { ASC, DESC }
```

#### Connection Types (Pagination)

All list fields use Relay-style cursor pagination:

```graphql
type BeadConnection {
  edges: [BeadEdge!]!
  pageInfo: PageInfo!
  totalCount: Int
}

type BeadEdge {
  node: Bead!
  cursor: String!
}

type PageInfo {
  hasNextPage: Boolean!
  hasPreviousPage: Boolean!
  startCursor: String
  endCursor: String
}
```

### 2. Root Query Type

```graphql
type Query {
  # Per-collection queries (one per registered collection)
  bead(id: ID!): Bead
  beads(filter: BeadFilter, sort: BeadSort, limit: Int, after: String): BeadConnection!

  # Generic collection access (for dynamic/unknown collections)
  entity(collection: String!, id: ID!): JSON
  entities(collection: String!, filter: JSON, sort: JSON, limit: Int, after: String): EntityConnection!

  # Schema introspection (Axon-specific, beyond GraphQL introspection)
  collections: [CollectionMeta!]!
  collection(name: String!): CollectionMeta

  # Audit log
  auditLog(
    collection: String
    entityId: ID
    actor: String
    mutation: String
    limit: Int
    after: String
  ): AuditConnection!
}

type CollectionMeta {
  name: String!
  schemaVersion: Int!
  entityCount: Int!
  schema: JSON!
  indexes: [IndexMeta!]!
  lifecycles: [LifecycleMeta!]!
}
```

Typed per-collection fields (`bead`, `beads`) are generated for each
registered collection. The generic `entity`/`entities` fields provide
access to any collection by name, returning untyped JSON — useful for
dynamic tooling.

### 3. Subscriptions (Change Feeds)

GraphQL subscriptions provide real-time change feeds backed by the audit
log:

```graphql
type Subscription {
  # Per-collection typed subscriptions
  beadChanged(filter: BeadFilter): BeadChangeEvent!

  # Generic subscription for any collection
  entityChanged(collection: String!, filter: JSON): ChangeEvent!
}

type BeadChangeEvent {
  mutation: MutationType!
  entity: Bead!
  previousVersion: Int
  actor: String
  timestamp: DateTime!
}

enum MutationType {
  CREATED
  UPDATED
  DELETED
}

type ChangeEvent {
  mutation: MutationType!
  collection: String!
  entityId: ID!
  data: JSON
  previousData: JSON
  actor: String
  timestamp: DateTime!
}
```

#### Subscription Implementation

Subscriptions use WebSocket (graphql-ws protocol) on `/graphql/ws`.

The backend implementation polls the audit log with a cursor (the last
seen audit log ID). When new entries appear, they are filtered against
active subscriptions and pushed to matching clients.

For V1, polling the audit log is sufficient (low latency at low scale).
A future optimization adds an in-process broadcast channel: the write
path publishes to the channel after committing, and the subscription
handler listens on the channel instead of polling.

```
Entity Write Path
    │
    ├── StorageAdapter.put()
    ├── AuditLog.append()
    └── Broadcast.send(ChangeEvent)  ← future optimization
            │
            ▼
Subscription Handler
    │
    ├── Filter against active subscriptions
    └── Push to matching WebSocket clients
```

### 4. Mutations

The GraphQL API includes full write operations. Mutations are
auto-generated from ESF schemas, just like query types.

#### Per-Collection Mutations

For a `beads` collection:

```graphql
type Mutation {
  # ── Entity CRUD ────────────────────────────────────────────

  createBead(input: CreateBeadInput!): CreateBeadPayload!
  updateBead(input: UpdateBeadInput!): UpdateBeadPayload!
  patchBead(input: PatchBeadInput!): PatchBeadPayload!
  deleteBead(input: DeleteBeadInput!): DeleteBeadPayload!

  # ── Links ──────────────────────────────────────────────────

  createBeadLink(input: CreateBeadLinkInput!): CreateBeadLinkPayload!
  deleteBeadLink(input: DeleteBeadLinkInput!): DeleteBeadLinkPayload!

  # ── Lifecycle transitions ──────────────────────────────────

  transitionBeadStatus(input: TransitionBeadStatusInput!): TransitionBeadStatusPayload!

  # ── Transactions (generic) ─────────────────────────────────

  commitTransaction(input: CommitTransactionInput!): CommitTransactionPayload!

  # ── Collection management ──────────────────────────────────

  createCollection(input: CreateCollectionInput!): CreateCollectionPayload!
  dropCollection(input: DropCollectionInput!): DropCollectionPayload!
  putSchema(input: PutSchemaInput!): PutSchemaPayload!
}
```

#### Entity CRUD Inputs and Payloads

```graphql
# ── Create ───────────────────────────────────────────────────

input CreateBeadInput {
  id: ID                        # optional — server generates UUIDv7
  data: CreateBeadDataInput!    # typed fields from entity_schema
  actor: String
}

input CreateBeadDataInput {
  beadType: BeadType!           # required fields are non-nullable
  title: String!
  status: BeadStatus            # optional — lifecycle default applies
  description: String
  priority: Int
  labels: [String!]
  owner: String
  assignee: String
  # ... all fields from entity_schema
}

type CreateBeadPayload {
  bead: Bead!                   # full entity with id, version, timestamps
}

# ── Update (full replacement) ────────────────────────────────

input UpdateBeadInput {
  id: ID!
  expectedVersion: Int!         # OCC — required
  data: UpdateBeadDataInput!    # all fields, same shape as create
  actor: String
}

type UpdateBeadPayload {
  bead: Bead!
}

# ── Patch (merge patch, RFC 7396) ────────────────────────────

input PatchBeadInput {
  id: ID!
  expectedVersion: Int!         # OCC — required
  patch: JSON!                  # RFC 7396 merge patch document
  actor: String
}

type PatchBeadPayload {
  bead: Bead!                   # entity after merge, or unchanged if no-op
  changed: Boolean!             # false if the patch was a no-op
}

# ── Delete ───────────────────────────────────────────────────

input DeleteBeadInput {
  id: ID!
  expectedVersion: Int          # optional — omit for unconditional delete
  force: Boolean = false        # force-delete removes links first
  actor: String
}

type DeleteBeadPayload {
  id: ID!
  success: Boolean!
}
```

#### OCC Version Handling

Every mutation that modifies an existing entity requires `expectedVersion`.
If the stored version doesn't match, the mutation returns a
**version conflict error** in the GraphQL `errors` array with an extension
containing the current entity state:

```json
{
  "errors": [{
    "message": "Version conflict: expected 5, found 7",
    "extensions": {
      "code": "VERSION_CONFLICT",
      "expectedVersion": 5,
      "currentVersion": 7,
      "currentEntity": { "id": "bead-42", "version": 7, "data": {...} }
    }
  }]
}
```

The client reads `currentEntity` from the error extension, merges its
changes, and retries with the correct `expectedVersion`. This is the same
pattern as the structured API, surfaced through GraphQL's standard error
model.

#### Merge Patch via JSON Scalar

Patch uses a `JSON` scalar for the merge patch document because RFC 7396
semantics (null = remove field) don't map to GraphQL input types, where
null means "not provided." Using `JSON` preserves the full RFC 7396
behavior:

```graphql
mutation {
  patchBead(input: {
    id: "bead-42"
    expectedVersion: 5
    patch: { status: "submitted", notes: null }
    actor: "agent-1"
  }) {
    bead { id version status }
    changed
  }
}
```

This sets `status` to "submitted" and removes `notes`.

#### Link Mutations

```graphql
input CreateBeadLinkInput {
  sourceId: ID!
  linkType: BeadLinkType!       # enum from schema link_types
  targetCollection: String!
  targetId: ID!
  metadata: JSON
}

type CreateBeadLinkPayload {
  link: Link!
}

input DeleteBeadLinkInput {
  sourceId: ID!
  linkType: BeadLinkType!
  targetCollection: String!
  targetId: ID!
}

type DeleteBeadLinkPayload {
  success: Boolean!
}

enum BeadLinkType {
  DEPENDS_ON
  PARENT_OF
}
```

#### Lifecycle Transition Mutations

When a collection has lifecycle declarations, a transition mutation is
generated:

```graphql
input TransitionBeadStatusInput {
  id: ID!
  to: BeadStatus!               # target state
  expectedVersion: Int!
  actor: String
}

type TransitionBeadStatusPayload {
  bead: Bead!
  previousStatus: BeadStatus!
}
```

The mutation validates that the transition is allowed per the lifecycle
definition. Invalid transitions return an error with the current state
and list of valid target states:

```json
{
  "errors": [{
    "message": "Invalid transition: cannot move from 'done' to 'draft'",
    "extensions": {
      "code": "INVALID_TRANSITION",
      "currentState": "done",
      "attemptedState": "draft",
      "validTransitions": []
    }
  }]
}
```

#### Transaction Mutations

Multi-entity atomic operations use a generic transaction mutation:

```graphql
input CommitTransactionInput {
  idempotencyKey: String
  operations: [TransactionOp!]!
  actor: String
}

input TransactionOp {
  # Exactly one of these must be set
  createEntity: CreateEntityOpInput
  updateEntity: UpdateEntityOpInput
  patchEntity: PatchEntityOpInput
  deleteEntity: DeleteEntityOpInput
  createLink: CreateLinkOpInput
  deleteLink: DeleteLinkOpInput
}

input CreateEntityOpInput {
  collection: String!
  id: ID
  data: JSON!
}

input UpdateEntityOpInput {
  collection: String!
  id: ID!
  expectedVersion: Int!
  data: JSON!
}

input PatchEntityOpInput {
  collection: String!
  id: ID!
  expectedVersion: Int!
  patch: JSON!
}

input DeleteEntityOpInput {
  collection: String!
  id: ID!
  expectedVersion: Int
  force: Boolean = false
}

input CreateLinkOpInput {
  sourceCollection: String!
  sourceId: ID!
  linkType: String!
  targetCollection: String!
  targetId: ID!
  metadata: JSON
}

input DeleteLinkOpInput {
  sourceCollection: String!
  sourceId: ID!
  linkType: String!
  targetCollection: String!
  targetId: ID!
}

type CommitTransactionPayload {
  transactionId: String!
  results: [TransactionOpResult!]!
}

type TransactionOpResult {
  index: Int!                   # position in the operations array
  entity: JSON                  # created/updated entity, if applicable
  success: Boolean!
}
```

Transaction operations use `JSON` scalars for data because they can
span multiple collections with different schemas. All operations commit
atomically — if any fails, none are applied.

`idempotencyKey` is the canonical retry key for transaction mutations. It is a
field on the mutation input, not an HTTP header. REST transaction compatibility
endpoints use the same body field as `idempotency_key`. Empty transaction
inputs follow FEAT-008: `operations: []` commits as a no-op and writes no audit
entry.

#### Collection Management Mutations

```graphql
input CreateCollectionInput {
  name: String!
  schema: JSON!                 # ESF schema document
}

type CreateCollectionPayload {
  collection: CollectionMeta!
}

input DropCollectionInput {
  name: String!
  confirm: Boolean!             # must be true
}

type DropCollectionPayload {
  name: String!
  success: Boolean!
}

input PutSchemaInput {
  collection: String!
  schema: JSON!
}

type PutSchemaPayload {
  collection: CollectionMeta!
  version: Int!
}
```

### 5. Execution Engine

#### Resolver Architecture

Each generated field has a resolver that maps to existing Axon operations:

| GraphQL operation | Axon operation |
|---|---|
| `bead(id: "...")` | `get_entity(collection, id)` |
| `beads(filter: ..., sort: ...)` | `query_entities(QueryEntitiesRequest)` |
| `bead.dependsOn(...)` | Query links table: `(source_collection, source_id, "depends-on")` → target entity IDs → batch `get_entity` |
| `bead.dependsOnInbound(...)` | Query links table via target index: `(target_collection, target_id, "depends-on")` → source entity IDs |
| `bead.validTransitions` | Lookup current status in lifecycle transition map |
| `bead.auditLog(...)` | `query_audit(collection, entity_id, ...)` |
| `createBead(input: ...)` | `create_entity(CreateEntityRequest)` |
| `updateBead(input: ...)` | `update_entity(UpdateEntityRequest)` |
| `patchBead(input: ...)` | `patch_entity(PatchEntityRequest)` |
| `deleteBead(input: ...)` | `delete_entity(DeleteEntityRequest)` |
| `createBeadLink(input: ...)` | `create_link(CreateLinkRequest)` |
| `deleteBeadLink(input: ...)` | `delete_link(DeleteLinkRequest)` |
| `transitionBeadStatus(input: ...)` | `patch_entity` with lifecycle validation |
| `commitTransaction(input: ...)` | `begin_tx` + operations + `commit_tx` |

#### N+1 Prevention (DataLoader)

Relationship fields use the DataLoader pattern to batch entity fetches:

1. GraphQL engine resolves the parent entity
2. Relationship field resolver registers the needed link query with a
   DataLoader
3. After the parent batch completes, DataLoader fires a single batched
   link query + entity fetch
4. Results are distributed to the individual resolvers

This prevents N+1 queries when fetching `beads { dependsOn { title } }`
for a list of beads.

#### Index Integration

Filter arguments in GraphQL map to `FilterNode` in `QueryEntitiesRequest`.
The query planner from ADR-010/FEAT-013 routes to secondary indexes when
available. From the GraphQL consumer's perspective, all filters work —
indexed filters are just faster.

### 6. Server Integration

The axon-server binary gains GraphQL endpoints:

```
POST /graphql          — GraphQL queries and mutations (HTTP)
GET  /graphql          — GraphQL Playground (dev mode only)
WS   /graphql/ws       — GraphQL subscriptions (WebSocket)
```

These are served by axum alongside the existing REST gateway and gRPC
service. All three protocols (REST, gRPC, GraphQL) share the same
`AxonHandler` instance.

### 7. Schema Regeneration

When a collection schema changes (via `put_schema`):

1. The handler updates the stored schema (ADR-007 versioning)
2. The GraphQL schema generator is invoked
3. The new GraphQL schema replaces the active one (atomic swap)
4. Existing WebSocket subscriptions are unaffected (they reference
   collection/field names, not the schema object)
5. The next GraphQL request uses the new schema

Schema regeneration is synchronous and fast — it's string manipulation,
not compilation. A schema with 20 collections and 100 total fields
regenerates in < 1ms.

### 8. Crate and Dependencies

- **`async-graphql`** (v7.x): Mature Rust GraphQL library with dynamic
  schema support, DataLoader, subscriptions, and axum integration
- **`async-graphql-axum`**: axum integration for HTTP and WebSocket
- Implementation lives in `crates/axon-graphql/` — a new workspace crate
  that depends on `axon-core`, `axon-schema`, and `axon-api`

```
crates/
  axon-graphql/    # GraphQL schema generation + resolvers
    src/
      schema.rs    # ESF → GraphQL schema generator
      resolvers.rs # Entity, link, audit resolvers
      loader.rs    # DataLoader for batched entity/link fetches
      subscriptions.rs  # Audit-backed change feed
```

## Example

Given the beads collection with its ESF schema, a single GraphQL query
can express what currently requires multiple structured API calls:

```graphql
query {
  beads(filter: { status: { eq: "in_progress" } }, sort: { field: PRIORITY, direction: DESC }, limit: 10) {
    edges {
      node {
        id
        title
        status
        priority
        assignee
        validTransitions
        dependsOn(limit: 5) {
          edges {
            node {
              id
              title
              status
            }
          }
        }
        auditLog(limit: 3) {
          edges {
            node {
              mutation
              actor
              timestamp
            }
          }
        }
      }
    }
    pageInfo {
      hasNextPage
      endCursor
    }
    totalCount
  }
}
```

This returns the top 10 in-progress beads by priority, with their
dependencies, valid transitions, and recent audit history — in one
request.

A mutation transitioning a bead and creating a link in one request:

```graphql
mutation {
  transitionBeadStatus(input: {
    id: "bead-42"
    to: IN_PROGRESS
    expectedVersion: 3
    actor: "agent-1"
  }) {
    bead { id version status }
    previousStatus
  }

  createBeadLink(input: {
    sourceId: "bead-42"
    linkType: DEPENDS_ON
    targetCollection: "beads"
    targetId: "bead-99"
  }) {
    link { sourceId targetId linkType }
  }
}
```

An atomic transaction across collections:

```graphql
mutation {
  commitTransaction(input: {
    idempotencyKey: "billing-agent-transfer-2026-04-19T20:45Z"
    actor: "billing-agent"
    operations: [
      { updateEntity: {
          collection: "accounts"
          id: "acct-A"
          expectedVersion: 5
          data: { balance: 900 }
      }},
      { updateEntity: {
          collection: "accounts"
          id: "acct-B"
          expectedVersion: 12
          data: { balance: 1100 }
      }},
      { createEntity: {
          collection: "ledger"
          data: { type: "transfer", from: "acct-A", to: "acct-B", amount: 100 }
      }}
    ]
  }) {
    transactionId
    results { index success entity }
  }
}
```

## Consequences

**Positive**:
- Zero-maintenance GraphQL API derived from ESF schemas
- Relationship traversal in a single request (no N+1 from the client)
- Self-documenting API via GraphQL introspection
- Subscriptions provide change feeds without a separate protocol
- Admin UI (SvelteKit) gets a typed, ergonomic data layer
- Field selection reduces payload for bandwidth-sensitive clients
- Filter/sort map to existing secondary indexes (FEAT-013)

**Negative**:
- New dependency (`async-graphql`) and new crate (`axon-graphql`)
- Dynamic schema generation adds complexity vs. static schemas
- Subscription implementation requires WebSocket support and audit log
  polling
- GraphQL query depth/complexity limits needed to prevent abusive queries
  (e.g., deeply nested relationship traversals)
- Merge patch uses `JSON` scalar, losing GraphQL type safety for patch
  inputs (unavoidable — RFC 7396 null semantics conflict with GraphQL
  null semantics)
- Transaction mutations use `JSON` for data (cross-collection operations
  can't use typed inputs)

**Deferred**:
- Cypher graph query language (not scheduled)
- SQL DML for batch operations (not scheduled)
- Vector similarity search (`nearest` filter) (not scheduled)
- Full-text search (`match` filter) (not scheduled)

When deferred capabilities are implemented, they integrate as new filter
types in the GraphQL schema (e.g., `filter: { embedding: { near: {...} } }`)
or as new query root fields. The GraphQL layer is extensible without
breaking existing queries.

## References

- [ADR-002: Schema Format](ADR-002-schema-format.md)
- [ADR-010: Physical Storage and Secondary Indexes](ADR-010-physical-storage-and-secondary-indexes.md)
- [FEAT-002: Schema Engine](../../01-frame/features/FEAT-002-schema-engine.md)
- [FEAT-004: Entity Operations](../../01-frame/features/FEAT-004-entity-operations.md)
- [FEAT-009: Graph Traversal Queries](../../01-frame/features/FEAT-009-graph-traversal-queries.md)
- [FEAT-013: Secondary Indexes](../../01-frame/features/FEAT-013-secondary-indexes.md)
- [async-graphql crate](https://crates.io/crates/async-graphql)
- [Relay Cursor Connections Spec](https://relay.dev/graphql/connections.htm)
- [graphql-ws Protocol](https://github.com/enisdenjo/graphql-ws/blob/master/PROTOCOL.md)
