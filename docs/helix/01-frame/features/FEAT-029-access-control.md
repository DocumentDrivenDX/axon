---
ddx:
  id: FEAT-029
  depends_on:
    - helix.prd
  review:
    self_hash: f548dd83b06d298a7e8c575870ae1a06e5e9c53e94d6ccb64b2b876daf7b3b0c
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Feature Specification: FEAT-029 — Data-Layer Access Control Policies

**Feature ID**: FEAT-029
**Status**: approved
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Reusable Policy Enforcement
**Covered PRD Requirements**: FR-10, FR-11, FR-12, FR-13, FR-14
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: ACL

## Overview

Axon enforces application access control at the data layer, implementing PRD
FR-10 through FR-14. A browser, agent, SDK, GraphQL client, or MCP tool that
has network access to Axon receives only the entities and fields its resolved
identity is allowed to see, and can mutate only the rows and fields its
policies allow.

GraphQL is the primary application API surface for policy-aware reads,
writes, introspection, and approval workflows. MCP mirrors the same semantics
for agents. REST/JSON routes may expose compatibility or operational behavior
where GraphQL is intractable, but REST is not the baseline policy surface.

FEAT-012 and ADR-018 establish identity, tenant membership, credentials, and
tenant/database grants. FEAT-029 layers schema-declared data policies on top
of that foundation: entity-level visibility, row-level filtering evaluated
before pagination, and field-level redaction and write denial. Policies are
declared in or alongside the collection schema, not as ad-hoc code. They are
introspectable, testable, audited on change, and enforced uniformly below
GraphQL, MCP, SDK, and any compatibility routes. ADR-019 governs the
authoring model and mutation-intent binding; CONTRACT-004 is the normative
policy grammar, evaluation-order, denial-envelope, and reason-code surface.

Compiled policy output is also interface metadata: Axon generates the
policy-envelope, redaction, approval, and denial information that GraphQL,
MCP, SDK, CLI, and operator UI surfaces expose, while preserving the rule
that metadata is advisory and enforcement always repeats below the surface.

## Ideal Future State

An application developer declares who may see and change what — per
collection, per row, per field, per relationship — once, next to the schema,
in a closed declarative grammar. Every surface then behaves identically: a
consultant's GraphQL query, an agent's MCP tool call, and a CLI read all
return the same visible rows, the same redactions, and the same denial
reasons. Policy mistakes are caught at compile time, explored through dry
runs and fixtures, and explained on demand. Browser-side filtering becomes a
UX affordance, never a security boundary.

## Problem Statement

- **Current situation**: A static browser bundle can call Axon directly over
  the network. Browser-side filtering is the only "control" an application
  could otherwise apply.
- **Pain points**: Any user can open dev tools or script direct GraphQL/REST
  calls, bypassing UI filtering entirely. Downstream nexiq is the forcing
  function: consultants must see only their own engagements, contractors must
  lose budget/rate fields even on visible engagements, and operations
  managers must read billing records without seeing contract rate cards.
  Those guarantees cannot depend on UI code.
- **Desired outcome**: Row, entity, and field policies are enforced before
  data leaves Axon, identically on every surface, with stable machine-readable
  denial semantics. Until this feature is enforced, any browser-side filtering
  in downstream applications is an affordance only.

## Relationship to Existing Authorization

FEAT-029 refines access; it never grants access that FEAT-012/ADR-018 denied.
Evaluation order is:

1. Authenticate and resolve identity.
2. Check tenant membership, credential grants, and operation class
   (`read`, `write`, `admin`) from ADR-018.
3. Resolve collection schema and policy document.
4. Apply FEAT-029 collection, row, and field policies.
5. Validate entity schema, validation rules, lifecycle transitions, OCC, and
   transaction atomicity.

If a collection declares no access-control policy, FEAT-012 behavior applies
unchanged. Once a collection declares one, policy evaluation is default-deny
for the operations the policy covers. The normative evaluation-order text
lives in CONTRACT-004.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Policy authoring and compilation | How do I declare and safely activate access rules? | Schema-adjacent policy documents, closed grammar, compile-time validation, dry-run workflow |
| Entity and row visibility | Which records can this caller see at all? | Entity-level visibility, row filtering before pagination, leak-free read semantics |
| Field redaction and write control | Which fields can this caller read or change? | Field-level redaction on reads, field write denial with stable errors |
| Approval-routed decisions | Which writes need a human in the loop? | Three-decision envelopes (`allow`/`needs_approval`/`deny`) feeding FEAT-030 |
| Transactions and idempotency | What happens when one step is denied? | Whole-transaction abort, no partial writes, stable denied replays |
| Introspection and interface metadata | How do clients learn their effective capabilities? | Effective-policy and explanation queries; identical metadata across GraphQL, MCP, SDK, CLI |

## Requirements

### Functional Requirements by Area

#### Policy Authoring and Compilation

- **ACL-01**. Schemas must be able to declare access-control policies as
  schema-adjacent metadata covering collection operations, row predicates,
  field rules, and approval envelopes. Policy documents are part of the
  schema version: updating a policy is a schema change for audit, diff, and
  introspection purposes. Compatibility classification follows FEAT-017
  (broadening read visibility is compatible; narrowing visibility or write
  permission must be visible in schema diffs). The policy document location,
  block structure, rule grammar, predicate forms, and subject paths are
  normatively defined in CONTRACT-004.
- **ACL-02**. The policy language must be closed and declarative. Axon must
  reject arbitrary code, SQL snippets, scripts, and custom resolvers as
  policy.
- **ACL-03**. Policy compilation must type-check field paths, subject
  references, relationship predicates, redaction nullability, and approval
  envelopes before a schema version can become active. Invalid policies fail
  at schema write time with the stable invalid-expression reason code defined
  in CONTRACT-004.
- **ACL-04**. The authoring workflow must support: dry-run compile without
  activation; fixture evaluation of subjects, agents, and proposed mutations;
  optional replay of historical audit entries to identify changed decisions;
  and atomic activation that refreshes the generated GraphQL and MCP views
  together with the schema/policy version.

#### Subjects and Operations

- **ACL-05**. The policy engine must evaluate against a resolved subject that
  exposes the caller's stable user ID, tenant, tenant role, effective
  credential grants, and schema-configured application attributes (for
  example, an application role loaded from an application collection for the
  current user). Subject attributes must be resolved per request; a policy
  decision must never depend on stale cross-request identity state. Subject
  path names are defined in CONTRACT-004.
- **ACL-06**. Policies must cover the operation classes defined in
  CONTRACT-004 — read (point reads, list/query, link-traversal
  materialization, relationship resolution, audit after-state views), create,
  update (including patch, lifecycle transition, and rollback writes),
  delete, the write shorthand, and admin.

#### Decision Combination

- **ACL-07**. Policy decisions must combine explicitly, per the normative
  rules in CONTRACT-004: a matching deny overrides any matching allow; when
  an operation declares an allow list, at least one allow rule must match;
  when an operation has only field-level rules, the collection operation
  falls back to FEAT-012 grants and only the field rules apply; field rules
  evaluate after row visibility; and admin bypass is not implicit for
  application fields — deployment admins retain only documented break-glass
  recovery paths.

#### Entity and Row Visibility

- **ACL-08**. A caller must not be able to distinguish a hidden entity from a
  missing one: point reads of hidden entities return the same observable
  result as missing entities (not-found / null), and list, relationship, and
  connection results omit hidden rows without a policy error. Read denial
  must never use a forbidden-style response that leaks existence. Wire shapes
  per CONTRACT-001, CONTRACT-002, and CONTRACT-004.
- **ACL-09**. List and query operations must apply row policies before cursor
  construction, limits, offsets, and pagination, so page windows and total
  counts are computed over the caller-visible row set.
- **ACL-10**. The query planner must push indexable policy predicates into
  the storage plan (equality/range on declared FEAT-013 indexes, array
  membership backed by an index on the array path, relationship checks backed
  by link indexes, and target-policy joins whose target compiles to an
  indexed predicate). A non-indexable predicate may run as a post-filter only
  within configured query-cost limits; otherwise the operation fails with the
  stable unindexed-policy reason code defined in CONTRACT-004, naming the
  policy and required index, so unsafe launch-scale behavior is explicit.
- **ACL-11**. Relationship predicates must reference declared link types and
  compile to the same link indexes used by graph traversal. A policy must be
  able to declare that a row is visible when a related target entity is
  visible under the target's own read policy, so child collections reuse
  parent visibility rules without duplication. Recursive target-policy
  reference cycles are rejected at schema write time.

#### Field Redaction and Write Denial

- **ACL-12**. Field read denial must redact, not error: redacted fields
  return null on every read surface; any generated GraphQL field that can be
  redacted is nullable even when the JSON Schema marks it required; and audit
  before/after/diff payloads apply the same redaction for the caller before
  being returned.
- **ACL-13**. Field write denial must fail loudly: creates, updates, and
  patches that set, replace, or delete a denied field fail; lifecycle
  transitions fail if they would mutate a denied field; rollbacks fail if
  replaying history would write denied fields. Failures use the stable
  forbidden code with the denied field path (envelope per CONTRACT-004). Axon
  must never silently preserve or drop denied write fields.

#### Approval-Routed Decisions

- **ACL-14**. Write policies must support three decisions — allow,
  needs-approval, and deny — where deny overrides everything and
  needs-approval is not a soft allow: the write cannot commit directly and
  must produce or consume a FEAT-030 mutation intent bound to the pre-image
  versions, schema version, policy version, subject, grant version, and
  operation hash. Direct writes receiving a needs-approval decision return
  the approval-required reason code with no data mutation.

#### Transactions and Idempotency

- **ACL-15**. Policy checks must participate in the transaction engine: every
  operation is authorized before commit; a single denied operation aborts the
  whole transaction (identifying the operation index where the surface
  supports it); no audit mutation entry is written for a transaction that
  fails authorization; and idempotent replays of a denied transaction return
  the same terminal forbidden response for the idempotency TTL, scoped like
  successful commits — a later policy or data change must not make a replay
  of the same request suddenly succeed.

#### Introspection and Interface Metadata

- **ACL-16**. Authenticated clients must be able to query their effective
  policy for a collection or entity (allowed operations, redacted and denied
  fields, policy version) and request a dry-run explanation of a proposed
  operation (decision, reason, matching policy, field paths) without
  executing it. The GraphQL fields are defined in CONTRACT-002; semantics in
  CONTRACT-004. FEAT-030 defines mutation preview, which applies the same
  explanation to a concrete mutation and produces a bound intent token.
  Introspection is advisory: enforcement always repeats in the execution
  path.
- **ACL-17**. Every generated surface must derive its access-control metadata
  from the same compiled policy plan: GraphQL collection metadata and
  effective-policy fields, MCP tool descriptions (refreshing when schema or
  policy versions change), and SDK/CLI machine-readable output must preserve
  the same policy version, decision, reason, policy name, field paths,
  redacted fields, approval route, and audit reference. Fixture tests must be
  able to compare all surfaces for the same subject, resource, operation, and
  policy version.

### Non-Functional Requirements

Numeric targets below are proposed engineering targets, recorded as
assumptions to validate during implementation (see Constraints and
Assumptions).

- **Performance — per-operation overhead**: p95 policy-evaluation overhead at
  most 1 ms per single-entity read or write operation relative to the same
  operation on a collection without a policy document.
- **Performance — filtered queries**: p95 added latency for row-filtered list
  queries at most 10% over the unpoliced equivalent when all policy
  predicates are index-assisted.
- **Performance — compilation**: policy compile on schema save completes in
  at most 500 ms p95 per collection, and at most 2 s p95 for recompiling a
  full database schema set.
- **Performance — introspection**: effective-policy and explanation queries
  return in at most 50 ms p95.
- **Scalability**: at least 100 policy rules per collection and 1,000 rules
  per database evaluated within the targets above.
- **Security (fail closed)**: any policy-evaluation error denies the
  operation; hidden-row semantics never leak existence through errors,
  counts, aggregates, nullability, or traversal.
- **Reliability**: each request evaluates against one consistent
  schema/policy snapshot; concurrent policy activation never affects
  in-flight operations.

## User Stories

- [US-101 — Hide Inaccessible Entities](../user-stories/US-101-hide-inaccessible-entities.md)
- [US-102 — Redact Sensitive Fields](../user-stories/US-102-redact-sensitive-fields.md)
- [US-103 — Reject Denied Writes](../user-stories/US-103-reject-denied-writes.md)
- [US-104 — Explain Effective Policy](../user-stories/US-104-explain-effective-policy.md)
- [US-109 — Author And Test Policy Before Activation](../user-stories/US-109-author-and-test-policy-before-activation.md)
- [US-046 — Field-Level Masking](../user-stories/US-046-field-level-masking.md) (moved from FEAT-012)
- [US-047 — Attribute-Based Write Control](../user-stories/US-047-attribute-based-write-control.md) (moved from FEAT-012)

## Reference Behavior: Nexiq Policy Set

The first downstream consumer (nexiq) defines the expressiveness bar. A
reference policy set expressed in the CONTRACT-004 grammar (maintained with
that contract's examples and fixtures) must be able to prove all of the
following behaviors; none of them is built into Axon:

- Consultants see only engagements whose member list contains the current
  user.
- Contractors see their own engagements but receive budget and rate-card
  fields as null.
- Contracts and tasks reuse engagement visibility through relationship
  target-policy reuse rather than duplicating membership rules.
- Consultants and contractors cannot read invoices at all.
- Operations managers read firm-wide invoices and billing entities but
  receive contract rate-card fields as null.
- Operations managers gain no engagement-assignment, time-approval, or
  engagement-status-transition writes from the read policies.
- A consultant cannot update the status of an engagement they cannot read,
  and cannot write denied fields on one they can read.

## Edge Cases and Error Handling

- **Non-null GraphQL fields**: any field with a read policy is nullable in
  the generated GraphQL type.
- **Policy changes during query**: in-flight queries use the schema/policy
  snapshot active at query start.
- **Relationship reuse loops**: recursive target-policy references are
  rejected at schema write time with the invalid-expression reason code.
- **Audit reads**: audit rows remain immutable in storage, but returned
  before/after states and diffs are policy-filtered for the caller.
- **Admin break-glass**: documented deployment-admin recovery paths may
  bypass application policies; normal GraphQL and MCP application calls do
  not, and REST compatibility routes follow the same rule when present.
- **Approval-routed writes**: a needs-approval decision writes no data until
  FEAT-030 executes an approved mutation intent.
- **Create without existing row**: collection create policy decides whether
  the create is allowed; row predicates can evaluate only fields present in
  the new payload and the subject context.
- **Delete with hidden row**: if the entity exists but is hidden, delete
  returns forbidden when the caller has write grants but fails row policy; if
  the entity does not exist, it returns not-found.

## Success Metrics

- The reference nexiq policy set is expressible and its required behaviors
  hold on every surface — no downstream browser-side security filtering is
  required to launch.
- Surface-parity fixtures show identical decisions, redactions, reasons, and
  approval routes across GraphQL, MCP, SDK, and CLI for the same subject,
  resource, operation, and policy version.
- 100% of invalid policy documents are rejected at schema write time, before
  activation, with an actionable compile report.
- Zero existence leaks of hidden rows through reads, counts, aggregates,
  traversal, or error shapes in policy test suites.

## Constraints and Assumptions

- GraphQL is the primary policy-aware application surface; REST parity is a
  compatibility concern, not a launch blocker.
- All public surfaces route through the shared handler path (FR-22); policy
  enforcement assumes that single chokepoint exists.
- The numeric NFR targets are assumptions pending implementation
  measurement; they bound design choices (e.g., compiled policy plans, index
  pushdown) rather than recording measured behavior.
- Application attributes used by policies are resolved from application
  collections configured in the schema; their freshness is per-request.
- This specification is the frame-level closure for the access-control
  problem; ADR-019 governs the authoring model, CONTRACT-004 the grammar and
  wire semantics.

## Dependencies

- **Other features**: FEAT-002 (policies live in schema-adjacent metadata);
  FEAT-012 (identity, membership, grants — see Relationship section);
  FEAT-013 (index-assisted row filters); FEAT-014 (tenant/database scoping);
  FEAT-015 (policy-aware GraphQL reads, errors, introspection); FEAT-016
  (MCP mirrors policy semantics); FEAT-019 (policy predicates extend
  validation-rule condition grammar); FEAT-030 (approval-routed writes via
  mutation intents); FEAT-031 (operator UI for inspection, dry-run, and
  explanation).
- **External services**: None. Normative surfaces: CONTRACT-004 (policy
  grammar, evaluation order, denial envelopes, reason codes, introspection),
  CONTRACT-002 (GraphQL policy fields), CONTRACT-001 (REST compatibility
  shapes), CONTRACT-003 (MCP envelopes). Governing decisions: ADR-012,
  ADR-013, ADR-018, ADR-019.
- **PRD requirements**: FR-10, FR-11, FR-12, FR-13 (P0); FR-14 (P1).

## Out of Scope

- A general-purpose scripting language or arbitrary Rust policy hooks.
- Browser-side enforcement as a security boundary.
- Full-text or vector policy predicates.
- Cross-tenant policy joins.
- A visual policy-builder DSL beyond FEAT-031's raw policy editor, compile
  report, effective-policy inspector, and dry-run evaluator.
- Treating REST parity as a launch blocker for data policy.
