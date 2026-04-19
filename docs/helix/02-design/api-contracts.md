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

Preflight responses allow:

- Methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`
- Request headers: `authorization`, `content-type`, `x-axon-schema-hash`
- Credentials: allowed only when the deployment's origin policy enables them
- Max age: deployment-configurable, with `600` seconds as the recommended
  default

Browser-readable response headers include:

- `x-axon-schema-hash`
- `x-idempotent-cache`
- `x-request-id`
- `x-axon-query-cost` when query-cost reporting is enabled

Tailscale does not bypass browser same-origin enforcement. A Tailscale-hosted
SPA talking to a differently named Axon host still needs this CORS policy.

## Current User

Use the GraphQL current-user query before choosing a tenant/database route once
the control-plane GraphQL surface is available. During the compatibility
window, `GET /auth/me` returns the same identity envelope.

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

GraphQL list fields expose `filter`, `sort`, `limit`, and `afterId`:

```graphql
{
  items(
    filter: {
      and: [
        { field: "status", op: "eq", value: "approved" }
        { field: "week", op: "eq", value: "2026-W16" }
        { field: "hours", op: "gte", value: 4.0 }
      ]
    }
    sort: [{ field: "hours", direction: "desc" }]
    limit: 50
  ) {
    id
    version
    status
    hours
  }
}
```

Supported GraphQL operators are `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in`,
`contains`, `is_null`, and `is_not_null`. Use `and` and `or` for boolean
composition.

## Link Traversal

GraphQL relationship fields, `neighbors`, and `linkCandidates` are canonical
for application link traversal and discovery. REST traversal remains available
as a compatibility endpoint.

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

Common error bodies use `{"code": "...", "detail": ...}`. Browser clients can
switch on `code`; auth failures use `unauthorized` or `forbidden`, validation
uses `schema_validation`, missing records use `not_found`, and rate limiting
uses `rate_limit_exceeded`.
