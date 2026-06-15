---
ddx:
  id: CONTRACT-002
  depends_on:
    - ADR-012
    - ADR-018
    - ADR-019
    - FEAT-009
    - FEAT-015
    - FEAT-018
    - FEAT-030
  review:
    self_hash: 126f1e9b7594d1e7dd228c09479d775af112b74b67290aa41f3ad8091b7b8b90
    deps:
      ADR-012: cea81e56e4101b53f6b9a2e98c796278756bc657b895398ae226b6bc4f1f0188
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      ADR-019: 3d6482363128cb8e6bc2cb86023a0a66c6a1c3027fab72ad99938d8136bb9732
      FEAT-009: 08784dee672189395e039843c292e6513155f125f9c9ec50bb29f2cc593c7bca
      FEAT-015: c75ebd606ba19b7ac509eefcd0bb47c229433b5a14b1110fcae70d6c3898bd6f
      FEAT-018: 32736251fbe98379326a28a9517474ad1b69ba9cbfb29b710f2cfaab1d3b8d08
      FEAT-030: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Contract

**Contract ID**: CONTRACT-002
**Type**: HTTP API (GraphQL)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-012, ADR-018, ADR-019, FEAT-009, FEAT-015, FEAT-018, FEAT-029, FEAT-030, CONTRACT-001 (routes/auth), CONTRACT-007 (Cypher language)

## Purpose

Defines the normative GraphQL surface: schema-generation rules from ESF,
the JSON-Schema→GraphQL type mapping, generated query/mutation/subscription
naming, filter operators, error extensions, policy and mutation-intent
fields, aggregation projections, the `axonQuery` entry point, and the
control-plane GraphQL schema. GraphQL is Axon's primary documented
interface for end-user and developer workflows (ADR-018 §5).

## Scope and Boundaries

- In scope: GraphQL endpoints, generated and generic type/field shapes,
  naming determinism, filter/sort/pagination inputs, mutation inputs and
  payloads, subscription protocol and event shape, error extensions,
  depth/complexity limits, control-plane GraphQL.
- Out of scope: HTTP routing/auth/idempotency envelope (CONTRACT-001),
  Cypher language semantics (CONTRACT-007 — only the `axonQuery` field
  shape is here), policy grammar (CONTRACT-004), ESF format (CONTRACT-010).
- Owning system: `axon-graphql` (schema generation + resolvers).

## Normative Surface

### Endpoints

| Endpoint | Transport | Purpose |
|----------|-----------|---------|
| `POST /tenants/{t}/databases/{d}/graphql` | HTTP | Queries and mutations (data plane) |
| `WS /tenants/{t}/databases/{d}/graphql/ws` | WebSocket, `graphql-ws` protocol | Subscriptions |
| `POST /control/graphql` | HTTP | Control-plane administration |

GraphQL operations resolve their required grant op at execute time:
query → `read`; mutation → per mutation type (most `write`, schema
mutations `admin`). WS connect requires `read`; subscriptions inherit.

### Schema generation (ESF → GraphQL)

- Each registered collection produces GraphQL types automatically; no
  hand-written `.graphql` files.
- `putSchema`/`createCollection`/`dropCollection` regenerate the schema;
  the swap is atomic. An executing request continues against the schema it
  started with; the next request observes the change.
- JSON Schema `description` text is preserved verbatim into GraphQL
  descriptions; Axon does not translate, strip, or synthesize it.

Type mapping (normative):

| JSON Schema | GraphQL |
|-------------|---------|
| `string` | `String` |
| `string` with `enum` | generated enum type |
| `integer` | `Int` |
| `number` | `Float` |
| `boolean` | `Boolean` |
| nested `object` | nested generated type (input fallback: `JSON`) |
| `array` | list type (input fallback: `JSON`) |
| anything unrepresentable | `JSON` scalar |

- System fields on every entity type: `id`, `version`, `createdAt`,
  `updatedAt`, `createdBy`, `updatedBy`.
- Required JSON Schema fields become non-null in `Create<Type>Input` when
  representable as scalars.
- **Redaction overrides requiredness**: any field that FEAT-029 policy can
  redact MUST be generated as nullable, even if ESF marks it required.
- Generated schema metadata includes policy version, schema version,
  redactable fields, approval-routed operations, autonomous write
  envelopes, and supported audit/change cursor fields per collection.

### Naming determinism

| Rule | Example |
|------|---------|
| Typed object names: PascalCase from ASCII alphanumeric words | `time_entries`, `time-entries` → `TimeEntries` |
| Typed singular field: lower camelCase | `time_entries` → `timeEntries(id:)` |
| Typed list field, simple singular name: append `s` | `item` → `items` |
| Names ending in `s`, with separators, irregular plurals, normalized names: append `List` | `tasks` → `tasksList`; `time_entries` → `timeEntriesList` |
| Relay alias: append `Connection` | `itemsConnection`, `tasksListConnection` |
| Collision with root fields (`entity`, `entities`, `collection`, `collections`, `auditLog`): append `Collection` to the typed field | — |
| Type-name collision with root/scalar types: append `Record` | — |
| Field names: snake_case/kebab-case → camelCase | `bead_type` → `beadType` |
| Forward relationship field: link type in camelCase | `depends-on` → `dependsOn` |
| Reverse relationship field: forward name + `Inbound` | `dependsOnInbound` |
| Per-collection aggregation query | `<typedListBase>Aggregate`, e.g. `timeEntriesAggregate` |
| Generated CRUD mutations | `create<Type>`, `update<Type>`, `patch<Type>`, `delete<Type>` |
| Link mutations | `create<Type>Link`, `delete<Type>Link` |
| Lifecycle mutation | `transition<Type><LifecycleField>`, e.g. `transitionBeadStatus` |
| Generated inputs | `<Type>Filter`, `<Type>Sort`, `<Type>SortField`, `Create/Update/Patch/Delete<Type>Input/Payload` |

Generic root fields always take the stored collection name as a string and
are authoritative for unusual names.

### Queries

Generic root fields (always present):

```graphql
entity(collection: String!, id: ID!): Entity
entities(collection: String!, filter: JSON, sort: JSON, limit: Int, after: String): EntityConnection!
collection(name: String!): CollectionInfo
collections: [CollectionInfo!]!
auditLog(collection: String, entityId: ID, actor: String, operation: String,
         sinceNs: String, untilNs: String, after: String, limit: Int): AuditConnection!
axonQuery(cypher: String!, parameters: JSON): AxonQueryResult!
currentUser: CurrentUser!   # { actor, role, userId, tenantId }
```

- Typed per-collection queries: `<typed>(id: ID!)` and
  `<typedList>(filter, sort, limit, after)`.
- All list fields return Relay connections: `edges { cursor node }`,
  `pageInfo { hasNextPage hasPreviousPage startCursor endCursor }`,
  `totalCount`. Compatibility typed list fields return plain arrays; the
  `...Connection` aliases return connections.
- **Policy-safe pagination**: row policies apply before edges, cursors,
  and `totalCount` are constructed. Hidden rows never count. Point reads
  of hidden entities resolve nullable fields to `null`; field-level read
  denial returns `null` for the field. Relationship fields omit hidden
  targets without leaking existence.
- `axonQuery` executes a read-only Cypher query (language semantics:
  CONTRACT-007 / ADR-021). `AxonQueryResult` exposes `rows` (JSON),
  `schema` (column type metadata), and `metadata` (plan info, index usage,
  policy decisions). Schema-declared named queries additionally generate
  one typed `Query` field each, returning typed connections.
- Lifecycle: entity types with lifecycle declarations expose `lifecycles`
  and `validTransitions(lifecycleName: String!)`.

### Filtering and sorting

Typed filters use scalar operator inputs (`AxonStringFilterInput`,
`AxonIntFilterInput`, ...). Supported operators:

| Operator | Applies to |
|----------|-----------|
| `eq`, `ne`, `in` | all scalars |
| `gt`, `gte`, `lt`, `lte` | ordered scalars |
| `contains` | strings only; case-sensitive substring; no normalization, tokenization, or ranking |
| `isNull`, `isNotNull` | all fields |
| `and`, `or` | boolean composition (arrays of filters) |

The legacy `{ field, op, value }` form remains accepted inside typed
filters and in generic `entities(filter:)` during the compatibility
window. Sort inputs: `[{ field: <FieldEnum>, direction: "asc"|"desc" }]`
(generic) / `<Type>Sort` with `<Type>SortField` enum + `ASC`/`DESC`.

### Mutations

Per-collection generated mutations (typed `input`; `legacyInput: JSON`
retained for old JSON-string callers):

| Mutation | Required arguments | Notes |
|----------|--------------------|-------|
| `create<Type>(id, input)` | typed input | non-null for required scalar fields |
| `update<Type>(id, version/expectedVersion, input)` | OCC required | full replacement |
| `patch<Type>(id, version, typedInput: { patch: JSON })` | OCC required | RFC 7396 merge patch; `patch` stays JSON to preserve null-removal semantics |
| `delete<Type>(id, expectedVersion)` | OCC | returns `{ deleted, id }` |
| `create<Type>Link` / `delete<Type>Link` | link type enum from schema `link_types` | |
| `transition<Type><Lifecycle>(input: { id, to, expectedVersion })` | lifecycle validation | invalid transition error lists valid target states |

Collection/schema management:

- `createCollection(input: { name, schema })` → `{ name, schemaVersion, schema }`
- `putSchema(input: { collection, schema, force, dryRun })` →
  `{ schema, compatibility, diff, dryRun }`; breaking change without
  `force: true` → error `extensions.code: "INVALID_OPERATION"`
- `dropCollection(input: { name, confirm })` requires `confirm: true`;
  returns `{ name, entitiesRemoved }`

Transactions:

```graphql
commitTransaction(input: {
  idempotencyKey: String
  operations: [TransactionOp!]!   # exactly ONE field set per op:
    # createEntity { collection, id, data }
    # updateEntity { collection, id, expectedVersion, data }
    # patchEntity  { collection, id, expectedVersion, patch }
    # deleteEntity { collection, id, expectedVersion }
    # createLink   { sourceCollection, sourceId, targetCollection, targetId, linkType, metadata }
    # deleteLink   { sourceCollection, sourceId, targetCollection, targetId, linkType }
  auditEvents: [AuditEventInput!]  # application audit events (CONTRACT-005 record shape)
}): CommitTransactionPayload!     # { transactionId, replayHit, auditEventIds, results { index success entity } }
```

Recovery: `rollbackEntity(input: { collection, id, toVersion,
expectedVersion, dryRun })` is part of the data-plane GraphQL surface;
transaction and point-in-time rollback remain REST break-glass
(CONTRACT-001) until the GraphQL recovery surface is hardened.

### Policy and mutation-intent fields (ADR-019, FEAT-030)

```graphql
type Query {
  effectivePolicy(collection: String!, entityId: ID): EffectivePolicy!
  pendingMutationIntents(filter: MutationIntentFilter): MutationIntentConnection!
  mutationIntent(id: ID!): MutationIntent
}

type Mutation {
  explainPolicy(input: ExplainPolicyInput!): PolicyExplanation!
  previewMutation(input: MutationPreviewInput!): MutationPreviewResult!
  approveMutationIntent(input: ApproveIntentInput!): MutationIntent!
  rejectMutationIntent(input: RejectIntentInput!): MutationIntent!
  commitMutationIntent(input: CommitIntentInput!): CommitIntentResult!
}
```

- `explainPolicy` returns allow / deny / needs-approval with rule names
  and denied/redacted field paths.
- `previewMutation` validates a proposed write, returns a diff and policy
  explanation, and creates a bound intent token when allowed or
  approval-routed.
- A direct generated mutation that policy classifies as `needs_approval`
  returns an approval-required result and MUST NOT commit.
- Generated collection mutations MAY accept `mode: PREVIEW | COMMIT`; the
  generic intent fields above are canonical.
- Commit validation re-checks: token HMAC, tenant/database match, caller
  grants, subject/delegation constraints, schema and policy versions,
  operation hash, every pre-image version, approval state (ADR-019 §6).

### Aggregations (FEAT-018)

Per-collection `<base>Aggregate(filter, groupBy: [<FieldEnum>!],
aggregations: [{ function: COUNT|SUM|AVG|MIN|MAX, field }!])` returns:

```graphql
{ totalCount groups { key keyFields count values { function field value count } } }
```

- `COUNT` needs no field; `SUM`/`AVG`/`MIN`/`MAX` require a numeric field.
- `groupBy` accepts one or more field enum values; multi-field groups
  return a compact `key` and a `keyFields` object.
- Null/missing grouped values form their own `null` group. Null/missing
  numeric values are excluded from numeric aggregates and reported via
  each value's `count`; group `count` reflects all matching entities.
- Empty collections: `totalCount: 0`, `groups: []`.
- Invalid numeric aggregation (e.g. `SUM` on a string field) → error with
  `extensions.code: "INVALID_ARGUMENT"`, `extensions.category: "AGGREGATION"`.

### Subscriptions

- Generic: `entityChanged(collection: String, filter: JSON)`.
- Per-collection: `<typed>Changed(filter)` (e.g. `beadChanged`).
- Transport: `graphql-ws` protocol on `/graphql/ws` under the tenant
  prefix.
- Event shape: mutation type, entity data, `previousVersion`, `actor`,
  timestamp, audit cursor, transaction ID.
- Events carry the audit cursor needed to resume via
  `auditLog(after: <cursor>)` after disconnect; clients SHOULD reconcile
  through `auditLog` before trusting a resumed live stream.
- Named queries (FEAT-009) are subscribable; ad-hoc `axonQuery` is not.

### Error extensions

GraphQL errors return HTTP 200 with an `errors[]` array. Stable
`extensions.code` values (upper snake on data plane):

| `extensions.code` | Extra extensions |
|-------------------|------------------|
| `VERSION_CONFLICT` | `expected`, `actual`, `currentEntity` |
| `SCHEMA_VALIDATION` | `detail` (raw message), `fieldErrors` |
| `INVALID_OPERATION` | breaking schema change without force |
| `INVALID_ARGUMENT` | `category` (e.g. `AGGREGATION`) |
| `UNSUPPORTED_AUDIT_FILTER` | `filter` |
| `FORBIDDEN` policy denial | camelCase mirror of CONTRACT-001 denial detail (`reason`, `collection`, `entityId`, `fieldPath`, `policy`) |

Transaction validation errors include `extensions.operationIndex` when the
failing operation is identifiable before execution. `axonQuery` errors
carry stable codes: `unsupported_clause`, `unknown_label`,
`unsupported_query_plan`, `policy_required_bypass`, `query_too_large`,
`query_timeout`.

Request limits: depth ≤ 10 and complexity ≤ 256 by default, validated
before resolver execution; operators override via
`AXON_GRAPHQL_MAX_DEPTH` / `AXON_GRAPHQL_MAX_COMPLEXITY`. Limit failures
return a GraphQL `errors` response.

### Control-plane GraphQL (`POST /control/graphql`)

Same `Authorization: Bearer <jwt>` model as REST control-plane routes;
falls back to legacy HTTP identity in `--no-auth`/Tailscale modes.

Queries:

```graphql
currentUser { actor role userId tenantId }
tenants { id name createdAt }
tenant(id: ID!) { id name createdAt }
users { id displayName email createdAtMs suspendedAtMs }
tenantMembers(tenantId: ID!) { tenantId userId role }
tenantDatabases(tenantId: ID!) { tenantId name createdAtMs }
credentials(tenantId: ID!) { jti userId tenantId issuedAtMs expiresAtMs revoked grants }
```

Mutations:

```graphql
createTenant(name: String!) { id name createdAt }
deleteTenant(id: ID!) { deleted tenantId }
provisionUser(displayName: String!, email: String) { id }
suspendUser(userId: ID!) { userId suspended }
upsertTenantMember(tenantId: ID!, userId: ID!, role: String!) { tenantId userId role }
removeTenantMember(tenantId: ID!, userId: ID!) { deleted }
createTenantDatabase(tenantId: ID!, name: String!) { tenantId name }
deleteTenantDatabase(tenantId: ID!, name: String!) { deleted }
issueCredential(tenantId: ID!, targetUser: ID!, grants: GrantsInput!, ttlSeconds: Int) { jwt jti expiresAt }
revokeCredential(tenantId: ID!, jti: ID!) { jti revoked }
```

- Tenant types MUST NOT expose `dbName` or `dbPath` (ADR-018 dropped
  `tenants.db_name`; databases are addressed via `tenantDatabases` and the
  URL path only).
- `issueCredential` returns signed token material exactly once;
  `credentials` returns metadata only and MUST NOT return `jwt` or other
  signed secret material.
- Authorization: deployment admins manage everything; tenant admins manage
  tenant databases and list members/databases and list/revoke tenant
  credentials; regular credential holders list/revoke only their own.
- Control-plane errors use REST-compatible lower-case `extensions.code`:
  `forbidden`, `not_found`, `already_exists`, `invalid_identifier`,
  `invalid_role`, `not_a_tenant_member`, `grants_exceed_role`,
  `invalid_jti`, `not_configured`, `storage_error`.

## Precedence and Compatibility

- ADR-018 routing and the no-`dbName`/`dbPath` rule take precedence over
  any older control-plane SDL.
- Generic root fields are authoritative when typed naming is ambiguous.
- Additive collection-schema changes regenerate the GraphQL schema
  compatibly; breaking ESF changes require `force` and may break generated
  types (clients re-introspect).
- Compatibility forms (`legacyInput`, legacy `field/op/value` filters,
  plain-array typed list fields) are deprecated; the typed forms are
  canonical.
- `extensions.code` strings are stable; data-plane codes are UPPER_SNAKE,
  control-plane codes lower_snake (REST-compatible). New codes may be
  added; existing meanings MUST NOT change.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|-----------------|-------|----------------------|
| OCC version mismatch | `errors[]` with `VERSION_CONFLICT` + `currentEntity` | Yes after re-read | Re-read, merge, resubmit |
| Schema validation failure | `SCHEMA_VALIDATION` + `fieldErrors` | No | Fix payload |
| Breaking `putSchema` without force | `INVALID_OPERATION` | No | Re-run with `force: true` or amend schema |
| Approval-required mutation called directly | approval-required result, no commit | N/A | Use `previewMutation` → approval → `commitMutationIntent` |
| Stale intent commit (version/policy/schema drift) | structured stale/mismatch error with stale dimension | After fresh preview | Re-preview |
| Depth/complexity exceeded | GraphQL `errors` (limit failure) | No | Simplify query |
| Hidden row / redacted field | `null` field resolution, omitted rows; no error | N/A | Indistinguishable from absence |
| Unsupported audit filter | `UNSUPPORTED_AUDIT_FILTER` | No | Filter client-side |
| Cypher errors via `axonQuery` | lower-snake stable codes (see above) | `query_timeout` retryable | Adjust query / declare index |

## Examples

```graphql
{
  timeEntriesList(
    filter: { and: [{ status: { eq: "approved" } }, { hours: { gte: 4.0 } }] }
    sort: [{ field: hours, direction: "desc" }]
    limit: 50
  ) { id version status hours }

  timeEntriesAggregate(
    filter: { status: { eq: "approved" } }
    groupBy: [status, week]
    aggregations: [{ function: COUNT }, { function: SUM, field: hours }]
  ) { totalCount groups { keyFields count values { function field value count } } }
}

mutation {
  previewMutation(input: {
    operation: { updateEntity: { collection: "engagements", id: "eng-1",
                                 expectedVersion: 7, data: { status: "closed" } } }
  }) { decision diff intentToken policyExplanation { rule } }
}
```

## Non-Normative Notes

- DataLoader batching for relationship resolution and audit-log-backed
  subscription polling are implementation strategies (ADR-012), not
  contract surface.

## Validation Checklist

- [x] Normative fields and rules are explicit.
- [x] Compatibility and precedence rules are explicit.
- [x] Error handling is explicit.
- [x] At least one executable test can be derived from this contract.
- [x] Non-normative notes cannot be mistaken for contract requirements.
