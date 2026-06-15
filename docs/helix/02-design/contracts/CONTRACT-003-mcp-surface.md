---
ddx:
  id: CONTRACT-003
  depends_on:
    - ADR-013
    - ADR-018
    - ADR-019
    - FEAT-016
  review:
    self_hash: 1d3728b682b95873fd717b5333a9e86f10b4e3f432ece93cafa1138433793586
    deps:
      ADR-013: 3c5d06aa567303e3947976b4f827908cf6f7fd881f93c865666dcf56ca478f59
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      ADR-019: 3d6482363128cb8e6bc2cb86023a0a66c6a1c3027fab72ad99938d8136bb9732
      FEAT-016: 9a2522adbeae59163b67207dc28717d0abc0f7ff65bdb155bd6b23d490d1ba5e
    reviewed_at: "2026-06-15T00:35:16Z"
---

# Contract

**Contract ID**: CONTRACT-003
**Type**: protocol (MCP)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-013, ADR-018, ADR-019, FEAT-016, FEAT-030, CONTRACT-001 (routes/auth), CONTRACT-002 (GraphQL semantics MCP mirrors)

## Purpose

Defines the normative MCP (Model Context Protocol) surface: tool names and
parameter schemas, the resource URI grammar, transports and endpoints,
prompts, and structured tool error/outcome codes. The MCP server is a
protocol adapter over the same handler used by GraphQL; it mirrors GraphQL
policy and mutation-intent semantics and defines no second authorization
model.

## Scope and Boundaries

- In scope: MCP tool/resource/prompt names and schemas, URI grammar,
  transport endpoints, notification semantics, structured outcomes and
  result metadata.
- Out of scope: GraphQL document semantics executed via `axon.query`
  (CONTRACT-002), HTTP auth envelope (CONTRACT-001), policy grammar
  (CONTRACT-004), Cypher (CONTRACT-007).
- Owning system: `axon-mcp`.

## Normative Surface

### Transports and endpoints

| Transport | Invocation | Auth |
|-----------|------------|------|
| stdio | `axon mcp` (unified binary, FEAT-028); legacy `axon-server --mcp-stdio` predates the unified binary and is deprecated | none — same trust boundary as `--no-auth` |
| HTTP JSON-RPC | `POST /tenants/{t}/databases/{d}/mcp` | same auth middleware as GraphQL/REST (CONTRACT-001) |
| SSE notifications | `GET /tenants/{t}/databases/{d}/mcp/sse` | same |

Per ADR-018 all data-plane routes are tenant-prefixed; ADR-013's
un-prefixed `/mcp` and `/mcp/sse` forms predate ADR-018 and are recorded
here in the prefixed form as normative. Both transports speak the same MCP
JSON-RPC protocol. The `actor` tool parameter is optional; when omitted the
authenticated identity is used, when provided it MUST be validated against
the authenticated identity's permissions.

### Core tools (always present)

| Tool | Parameters | Returns |
|------|------------|---------|
| `axon.query` | `query: string` (required, GraphQL text), `variables: object` (optional) | GraphQL response JSON; full CONTRACT-002 semantics incl. policy/intents |
| `axon.mutation.preview` | `operation: object` (required) | mutation preview result: diff, policy decision, intent token |
| `axon.mutation.commit_intent` | `intent_token: string` (required) | commit result or stale/mismatch error |
| `axon.collection.create` | `name: string`, `schema: object` (ESF) — both required | collection metadata |
| `axon.collection.list` | none | array of collection metadata |
| `axon.collection.drop` | `name: string` (required), `confirm: boolean` (required, must be `true`) | confirmation |
| `axon.schema.get` | `collection: string` (required) | ESF schema document |
| `axon.schema.put` | `collection: string`, `schema: object` — both required | schema version number |

### Collection-specific tools (auto-generated from ESF)

For each registered collection `{c}` (≤ 15 tools per collection):

| Tool | Key parameters | Returns |
|------|----------------|---------|
| `{c}.create` | `id` (optional — server generates UUIDv7), `data` (typed from entity_schema), `actor?` | created entity (id, version, timestamps) |
| `{c}.get` | `id` (required) | entity or not-found error |
| `{c}.update` | `id`, `data` (full replacement), `expected_version` (required), `actor?` | updated entity |
| `{c}.patch` | `id`, `patch` (RFC 7396; null removes), `expected_version` (required), `actor?` | updated entity (unchanged on no-op) |
| `{c}.delete` | `id`, `expected_version?`, `force?` (default false — force deletes links), `actor?` | confirmation |
| `{c}.query` | `filter?` (fields from indexed fields), `sort? { field, direction }`, `limit?` (default 50), `after?` (cursor) | entities + pagination cursor |
| `{c}.aggregate` | `filter?`, `aggregations: [{function, field}]`, `group_by: [field]` | grouped aggregation results |
| `{c}.link` | `source_id`, `link_type` (enum from schema `link_types`), `target_collection`, `target_id`, `metadata?` | link confirmation |
| `{c}.unlink` | `source_id`, `link_type`, `target_collection`, `target_id` | confirmation |
| `{c}.traverse` | `start_id`, `link_type`, `direction?` (`forward`\|`reverse`, default forward), `max_depth?` (default 3), `filter?` (hop filter) | entities with paths |
| `{c}.audit` | `id` (required), `limit?` (default 20) | audit entries |
| `{c}.transition` | `id`, `to` (enum: valid targets), `expected_version` (required), `actor?` — generated only when the schema declares lifecycles; description includes the full transition map | updated entity |

Tool generation rules (normative):

1. One CRUD tool set per registered collection.
2. Parameter schemas derive from ESF Layer 1 (JSON Schema).
3. Filter parameters derive from ESF Layer 4; indexed fields are flagged
   "fast lookup" in descriptions.
4. Link tool `link_type` enums derive from ESF Layer 2.
5. Transition `to` enums derive from ESF Layer 3.
6. Tool descriptions include caller-visible policy envelopes generated
   from the policy plan (e.g. "autonomous below $10,000; approval required
   above $10,000").
7. Schema changes regenerate tool definitions and emit `tools/list_changed`
   to connected agents.
8. Named queries declared in a collection schema (FEAT-009) each generate
   one MCP tool in the collection's tool group.

### Resource URI grammar

The tenant-aware 4-level form is **normative**. ADR-013's 2-level
`axon://{collection}/{id}` and the `axon://default/default/...`
database/schema form predate ADR-018's tenant/database hierarchy and are
superseded.

```
axon://{tenant}/{database}/{collection}                 collection listing (paginated)
axon://{tenant}/{database}/{collection}/{id}            single entity
axon://{tenant}/{database}/{collection}/{id}/links          outbound links
axon://{tenant}/{database}/{collection}/{id}/links/inbound  inbound links
axon://{tenant}/{database}/{collection}/{id}/audit          audit history (cursor pagination)
axon://{tenant}/{database}/_schemas                     all schemas
axon://{tenant}/{database}/_schemas/{collection}        ESF schema
axon://{tenant}/{database}/_collections                 collection metadata list
```

- On the HTTP transport, `{tenant}/{database}` MUST match the endpoint's
  URL prefix; a mismatch is rejected.
- On stdio, `{tenant}` and `{database}` default to `default`/`default`
  when the URI omits them (2-level URIs are accepted on stdio only, as a
  default-expansion convenience).
- Resources return JSON (`mimeType: application/json`). Entity resources
  include system metadata; collection listings are paginated summaries.
- Resource templates MUST be published so agents can discover the
  grammar, e.g. `{"uriTemplate": "axon://{tenant}/{database}/{collection}/{id}", ...}`.

### Resource subscriptions

- Agents may subscribe to entity URIs (notified on that entity's changes)
  and collection URIs (notified on any mutation in the collection).
- Notification: `resource_updated`; per MCP it carries no data — the agent
  re-reads the resource.
- Notifications include the audit cursor needed to resume through the
  `/audit` resource after reconnect.

### Prompts

| Prompt | Arguments |
|--------|-----------|
| `axon.explore_collection` | `collection` (required) |
| `axon.dependency_analysis` | `collection`, `id` (required), `link_type?` (default: all) |
| `axon.audit_review` | `collection` (required), `id?` (omit for collection-wide), `limit?` (default 20) |
| `axon.schema_review` | `collection` (required) |

Prompts are optional; tools and resources are sufficient.

### Structured outcomes and result metadata

Write tools MUST return structured outcomes mirroring GraphQL policy
semantics (ADR-019 §8):

| Outcome | Payload |
|---------|---------|
| `allowed` | committed result |
| `needs_approval` | intent token + approval summary; the write is NOT committed |
| `denied` | policy explanation (rule names, denied/redacted field paths) |
| `conflict` | stale pre-image details (expected/actual versions, stale dimension) |

Tool results MUST preserve, where applicable: schema version, policy
version, stale dimension, current version, denied/redacted field paths,
intent ID, transaction ID, and audit references. Tool errors carry the
same stable `code` strings as the shared error model (CONTRACT-001 error
envelope; CONTRACT-002 extensions), so agents can switch programmatically.

## Precedence and Compatibility

- ADR-018 tenancy takes precedence over ADR-013 transport/URI shapes: the
  tenant-prefixed endpoints and 4-level URI grammar are canonical.
- Policy parity is mandatory: MCP tool behavior MUST match GraphQL policy
  decisions for the same subject, operation, and policy version; tool
  metadata MUST match GraphQL collection metadata (schema shape, policy
  envelopes, redaction, approval requirements, conflict/stale fields,
  audit references).
- Tool definitions are regenerated on schema change; agents MUST re-fetch
  after `tools/list_changed`. Tool names are stable for a given collection
  name.
- MCP protocol versioning follows the upstream MCP specification.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|-----------------|-------|----------------------|
| OCC mismatch on write tool | `conflict` outcome with current version | Yes after re-read | Re-read entity, resubmit |
| Approval-required write via direct tool | `needs_approval` + intent metadata; no commit | N/A | Route through approval, then `axon.mutation.commit_intent` |
| Policy denial | `denied` + policy explanation | No | Surface to operator |
| Stale intent commit | stale/mismatch error with stale dimension | After fresh preview | Re-run `axon.mutation.preview` |
| Unknown entity | not-found error (`not_found`) | No | — |
| Invalid tool parameters | JSON-RPC invalid-params / `invalid_argument` | No | Fix arguments per tool schema |
| `axon.collection.drop` without `confirm: true` | rejected | No | Set `confirm: true` |
| Auth failure (HTTP transport) | same 401/403 codes as CONTRACT-001 | 401 after refresh | Re-issue credential |
| Schema changed under agent | `tools/list_changed` notification | N/A | Re-fetch tool list |

## Examples

```json
{
  "tool": "axon.query",
  "arguments": {
    "query": "{ beads(filter: {status: {eq: \"in_progress\"}}, limit: 5) { edges { node { id title } } } }"
  }
}
```

```json
{
  "tool": "beads.transition",
  "arguments": { "id": "bead-42", "to": "in_progress", "expected_version": 3 }
}
```

Resource read: `axon://acme/default/beads/bead-42/audit`

## Non-Normative Notes

- Subscription delivery is implemented by audit-log polling in V1 (same
  mechanism as GraphQL subscriptions); this is an implementation detail,
  not contract surface. Target notification latency < 500ms.

## Validation Checklist

- [x] Normative fields and rules are explicit.
- [x] Compatibility and precedence rules are explicit.
- [x] Error handling is explicit.
- [x] At least one executable test can be derived from this contract.
- [x] Non-normative notes cannot be mistaken for contract requirements.
