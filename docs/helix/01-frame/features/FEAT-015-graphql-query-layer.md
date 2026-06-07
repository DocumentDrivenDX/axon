---
ddx:
  id: FEAT-015
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-004
    - FEAT-005
    - FEAT-009
    - FEAT-013
    - ADR-012
---
# Feature Specification: FEAT-015 - GraphQL Query Layer

**Feature ID**: FEAT-015
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-06-06

## Overview

A full read/write GraphQL API auto-generated from Entity Schema Format (ESF)
declarations. Entity types, relationship fields, filter/sort inputs,
mutations, policy metadata, mutation-intent workflows, and Relay-style
pagination are derived from the active collection schemas at runtime.
WebSocket subscriptions provide real-time change feeds backed by the audit log.

GraphQL is Axon's primary application API surface. MCP mirrors the same
semantics for agents. REST/JSON endpoints remain compatibility and operational
fallbacks for cases where GraphQL is genuinely intractable.

GraphQL also carries the first-class discoverability contract for application
developers: generated types and metadata expose schema shape, policy envelopes,
redactions, approval requirements, stale/conflict causes, and audit references
that MCP, SDK, CLI, and operator UI surfaces must preserve.

See [ADR-012](../../02-design/adr/ADR-012-graphql-query-layer.md) for
the full design.

## Problem Statement

Agents and the admin UI need to fetch entities with their relationships
in a single request. The current structured API requires multiple calls
to traverse links and assemble related data. There is no subscription
protocol for change feeds — clients must poll the audit log.

GraphQL solves both: declarative queries with nested relationship
resolution, mutations that share the same type surface, and subscriptions for
push-based change notification. Because GraphQL is also the primary policy
surface, resolver correctness under redaction, row filtering, relationship
traversal, and pagination is a V1 proof point.

## Requirements

### Functional Requirements

#### Schema Generation

- **Auto-generated from ESF**: Each registered collection produces
  GraphQL types. No hand-written `.graphql` files
- **Regenerated on schema change**: When `put_schema` is called, the
  GraphQL schema is regenerated and swapped atomically
- **JSON Schema → GraphQL type mapping**: string→String, integer→Int,
  number→Float, boolean→Boolean, enum→generated enum, nested objects→
  nested types, arrays→list types
- **System fields**: Every entity type includes `id`, `version`,
  `createdAt`, `updatedAt`, `createdBy`, `updatedBy`
- **Interface metadata**: Generated schema metadata includes policy version,
  schema version, redactable fields, approval-routed operations, autonomous
  write envelopes, and supported audit/change cursor fields for each
  collection

#### Relationship Fields

- **Forward links**: Each link type declared in the schema produces a
  relationship field. `depends-on` targeting `beads` → `dependsOn: BeadConnection!`
- **Reverse links**: Auto-generated for every link type. `depends-on` →
  `dependsOnInbound: BeadConnection!`
- **Filtering on relationships**: Relationship fields accept filter
  arguments to narrow results
- **DataLoader batching**: Relationship resolution uses DataLoader to
  prevent N+1 queries

#### Queries

- **Per-collection typed queries**: `bead(id: ID!)` and
  `beads(filter, sort, limit, after)` for each collection
- **Generic queries**: `entity(collection, id)` and
  `entities(collection, filter, ...)` returning untyped JSON
- **Collection introspection**: `collections` and `collection(name)` for
  metadata, schema, indexes, lifecycles
- **Audit log**: `auditLog(collection, entityId, actor, mutation, ...)`
  returns audit references and supports cursor-based pagination/resume
- **Relay pagination**: All list fields return Connection types with
  edges, pageInfo, and totalCount
- **Policy-safe pagination**: Row policies are applied before edges,
  cursors, and `totalCount` are constructed

#### Filters and Sorting

- **Filter inputs**: Generated per entity type. Fields with declared
  secondary indexes (FEAT-013) are included, plus non-indexed fields
  (which fall back to scan)
- **Filter operators**: eq, ne, gt, gte, lt, lte, in, contains
- **Compound filters**: `and` / `or` arrays
- **Sort inputs**: Generated per entity type with field enum and
  direction (ASC/DESC)

#### Mutations

- **Entity CRUD**: `createBead`, `updateBead`, `patchBead`, `deleteBead`
  mutations generated per collection. Create and update use typed input
  types from ESF. Patch uses a `JSON` scalar for RFC 7396 merge patch
- **OCC on mutations**: Update, patch, and delete mutations require
  `expectedVersion`. Version conflicts return a structured GraphQL error
  with the current entity state in error extensions
- **Link mutations**: `createBeadLink`, `deleteBeadLink` per collection.
  Link type constrained to enum from schema link_types
- **Lifecycle transitions**: `transitionBeadStatus` mutation with
  lifecycle validation. Invalid transitions return error with valid
  target states
- **Transactions**: `commitTransaction` mutation accepts a list of
  operations across collections. All-or-nothing atomic commit. Uses
  `JSON` scalars for cross-collection data
- **Collection management**: `createCollection`, `dropCollection`,
  `putSchema` mutations for admin operations
- **Safe write default**: Generated write documentation and SDK generation
  prefer `previewMutation` plus `commitMutationIntent` for approval-routed
  operations. A direct mutation that policy classifies as `needs_approval`
  returns an approval-required result and does not commit

#### Policy And Mutation Intents

- **Effective policy**: `effectivePolicy(collection, entityId)` exposes the
  caller's current collection/entity capabilities for UI and SDK affordances
- **Policy explanation**: `explainPolicy(input)` returns allow, deny, or
  needs-approval decisions with rule names and denied/redacted field paths
- **Mutation preview**: `previewMutation(input)` validates a proposed write,
  returns a diff and policy explanation, and creates a bound intent token when
  allowed or approval-routed
- **Approval workflow**: `approveMutationIntent`, `rejectMutationIntent`, and
  `commitMutationIntent` expose FEAT-030 through GraphQL
- **Policy envelope metadata**: GraphQL exposes enough metadata for SDKs and
  UIs to distinguish autonomous, approval-routed, and denied operations before
  attempting a commit
- **Redaction-aware types**: Any field that can be redacted by FEAT-029 is
  nullable in the generated GraphQL type, even if it is required in ESF

#### Policy-Safe Relationship Resolution

- **No hidden target leaks**: Relationship fields omit hidden target entities
  rather than returning policy errors
- **Target policy reuse**: Relationship predicates can reuse the target
  collection's read policy without duplicating membership rules
- **Count safety**: `totalCount` never includes hidden rows
- **Error safety**: Policy denials for hidden rows are indistinguishable from
  not-found/null results where existence would otherwise leak

#### Subscriptions (Change Feeds)

- **Per-collection subscriptions**: `beadChanged(filter)` pushes events
  when matching entities are created, updated, or deleted
- **Generic subscription**: `entityChanged(collection, filter)` for any
  collection
- **Event shape**: mutation type, entity data, previous version, actor,
  timestamp, audit cursor, and transaction ID
- **WebSocket transport**: graphql-ws protocol on `/graphql/ws`
- **Backed by audit log**: Subscription handler polls audit log (V1) or
  listens on broadcast channel (future optimization)
- **Resume guidance**: Subscription events carry the audit cursor needed to
  resume through `auditLog(after: ...)` after disconnect

#### Lifecycle Fields

- **Valid transitions**: Each entity type with a lifecycle declaration
  exposes `validTransitions` returning the list of states reachable from
  the current state

### Non-Functional Requirements

- **Schema generation**: < 1ms for 20 collections with 100 total fields
- **Query latency**: GraphQL overhead < 2ms above the underlying Axon
  operation latency
- **Query depth limit**: Default max depth of 10 nested levels (prevents
  abusive recursive queries)
- **Query complexity limit**: Configurable max complexity score based on
  field weights
- **Policy correctness**: Policy filtering, redaction, relationship traversal,
  and pagination must be tested against realistic business schemas before V1
- **Interface parity**: Generated GraphQL metadata must match MCP tool
  metadata for policy envelopes, approval requirements, redactions,
  stale/conflict fields, and audit references
- **Subscription latency**: < 500ms from entity write to subscriber
  notification (polling interval)

## User Stories

### Story US-048: Query Entities with Relationships [FEAT-015]

**As an** agent
**I want** to fetch entities and their related entities in one request
**So that** I can understand the full context without multiple API calls

**Acceptance Criteria:**
- [ ] A GraphQL query fetching a bead with its `dependsOn` relationships
  returns the bead and its dependencies in one response
- [ ] Nested relationship queries work to arbitrary depth (within limits)
- [ ] Filter and sort arguments work on relationship fields
- [ ] Total count is available on connection types
- [ ] Invalid filter argument returns a GraphQL error with field path and expected type

### Story US-049: Discover the API via Introspection [FEAT-015]

**As a** developer integrating with Axon
**I want** the GraphQL schema to reflect the current collection schemas
**So that** I can use GraphQL tooling to explore and query the API

**Acceptance Criteria:**
- [ ] GraphQL introspection returns types for all registered collections
- [ ] Adding a new collection immediately makes its type available
- [ ] Modifying a schema updates the GraphQL type definition
- [ ] Introspection or collection metadata exposes policy envelopes,
  redactable fields, approval-routed operations, schema/policy versions, and
  audit cursor support
- [ ] GraphQL Playground is available at `/graphql` in dev mode
- [ ] GraphQL introspection query returns schema for all collections in < 100ms

### Story US-050: Subscribe to Entity Changes [FEAT-015]

**As an** agent
**I want** to receive notifications when entities I care about change
**So that** I can react to state changes without polling

**Acceptance Criteria:**
- [ ] A WebSocket subscription to `beadChanged` receives events when
  beads are created, updated, or deleted
- [ ] Filter argument narrows which changes are pushed (e.g., only
  `status = "blocked"`)
- [ ] Events include the mutation type, new entity data, and actor
- [ ] Events include an audit cursor that can be used with `auditLog(after:)`
  to resume after reconnect
- [ ] Multiple concurrent subscriptions work independently
- [ ] If a collection is dropped during an active subscription, the subscription closes with an error event

### Story US-057: Mutate Entities via GraphQL [FEAT-015]

**As an** agent or UI client
**I want** to create, update, patch, and delete entities via GraphQL
**So that** I can use a single API for both reads and writes

**Acceptance Criteria:**
- [ ] `createBead` mutation creates an entity and returns it with ID
  and version
- [ ] `updateBead` with correct `expectedVersion` succeeds
- [ ] `updateBead` with wrong `expectedVersion` returns a version
  conflict error with the current entity state
- [ ] `patchBead` with a JSON merge patch modifies only specified fields
- [ ] `deleteBead` removes the entity
- [ ] `transitionBeadStatus` validates lifecycle transitions
- [ ] `commitTransaction` atomically commits multiple operations
- [ ] `commitTransaction` with multiple operations either commits all or rolls back all; partial success is impossible
- [ ] Version conflict error includes current entity state in GraphQL error extensions
- [ ] A direct generated mutation that receives `needs_approval` from policy
  returns an approval-required result and does not mutate entity/link state

### Story US-110: Enforce Policy Across GraphQL Traversal [FEAT-015]

**As an** application developer
**I want** GraphQL queries to enforce row and field policies across nested
relationships and pagination
**So that** direct GraphQL access cannot leak hidden business records

**Acceptance Criteria:**
- [ ] A denied point read resolves to `null` without revealing hidden existence
- [ ] Connection edges and `totalCount` are computed after FEAT-029 row filters
- [ ] Redactable fields are nullable in generated GraphQL types and resolve to
  `null` when denied
- [ ] Nested relationship fields omit hidden targets and do not leak counts
- [ ] Policy explanations are available through GraphQL without weakening
  enforcement on the real operation

### Story US-111: Preview And Commit Mutation Intents [FEAT-015]

**As an** agent or UI client
**I want** GraphQL to preview, approve, and commit mutation intents
**So that** governed writes use one primary API surface

**Acceptance Criteria:**
- [ ] `previewMutation` returns diff, policy decision, pre-image versions, and
  intent token when applicable
- [ ] `approveMutationIntent` and `rejectMutationIntent` audit operator action
- [ ] `commitMutationIntent` rejects stale entity versions, stale policy
  versions, and operation hash mismatches
- [ ] The committed mutation audit entry links to the approved intent
- [ ] Preview, stale, conflict, approval-required, and committed responses
  expose stable machine-readable fields that SDKs and MCP tools preserve

### Story US-051: Use GraphQL from the Admin UI [FEAT-015]

**As the** admin web UI
**I want** GraphQL endpoints for tenant data-plane and control-plane workflows
**So that** I can build efficient, type-safe data views and mutations

**Acceptance Criteria:**
- [x] The SvelteKit admin UI fetches collection data via GraphQL
- [x] Collection list view uses the `collections` query
- [x] Filtering and pagination work through GraphQL filter inputs
- [x] Entity create/read/update/delete, links, lifecycle transitions, entity
  rollback, audit revert, markdown template CRUD/rendering, and schema/
  collection admin flows use tenant-scoped GraphQL in the native UI
- [x] Tenant, user, tenant member, credential, and database control-plane UI
  flows use `/control/graphql`
- [ ] Entity detail view uses one consolidated GraphQL query for entity +
  links + recent audit where practical
- [ ] Admin UI entity detail query (entity + links + recent audit) completes in < 200ms p99

### Story US-078: JSON-LD Content Negotiation [FEAT-015]

**As a** linked-data-aware client (an external integrator, a knowledge-graph
tool, or any consumer that wants standardized provenance/context)
**I want** to request entity payloads as JSON-LD via content negotiation
**So that** I can consume Axon data with `@context`, `@id`, and `@type`
without bespoke translation

**Background:** ADR-020 selected document-shaped storage with selective RDF
concept adoption. Entity URLs are dereferenceable IRIs (per technical-
requirements.md §4c). JSON-LD adoption is additive: the canonical surface
remains plain JSON; JSON-LD is available on request.

**Acceptance Criteria:**
- [ ] A request with `Accept: application/ld+json` to a GraphQL entity
  query returns the response body as JSON-LD with a generated `@context`,
  `@id` set to the canonical entity URL
  (`/tenants/{t}/databases/{d}/collections/{c}/entities/{id}`), and
  `@type` derived from the collection schema.
- [ ] The default `Accept: application/json` (and any unspecified Accept)
  returns the existing JSON response shape unchanged.
- [ ] The `@context` is generated from the active ESF schema. Field names
  that collide with JSON-LD reserved keywords (`@id`, `@type`, `@graph`,
  `@context`, etc.) are remapped via `@context` aliases; the schema
  validator (FEAT-002) emits a warning when a collision is detected at
  schema-write time.
- [ ] Linked entities (relationship traversals) render as nested
  `@id`-bearing nodes; clients can dereference the `@id` to fetch the
  full entity.
- [ ] Validates against a JSON-LD 1.1 processor (e.g., `jsonld.js`,
  `pyld`).
- [ ] No performance regression on the plain-JSON path.

## Edge Cases

- **Empty collection**: GraphQL type is generated but queries return
  empty connections
- **Schema with no link types**: Entity type has only scalar fields, no
  relationship fields
- **Collection with no schema**: Uses the generic `entity`/`entities`
  query returning JSON. No typed query is generated
- **Deeply nested query**: Depth limit (default 10) rejects queries
  exceeding the maximum with a clear error
- **Subscription to dropped collection**: Subscription ends with an
  error event. Client must resubscribe
- **Concurrent schema change during query**: In-flight queries use the
  schema version that was active when the query started. No mid-query
  schema change
- **Large result sets**: Pagination is mandatory for list fields.
  Default limit applies if none specified
- **Policy changes during query**: In-flight queries use the policy snapshot
  active when execution starts
- **Policy changes during intent approval**: FEAT-030 marks the intent stale
  and requires preview again

## Dependencies

- **FEAT-002** (Schema Engine): ESF provides the source for GraphQL
  schema generation
- **FEAT-004** (Entity Operations): GraphQL resolvers delegate to
  existing entity operations
- **FEAT-005** (API Surface): GraphQL endpoint served by the shared server
- **FEAT-009** (Graph Traversal): Relationship field resolution uses
  link traversal
- **FEAT-013** (Secondary Indexes): Filter arguments route through the
  query planner to use indexes
- **FEAT-029** (Data-Layer Access Control Policies): GraphQL enforces row
  filters, field redaction, policy explanation, and safe pagination
- **FEAT-030** (Mutation Intents and Approval): GraphQL exposes preview,
  approval, and intent commit workflows
- **ADR-012**: Full design for schema generation, resolvers, subscriptions

### Crate Dependencies

- `async-graphql` v7.x — GraphQL execution engine with dynamic schema
- `async-graphql-axum` — axum integration for HTTP and WebSocket

## Out of Scope

- **Schema stitching / federation**: Single Axon instance only
- **Persisted queries**: Client-sent query strings only. No server-side
  query storage
- **Custom resolvers / computed fields**: All fields derive from ESF.
  No user-defined resolvers
- **Cypher integration**: Graph pattern matching language (not scheduled)
- **SQL integration**: SQL query frontend (not scheduled)
- **Vector similarity filter**: `near` filter for semantic search (not
  scheduled)
- **Full-text filter**: `match` filter for document search (not scheduled)

## Traceability

### Related Artifacts
- **Parent PRD Section**: P0 #7 safe, discoverable interface parity; FR-20,
  FR-28, FR-29, and FR-31
- **User Stories**: US-048, US-049, US-050, US-051, US-057, US-110, US-111
- **Architecture**: ADR-012 (GraphQL Query Layer)
- **Implementation**: `crates/axon-graphql/`

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-004, FEAT-005, FEAT-009, FEAT-013
- **Depended By**: FEAT-011 (Admin UI uses GraphQL for data fetching),
  FEAT-016 (MCP GraphQL bridge), FEAT-029 (policy enforcement), FEAT-030
  (mutation intents)
