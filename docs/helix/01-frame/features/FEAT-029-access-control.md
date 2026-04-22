---
ddx:
  id: FEAT-029
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-012
    - FEAT-013
    - FEAT-014
    - FEAT-015
    - FEAT-019
    - ADR-012
    - ADR-018
    - ADR-019
---
# Feature Specification: FEAT-029 - Data-Layer Access Control Policies

**Feature ID**: FEAT-029
**Status**: Specified
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-19
**Updated**: 2026-04-22

## Overview

Axon enforces application access control at the data layer. A browser, agent,
SDK, GraphQL client, or MCP tool that has network access to Axon must receive
only the entities and fields its resolved identity is allowed to see, and must
be able to mutate only the rows and fields its policies allow.

GraphQL is the primary application API surface for policy-aware reads, writes,
introspection, and approval workflows. MCP mirrors the same semantics for
agents. REST/JSON routes may expose compatibility or operational behavior where
GraphQL is intractable, but REST is not the baseline policy surface.

FEAT-012 and ADR-018 establish identity, tenant membership, JWT credentials,
and tenant/database grants. FEAT-029 layers schema-declared data policies on top
of that foundation:

1. **Entity-level visibility**: a caller cannot see an entity at all if no
   policy grants visibility.
2. **Row-level filtering**: list/query operations return only rows matching a
   policy predicate, evaluated before pagination.
3. **Field-level redaction and write denial**: on rows the caller may see,
   specific fields can be returned as `null` and writes to denied fields fail.

Policies are declared in or alongside the collection schema, not as ad-hoc Rust
closures. They are introspectable, testable, audited on change, and enforced
uniformly below GraphQL, MCP, SDK, and any compatibility routes. ADR-019 governs
the authoring model and mutation-intent binding.

## Problem Statement

A static browser bundle can call Axon directly over Tailscale. Browser-side
filtering is not a security boundary: any tailnet user can open dev tools or
write a script that calls GraphQL or REST endpoints directly. Axon therefore
must enforce application row, entity, and field policies before returning data.

Downstream nexiq is the forcing function. Nexiq needs consultants to see only
their own engagements, contractors to lose budget/rate fields even on otherwise
visible engagements, and operations managers to read billing records without
seeing contract rate cards. Those guarantees cannot depend on UI code.

This specification is the frame-level closure for `axon-c5cc071a`: it chooses a
schema-declared policy model and pins the GraphQL/MCP-visible denial contract.
Backend implementation remains separate work. Until that implementation ships,
any browser-side filtering in downstream applications is an affordance only, not
a security boundary.

## Relationship To Existing Authorization

FEAT-029 refines access; it never grants access that FEAT-012/ADR-018 denied.
Evaluation order is:

1. Authenticate and resolve identity.
2. Check tenant membership, credential grants, and operation class
   (`read`, `write`, `admin`) from ADR-018.
3. Resolve collection schema and policy document.
4. Apply FEAT-029 collection, row, and field policies.
5. Validate entity schema, validation rules, lifecycle transitions, OCC, and
   transaction atomicity.

If a collection has no `access_control` block, current FEAT-012 behavior
applies. Once a collection declares `access_control`, policy evaluation is
default-deny for operations covered by that block.

## Policy Location

Policies are schema-adjacent ESF metadata:

```yaml
collection: engagements
entity_schema:
  type: object
  required: [name, status, members]
  properties:
    name: { type: string }
    status: { type: string, enum: [draft, active, closed] }
    budget_cents: { type: integer }
    rate_card_id: { type: string }
    members:
      type: array
      items:
        type: object
        required: [user_id, role]
        properties:
          user_id: { type: string }
          role: { type: string }

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

  write:
    allow:
      - name: admins-write-all
        when: { subject: role, eq: admin }
      - name: partners-write-led-engagements
        when: { subject: role, eq: partner }
        where:
          field: lead_partner_id
          eq_subject: user_id

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

    rate_card_id:
      read:
        deny:
          - name: contractors-do-not-see-rate-card
            when: { subject: role, eq: contractor }
            redact_as: null
```

Policy documents are part of the schema version. Updating a policy is a schema
change for audit and introspection purposes. Compatibility rules for policy
changes follow FEAT-017: broadening read visibility is compatible; narrowing
read visibility or write permissions is operationally safe but must be visible
in schema diffs because clients may lose fields or operations.

## Policy Authoring Model

ADR-019 defines the governing authoring model:

- ESF `access_control` metadata is the source of truth.
- GraphQL SDL, MCP tool descriptions, SDK metadata, and REST compatibility
  behavior are generated views of the same compiled policy plan.
- The policy language is closed and declarative. Axon rejects arbitrary code,
  SQL snippets, JavaScript, Rust hooks, and custom GraphQL resolvers as policy.
- Policy compilation type-checks field paths, subject references,
  relationship predicates, redaction nullability, and approval envelopes before
  a schema version can become active.
- Policy changes are schema changes. They are diffed, audited, and evaluated
  against a fixed policy snapshot during each request.

The authoring workflow is:

1. Edit ESF and `access_control`.
2. Run a dry-run schema/policy compile.
3. Test fixture subjects, agents, and proposed mutations.
4. Optionally replay historical audit entries to identify changed decisions.
5. Apply the schema/policy version and atomically refresh GraphQL and MCP.

### Policy Envelopes

Application policies can return three decisions:

| Decision | Meaning |
|---|---|
| `allow` | The operation can commit without human approval |
| `needs_approval` | The operation is valid but must flow through FEAT-030 |
| `deny` | The operation fails |

`deny` overrides every other decision. `needs_approval` is not a soft allow:
the write cannot commit directly. It must produce or consume a mutation intent
bound to the pre-image versions, schema version, policy version, subject, grant
version, and operation hash.

Example envelope:

```yaml
access_control:
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

## Policy Model

### Subjects

The policy engine evaluates against a resolved subject:

| Subject Path | Description |
|---|---|
| `subject.user_id` | Stable Axon user ID from FEAT-012/ADR-018 |
| `subject.tenant_id` | Tenant from the authenticated route context |
| `subject.tenant_role` | Tenant membership role: `admin`, `write`, or `read` |
| `subject.grants` | Effective database grants from the credential |
| `subject.attributes.*` | Application attributes resolved from a configured source |

Application attributes are schema-configured. For nexiq, `user_role` is loaded
from the application `users` collection for the current `subject.user_id`.
Implementations must cache resolved subject attributes for the request only; a
policy decision cannot depend on stale cross-request identity state.

### Operations

Policies cover these operation classes:

| Operation | Applies To |
|---|---|
| `read` | point reads, list/query, link traversal target materialization, GraphQL relationship resolution, audit `data_after` views |
| `create` | entity creation and link creation |
| `update` | full update, patch, lifecycle transition, rollback write |
| `delete` | entity and link deletion |
| `write` | shorthand covering `create`, `update`, and `delete` |
| `admin` | collection/schema/policy administration |

### Predicates

Policy predicates use a declarative expression grammar derived from FEAT-019
conditions and extended for subject references, arrays, and relationship-backed
scope checks.

Supported predicate forms:

```yaml
{ subject: role, eq: admin }
{ subject: role, in: [admin, partner] }
{ field: status, ne: closed }
{ field: members[].user_id, contains_subject: user_id }
{ field: lead_partner_id, eq_subject: user_id }
{ all: [ { subject: role, eq: partner }, { field: archived_at, is_null: true } ] }
{ any: [ { subject: role, eq: admin }, { subject: role, eq: ops_manager } ] }
{ not: { subject: role, eq: contractor } }
```

Relationship predicates reference declared link types and must compile to the
same link indexes used by graph traversal:

```yaml
where:
  related:
    link_type: belongs_to_engagement
    direction: outgoing
    target_collection: engagements
    target_policy: read
```

`target_policy: read` means "this row is visible if the related target entity
would be visible under its own read policy." This lets contracts, tasks, and
other child collections reuse engagement membership rules without duplicating
them.

### Policy Combination

Policy decisions are explicit:

- A matching `deny` overrides any matching `allow`.
- If an operation has an `allow` list, at least one allow rule must match.
- If an operation has no `allow` list but has field-level rules, the collection
  operation falls back to FEAT-012 grants and only the field rules apply.
- Field-level rules are evaluated after row visibility.
- Admin bypass is not implicit for application fields. If application admins
  should see or write a field, the policy must say so. Deployment admins can
  still perform break-glass recovery through documented admin surfaces.

## Entity-Level Visibility

Point reads first determine whether the entity exists, then whether it is
visible to the caller. If the entity exists but is not visible, the read surface
returns the same shape as a missing entity:

- REST compatibility point read: `404` with `code: "not_found"`.
- GraphQL point read: nullable entity field resolves to `null` with no policy
  error.
- GraphQL list/relationship field: the denied entity is omitted.

This prevents callers from using read errors to infer hidden entity existence.

## Row-Level Filtering

List and query operations must apply row policies before cursor construction,
`limit`, `offset`, `after`, or GraphQL connection pagination. A consultant's
`LIMIT 100` over engagements therefore scans the consultant-visible row set,
not the first 100 physical rows followed by client-side filtering.

The query planner must push indexable policy predicates into the storage plan.
Indexable policy terms include:

- equality/range checks on declared FEAT-013 indexes;
- array membership checks backed by an EAV index on the array path;
- relationship checks backed by the links table forward/reverse indexes;
- target-policy joins where the target policy itself compiles to an indexed
  predicate.

If a policy predicate is not indexable, Axon may execute it as an
application-layer post-filter only when the query cost stays within configured
limits. Otherwise it returns `policy_filter_unindexed` with details naming the
policy and required index. This makes unsafe launch-scale behavior explicit.

## Field-Level Redaction And Writes

For visible rows, field policies decide whether each selected field may be read
or written.

Read denial:

- GraphQL generated fields that can be redacted are nullable, even if JSON
  Schema marks the field as required.
- Redacted GraphQL fields resolve to `null`.
- Generic JSON and REST compatibility responses return the field as `null` by
  default.
- Audit `data_before`, `data_after`, and diff payloads apply the same redaction
  before returning to callers.

Write denial:

- Creates and updates that include a denied field fail.
- Patches that set, replace, or delete a denied field fail.
- Lifecycle transitions fail if they would mutate a denied lifecycle field.
- Rollbacks fail if replaying the historical state would write denied fields.

Field write failures use `code: "forbidden"` and include the denied field path.
Axon does not silently preserve or drop denied write fields; callers need a
clear error so SDKs and UIs can correct the request.

## Mutations And Transactions

Policy checks participate in the existing transaction engine:

- Every operation is authorized before commit.
- A single denied operation aborts the whole transaction.
- The error identifies the operation index when the API surface supports it.
- No audit mutation entry is written for a transaction that fails authorization.
- A separate application audit-write feature may later record access-denied
  events, but access policy denial itself must not create partial data changes.

Idempotent transaction requests cache terminal `forbidden` responses for the
idempotency TTL using the same tenant/database/key scope as successful commits.
Replaying the same denied write must return the same forbidden response rather
than succeeding because a later policy or data change made the operation
allowable.

## Denial Contract

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

GraphQL errors use the same code and detail under `extensions`:

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

Stable `reason` values:

| Reason | Meaning |
|---|---|
| `collection_read_denied` | Caller lacks read permission for the collection |
| `row_write_denied` | Caller can address the entity but may not mutate that row |
| `field_write_denied` | Caller may mutate the row but not the named field |
| `policy_filter_unindexed` | Required policy predicate cannot be executed within query limits |
| `policy_expression_invalid` | Schema policy expression is invalid at schema write time |

Read denial for hidden rows intentionally uses `not_found` or omission, not
`forbidden`, to avoid existence leaks.

## Introspection And Dry Run

Authenticated clients need policy metadata to hide unavailable UI affordances
without treating browser checks as security.

GraphQL exposes:

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

`effectivePolicy(collection, entityId)` reports the caller's effective
capabilities for a collection or specific entity. `explainPolicy(input)` is a
dry-run query that returns `allowed`, `needsApproval`, `reason`, `policy`, and
field paths without executing a mutation. FEAT-030 defines `previewMutation`,
which applies the same policy explanation to a concrete mutation and produces a
bound intent token when the operation is allowed or approval-routed.

REST may expose the same capability as a compatibility endpoint, but GraphQL is
the primary UI/SDK surface.

Introspection is advisory. Enforcement always happens again in the mutation or
query execution path.

## Reference Nexiq Policy Set

This reference policy demonstrates the expressiveness required by the first
downstream consumer. It is not built into Axon.

```yaml
collections:
  engagements:
    read:
      allow:
        - when: { subject: role, in: [admin, partner] }
        - when: { subject: role, in: [consultant, contractor] }
          where: { field: members[].user_id, contains_subject: user_id }
    fields:
      budget_cents:
        read:
          deny:
            - when: { subject: role, eq: contractor }
              redact_as: null
      rate_card_id:
        read:
          deny:
            - when: { subject: role, eq: contractor }
              redact_as: null

  contracts:
    read:
      allow:
        - when: { subject: role, in: [admin, partner, ops_manager] }
        - where:
            related:
              link_type: belongs_to_engagement
              target_collection: engagements
              target_policy: read
    fields:
      rate_card_entries:
        read:
          deny:
            - when: { subject: role, eq: ops_manager }
              redact_as: null

  tasks:
    read:
      allow:
        - when: { subject: role, in: [admin, partner, ops_manager] }
        - when: { subject: role, in: [consultant, contractor] }
          where:
            related:
              link_type: belongs_to_engagement
              target_collection: engagements
              target_policy: read

  invoices:
    read:
      allow:
        - when: { subject: role, in: [admin, ops_manager] }
        - when: { subject: role, eq: partner }
          where: { field: lead_partner_id, eq_subject: user_id }

  rate_card:
    read:
      allow:
        - when: { subject: role, in: [admin, partner, ops_manager] }

  time_entries:
    read:
      allow:
        - when: { subject: role, eq: admin }
        - when: { subject: role, in: [consultant, contractor] }
          where: { field: user_id, eq_subject: user_id }
        - when: { subject: role, eq: partner }
          where:
            related:
              link_type: belongs_to_engagement
              target_collection: engagements
              target_policy: read
        - when: { subject: role, eq: ops_manager }
          where: { field: status, in: [submitted, approved, invoiced, paid] }
    fields:
      rate_cents:
        read:
          deny:
            - when: { subject: role, in: [consultant, contractor] }
              redact_as: null
      cost_cents:
        read:
          deny:
            - when: { subject: role, in: [consultant, contractor] }
              redact_as: null

  users:
    read:
      allow:
        - when: { subject: role, in: [admin, partner, ops_manager] }
        - when: { subject: role, in: [consultant, contractor] }
          where:
            shares_relation:
              collection: engagements
              field: members[].user_id
              subject_field: user_id
              target_field: id
    fields:
      email:
        read:
          deny:
            - when: { subject: role, in: [consultant, contractor] }
              redact_as: null
      tailscale_login:
        read:
          deny:
            - when: { subject: role, in: [consultant, contractor] }
              redact_as: null
```

Required behaviors proven by this policy set:

- Consultants see only engagements where `members[].user_id` contains the
  current user ID.
- Contractors see their own engagements but receive `budget_cents` and
  `rate_card_id` as `null`.
- Contracts and tasks reuse engagement visibility through `target_policy: read`
  rather than duplicating membership rules.
- Consultants and contractors cannot read invoices at all.
- Operations managers read firm-wide invoices and billing entities but receive
  contract rate-card fields as `null`.
- Operations managers are not granted engagement assignment, time approval, or
  engagement-status transition writes by this reference policy.
- A consultant cannot update `engagements.status` on an engagement they cannot
  read, and cannot write denied fields on one they can read.

## User Stories

### Story US-101: Hide Inaccessible Entities [FEAT-029]

**As a** consultant using a direct browser-to-Axon app
**I want** Axon to omit engagements I am not assigned to
**So that** bypassing the UI cannot reveal other client work

**Acceptance Criteria:**
- [ ] Point reads for hidden entities return `not_found`/`null`, not
  `forbidden`
- [ ] List and GraphQL connection results omit hidden rows
- [ ] Pagination and total counts are computed after policy filtering
- [ ] Link traversal does not materialize hidden target entities

### Story US-102: Redact Sensitive Fields [FEAT-029]

**As a** contractor
**I want** visible engagement rows to omit budget and rate-card data
**So that** I can work with assignment context without seeing commercial terms

**Acceptance Criteria:**
- [ ] GraphQL generated fields that may be redacted are nullable
- [ ] Redacted fields return `null`
- [ ] Generic JSON, REST compatibility, and audit read payloads apply the same redaction
- [ ] Required JSON Schema fields can still be redacted on read

### Story US-103: Reject Denied Writes [FEAT-029]

**As an** application developer
**I want** denied writes to fail with stable policy errors
**So that** my SDK and UI can distinguish policy failures from validation and
missing-record failures

**Acceptance Criteria:**
- [ ] Updating a row the caller cannot mutate returns `forbidden`
- [ ] Writing a denied field returns `forbidden` with `field_path`
- [ ] A denied operation inside a transaction aborts the entire transaction
- [ ] Idempotent replays of denied writes return the same forbidden response

### Story US-104: Explain Effective Policy [FEAT-029]

**As a** browser client
**I want** to query my effective collection/entity policy
**So that** I can hide unavailable controls without trusting the browser for
security

**Acceptance Criteria:**
- [ ] GraphQL exposes effective collection policy metadata
- [ ] GraphQL exposes dry-run policy explanation for a proposed operation
- [ ] Explanations name the matching policy and denied field paths
- [ ] Enforcement is repeated during the real operation

### Story US-109: Author And Test Policy Before Activation [FEAT-029]

**As an** application developer
**I want** schema policy changes to compile, explain, and run against fixtures
before activation
**So that** I can prove row, field, relationship, and approval behavior before
agents touch live data

**Acceptance Criteria:**
- [ ] A dry-run schema update returns a policy compile report without changing
  the active policy version
- [ ] Invalid field paths, subject references, and relationship-policy cycles
  are rejected at schema write time
- [ ] The compile report names GraphQL fields made nullable by redaction
- [ ] Fixture tests can evaluate policy decisions for named subjects and sample
  mutations
- [ ] Policy changes are audited with old and new policy versions

## Edge Cases

- **Non-null GraphQL fields**: any field with a read policy is nullable in the
  generated GraphQL type.
- **Policy changes during query**: in-flight queries use the schema/policy
  snapshot active at query start.
- **Relationship reuse loops**: recursive `target_policy` references are
  rejected at schema write time with `policy_expression_invalid`.
- **Audit reads**: audit rows remain immutable in storage, but returned
  `data_before`, `data_after`, and diffs are policy-filtered for the caller.
- **Admin break-glass**: documented deployment-admin recovery paths may bypass
  application policies, but normal GraphQL and MCP application calls do not.
  REST compatibility routes follow the same rule when present.
- **Approval-routed writes**: a `needs_approval` decision does not write data
  until FEAT-030 executes an approved mutation intent.
- **Create without existing row**: collection create policy decides whether the
  create is allowed; row predicates can evaluate only fields present in the new
  payload and subject context.
- **Delete with hidden row**: if the entity exists but is hidden, delete returns
  `forbidden` when the caller has write grants but fails row policy; if the
  entity does not exist, it returns `not_found`.

## Dependencies

- **FEAT-002**: Access policies live in schema-adjacent ESF metadata.
- **FEAT-012 / ADR-018**: Identity, tenant membership, credentials, and grants.
- **FEAT-013**: Row filters must be index-assisted where possible.
- **FEAT-014**: Policies are scoped by tenant/database path.
- **FEAT-015 / ADR-012**: GraphQL must expose filtered reads, redacted fields,
  stable errors, and policy introspection.
- **FEAT-016 / ADR-013**: MCP must expose the same effective policy and
  approval-envelope semantics as GraphQL.
- **FEAT-019**: Policy predicates reuse and extend validation-rule condition
  grammar.
- **FEAT-030 / ADR-019**: Approval-routed writes use mutation intents bound to
  the reviewed pre-image and policy version.
- **FEAT-031**: The admin web UI renders effective-policy inspection,
  policy dry-run, redaction states, denied write explanations, and the
  operator-facing policy surfaces required for each FEAT-029 user story.

## Out Of Scope

- A general-purpose scripting language or arbitrary Rust policy hooks.
- Browser-side enforcement as a security boundary.
- Full-text or vector policy predicates.
- Cross-tenant policy joins.
- A visual policy-builder DSL beyond FEAT-031's raw policy editor, compile
  report, effective-policy inspector, and dry-run evaluator.
- Treating REST parity as a launch blocker for data policy.

## Verification

Implementation beads must add tests for:

- GraphQL and MCP read omission of hidden rows; REST compatibility routes match
  when present.
- Row filters applied before pagination.
- GraphQL nullable generation for redactable fields.
- Field redaction in entity reads, list reads, relationship reads, and audit
  reads.
- Denied field writes on create, update, patch, lifecycle transition, rollback,
  and transaction commit.
- Stable GraphQL and MCP `forbidden` error detail, with REST compatibility
  matching when present.
- Idempotent replay of forbidden transaction responses.
- Indexed policy plan generation and `policy_filter_unindexed` fallback.
- The reference nexiq policy set above.
- Policy compile reports for invalid paths, cyclic relationship policies,
  missing indexes, GraphQL nullability changes, and approval envelopes.
