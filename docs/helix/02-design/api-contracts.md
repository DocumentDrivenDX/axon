# Axon Browser API Contracts

This document pins the HTTP and GraphQL surface a static browser bundle can call
directly over Tailscale.

## Current User

Use `GET /auth/me` before choosing a tenant/database route.

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

Use `GET /tenants/{tenant}/databases/{database}/schema` on app load. The
response includes the current schema hash, full collection schemas, and the
header name clients can use to assert compatibility.

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

REST collection queries use `POST /collections/{collection}/query` with the
existing `FilterNode` JSON body:

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

GraphQL list fields expose the same behavior with `filter`, `sort`, `limit`,
and `afterId`:

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

Simple traversal remains available as `GET`:

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

Use `POST /transactions` for atomic multi-entity writes. Safe network retries
use the `idempotency-key` header; successful transaction responses are cached
for five minutes per database and return `x-idempotent-cache: hit` on replay.
Conflicts and validation failures are not cached.

```http
POST /tenants/default/databases/default/transactions
idempotency-key: browser-retry-2026-04-19T15:00Z
```

```json
{
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

Common error bodies use `{"code": "...", "detail": ...}`. Browser clients can
switch on `code`; auth failures use `unauthorized` or `forbidden`, validation
uses `schema_validation`, missing records use `not_found`, and rate limiting
uses `rate_limit_exceeded`.
