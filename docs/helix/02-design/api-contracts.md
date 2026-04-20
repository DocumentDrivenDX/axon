# Axon Browser API Contracts

This document pins the HTTP and GraphQL surface a static browser bundle can call
directly over Tailscale.

## Interface Policy

GraphQL is Axon's primary documented interface for end-user and developer
workflows. That includes single-entity CRUD, filtered lists, metadata
discovery, relationships, aggregations, audit reads, schema and collection
management, control-plane administration, atomic transactions, and the native
Axon UI. REST remains available only where it is demonstrably better than
GraphQL: health and metrics, static asset serving, streaming/file-oriented
transports, compatibility endpoints, and break-glass administrative recovery
operations.

The native Axon UI is the canary consumer for this policy. UI routes should use
GraphQL for tenant, user, database, collection, entity, audit, relationship,
lifecycle, schema, and credential workflows unless the API contract names a
specific REST-only exception.

REST-only exceptions for V1 are:

- `GET /health`, metrics, and static UI assets.
- Streaming or file-oriented endpoints where GraphQL request/response semantics
  are a poor fit.
- Transaction-level and point-in-time rollback break-glass operations until the
  GraphQL recovery surface is hardened.

Entity-level rollback is part of the GraphQL data-plane surface because it is a
normal application recovery workflow for the entity detail UI.

## CORS

Browser clients may call Axon cross-origin when the origin is allowed by
deployment policy. Production deployments use an explicit allowlist. Development
and `--no-auth` deployments may opt into a wildcard origin for local iteration.
An empty allowlist disables CORS response headers.

Preflight responses allow:

- Methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`
- Request headers: `authorization`, `content-type`, `x-axon-schema-hash`,
  `x-axon-actor`
- Credentials mode: browser clients use `credentials: "omit"` and send bearer
  credentials with `Authorization`; Axon does not require cookies for the
  browser API contract
- Max age: `86400` seconds

Browser-readable response headers include:

- `x-axon-schema-hash`
- `x-idempotent-cache`
- `x-request-id`
- `x-axon-query-cost` when query-cost reporting is enabled

`x-request-id` is emitted on every HTTP response. If the request supplied a
valid `x-request-id`, Axon echoes it; otherwise Axon generates a UUIDv7. Browser
clients should include this value in issue reports and support logs.

`x-axon-query-cost` is reserved for dev-mode query regression detection and is
included in `Access-Control-Expose-Headers`. Axon does not emit a query-cost
header in the V1 browser contract until cost accounting is enabled; clients
must treat the header as optional.

`idempotency-key` is not part of the canonical browser CORS request-header
contract. Use the transaction input/body `idempotency_key` field instead. The
legacy HTTP header may remain accepted for non-browser compatibility, but a
browser preflight must not depend on it being allowed.

Tailscale does not bypass browser same-origin enforcement. A Tailscale-hosted
SPA talking to a differently named Axon host still needs this CORS policy.

Example preflight from a static SPA:

```http
OPTIONS /tenants/acme/databases/default/graphql HTTP/1.1
Origin: https://nexiq.tailnet.example
Access-Control-Request-Method: POST
Access-Control-Request-Headers: content-type, authorization, x-axon-schema-hash
```

Allowed-origin response:

```http
HTTP/1.1 200 OK
Access-Control-Allow-Origin: https://nexiq.tailnet.example
Access-Control-Allow-Methods: GET, POST, PUT, PATCH, DELETE, OPTIONS
Access-Control-Allow-Headers: Content-Type, Authorization, X-Axon-Schema-Hash, X-Axon-Actor
Access-Control-Max-Age: 86400
Vary: Origin
```

Actual browser responses for allowed origins include:

```http
Access-Control-Allow-Origin: https://nexiq.tailnet.example
Access-Control-Expose-Headers: X-Idempotent-Cache, X-Axon-Schema-Hash, X-Request-Id, X-Axon-Query-Cost
X-Request-Id: 018f4f9c-7cb2-7b38-a9f1-77b16d6a2e2a
```

Schema-manifest responses also emit `x-axon-schema-hash`. Transaction replays
served from the idempotency cache emit `x-idempotent-cache: hit`.

## Current User

Use the GraphQL current-user query before choosing a tenant/database route.
During the compatibility window, `GET /auth/me` returns the same identity
envelope.

```http
GET /auth/me
```

```json
{
  "actor": "erik@example.com",
  "role": "admin",
  "user_id": null,
  "tenant_id": null
}
```

`actor` is the audit actor Axon resolved from the auth layer. Browser clients
should not send body-level `actor` fields for normal writes; route handlers use
the authenticated caller identity. JWT-authenticated control-plane callers also
receive `user_id` and `tenant_id`. Tailscale/no-auth/guest callers receive null
for those fields because those modes do not require a users-collection lookup.

## Control-Plane GraphQL

`POST /control/graphql` is the primary UI and SDK surface for deployment
administration. It uses the same `Authorization: Bearer <jwt>` credential model
as the REST control-plane routes and falls back to the legacy HTTP identity in
`--no-auth`/Tailscale compatibility modes.

Canonical queries:

```graphql
query($tenantId: String!) {
  tenants { id name dbName createdAt }
  tenant(id: $tenantId) { id name dbName createdAt }
  users { id displayName email createdAtMs suspendedAtMs }
  tenantMembers(tenantId: $tenantId) { tenantId userId role }
  tenantDatabases(tenantId: $tenantId) { tenantId name createdAtMs }
  credentials(tenantId: $tenantId) {
    jti
    userId
    tenantId
    issuedAtMs
    expiresAtMs
    revoked
    grants
  }
}
```

Canonical mutations:

```graphql
mutation($tenantId: String!, $userId: String!, $jti: String!) {
  createTenant(name: "Acme") { id name dbName dbPath createdAt }
  deleteTenant(id: $tenantId) { deleted tenantId dbName }
  provisionUser(displayName: "Ada", email: "ada@example.com") { id }
  suspendUser(userId: $userId) { userId suspended }
  upsertTenantMember(tenantId: $tenantId, userId: $userId, role: "write") {
    tenantId
    userId
    role
  }
  removeTenantMember(tenantId: $tenantId, userId: $userId) { deleted }
  createTenantDatabase(tenantId: $tenantId, name: "orders") { tenantId name }
  deleteTenantDatabase(tenantId: $tenantId, name: "orders") { deleted }
  revokeCredential(tenantId: $tenantId, jti: $jti) { jti revoked }
}
```

Credential issuance returns signed token material exactly once:

```graphql
mutation($tenantId: String!, $targetUser: String!) {
  issueCredential(
    tenantId: $tenantId
    targetUser: $targetUser
    grants: { databases: [{ name: "orders", ops: ["read", "write"] }] }
    ttlSeconds: 3600
  ) {
    jwt
    jti
    expiresAt
  }
}
```

`credentials` never returns `jwt` or other signed secret material. Deployment
admins can manage all tenants, users, memberships, databases, and credentials.
Tenant admins can list tenant members/databases, manage tenant databases, and
list/revoke tenant credentials. Regular credential holders can list and revoke
only their own credentials.

GraphQL control-plane errors use REST-compatible lower-case
`extensions.code` values such as `forbidden`, `not_found`, `already_exists`,
`invalid_identifier`, `invalid_role`, `not_a_tenant_member`,
`grants_exceed_role`, `invalid_jti`, `not_configured`, and `storage_error`.

REST control-plane routes remain for compatibility, operational scripts, and
break-glass administration. New browser and SDK flows should use GraphQL except
for health/metrics/static assets and streaming or file-oriented endpoints.

## Data-Layer Access Control

FEAT-029 policies are enforced below REST, GraphQL, and MCP. Browser code may
query effective policy metadata to hide controls, but the server repeats policy
checks during every read and write.

Read denial for hidden rows does not leak existence:

- REST point reads return `404` with `code: "not_found"`.
- GraphQL point reads resolve nullable entity fields to `null`.
- GraphQL list, relationship, traversal, and connection fields omit hidden
  rows before pagination and total-count calculation.

Field-level read denial returns `null` for the field in GraphQL, generic JSON,
REST, and audit read payloads. Any GraphQL field that can be policy-redacted is
generated as nullable even when JSON Schema marks it required.

Denied writes return `forbidden` with stable detail:

```json
{
  "code": "forbidden",
  "detail": {
    "reason": "field_write_denied",
    "collection": "engagements",
    "entity_id": "eng-1",
    "field_path": "status",
    "policy": "contractors-cannot-transition-engagements"
  }
}
```

GraphQL returns the same values under `extensions` using camelCase field names.
Stable denial reasons are `collection_read_denied`, `row_write_denied`,
`field_write_denied`, `policy_filter_unindexed`, and
`policy_expression_invalid`. Idempotent transaction requests cache terminal
`forbidden` responses for the idempotency TTL so replaying the same denied
write returns the same denial.

## Schema Handshake

Use the GraphQL metadata/schema query on app load once the comprehensive
GraphQL surface is available. During the compatibility window, the REST
handshake `GET /tenants/{tenant}/databases/{database}/schema` returns the same
schema hash, full collection schemas, and the header name clients can use to
assert compatibility.

```http
GET /tenants/default/databases/default/schema
```

```json
{
  "database": "default",
  "collections": [
    {
      "name": "time_entries",
      "version": 1,
      "entity_count": 0,
      "schema": {
        "collection": "time_entries",
        "version": 1,
        "entity_schema": {"type": "object"}
      }
    }
  ],
  "schema_hash": "fnv64:1b4c7d...",
  "expected_header": "x-axon-schema-hash",
  "compatibility": {
    "additive_changes": "compatible",
    "breaking_changes": "rejected_without_force",
    "client_policy": "static clients should compare schema_hash on app load and fail closed on mismatch"
  }
}
```

Clients may send `x-axon-schema-hash` or `?expected_hash=`. A mismatch returns
`409`:

```json
{
  "code": "schema_mismatch",
  "detail": {
    "expected": "fnv64:stale",
    "actual": "fnv64:1b4c7d...",
    "manifest": {}
  }
}
```

## Filtering

GraphQL list fields are canonical for filtering. REST collection queries remain
available as compatibility endpoints using `POST /collections/{collection}/query`
with the existing `FilterNode` JSON body:

```http
POST /tenants/default/databases/default/collections/time_entries/query
```

```json
{
  "filter": {
    "type": "and",
    "filters": [
      {"type": "field", "field": "status", "op": "eq", "value": "approved"},
      {"type": "field", "field": "week", "op": "eq", "value": "2026-W16"},
      {"type": "field", "field": "hours", "op": "gte", "value": 4}
    ]
  },
  "sort": [{"field": "hours", "direction": "desc"}],
  "limit": 50,
  "after_id": "time-123"
}
```

Generic GraphQL root queries are the preferred browser contract:

```graphql
{
  collections {
    name
    entityCount
    schemaVersion
    schema
  }

  collection(name: "time_entries") {
    name
    entityCount
    createdAt
    updatedAt
  }

  entity(collection: "time_entries", id: "time-123") {
    id
    collection
    version
    data
    createdAt
    updatedAt
  }

  entities(
    collection: "time_entries"
    filter: {
      and: [
        { field: "status", op: "eq", value: "approved" }
        { field: "week", op: "eq", value: "2026-W16" }
        { field: "hours", op: "gte", value: 4.0 }
      ]
    }
    sort: [{ field: "hours", direction: "desc" }]
    limit: 50
    after: "time-123"
  ) {
    totalCount
    edges {
      cursor
      node { id collection version data }
    }
    pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
  }
}
```

Generated typed fields are available for ergonomic collection-specific queries.
Each collection exposes typed filter and sort inputs: `<Type>Filter`,
`<Type>SortField`, and `<Type>Sort`. Field filters use scalar operator inputs
such as `AxonStringFilterInput` and `AxonIntFilterInput`; the legacy
`field/op/value` form remains accepted inside typed filters during the
compatibility window. Compatibility list fields return arrays; the Relay-style
aliases append `Connection`:

```graphql
{
  items(
    filter: {
      and: [
        { status: { eq: "approved" } }
        { week: { eq: "2026-W16" } }
        { hours: { gte: 4.0 } }
      ]
    }
    sort: [{ field: hours, direction: "desc" }]
    limit: 50
  ) {
    id
    version
    status
    hours
  }

  itemsConnection(limit: 50) {
    totalCount
    edges { cursor node { id version status hours } }
    pageInfo { hasNextPage endCursor }
  }
}
```

Supported GraphQL operators are `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in`,
`contains`, `isNull`, and `isNotNull`. Use `and` and `or` for boolean
composition. String fields support `contains`; numeric fields support ordering
operators.

### Search And `contains`

GraphQL filters and REST `FilterNode` bodies share the same `contains`
semantics:

- `contains` is case-sensitive.
- The stored field value and filter value must both be strings. Non-string
  fields do not match.
- Axon does not perform locale folding, Unicode normalization, stemming,
  tokenization, typo tolerance, or ranking.
- `contains` is a substring check, not full-text search.

FEAT-013 secondary indexes accelerate equality and range predicates on declared
single-field indexes. For `and` filters, the planner may use one indexed
equality predicate to narrow candidate IDs and then apply the remaining
predicates after fetch. `contains`, `ne`, `in`, `or`, and gate predicates are
not index-backed in the current planner and fall back to scanning the candidate
collection or candidate set.

For nexiq launch-scale browser search, use indexed filters first
(`engagement_id`, `status`, owner/assignee fields, date ranges), then apply
`contains` only on the narrowed result set. If case-insensitive search is
required, store a normalized lowercase search field and send a lowercase
`contains` value; this still uses scan semantics, so keep the indexed scope
small. Ranked multi-field full-text search is deferred; Axon does not expose an
FTS/tsvector search index in this contract.

Generated mutations expose typed input and payload types for each collection:
`Create<Type>Input/Payload`, `Update<Type>Input/Payload`,
`Patch<Type>Input/Payload`, and `Delete<Type>Input/Payload`. Required JSON
Schema fields become non-null in `Create<Type>Input` when GraphQL can represent
the field as a scalar; nested objects and arrays fall back to `JSON`. For
compatibility, create and update mutations retain an explicit `legacyInput:
JSON` argument for old JSON-string callers; new callers should use the
generated `input` object so invalid fields and scalar mismatches are rejected by
GraphQL validation:

```graphql
mutation {
  createTimeEntries(id: "time-123", input: {
    status: "draft"
    hours: 2.5
  }) {
    id
    version
    status
    entity { id status hours }
  }

  patchTimeEntries(id: "time-123", version: 1, typedInput: {
    patch: { status: "submitted" }
  }) {
    id
    version
    status
  }

  deleteTimeEntries(id: "time-123") {
    deleted
    id
  }
}
```

Patch remains JSON because RFC 7396 null-removal semantics require preserving
the difference between omitted fields and explicit `null`.

Generated schemas also expose per-collection aggregation queries named
`<collection>Aggregate`. Aggregations reuse the same typed filter inputs as list
queries and accept one or more aggregate functions in a single request. `COUNT`
counts matching entities; `SUM`, `AVG`, `MIN`, and `MAX` require a numeric
field. `groupBy` accepts one or more generated field enum values and returns
both a compact `key` and a `keyFields` object for multi-field groups:

```graphql
{
  timeEntriesAggregate(
    filter: { status: { eq: "approved" } }
    groupBy: [status, week]
    aggregations: [
      { function: COUNT }
      { function: SUM, field: hours }
      { function: AVG, field: hours }
    ]
  ) {
    totalCount
    groups {
      keyFields
      count
      values { function field value count }
    }
  }
}
```

Null or missing numeric values are excluded from numeric aggregates and reported
through each value's `count`; group `count` still reflects all matching entities
in the group. Empty collections return `totalCount: 0` and an empty `groups`
array. Invalid numeric aggregations, such as `SUM` on a string field, return a
GraphQL error with `extensions.code = "INVALID_ARGUMENT"` and
`extensions.category = "AGGREGATION"`.

Collection-to-GraphQL naming is deterministic:

- Generic root fields always take the stored collection name as a string and
  are authoritative for unusual names.
- Typed object names are PascalCase from ASCII alphanumeric words:
  `time_entries` and `time-entries` both map to `TimeEntries`.
- Typed singular fields are lower camelCase: `time_entries` maps to
  `timeEntries`.
- For simple singular names, typed list fields append `s`: `item` maps to
  `items`.
- Names already ending in `s`, names with separators, irregular plurals, and
  normalized names use `List`: `tasks` maps to `tasksList`,
  `time_entries` maps to `timeEntriesList`.
- Relay aliases append `Connection`: `itemsConnection`,
  `tasksListConnection`.
- Root field name collisions such as `entity`, `entities`, `collection`,
  `collections`, and `auditLog` append `Collection` for typed fields.
- Type name collisions with root and scalar types append `Record`.

All GraphQL requests are validated with server-configured depth and complexity
limits before resolver execution. Defaults are depth `10` and complexity `256`.
Operators may override them with `AXON_GRAPHQL_MAX_DEPTH` and
`AXON_GRAPHQL_MAX_COMPLEXITY`. Limit failures return a GraphQL `errors`
response.

## Audit Log

Use `auditLog` for application audit browsing. It supports collection, entity,
actor, operation, time range, cursor, and limit filters and returns the same
connection envelope as entity lists. `collection` is optional; actor and time
range filters may be used without a collection to troubleshoot activity across
the whole database.

```graphql
{
  auditLog(
    collection: "time_entries"
    entityId: "time-123"
    actor: "alice@example.com"
    operation: "entity.update"
    sinceNs: "1776672000000000000"
    untilNs: "1776758399999999999"
    after: "42"
    limit: 50
  ) {
    totalCount
    edges {
      cursor
      node {
        id
        timestampNs
        collection
        entityId
        mutation
        actor
        dataBefore
        dataAfter
        metadata
      }
    }
    pageInfo { hasNextPage hasPreviousPage endCursor }
  }
}
```

REST compatibility uses `GET /audit/query` with the same supported filter set:

- `collection` or comma-separated `collections`
- `entity_id`
- `actor`
- `operation`
- `since_ns` / `until_ns`
- `after_id`
- `limit`

`metadata.*` and `dataAfter`/`data_after.*` filters are not supported in V1.
REST returns:

```json
{
  "code": "unsupported_audit_filter",
  "detail": {
    "filter": "metadata.kind",
    "supported_filters": ["collection", "collections", "entity_id", "actor", "operation", "since_ns", "until_ns", "after_id", "limit"]
  }
}
```

GraphQL exposes `metadataPath`, `metadataEq`, `dataAfterPath`, and
`dataAfterEq` only to return a stable unsupported-filter error:

```json
{
  "errors": [
    {
      "message": "unsupported audit filter: metadataPath",
      "extensions": {
        "code": "UNSUPPORTED_AUDIT_FILTER",
        "filter": "metadataPath"
      }
    }
  ]
}
```

Nexiq status-history panels should query `auditLog` by entity, actor, mutation,
or time range, then filter `metadata.kind` or `dataAfter.status` client-side for
small result windows. Large metadata/data-field audit search is deferred until
Axon adds indexed audit payload filters.

## Audit Writes

Application-level audit events are written through the transaction API, not a
separate audit endpoint. The pinned contract is:

- GraphQL `commitTransaction(input:)` accepts `auditEvents`.
- REST `POST /transactions` accepts `audit_events`.
- `operations: []` with one or more audit events is a valid transaction.
- `operations: []` with no audit events remains the existing no-op.
- No `POST /audit/events` endpoint is canonical for V1.
- No standalone GraphQL `emitAuditEvent` mutation is canonical for V1.

This keeps idempotency, replay behavior, actor resolution, tenant/database
scoping, and write rate limiting identical to normal entity transactions.

GraphQL shape:

```graphql
mutation {
  commitTransaction(input: {
    idempotencyKey: "audit-access-denied-018f4f9c"
    operations: []
    auditEvents: [
      {
        eventKind: "access_denied"
        subjectRef: {
          collection: "engagements"
          id: "eng-1"
        }
        payloadJson: {
          route: "/engagements/eng-1/rate-card"
          attempted_action: "view_rate_card"
          caller_identity: "ada@example.com"
          reason: "missing_grant"
        }
        origin: {
          system: "nexiq"
          surface: "browser"
          route: "/engagements/:id/rate-card"
          request_id: "018f4f9c-7cb2-7b38-a9f1-77b16d6a2e2a"
        }
      }
    ]
  }) {
    transactionId
    auditEventIds
    replayHit
  }
}
```

REST shape:

```http
POST /tenants/default/databases/default/transactions
Authorization: Bearer eyJ...
Content-Type: application/json
```

```json
{
  "idempotency_key": "audit-login-attempted-018f4f9c",
  "operations": [],
  "audit_events": [
    {
      "event_kind": "login_attempted",
      "subject_ref": null,
      "payload_json": {
        "identifier": "ada@example.com",
        "result": "failed",
        "reason": "bad_password"
      },
      "origin": {
        "system": "nexiq",
        "surface": "integration_worker",
        "worker": "auth-sync",
        "request_id": "018f4f9c-7cb2-7b38-a9f1-77b16d6a2e2a"
      }
    }
  ]
}
```

Event schema:

- `event_kind`: required string, `^[a-z][a-z0-9_.-]{0,127}$`. Consumers may
  define new values. Axon reserves the `axon.` prefix for built-in events.
- `subject_ref`: optional object `{ collection, id }`. Use `null` for events
  not tied to a single entity, such as login attempts.
- `payload_json`: required JSON object. It is consumer-defined event data, not
  an entity mutation payload.
- `origin`: required JSON object describing the producer. Browser calls should
  include `system`, `surface`, `route`, and `request_id`; integration workers
  should include `system`, `surface`, `worker`, and `request_id`.
- `actor`: never accepted from the event body. Axon resolves actor from the
  authenticated request exactly as it does for entity writes.

Application audit events are stored in the same append-only audit log as entity
mutations and have the same retention and export guarantees. They are visible
through `auditLog` and REST `/audit/query`:

- `mutation` is `application.event`.
- `actor` is the authenticated actor.
- `metadata.kind` is the submitted `event_kind`.
- `metadata.origin` stores the submitted origin object.
- `dataAfter` stores `payload_json`.
- When `subject_ref` is present, `collection` and `entityId` match it.
- When `subject_ref` is null, `collection` is `application_events` and
  `entityId` is the server-assigned audit event id.

Idempotency and atomicity:

- `idempotency_key` covers both `operations` and `audit_events`.
- Replaying the same key after success returns the cached transaction response
  and does not append duplicate audit events.
- If any entity operation or audit event is invalid, the whole transaction
  aborts and no audit events are appended.
- A transaction with multiple audit events appends all of them with the same
  `transactionId`.

Rejection shapes:

```json
{
  "code": "rate_limit_exceeded",
  "detail": {
    "message": "write rate limit exceeded",
    "retry_after_seconds": 60,
    "scope": "actor_write"
  }
}
```

```json
{
  "code": "unknown_event_kind",
  "detail": {
    "event_kind": "axon.internal.override",
    "message": "event_kind uses a reserved prefix"
  }
}
```

```json
{
  "code": "missing_actor",
  "detail": {
    "message": "audit events require an authenticated actor"
  }
}
```

Browser clients should use the same route-scoped bearer token they use for
entity writes. Integration workers should use a service credential whose actor
is meaningful in audit history, for example `auth-sync@nexiq`.

## Collection, Schema, And Lifecycle GraphQL

GraphQL is the primary developer surface for collection and schema lifecycle
operations:

```graphql
mutation {
  createCollection(input: {
    name: "time_entries"
    schema: {
      version: 1
      entitySchema: {
        type: "object"
        required: ["status"]
        properties: { status: { type: "string" } }
      }
      lifecycles: {
        status: {
          field: "status"
          initial: "draft"
          transitions: { draft: ["submitted"], submitted: ["approved"] }
        }
      }
    }
  }) {
    name
    schemaVersion
    schema
  }
}
```

`putSchema(input: { collection, schema, force, dryRun })` returns `schema`,
`compatibility`, `diff`, and `dryRun`. Breaking changes without `force: true`
return a GraphQL error with `extensions.code: "INVALID_OPERATION"`.

`dropCollection(input: { name, confirm })` requires `confirm: true` and returns
`name` plus `entitiesRemoved`. REST collection/schema routes remain available as
compatibility and break-glass endpoints, but examples should prefer GraphQL.

Entity reads expose lifecycle metadata:

```graphql
{
  timeEntries(id: "time-1") {
    status
    lifecycles
    validTransitions(lifecycleName: "status")
  }
}
```

The GraphQL schema is rebuilt per request from stored collection schemas. A
request already executing continues against the schema it started with; the next
request observes a completed `createCollection`, `dropCollection`, or
`putSchema` change.

## Link Traversal

GraphQL relationship fields, `neighbors`, and `linkCandidates` are canonical
for application link traversal and discovery. REST traversal remains available
as a compatibility endpoint.

Declared `link_types` generate typed relationship fields on entity object
types. For a `users` link type named `assigned-to` targeting `tasks`, the
forward field is `assignedTo` on `User`; the reverse field is
`assignedToInbound` on `Task`. Relationship fields accept `limit`, `after`,
and typed `filter` arguments and return connection edges with the related node,
cursor, and link metadata.

```graphql
query {
  user(id: "u1") {
    assignedTo(filter: { status: { eq: "open" } }, limit: 10) {
      edges {
        cursor
        metadata
        node { id title status }
      }
      pageInfo { hasNextPage endCursor }
      totalCount
    }
  }

  task(id: "t1") {
    assignedToInbound {
      edges {
        metadata
        node { id name }
      }
    }
  }
}
```

Link discovery and autocomplete workflows should use the dedicated
`linkCandidates`/neighbor-discovery surface rather than overloading
relationship reads.

Candidate discovery is the browser autocomplete contract for creating links:

```graphql
query {
  linkCandidates(
    sourceCollection: "users"
    sourceId: "u1"
    linkType: "assigned-to"
    search: "invoice"
    filter: { status: { eq: "open" } }
    limit: 10
  ) {
    targetCollection
    linkType
    cardinality
    existingLinkCount
    candidates {
      alreadyLinked
      entity { id collection data }
    }
  }
}
```

Graph exploration uses `neighbors`, which returns inbound and outbound one-hop
neighbors grouped by link type and direction. Edges include link metadata and
the full source/target coordinates.

```graphql
query {
  neighbors(collection: "tasks", id: "task-1", direction: "outbound", limit: 50) {
    groups {
      linkType
      direction
      edges {
        cursor
        metadata
        node { id collection data }
      }
    }
    pageInfo { hasNextPage endCursor }
    totalCount
  }
}
```

Simple REST traversal:

```http
GET /tenants/default/databases/default/traverse/engagements/eng-1?link_type=has_phase&max_depth=2&direction=forward
```

Use `POST` for filtered traversal:

```http
POST /tenants/default/databases/default/traverse/engagements/eng-1
```

```json
{
  "link_type": "has_phase",
  "max_depth": 2,
  "direction": "forward",
  "hop_filter": {
    "type": "field",
    "field": "status",
    "op": "eq",
    "value": "approved"
  }
}
```

Responses include matched entities and flattened path hops with link metadata.

```json
{
  "entities": [{"collection": "phases", "id": "phase-1", "version": 1, "data": {}}],
  "paths": [
    {
      "source_collection": "engagements",
      "source_id": "eng-1",
      "target_collection": "phases",
      "target_id": "phase-1",
      "link_type": "has_phase",
      "metadata": {"order": 1}
    }
  ]
}
```

## Mutations, OCC, And Retries

Entity updates use optimistic concurrency through `expected_version`. Version
conflicts return `409` with the live version and entity:

```json
{
  "code": "version_conflict",
  "detail": {
    "expected": 2,
    "actual": 3,
    "current_entity": {}
  }
}
```

Use GraphQL `commitTransaction` for atomic multi-entity writes. The REST
`POST /transactions` endpoint remains a compatibility endpoint with the same
request body.

Safe network retries use `idempotency_key` in the transaction input/body. Axon
does not define an idempotency HTTP header as part of the canonical contract.
Successful transaction responses are cached for five minutes per tenant/database
and return `x-idempotent-cache: hit` on replay. Conflicts and validation
failures are not cached.

`idempotency_key` rules:

- Field name: `idempotency_key`
- Type: string
- Length: `1..128` characters
- Character set: ASCII `[A-Za-z0-9_.:-]`
- Scope: tenant plus database
- Case-sensitive
- Optional; absent means non-idempotent

If the same key is reused with a different payload after a successful commit,
the replay returns the original cached success until the TTL expires. Clients
must generate a fresh key per logical transaction.

Empty transactions follow FEAT-008: `operations: []` commits as a no-op, writes
no audit entry, and returns a successful empty result.

```http
POST /tenants/default/databases/default/transactions
```

```json
{
  "idempotency_key": "browser-retry-2026-04-19T15:00Z",
  "operations": [
    {
      "op": "update",
      "collection": "time_entries",
      "id": "time-1",
      "expected_version": 4,
      "data": {"status": "approved"}
    }
  ]
}
```

The GraphQL form uses the same field:

```graphql
mutation {
  commitTransaction(input: {
    idempotencyKey: "browser-retry-2026-04-19T15:00Z"
    operations: [
      { updateEntity: {
          collection: "time_entries"
          id: "time-1"
          expectedVersion: 4
          data: { status: "approved" }
      }}
    ]
  }) {
    transactionId
    results { index success entity }
  }
}
```

`CommitTransactionInput.operations` is a list of operation wrapper objects.
Exactly one field must be set per operation:

- `createEntity`: `{ collection, id, data }`
- `updateEntity`: `{ collection, id, expectedVersion, data }`
- `patchEntity`: `{ collection, id, expectedVersion, patch }`
- `deleteEntity`: `{ collection, id, expectedVersion }`
- `createLink`: `{ sourceCollection, sourceId, targetCollection, targetId, linkType, metadata }`
- `deleteLink`: `{ sourceCollection, sourceId, targetCollection, targetId, linkType }`

The GraphQL payload returns `transactionId`, `replayHit`, and per-operation
`results`. GraphQL errors include stable `extensions.code`; validation and
operation-shape errors also include `extensions.operationIndex` when Axon can
identify the failing operation before execution.

Common error bodies use `{"code": "...", "detail": ...}`. Browser clients can
switch on `code`; auth failures use `unauthorized` or `forbidden`, validation
uses `schema_validation`, missing records use `not_found`, and rate limiting
uses `rate_limit_exceeded`.

## Browser Error Details

REST schema validation failures use HTTP `422` and a stable lower-case code:

```json
{
  "code": "schema_validation",
  "detail": {
    "message": "/: Required field 'amount' is missing Fix: Add a 'amount' field",
    "field_errors": [
      {
        "field_path": "/",
        "message": "Required field 'amount' is missing",
        "severity": "error",
        "fix": "Add a 'amount' field"
      }
    ]
  }
}
```

`detail.message` is the complete server validation string. `detail.field_errors`
is an array of JSON-Schema field failures when Axon can reconstruct field
locations. Each entry contains `field_path`, `message`, and `severity`; `fix`
may be present. Non-JSON-Schema validation failures, such as template or
cross-field rule failures, keep `field_errors: []` and remain switchable by the
top-level `code`.

GraphQL schema validation failures return HTTP `200` with an `errors[]` entry.
Clients switch on `errors[].extensions.code == "SCHEMA_VALIDATION"`. The raw
message remains in `extensions.detail` for compatibility, and field-level
details are exposed as `extensions.fieldErrors`.

Write rate limits use HTTP `429`, `code: "rate_limit_exceeded"`, and
`Retry-After` in whole seconds:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 60
X-Request-Id: 018f4f9c-7cb2-7b38-a9f1-77b16d6a2e2a
```

```json
{
  "code": "rate_limit_exceeded",
  "detail": {
    "message": "write rate limit exceeded",
    "retry_after_seconds": 60,
    "scope": "actor_write"
  }
}
```

The V1 write limiter is a per-server, per-actor sliding window shared across
tenant, database, and write route. It is not a separate per-tenant or per-route
bucket. Each write endpoint call consumes one slot. `POST /transactions` also
consumes one slot for the whole transaction request, regardless of operation
count; the existing hard limit of 100 operations per transaction still applies.
