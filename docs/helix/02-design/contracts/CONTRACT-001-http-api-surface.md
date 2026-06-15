---
ddx:
  id: CONTRACT-001
  depends_on:
    - ADR-016
    - ADR-018
    - FEAT-004
    - FEAT-005
    - FEAT-008
    - FEAT-010
    - FEAT-014
    - FEAT-023
    - FEAT-026
  review:
    self_hash: 56f09de1d818257f569cfef9c6845797d6a5c9c1a78a09fbec23789bbcca1bd2
    deps:
      ADR-016: d023701c0bedc5ada8a9121fa850a6b78d7b2b2f39d2b7ac41d7d2c48de7a1b9
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      FEAT-004: 1ba0ba90778c2e6b4a38b11632d8ca73d3b328ac19ad326e151534c26ecd0b46
      FEAT-005: 1fab4e58214106451af84deee1a1bfb5c2b520333e6be2a7cd723153730c829c
      FEAT-008: de4e47fda5c2045ef2c4765371cac1caf29353ec4b5c78dbffb651d02b6eab82
      FEAT-010: f5e9cc42a1a1e5b377b069b10a011414e361565a26c13d7bead25314b5d3bf34
      FEAT-014: 89f20cc345d46dc650c9c0f1042da643fbc4e57b3e9278c287b2fb625cc6fd4f
      FEAT-023: 24416c13b9a48e864ae43e3967c63d2711763c745905850dbb4f03768ffc7949
      FEAT-026: 8751e34ac2140fb80077b881290769d82b1d39e7cb1fbaa60404bc82eae1b07b
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Contract

**Contract ID**: CONTRACT-001
**Type**: HTTP API
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-016, ADR-018, FEAT-004, FEAT-005, FEAT-008, FEAT-010, FEAT-014, FEAT-023, FEAT-026, FEAT-029, CONTRACT-002 (GraphQL), CONTRACT-003 (MCP), CONTRACT-005 (audit record), CONTRACT-008 (CLI/config)

## Purpose

Defines the normative REST/HTTP surface of an Axon node: route grammar and
tenancy addressing, the error envelope and status-code semantics, the
transaction idempotency protocol, rate-limit responses, markdown-template
endpoints, entity/collection/lifecycle endpoints, and rollback/audit
endpoints. GraphQL request/response shapes are owned by CONTRACT-002; MCP by
CONTRACT-003. This contract specifies desired future state, not
implementation status.

## Scope and Boundaries

- In scope: HTTP route paths, methods, request/response headers, JSON body
  shapes for REST endpoints, status codes, the `{code, detail}` error
  envelope, idempotency and rate-limit semantics, CORS policy.
- Out of scope: GraphQL document shapes (CONTRACT-002), MCP tool schemas
  (CONTRACT-003), policy grammar (CONTRACT-004), audit record schema
  (CONTRACT-005), CDC envelope (CONTRACT-006), Cypher query language
  (CONTRACT-007), ESF schema format (CONTRACT-010).
- Owning system: `axon-server` HTTP gateway (axum).

## Normative Surface

### Route grammar and tenancy addressing (ADR-018)

All data-plane routes MUST nest under a fixed prefix; control-plane routes
MUST nest under `/control`:

```
/tenants/{tenant}/databases/{database}/{resource...}   data plane
/control/{resource...}                                 control plane
/health, /ui/*                                         non-scoped (public)
```

- `{tenant}` and `{database}` are required path segments. A request to an
  un-prefixed data-plane path (e.g. `POST /entities/tasks/t-001`) MUST
  return 404. There is no `X-Axon-Database` or `X-Axon-Tenant` header and
  no `/db/{name}/...` form.
- The canonical entity URL is
  `/tenants/{tenant}/databases/{database}/entities/{collection}/{id}`. It is
  simultaneously the entity's identifier, routing key, and HTTP cache key.
- A URL addresses exactly one tenant and one database. Cross-tenant
  operations go through `/control/...`.
- gRPC methods, when exposed, use the same prefix:
  `/tenants/{t}/databases/{d}/axon.EntityService/CreateEntity`.

Legacy un-prefixed routes (`GET /auth/me`, `/databases/...`,
`/collections/...`) are live in the current gateway and are **deprecated**:
they MUST be removed in favor of the prefixed forms below and the
control-plane GraphQL `currentUser` query (CONTRACT-002).

### Data-plane route inventory

All paths below are relative to `/tenants/{tenant}/databases/{database}`.
"Op" is the credential grant op required per ADR-018.

| Route | Method | Op | Description |
|-------|--------|----|-------------|
| `/collections` | GET | read | List collections (name, schema version, entity count) |
| `/collections/{name}` | POST | admin | Create collection with ESF schema |
| `/collections/{name}` | GET | read | Describe collection |
| `/collections/{name}` | DELETE | admin | Drop collection (`confirm` required) |
| `/collections/{name}/schema` | GET | read | Get collection schema |
| `/collections/{name}/schema` | PUT | admin | Put schema; supports `force`, `dry_run` |
| `/collections/{name}/template` | PUT | admin | Save/update markdown template |
| `/collections/{name}/template` | GET | read | Retrieve current template |
| `/collections/{name}/template` | DELETE | admin | Remove template |
| `/collections/{name}/query` | POST | read | Filtered query (`FilterNode` body) |
| `/collections/{name}/rollback` | POST | admin | Point-in-time collection rollback |
| `/collections/{name}/state-machine` | GET | read | Full lifecycle state-machine definition |
| `/entities/{collection}/{id}` | POST | write | Create entity |
| `/entities/{collection}/{id}` | GET | read | Read entity (`?format=markdown` opt-in) |
| `/entities/{collection}/{id}` | PUT | write | Replace entity (OCC `expected_version`) |
| `/entities/{collection}/{id}` | PATCH | write | RFC 7396 merge patch (OCC) |
| `/entities/{collection}/{id}` | DELETE | write | Delete entity (OCC) |
| `/entities/{collection}/{id}/rollback` | POST | admin | Entity rollback (supports `dry_run`) |
| `/entities/{collection}/{id}/transitions` | GET | read | Valid next states with guard status |
| `/lifecycle/{collection}/{id}/transition` | POST | write | Execute lifecycle transition |
| `/transactions` | POST | write on every db touched | Atomic multi-op transaction + audit events |
| `/transactions/{tx_id}/rollback` | POST | admin on every db touched | Transaction rollback (supports `dry_run`) |
| `/snapshot` | POST | read | Read-only bulk export |
| `/audit/query` | GET | read | Audit log query (filters below) |
| `/audit/tail` | GET | read | Audit streaming tail |
| `/traverse/{collection}/{id}` | GET | read | Simple traversal (`link_type`, `max_depth`, `direction`) |
| `/traverse/{collection}/{id}` | POST | read | Filtered traversal (`hop_filter` body) |
| `/schema` | GET | read | Schema handshake manifest (hash + full schemas) |
| `/graphql` | POST | per-operation | GraphQL (CONTRACT-002) |
| `/graphql/ws` | WS | read at connect | GraphQL subscriptions (CONTRACT-002) |
| `/mcp` | POST | per-tool | MCP JSON-RPC (CONTRACT-003) |
| `/mcp/sse` | GET | read | MCP SSE notifications (CONTRACT-003) |

Tenant-level database management:

| Route | Method | Description |
|-------|--------|-------------|
| `/tenants/{tenant}/databases` | GET | List databases in tenant |
| `/tenants/{tenant}/databases` | POST | Create database under tenant |
| `/tenants/{tenant}/databases/{db}` | GET | Database metadata |
| `/tenants/{tenant}/databases/{db}` | DELETE | Drop database; non-empty requires `?force=true`; dropping `default` is rejected even with force |

### Control-plane route inventory

Authorized by caller **role** (tenant admin / deployment admin), not grants.

| Route | Method | Privilege |
|-------|--------|-----------|
| `/control/graphql` | POST | per-operation (CONTRACT-002) |
| `/control/tenants` | GET | any authenticated user (filtered to memberships) |
| `/control/tenants` | POST | deployment admin |
| `/control/tenants/{id}` | GET | member of tenant |
| `/control/tenants/{id}` | DELETE | tenant admin or deployment admin |
| `/control/tenants/{id}/users` | GET | tenant admin |
| `/control/tenants/{id}/users/{user_id}` | POST/PUT/DELETE | tenant admin |
| `/control/tenants/{id}/credentials` | POST | tenant admin, or self-issue with grants ≤ own role |
| `/control/tenants/{id}/credentials` | GET | tenant admin (all) or self (own only); metadata only, never JWT material |
| `/control/tenants/{id}/credentials/{jti}` | DELETE | tenant admin or jti owner (revoke) |
| `/control/users` | GET/POST | deployment admin |
| `/control/users/{id}` | GET/DELETE | deployment admin |

Tenant payloads MUST NOT expose `dbName`/`dbPath` fields; databases are
addressed only via the `tenant_databases` relationship and the URL path.

### Headers

| Header | Direction | Rules |
|--------|-----------|-------|
| `Authorization: Bearer <jwt>` | request | Required outside `--no-auth`/Tailscale modes; claim/verification rules per ADR-018 |
| `x-request-id` | both | Emitted on every response; echoed if a valid value was supplied, else server generates UUIDv7 |
| `x-axon-schema-hash` | both | Client asserts expected schema hash; emitted on schema-manifest responses; mismatch → 409 `schema_mismatch` |
| `x-axon-actor` | request | Allowed in CORS preflight; route handlers use authenticated identity for writes |
| `x-idempotent-cache` | response | `hit` when a transaction replay was served from the idempotency cache |
| `x-axon-query-cost` | response | Optional; reserved for query-cost reporting; clients MUST treat as optional |
| `Retry-After` | response | Whole seconds, on 429 |
| `Accept: text/markdown` | request | With `?format=markdown` selects markdown rendering |

CORS: allowlist per deployment policy (wildcard allowed only in dev /
`--no-auth`; empty allowlist disables CORS headers). Preflight allows
methods `GET, POST, PUT, PATCH, DELETE, OPTIONS`; request headers
`authorization, content-type, x-axon-schema-hash, x-axon-actor`; max age
86400; credentials mode `omit` (bearer tokens, no cookies). Exposed
response headers: `x-idempotent-cache, x-axon-schema-hash, x-request-id,
x-axon-query-cost`. `idempotency-key` is NOT part of the browser CORS
contract — use the body field.

### Error envelope

Every non-2xx REST response body is:

```json
{ "code": "<stable_lower_snake_code>", "detail": { } }
```

Clients MUST switch on `code`, never on the human-readable message.
`detail` is code-specific structured data.

### Status-code semantics

| Status | Codes | Meaning |
|--------|-------|---------|
| 400 | `invalid_argument`, missing-template render error | Malformed request, field-level details in `detail` |
| 401 | `unauthenticated`, `credential_malformed`, `credential_invalid`, `credential_expired`, `credential_not_yet_valid`, `credential_revoked`, `credential_foreign_issuer`, `user_suspended` | Credential broken — obtain a new one (ADR-018 table is normative) |
| 403 | `credential_wrong_tenant`, `not_a_tenant_member`, `database_not_granted`, `op_not_granted`, `forbidden`, `grants_exceed_issuer_role` | Credential/identity valid but not permitted here |
| 404 | `not_found` | Missing resource; also returned for policy-hidden rows (no existence leak) and all un-prefixed routes |
| 409 | `version_conflict`, `schema_mismatch`, in-flight idempotency conflict | OCC conflict / stale schema hash / concurrent idempotent request |
| 422 | `schema_validation` | Entity data fails JSON Schema validation |
| 429 | `rate_limit_exceeded` | Write rate limit; `Retry-After` header present |
| 500 | `internal`, `storage_error`, render failure | Server error |

`version_conflict` detail: `{ "expected": n, "actual": n, "current_entity": {...} }`.
`schema_mismatch` detail: `{ "expected": "...", "actual": "...", "manifest": {...} }`.
`schema_validation` detail: `{ "message": "...", "field_errors": [{ "field_path", "message", "severity", "fix"? }] }`
(`field_errors` is `[]` for non-JSON-Schema validation failures).
Unsupported audit filters return `unsupported_audit_filter` with
`{ "filter": "...", "supported_filters": [...] }`.

### Entity system-metadata envelope (FEAT-004)

Every entity (and link) response carries server-managed system fields
alongside user data. System fields live outside the user schema namespace
(CONTRACT-010) and MUST NOT be writable through entity payloads.

| Field | Type | Rules |
|-------|------|-------|
| `_id` | string | UUIDv7 server-generated by default, or client-provided at create; immutable; unique within collection |
| `_version` | integer | Starts at 1; incremented by exactly 1 on every committed mutation |
| `_created_at` | RFC 3339 UTC timestamp | Server-assigned at create; immutable |
| `_updated_at` | RFC 3339 UTC timestamp | Server-assigned on every committed mutation; equals `_created_at` after create |
| `_created_by` | string | Authenticated actor identity at create; `anonymous` when no identity is available (no-auth mode) |
| `_updated_by` | string | Authenticated actor identity of the latest committed mutation |

- All six fields are present on every entity read/list/query response.
- Links carry the same envelope semantics with at minimum `_id`,
  `_version`, `_created_at`, `_created_by`.
- A client payload that attempts to set a system field is rejected with
  400 `invalid_argument`.

### Transaction and idempotency protocol (FEAT-008)

`POST /tenants/{t}/databases/{d}/transactions` body:

```json
{
  "idempotency_key": "optional-string",
  "operations": [ { "op": "create|update|patch|delete|create_link|delete_link", ... } ],
  "audit_events": [ ... ]
}
```

- Maximum 100 operations per transaction; the 101st MUST be rejected.
- `operations: []` commits as a no-op (no audit entry) unless
  `audit_events` is non-empty, in which case the audit events are appended
  atomically.
- One transaction request consumes exactly one rate-limit slot regardless
  of operation count.

`idempotency_key` rules (normative):

| Property | Rule |
|----------|------|
| Field name | `idempotency_key` (body) / `idempotencyKey` (GraphQL input) |
| Type / length | string, 1..128 characters |
| Character set | ASCII `[A-Za-z0-9_.:-]` |
| Scope | per tenant + database; same key in different databases is independent |
| Case | case-sensitive |
| Optional | absent means non-idempotent |
| TTL | successful responses cached **5 minutes** |
| Replay within TTL | returns the original response without re-execution; `x-idempotent-cache: hit` |
| Replay after TTL | re-executes (the key has no memory) |
| Failed original | schema/conflict failures are NOT cached — retry re-executes. Exception: terminal `forbidden` policy denials ARE cached for the TTL |
| Same key, different payload | returns the original cached success until TTL expiry; clients MUST mint a fresh key per logical transaction |
| Concurrent in-flight duplicate | 409 with `retryable: true` and a `retry_after_ms` hint |

The legacy `Idempotency-Key` HTTP header MAY remain accepted for
non-browser compatibility but is deprecated; the body field is canonical.

### Request and payload limits

Server-enforced data-shape limits (lineage: niflheim production limits,
formerly Technical Requirements §4). Violations reject the request with the
standard envelope: payload/size and count violations return 400
`invalid_argument`; schema-shape violations (nesting, field counts) return
422 `schema_validation`.

| Constraint | Limit | Notes |
|-----------|-------|-------|
| Entity size (serialized) | 1 MB default, 10 MB hard max | Configurable per collection up to the hard max |
| Entity nesting depth | 8 levels | Enforced at schema definition and at write time |
| Fields per nesting level | 65,535 (u16) | |
| Array/list elements | 4,294,967,295 (u32) | |
| User-defined fields per entity | ≥ 1 | Beyond the system-metadata envelope |
| String/blob field size | Bounded by entity size | No independent per-field limit |
| Link metadata size | 64 KB | Links are lightweight |
| Traversal depth (`max_depth`) | 10 hops default | Configurable; warning emitted above 10 |
| Operations per transaction | 100 | Normative rule above; the 101st is rejected |
| Transaction timeout | 30 seconds | Configurable; expired transactions abort cleanly |
| Idempotency key length | 1..128 chars | Normative rule above |

Entities-per-collection, collections-per-database, links-per-entity, and
link-types-per-database have no contract-level hard limit; they are bounded
by backing-store capacity.

### Rate limiting (write paths)

The V1 write limiter is a **per-server, per-actor sliding window** shared
across tenant, database, and write route (not per-tenant or per-route
buckets). Each write endpoint call consumes one slot. Read/query
operations are not rate-limited.

Response: HTTP 429, `Retry-After: <whole seconds>` header, and body:

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

ADR-016's token-bucket description is superseded in practice by this
sliding-window contract; the rejection envelope above is normative.

### Markdown template endpoints (FEAT-026)

Render (single entity):

- `GET .../entities/{collection}/{id}?format=markdown` (or
  `Accept: text/markdown`) returns `200` with
  `Content-Type: text/markdown; charset=utf-8` and the rendered template.
- Default format is JSON; `format` is opt-in.
- No template defined for the collection → `400` with error
  `collection 'X' has no markdown template defined`.
- Render failure → `500` with error details; the entity JSON MUST still be
  included in the response body.

Template management (`/collections/{collection}/template`):

- `PUT` accepts `Content-Type: text/plain` (raw Mustache body) or
  `application/json` with `{ "template": "..." }`. Mustache syntax and
  schema field references are validated before persisting; optional-field
  warnings are returned but do not block the save.
- `GET` returns the current template; `DELETE` removes it.
- Template variables: entity `data` fields plus system fields `_id`,
  `_version`, `_created_at`, `_updated_at`, `_created_by`, `_updated_by`.
  Missing optional fields render empty; null is falsy.

### Lifecycle endpoints (FEAT-010)

- `GET .../collections/{coll}/state-machine` returns the full state-machine
  definition.
- `GET .../entities/{coll}/{id}/transitions` returns valid next states,
  each with current guard pass/fail status and specific failing-guard
  reasons (all failing guards reported when multiple fail).
- `POST .../lifecycle/{coll}/{id}/transition` executes a transition;
  invalid transitions return the valid target states; guard failures
  identify the guard condition and the failing entity/field. Transition +
  update + audit entry are atomic; transition metadata (`reason`) is
  recorded in the audit entry.

### Rollback and audit endpoints (FEAT-023)

- `POST .../entities/{coll}/{id}/rollback`,
  `POST .../transactions/{tx_id}/rollback`, and
  `POST .../collections/{name}/rollback` (point-in-time) all accept a
  `dry_run` boolean.
- `dry_run: true` MUST NOT acquire write locks or modify state. It returns
  the compensating operations, OCC conflict detection (entities modified
  since the rollback target), and repair-plan metadata: original audit
  IDs, original transaction IDs, actor/delegated authority, tool/API
  origin, policy decision, approval decision, and before/after values
  subject to caller redaction.
- Rollback commits are ordinary governed writes: policy (FEAT-029) and
  approval routing (FEAT-030) apply, and the rollback is itself audited.
- `GET .../audit/query` supported filters: `collection` (or comma-separated
  `collections`), `entity_id`, `actor`, `operation`, `since_ns`,
  `until_ns`, `after_id`, `limit`. `metadata.*` and `data_after.*` filters
  are not supported in V1 and return `unsupported_audit_filter`.

## Precedence and Compatibility

- Versioning: API v1; breaking changes require a version bump.
- Precedence: ADR-018 routing governs over any older route shape. Where a
  legacy un-prefixed route is live, the prefixed form is canonical and the
  legacy form is deprecated with no compatibility guarantee (pre-release
  clean break).
- Schema compatibility: additive schema changes are compatible; breaking
  changes are rejected without `force`. Clients SHOULD compare
  `schema_hash` on app load and fail closed on mismatch.
- The `{code, detail}` envelope and code strings are stable; new codes MAY
  be added, existing codes MUST NOT change meaning.
- Deprecation rules: deprecated routes/headers (un-prefixed routes,
  `Idempotency-Key` header) are removed without a migration window while
  Axon is pre-release.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|-----------------|-------|----------------------|
| Missing/invalid bearer token | 401 + code per ADR-018 table | After credential refresh | Re-issue credential |
| Valid credential, wrong tenant/database/op | 403 (`credential_wrong_tenant`, `database_not_granted`, `op_not_granted`) | No | Surface as permission error; re-issue with broader grants |
| Policy write denial | 403 `forbidden` with `detail.reason` ∈ {`collection_read_denied`, `row_write_denied`, `field_write_denied`, `policy_filter_unindexed`, `policy_expression_invalid`} + `collection`, `entity_id`, `field_path`, `policy` | No | Request approval / policy change |
| Hidden row point read | 404 `not_found` | No | Indistinguishable from absence |
| OCC mismatch | 409 `version_conflict` (expected, actual, current_entity) | Yes, after re-read | Re-read, reapply, resubmit |
| Stale schema hash | 409 `schema_mismatch` | Yes, after refresh | Refresh schema manifest |
| In-flight duplicate idempotency key | 409, `retryable: true`, `retry_after_ms` | Yes | Wait and retry same key |
| Invalid entity data | 422 `schema_validation` + `field_errors` | No (fix payload) | Correct fields per `field_path`/`fix` |
| Write rate limit | 429 `rate_limit_exceeded` + `Retry-After` | Yes, after `retry_after_seconds` | Back off |
| Un-prefixed route | 404 | No | Use `/tenants/{t}/databases/{d}/...` |
| Transaction op invalid | Whole transaction aborts; no operations or audit events applied | Per cause | Atomicity guaranteed |

## Examples

Idempotent transaction with OCC update:

```http
POST /tenants/acme/databases/default/transactions HTTP/1.1
Authorization: Bearer eyJ...
Content-Type: application/json

{
  "idempotency_key": "approve-task-1-018f4f9c",
  "operations": [
    { "op": "update", "collection": "tasks", "id": "task-1",
      "expected_version": 4, "data": { "status": "approved" } }
  ]
}
```

Replay of the same request within 5 minutes:

```http
HTTP/1.1 200 OK
X-Idempotent-Cache: hit
X-Request-Id: 018f4f9c-7cb2-7b38-a9f1-77b16d6a2e2a
```

Rate-limit rejection:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 60

{ "code": "rate_limit_exceeded",
  "detail": { "message": "write rate limit exceeded",
              "retry_after_seconds": 60, "scope": "actor_write" } }
```

## Non-Normative Notes

- The grant-op column duplicates ADR-018's op-to-route mapping for reader
  convenience; ADR-018 remains the rationale record for the auth model.
- RFC 9457 problem-details was considered; Axon's pinned envelope is the
  simpler `{code, detail}` shape already relied on by SDKs.

## Validation Checklist

- [x] Normative fields and rules are explicit.
- [x] Compatibility and precedence rules are explicit.
- [x] Error handling is explicit.
- [x] At least one executable test can be derived from this contract.
- [x] Non-normative notes cannot be mistaken for contract requirements.
