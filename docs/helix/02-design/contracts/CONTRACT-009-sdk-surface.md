---
ddx:
  id: CONTRACT-009
  depends_on:
    - ADR-018
    - FEAT-005
    - FEAT-023
    - FEAT-030
  review:
    self_hash: 90f207ed579bf102e548e3e9ed45afdf46c6f4befc01bed1f96ce9e4b2a114b2
    deps:
      ADR-018: 6282a6ac66a0dcfd400663681132c9f5f85ed7c78793a1cf7f8bf06853cf1d97
      FEAT-005: 1fab4e58214106451af84deee1a1bfb5c2b520333e6be2a7cd723153730c829c
      FEAT-023: 24416c13b9a48e864ae43e3967c63d2711763c745905850dbb4f03768ffc7949
      FEAT-030: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
    reviewed_at: "2026-07-11T04:22:34Z"
---

# Contract

**Contract ID**: CONTRACT-009
**Type**: library (TypeScript SDK)
**Version**: 0.1.0
**Status**: draft
**Related**: PRD FR-29, FEAT-005, FEAT-023, FEAT-030, ADR-018, CONTRACT-001 (REST), CONTRACT-002 (GraphQL), `sdk/typescript/`

## Purpose

Defines the normative public surface of the first-party TypeScript SDK
(`@axon/client`): client entry points, the tenant/database fluent API,
CRUD/query/transaction methods, governed-workflow methods (PRD FR-29), and
error types. Where this contract and older docs conflict, the published
SDK export names in `sdk/typescript/src/index.ts` are preferred unless a
spec deliberately changed them (discrepancies noted below).

## Scope and Boundaries

- In scope: exported classes, method names and signatures (shape level),
  error types and codes, subscription helpers, governed-workflow verbs.
- Out of scope: wire formats the SDK emits (CONTRACT-001/002), generated
  gRPC proto types (re-exported as `proto.*` for advanced usage, not
  independently versioned here), internal fetch plumbing.
- Owning system: `sdk/typescript` (`@axon/client`).

## Normative Surface

### Entry points (published exports)

| Export | Role |
|--------|------|
| `AxonGraphQLClient` | GraphQL-first paved path for browser/app code |
| `GraphQLTenantClient`, `GraphQLDatabaseClient`, `ControlGraphQLClient` | Fluent scoping types returned by the client |
| `AxonGraphQLError` | GraphQL error with preserved `extensions` |
| `AxonGraphQLDocuments`, `buildAggregateDocument`, `buildEntityChangedSubscriptionDocument`, `buildTransitionLifecycleDocument`, `collectionFieldName`, `pascalCase` | Document builders / naming helpers (mirror CONTRACT-002 naming rules) |
| `HttpAxonClient`, `TenantClient`, `DatabaseClient`, `AxonHttpError` | Lower-level REST compatibility helpers (CONTRACT-001 exception list only) |
| `AxonClient` | gRPC convenience client (compatibility surface) |
| `AxonError`, `AxonErrorCode` | Structured error type + stable code enum |
| `AUTH_ERROR_CODES`, `AUTH_ERROR_STATUS`, `AuthErrorCode` | ADR-018 auth failure code table |
| `Entity`, `Link`, `AuditEntry`, `TraverseResult` | Shared result types |

Fluent scoping (ADR-018 — no legacy single-database entry point):

```ts
const axon = new AxonGraphQLClient({ baseUrl, authToken });
const db = axon.tenant("acme").database("default");   // data plane
const control = axon.control();                        // control plane
```

### Database-scoped methods (`GraphQLDatabaseClient`)

| Method | Notes |
|--------|-------|
| `metadata(expectedSchemaHash?)` / `refreshSchema(expectedSchemaHash?)` | Schema handshake; mismatch surfaces `schema_mismatch` |
| `collections()` / `collection(name)` | Collection metadata |
| `getEntity(collection, id)` | Point read |
| `listEntities(collection, { filter, sort, limit, after })` | Typed filter operators per CONTRACT-002 |
| `createEntity(collection, id, data)` | |
| `updateEntity(collection, id, expectedVersion, data)` | OCC required |
| `patchEntity(collection, id, expectedVersion, patch)` | RFC 7396 |
| `deleteEntity(collection, id, expectedVersion)` | OCC |
| `commitTransaction(operations, { idempotencyKey })` | Operation wrappers per CONTRACT-002; `idempotencyKey` rules per CONTRACT-001 |
| `createCollection(name, schema)` / `putSchema(collection, schema, opts)` / `dropCollection(name, confirm = true)` | Schema lifecycle |
| `createLink(...)` / `deleteLink(...)` | Link mutations |
| `linkCandidates(sourceCollection, sourceId, linkType, { search, filter, limit })` | Autocomplete discovery |
| `neighbors(collection, id, { direction, linkType, limit, after })` | One-hop graph exploration |
| `auditLog({ collection, entityId, actor, operation, sinceNs, untilNs, after, limit })` | Audit browsing |
| `aggregate(collection, { filter, groupBy, aggregations })` | CONTRACT-002 aggregation projection |
| `transitionLifecycle(...)` | Lifecycle transition |
| `rollbackEntity(collection, id, { toVersion, expectedVersion, dryRun })` | Entity recovery; `dryRun: true` previews |
| `subscriptionUrl()` / `entityChangedSubscription(collection?)` | Returns the WS URL and GraphQL document; the SDK deliberately does NOT wrap the WebSocket transport — apps bring their own `graphql-ws`-compatible client |

### Governed-workflow methods (PRD FR-29, FEAT-005 — desired state)

The SDK MUST expose these typed methods for the governed write path. They
submit the same handler operations as the GraphQL fields in CONTRACT-002:

| SDK method | GraphQL field |
|------------|---------------|
| `previewMutation(input)` | `previewMutation` |
| `commitIntent(input)` | `commitMutationIntent` |
| `approveIntent(input)` | `approveMutationIntent` |
| `rejectIntent(input)` | `rejectMutationIntent` |
| `explainPolicy(input)` | `explainPolicy` |
| `queryAudit(options)` | `auditLog` (MAY be implemented as an alias of the published `auditLog` method) |
| `rollbackDryRun(input)` | rollback fields with `dryRun: true`; spans entity, transaction, and point-in-time recovery as those GraphQL fields land |

Requirements:

- Structured error types agents can match on programmatically, preserving
  policy, intent, conflict, stale-dimension, and audit-reference fields
  from the shared handler contract.
- A direct write that policy routes to approval resolves to an
  approval-required result (intent metadata), never a silent commit.
- The SDK works identically against embedded and server modes.

### Control-plane methods (`ControlGraphQLClient`)

`overview(tenantId)`, `tenants()`, `tenant(id)`, `users()`,
`tenantMembers(tenantId)`, `tenantDatabases(tenantId)`,
`credentials(tenantId)`, `createTenant(name)`, `deleteTenant(id)`,
`provisionUser(displayName, email?)`, `suspendUser(userId)`,
`upsertTenantMember(tenantId, userId, role)`,
`removeTenantMember(tenantId, userId)`,
`createTenantDatabase(tenantId, name)`,
`deleteTenantDatabase(tenantId, name)`,
`issueCredential(tenantId, targetUser, grants, ttlSeconds)`,
`revokeCredential(tenantId, jti)`.

- `credentials(tenantId)` returns metadata only (`jti`, user, tenant,
  expiry, revocation state, grants); it MUST NOT select or return signed
  JWT material. Only `issueCredential` returns the newly minted `jwt`,
  exactly once.
- Tenant results MUST NOT expose `dbName`/`dbPath` (ADR-018).

### REST compatibility helpers (`HttpAxonClient`)

`me()`, `tenant(name).database(name)` →
`createEntity`/`getEntity`/`updateEntity`/`deleteEntity`,
`listCollections`, `createCollection`, `schemaManifest(expectedHash?)`,
`query`, `traverse`, `commitTransaction`, `snapshot(collection)`,
`queryAudit(params)`. These exist only for the CONTRACT-001 REST exception
list (health/metrics/streaming/break-glass/compat); application code
SHOULD start from `AxonGraphQLClient`.

### Error types

- `AxonGraphQLError`: thrown on GraphQL `errors[]`; exposes `code`
  (= `extensions.code`, e.g. `VERSION_CONFLICT`) and preserves all
  server-provided extension fields (`expected`, `actual`, `currentEntity`,
  `fieldErrors`, `operationIndex`, policy denial detail) on
  `error.extensions`.
- `AxonError` with `AxonErrorCode` enum: `not_found`, `version_conflict`,
  `schema_validation`, `already_exists`, `invalid_argument`, `internal`,
  `unknown`. Codes are stable; new members are additive.
- `AxonHttpError`: carries HTTP `status`, `code`, body;
  `isAuthError()` narrows `code` to `AuthErrorCode` (the ADR-018 table:
  `unauthenticated`, `credential_malformed`, `credential_invalid`,
  `credential_expired`, `credential_not_yet_valid`, `credential_revoked`,
  `credential_foreign_issuer`, `credential_wrong_tenant`,
  `user_suspended`, `not_a_tenant_member`, `database_not_granted`,
  `op_not_granted`).

## Precedence and Compatibility

- Published export names in `sdk/typescript/src/index.ts` are canonical
  for existing surface; specs that show other names for the same
  operation (e.g. older doc examples) defer to the SDK names.
- FR-29 verb names (`previewMutation`, `commitIntent`, `approveIntent`,
  `rejectIntent`, `explainPolicy`, `queryAudit`, `rollbackDryRun`) are
  canonical for the governed workflow even where the underlying GraphQL
  field names differ (mapping table above).
- Known discrepancies (recorded, not breaking): FR-29 says `commitIntent`/
  `approveIntent`/`rejectIntent`; GraphQL fields are
  `commitMutationIntent`/`approveMutationIntent`/`rejectMutationIntent` —
  the SDK uses the shorter FR-29 names. FR-29 says `queryAudit`; the
  published GraphQL client method is `auditLog` (REST helper already uses
  `queryAudit`).
- Semver: the package follows semver; removing or renaming any export in
  this contract is a breaking change.
- gRPC `AxonClient` and `proto.*` re-exports are compatibility surface and
  may lag the GraphQL client.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|-----------------|-------|----------------------|
| Server unreachable | connection error with retry guidance | Yes | Backoff and retry |
| GraphQL `errors[]` | `AxonGraphQLError` with `code` + extensions | Per code | Switch on `error.code` |
| OCC conflict | `code === "VERSION_CONFLICT"`, `extensions.currentEntity` available | Yes after merge | Re-read via extension payload, resubmit |
| 401 auth failures | `AxonHttpError`/auth code | After credential refresh | SDKs retry after refresh on 401 |
| 403 auth failures | `AxonHttpError`/auth code | No | Surface as permission error to caller |
| Approval-routed write | approval-required result with intent metadata; no exception, no commit | N/A | Drive `approveIntent` → `commitIntent` |
| Stale intent | structured stale error with stale dimension | After fresh `previewMutation` | Re-preview |
| Transaction retry | reuse same `idempotencyKey`; replay returns cached success | Yes | Per CONTRACT-001 idempotency rules |

## Examples

```ts
import { AxonGraphQLClient } from "@axon/client";

const axon = new AxonGraphQLClient({ baseUrl: "https://axon.tailnet.example", authToken: jwt });
const db = axon.tenant("acme").database("default");

const open = await db.listEntities("tasks", {
  filter: { status: { eq: "open" }, priority: { gte: 3 } },
  sort: [{ field: "priority", direction: "DESC" }],
  limit: 50,
});

const preview = await db.previewMutation({
  operation: { updateEntity: { collection: "tasks", id: "task-1", expectedVersion: 4,
                               data: { status: "approved" } } },
});
if (preview.decision === "needs_approval") {
  await db.approveIntent({ intentId: preview.intentId });
  await db.commitIntent({ intentToken: preview.intentToken });
}

await db.commitTransaction(
  [{ updateEntity: { collection: "tasks", id: "task-1", expectedVersion: 5,
                     data: { status: "archived" } } }],
  { idempotencyKey: "archive-task-1-018f4f9c" },
);
```

## Non-Normative Notes

- The published SDK does not yet implement the governed-workflow methods;
  that delta is tracked as an implementation gap, not a contract change.

## Validation Checklist

- [x] Normative fields and rules are explicit.
- [x] Compatibility and precedence rules are explicit.
- [x] Error handling is explicit.
- [x] At least one executable test can be derived from this contract.
- [x] Non-normative notes cannot be mistaken for contract requirements.
