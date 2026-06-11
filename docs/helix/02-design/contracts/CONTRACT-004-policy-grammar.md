---
ddx:
  id: CONTRACT-004
  depends_on:
    - ADR-019
    - FEAT-029
    - FEAT-030
    - FEAT-012
---

# Contract

**Contract ID**: CONTRACT-004
**Type**: schema + protocol (policy authoring grammar and enforcement envelope)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-019, FEAT-029, FEAT-030, FEAT-012, FEAT-019, ADR-018, CONTRACT-010

## Purpose

Defines the normative surface for Axon's data-layer access-control policies:
the `access_control` YAML grammar, predicate forms, decision and approval
envelopes, the evaluation order, the denial wire shapes on REST and GraphQL,
the stable `reason` code set, and the policy introspection SDL. Any policy
compiler, enforcement layer, SDK, or UI implements against this document.

## Scope and Boundaries

- In scope: policy document grammar, predicate grammar, decision semantics,
  evaluation order, denial JSON shapes, `reason` codes, introspection types.
- Out of scope: identity/credential model (ADR-018), mutation-intent record
  and token format (FEAT-030 / ADR-019 §6), ESF layers other than
  `access_control` (CONTRACT-010), GraphQL/MCP surface generation rules
  beyond the types named here.
- Owning system: Axon policy compiler and shared handler authorization
  (`axon-schema`, `axon-api`).

## Normative Surface

### Policy document location and blocks

Policies are schema-adjacent ESF metadata under a top-level `access_control`
key in the collection schema document. A policy document MUST be part of the
schema version: changing a policy creates a new schema version and an
administrative audit entry. The grammar is closed and declarative; arbitrary
code, SQL, JavaScript, Rust hooks, or custom resolvers MUST be rejected.

| Block | Required | Shape | Rules |
|---|---|---|---|
| `identity` | no | map of subject aliases and `attributes` sources | Maps request context to subject fields; declares request-scoped attribute lookups (`from: collection`, `collection`, `key_field`, `key_subject`, `value_field`). Attribute lookups MUST be cached per request only |
| `read` / `create` / `update` / `delete` / `write` / `admin` | no | `{ allow: [rule], deny: [rule] }` | Row/operation rules. `write` is shorthand for `create` + `update` + `delete` |
| `fields` | no | `{ <field_path>: { read: { deny: [rule] }, write: { allow: [rule], deny: [rule] } } }` | Field read redaction and field write rules. Read-deny rules MAY carry `redact_as` (V1: `null` only) |
| `transitions` | no | `{ <lifecycle_name>: { <transition>: { allow: [rule] } } }` | Lifecycle transition guards |
| `envelopes` | no | `{ write: [envelope_rule] }` | Autonomous-write limits and approval routing |

### Rule grammar

```yaml
- name: <string, unique within its list>   # required
  when: <predicate>                        # optional; subject/operation condition
  where: <predicate>                       # optional; row-data condition
  redact_as: null                          # field read-deny rules only
  decision: allow | needs_approval | deny  # envelope rules only
  approval:                                # envelope rules with needs_approval only
    role: <string>
    reason_required: <bool>
```

### Predicate forms

Predicates use the closed expression grammar (derived from FEAT-019
conditions, extended for subjects, arrays, and relationships). The complete
V1 predicate forms are:

```yaml
{ subject: <attr>, eq: <value> }
{ subject: <attr>, in: [<value>, ...] }
{ field: <path>, eq: <value> }            # also: ne, in, gt, gte, lt, lte
{ field: <path>, ne: <value> }
{ field: <path>, is_null: true }
{ field: <path>, not_null: true }
{ field: members[].user_id, contains_subject: <subject_attr> }
{ field: <path>, eq_subject: <subject_attr> }
{ operation: create | update | delete }   # envelope predicates
{ all: [<predicate>, ...] }
{ any: [<predicate>, ...] }
{ not: <predicate> }
```

Relationship predicates MUST reference declared link types and MUST compile
to the link indexes used by graph traversal:

```yaml
where:
  related:
    link_type: <declared link type>
    direction: outgoing | incoming
    target_collection: <collection>
    target_policy: read
```

`target_policy: read` means: the row is visible iff the related target entity
would be visible under the target collection's own read policy. Recursive
`target_policy` cycles MUST be rejected at schema write time with
`policy_expression_invalid`.

### Subject paths

| Subject path | Meaning |
|---|---|
| `subject.user_id` | Stable Axon user ID (FEAT-012 / ADR-018) |
| `subject.agent_id` | Delegated or service-originated agent identity |
| `subject.delegated_by` | Principal that granted the agent authority |
| `subject.tenant_id` | Tenant from the authenticated route context |
| `subject.database_id` | Database from the route context |
| `subject.tenant_role` | Tenant membership role: `admin`, `write`, `read` |
| `subject.credential_id` | Credential used for the request |
| `subject.grant_version` | Credential grant snapshot version |
| `subject.grants` | Effective database grants |
| `subject.attributes.*` | Request-scoped application attributes declared in `identity` |

### Decisions and combination

Every covered operation resolves to exactly one decision:

| Decision | Meaning |
|---|---|
| `allow` | Operation may commit without human approval |
| `needs_approval` | Operation is valid but MUST flow through a FEAT-030 mutation intent |
| `deny` | Operation MUST fail |

Combination rules (normative):

- A matching `deny` overrides any matching `allow` or `needs_approval`.
- If an operation declares an `allow` list, at least one allow rule MUST match.
- If a collection declares no `access_control` block, FEAT-012 grant behavior
  applies. Once `access_control` covers an operation, evaluation for that
  operation is default-deny.
- If an operation has no `allow` list but has field-level rules, the operation
  falls back to FEAT-012 grants and only the field rules apply.
- Field-level rules are evaluated after row visibility.
- Field write denial aborts the containing operation; a single denied
  operation aborts the whole transaction. No data-mutation audit entry is
  written for a transaction that fails authorization.
- Admin bypass is not implicit for application fields.
- `needs_approval` MUST NOT commit directly; it MUST produce or consume a
  mutation intent bound to pre-image versions, schema version, policy
  version, subject, grant version, and operation hash.
- There is no policy inheritance; policies are collection-local except for
  explicit `target_policy` relationship predicates.

### Evaluation order

The five-step order is normative (FEAT-029):

1. Authenticate and resolve identity.
2. Check tenant membership, credential grants, and operation class
   (`read`, `write`, `admin`) per ADR-018 / FEAT-012.
3. Resolve collection schema and policy document (the snapshot active when
   the request begins).
4. Apply collection, row, and field policies — within this step, evaluate in
   order: collection operation policy, row predicate policy, field
   redaction/write policy, transition guard policy, envelope decision
   (ADR-019 §5).
5. Validate entity schema, validation rules, lifecycle transitions, OCC, and
   transaction atomicity.

FEAT-029 never grants access that step 2 denied.

### Read denial semantics

Hidden rows MUST be indistinguishable from missing rows:

- REST compatibility point read: `404` with `code: "not_found"`.
- GraphQL point read: nullable entity field resolves to `null`, no error.
- GraphQL list/relationship/connection: hidden entities are omitted; row
  policies apply before cursors, `limit`/`offset`/`after`, and counts.
- Redacted fields resolve to `null` on every surface, including audit
  `before`/`after`/diff payloads returned to callers.

### Denial wire shapes

REST compatibility error shape:

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

GraphQL errors carry the same code and detail under `extensions`
(camelCase keys):

```json
{
  "errors": [
    {
      "message": "field write denied",
      "path": ["updateEngagement"],
      "extensions": {
        "code": "forbidden",
        "reason": "field_write_denied",
        "collection": "engagements",
        "entityId": "eng-1",
        "fieldPath": "status",
        "policy": "contractors-cannot-transition-engagements"
      }
    }
  ],
  "data": { "updateEngagement": null }
}
```

### Stable `reason` codes

| Reason | Meaning |
|---|---|
| `collection_read_denied` | Caller lacks read permission for the collection |
| `row_write_denied` | Caller can address the entity but may not mutate that row |
| `field_write_denied` | Caller may mutate the row but not the named field |
| `approval_required` | Policy returned `needs_approval`; caller must use the FEAT-030 intent flow |
| `policy_filter_unindexed` | Required policy predicate cannot execute within configured query limits |
| `policy_expression_invalid` | Policy expression rejected at schema write time |

Implementations MUST NOT introduce new `reason` values for these conditions
and MUST treat the set as extend-only across versions. Read denial for hidden
rows intentionally uses `not_found` or omission, never `forbidden`.

### Policy introspection

```graphql
type EffectiveCollectionPolicy {
  collection: String!
  canRead: Boolean!
  canCreate: Boolean!
  canUpdate: Boolean!
  canDelete: Boolean!
  redactedFields: [String!]!
  deniedFields: [String!]!
  policyVersion: Int!
}
```

- `effectivePolicy(collection, entityId)` returns the caller's effective
  capabilities for a collection or a specific entity.
- `explainPolicy(input)` is a dry-run returning `allowed`, `needsApproval`,
  `reason`, `policy`, and affected field paths without executing a mutation.
- `previewMutation` (FEAT-030) applies the same explanation to a concrete
  mutation and produces a bound intent token.
- Introspection is advisory: enforcement MUST be repeated in the execution
  path. Every generated surface (GraphQL, MCP, SDK, CLI) MUST derive its
  metadata from the same compiled policy plan and preserve the
  machine-readable fields `policy_version`, `decision`, `reason`, `policy`,
  `field_path`, `redacted_fields`, `approval_route`, and `audit_ref` where
  available.

### Legacy FEAT-012 policy-rule schema

FEAT-012's conceptual policy entities map onto this grammar where consistent;
FEAT-029 governs on conflict:

```json
{ "id": "pol-001", "effect": "allow",
  "principal": { "email": "erik@example.com" },
  "action": ["write"],
  "resource": { "collection": "technical-designs" } }
```

```json
{ "id": "pol-002", "effect": "deny",
  "principal": { "tag": "tag:axon-agent" },
  "action": ["update"],
  "resource": { "collection": "invoices",
    "condition": { "field": "status", "eq": "approved" } } }
```

Mapping (normative):

| FEAT-012 effect | FEAT-029 equivalent |
|---|---|
| `allow` / `deny` with `resource.condition` | operation `allow` / `deny` rule with a `where` row predicate |
| `mask` on `resource.fields` | `fields.<f>.read.deny` with `redact_as: null` |
| `immutable` on `resource.fields` | `fields.<f>.write` denial |

Deny-overrides-allow precedence is shared by both models and is normative.

## Precedence and Compatibility

- Versioning: the policy document versions with the collection schema
  (ADR-007 auto-increment). Requests evaluate against the schema/policy
  snapshot active at request start; in-flight queries do not observe
  mid-request policy changes.
- Ordering: the five-step evaluation order above is fixed.
- Compatibility (FEAT-017 classification): broadening read visibility is
  compatible; narrowing read visibility or write permission is operationally
  safe but MUST be visible in schema diffs. Any field with a read policy MUST
  be nullable in generated GraphQL, even if the JSON Schema marks it required.
- Idempotency: terminal `forbidden` responses for idempotent transaction
  requests MUST be cached for the idempotency TTL in the same
  tenant/database/key scope as successful commits; replays MUST return the
  same forbidden response.
- Compilation gate: policy compilation MUST type-check field paths, subject
  references, relationship predicates, redaction nullability, and approval
  envelopes before a schema version can become active.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|------------------|-------|----------------------|
| Hidden entity point read | `not_found` / GraphQL `null` | no | None; existence must not leak |
| Row write denied | `forbidden` + `reason: row_write_denied` | no | Use an allowed row or request approval routing |
| Denied field in create/update/patch/transition/rollback | `forbidden` + `reason: field_write_denied` + `field_path` | yes (after removing the field) | Remove or correct the denied field; fields are never silently dropped or preserved |
| Envelope returns `needs_approval` | `forbidden` + `reason: approval_required`, no data mutation | no (directly) | Preview and route through FEAT-030 mutation intent |
| Unindexable policy predicate beyond cost limits | `reason: policy_filter_unindexed`, names policy and required index | no | Declare the required FEAT-013 index |
| Invalid policy at schema write (bad path, subject ref, `target_policy` cycle) | `reason: policy_expression_invalid`; schema version not activated | yes (after fix) | Fix the policy document; dry-run compile |
| Denied operation inside a transaction | whole transaction aborts; error identifies operation index where supported | per above | No partial data changes; no mutation audit entry |

## Examples

```yaml
access_control:
  identity:
    user_id: subject.user_id
    role: subject.attributes.user_role

  read:
    allow:
      - name: firm-admins-see-all
        when: { subject: role, in: [admin, partner] }
      - name: assigned-consultants-see-own
        when: { subject: role, in: [consultant, contractor] }
        where:
          field: members[].user_id
          contains_subject: user_id

  fields:
    budget_cents:
      read:
        deny:
          - name: contractors-do-not-see-budget
            when: { subject: role, eq: contractor }
            redact_as: null
      write:
        allow:
          - name: admins-only-budget
            when: { subject: role, eq: admin }

  envelopes:
    write:
      - name: auto-small-invoice-adjustment
        when:
          all:
            - { operation: update }
            - { field: amount_cents, lte: 1000000 }
            - { subject: role, in: [finance, admin] }
        decision: allow
      - name: approve-large-invoice-adjustment
        when:
          all:
            - { operation: update }
            - { field: amount_cents, gt: 1000000 }
        decision: needs_approval
        approval:
          role: finance_approver
          reason_required: true
```

## Non-Normative Notes

- FEAT-012's `__axon_policies__` entity store and `principal`
  email/tag/role matching predate the schema-adjacent model. Where they
  conflict with this contract — notably FEAT-012's "silently preserved"
  option for immutable fields — FEAT-029 governs: denied field writes always
  fail loudly.
- ADR-019 §5 enumerates an eight-step order that refines steps 2–5 of the
  normative five-step order (guardrail checks, transition guards, and
  envelope decisions are interior detail of steps 2 and 4). The two orders
  are consistent; FEAT-029's five-step framing is the contract surface.

## Validation Checklist

- [ ] Normative fields and rules are explicit.
- [ ] Compatibility and precedence rules are explicit.
- [ ] Error handling is explicit.
- [ ] At least one executable test can be derived from this contract.
- [ ] Non-normative notes cannot be mistaken for contract requirements.
