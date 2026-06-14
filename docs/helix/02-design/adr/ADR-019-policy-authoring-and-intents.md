---
ddx:
  id: ADR-019
  depends_on:
    - ADR-012
    - ADR-013
    - ADR-018
    - FEAT-002
    - FEAT-012
    - FEAT-015
    - FEAT-016
    - FEAT-017
    - FEAT-019
  review:
    self_hash: 3d6482363128cb8e6bc2cb86023a0a66c6a1c3027fab72ad99938d8136bb9732
    deps:
      ADR-012: cea81e56e4101b53f6b9a2e98c796278756bc657b895398ae226b6bc4f1f0188
      ADR-013: 3c5d06aa567303e3947976b4f827908cf6f7fd881f93c865666dcf56ca478f59
      ADR-018: 88bbe812ae5dfd953cc504c367b32f176ca8c182318c3bbbb16a60a962f94057
      FEAT-002: 0e2c69a223cadb6a5d1421cf36a9f91ce49880b66edb0680fd0c229cf1445533
      FEAT-012: d37c0b05aaef5e6da2c11ad0f7433660198cf96113dec4bf07fee4e095521eea
      FEAT-015: c75ebd606ba19b7ac509eefcd0bb47c229433b5a14b1110fcae70d6c3898bd6f
      FEAT-016: 9a2522adbeae59163b67207dc28717d0abc0f7ff65bdb155bd6b23d490d1ba5e
      FEAT-017: 7589f2ef1950a23cd5b4572f4ab88b8c30a9cb3421a6a63138dde3e6a0619f97
      FEAT-019: ddf48d3192c435e1b9a40b2dc77ec60f363bfd91230e99fab336ebf4232785c4
    reviewed_at: "2026-06-14T04:39:42Z"
---
# ADR-019: Policy Authoring and Mutation Intents

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-22 | Accepted | Erik LaBianca | FEAT-029, FEAT-030, ADR-012, ADR-013, ADR-018 | Medium |

## Context

Axon's product wedge is governed agent writes to durable business state. That
requires a policy model that can be authored by application developers,
understood by agents, enforced by the data layer, explained to operators, and
compiled into GraphQL and MCP without divergent behavior.

GraphQL is the primary public API surface. MCP is the agent-native surface.
REST/JSON endpoints may remain for operational compatibility, health checks,
binary or streaming edges, and cases where GraphQL is genuinely awkward, but
REST is not the primary design target for policy authoring or approval flows.

The policy layer must solve harder problems than endpoint authorization:

- GraphQL relationship traversal must not leak hidden entities.
- Connection pagination and counts must be computed after row policy filters.
- Required schema fields may need to become nullable in GraphQL when redacted.
- Agents need discoverable policy envelopes before they attempt a write.
- Preview and approval must be transactionally bound to the eventual mutation.
- Policy changes must be versioned, explainable, and auditable over time.

| Aspect | Description |
|--------|-------------|
| Problem | No policy model that is authorable, agent-discoverable, enforceable at the data layer, and identical across GraphQL and MCP |
| Current State | FEAT-012 grants give coarse per-database ops; no row/field/transition policy, no preview/approval binding |
| Requirements | Closed, analyzable grammar; row/field/transition/envelope rules; mutation intents binding preview, approval, and execution |
| Decision Drivers | Governed agent writes are the product wedge; GraphQL traversal must not leak hidden entities; time-of-check/time-of-use approval bugs must be impossible by construction |

## Decision

### 1. ESF Is The Policy Source Of Truth

Policies are authored as schema-adjacent ESF metadata under
`access_control`. GraphQL SDL directives are generated views, not the source of
truth. This keeps one policy document for GraphQL, MCP, SDKs, tests, and audit.

Policy documents are versioned with the collection schema. Applying a policy
change creates a new schema version and writes an administrative audit entry.
Runtime requests evaluate against the schema/policy snapshot active when the
request begins.

### 2. Policies Use A Closed Declarative Grammar

> Normative policy grammar is now owned by
> [CONTRACT-004](../contracts/CONTRACT-004-policy-grammar.md); this section is
> the decision-time record.

Policy authoring uses YAML or JSON in the same document family as ESF. The
grammar is closed, typed, and analyzable. Axon does not accept arbitrary code,
SQL snippets, embedded Rust, JavaScript, or server-side resolver functions as
policy.

The policy compiler produces a `PolicyPlan` with:

- normalized subject and field references;
- typed row, field, relationship, and transition predicates;
- indexability requirements for row filters;
- GraphQL type-shape consequences such as nullable redacted fields;
- MCP capability metadata and policy envelope descriptions;
- an explanation plan that records which rule can fire and why.

Invalid policies are rejected at schema write time. A policy that cannot be
compiled safely never becomes active.

### 3. Policy Documents Have Five Authoring Blocks

```yaml
access_control:
  identity:
    subject:
      user_id: subject.user_id
      tenant_role: subject.tenant_role
      agent_id: subject.agent_id
      delegated_by: subject.delegated_by
    attributes:
      app_role:
        from: collection
        collection: users
        key_field: id
        key_subject: user_id
        value_field: role

  read:
    allow:
      - name: assigned-users-read
        where: { field: assignees[].user_id, contains_subject: user_id }

  fields:
    amount_cents:
      read:
        deny:
          - name: contractors-do-not-see-amounts
            when: { subject: app_role, eq: contractor }
            redact_as: null
      write:
        allow:
          - name: finance-writes-amounts
            when: { subject: app_role, in: [finance, admin] }

  transitions:
    invoice_status:
      submit:
        allow:
          - name: submitter-can-submit-draft
            when: { field: status, eq: draft }

  envelopes:
    write:
      - name: auto-approve-small-invoice
        when:
          all:
            - { operation: update }
            - { field: amount_cents, lte: 1000000 }
            - { subject: app_role, eq: finance }
        decision: allow
      - name: require-approval-large-invoice
        when:
          all:
            - { operation: update }
            - { field: amount_cents, gt: 1000000 }
        decision: needs_approval
        approval:
          role: finance_approver
          reason_required: true
```

The blocks are:

1. `identity`: maps the authenticated request context to stable subject fields
   and declares request-scoped attribute sources.
2. operation policies: `read`, `create`, `update`, `delete`, `write`, and
   `admin` row-level rules.
3. `fields`: field read redaction and field write rules.
4. `transitions`: entity state-machine transition guards.
5. `envelopes`: autonomous write limits and approval routing.

### 4. Subject And Delegation Are First-Class

A policy subject includes the authenticated human or service identity plus any
delegated agent identity:

| Subject Field | Meaning |
|---|---|
| `subject.user_id` | Stable Axon user ID, when a human principal exists |
| `subject.agent_id` | Stable agent/service identity, when delegated or service-originated |
| `subject.delegated_by` | User or service that granted the agent authority |
| `subject.tenant_id` | Tenant from ADR-018 route context |
| `subject.database_id` | Database from ADR-018 route context |
| `subject.tenant_role` | Tenant role after membership resolution |
| `subject.credential_id` | Credential used for the request |
| `subject.grant_version` | Version of the credential grant snapshot |
| `subject.attributes.*` | Request-scoped application attributes declared in policy |

Attribute lookups are cached for one request only. The audit record stores the
subject snapshot and the policy version used for the decision, so historical
decisions remain explainable after users, credentials, or attributes change.

### 5. Decision Semantics Are Explicit

Every operation resolves to one of:

| Decision | Meaning |
|---|---|
| `allow` | The operation may commit without human approval |
| `needs_approval` | The operation is valid but must be approved through a mutation intent |
| `deny` | The operation must fail |

Rules compose as follows:

- A matching `deny` overrides `needs_approval` and `allow`.
- If any matching envelope returns `needs_approval`, the write cannot commit
  directly and must produce or consume a mutation intent.
- If an operation declares `allow` rules, at least one allow must match.
- If no operation policy exists, FEAT-012 grants decide only until an
  `access_control` block opts that operation into default-deny.
- Field write denial aborts the containing operation.

There is no policy inheritance in V1. Policies are collection-local except for
explicit `target_policy` relationship predicates. This avoids hidden parent
policy behavior and keeps compile reports actionable. Shared policy snippets may
be introduced later only if they compile into the same explicit rule graph.

Evaluation order is fixed:

1. FEAT-012 identity, tenant membership, and credential grants.
2. Guardrail rate/scope checks that do not require policy evaluation.
3. Collection operation policy.
4. Row predicate policy.
5. Field redaction/write policy.
6. Transition guard policy.
7. Envelope decision: `allow`, `needs_approval`, or `deny`.
8. Schema validation, OCC, transaction atomicity, and audit append.

### 6. Mutation Intents Bind Preview, Approval, And Execution

GraphQL and MCP writes can run in preview mode. A preview produces a mutation
intent with:

- operation kind and canonical operation hash;
- subject, credential ID, grant version, tenant, and database;
- schema version and policy version;
- pre-image entity and link versions for every affected record;
- computed diff and policy explanation;
- decision: `allow`, `needs_approval`, or `deny`;
- expiration timestamp;
- approval route, if approval is required.

Executing an intent re-checks the operation hash, subject/grant scope, schema
version, policy version, and all pre-image versions. If anything changed, the
intent fails as stale and the caller must preview again. This prevents
time-of-check/time-of-use approval bugs.

Intent tokens are opaque references to a server-side intent record, not
self-authorizing bearer claims. The token format is:

```text
base64url(intent_id).base64url(hmac_sha256(intent_id, deployment_secret))
```

The intent record is stored in an Axon system collection scoped by tenant and
database:

```json
{
  "intent_id": "mint_01H...",
  "tenant_id": "acme",
  "database_id": "finance",
  "subject": {
    "user_id": "usr_...",
    "agent_id": "agent_ap_reconciler",
    "delegated_by": "usr_finance_ops",
    "credential_id": "cred_...",
    "grant_version": 7
  },
  "schema_version": 12,
  "policy_version": 12,
  "operation_hash": "sha256:...",
  "pre_images": [
    {
      "kind": "entity",
      "collection": "invoices",
      "id": "inv_001",
      "version": 5
    }
  ],
  "decision": "needs_approval",
  "approval_state": "pending",
  "expires_at": "2026-04-22T22:00:00Z"
}
```

The record stores only the durable binding metadata and review summary. Large
pre/post images remain in the normal audit path or are recomputed during
preview. Pending intents are short-lived review artifacts, not workflow
instances. They do not schedule work, retry, sleep, or advance steps.

Commit validation checks:

- token HMAC;
- tenant/database match;
- caller still satisfies FEAT-012 grants;
- subject/delegation constraints still hold, unless an approver role explicitly
  executes on behalf of the original subject;
- schema and policy versions still match;
- operation hash matches the stored canonical operation;
- every pre-image version still matches;
- approval state is valid for the decision.

### 7. GraphQL Is The Primary Policy Surface

> Normative GraphQL policy/intent field surface is now owned by
> [CONTRACT-002](../contracts/CONTRACT-002-graphql-surface.md) and
> [CONTRACT-004](../contracts/CONTRACT-004-policy-grammar.md); this section is
> the decision-time record.

GraphQL exposes policy and intent workflows as first-class fields and
mutations:

- `effectivePolicy(collection, entityId)`;
- `explainPolicy(input)`;
- `previewMutation(input)`;
- `approveMutationIntent(input)`;
- `rejectMutationIntent(input)`;
- `commitMutationIntent(input)`.

Generated GraphQL types reflect policy consequences. A field that can be
redacted is nullable even if the ESF entity schema marks it as required.
Connection results apply row policy before edges, cursors, and counts are
constructed. Relationship fields apply source and target policies without
leaking hidden target existence.

### 8. MCP Mirrors GraphQL Semantics

MCP tools are generated from the same `PolicyPlan`. Tool descriptions expose
policy envelopes in agent-readable form, such as "autonomous below $10,000;
approval required above $10,000." Tool results use structured outcomes:

- `allowed`;
- `needs_approval` with intent token and approval summary;
- `denied` with policy explanation;
- `conflict` with stale pre-image details.

The generic `axon.query` tool executes GraphQL and therefore follows the same
policy and intent semantics.

### 9. Authoring Workflow

Policy authoring must support a tight test loop:

1. Developer edits ESF and `access_control`.
2. `putSchema(dryRun: true)` or the equivalent CLI/API call returns a compile
   report: type errors, missing indexes, relationship-policy cycles,
   redaction nullability changes, and approval routes.
3. Developer runs fixture tests against simulated subjects, agents, and
   example mutations.
4. Developer optionally dry-runs the policy against historical audit entries to
   find decisions that would change under the new version.
5. Developer applies the schema/policy change. Axon writes an administrative
   audit entry and atomically swaps the GraphQL/MCP policy view.

### 10. Compliance And Erasure

Audit records remain immutable, but audit reads are policy-filtered for the
caller. For sensitive values, deployments may enable field or tenant encryption
keys. Erasure deletes or destroys the key material, preserves non-sensitive
audit metadata, and records an erasure tombstone. Policy explanations must not
reveal redacted values after erasure.

## Alternatives

*Alternatives reconstructed retrospectively (2026-06-10).*

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Cedar (AWS) | Proven policy language; formal analysis tooling | Foreign subject/resource model; no native row→GraphQL nullability or intent binding; external dependency in the hot path | Rejected: cannot drive GraphQL type-shape consequences or intent workflow |
| OPA / Rego | Very expressive; large ecosystem | Turing-complete-ish; hard to statically analyze for index requirements; policy drift from ESF | Rejected: explainability and compile-time guarantees lost |
| SQL row-level security (RLS) | Battle-tested enforcement in the store | PostgreSQL-only; invisible to GraphQL/MCP generation; no envelopes/intents; per-backend divergence | Rejected: policy must compile to all surfaces, not one backend |
| **Closed declarative grammar in ESF `access_control`** | Typed, analyzable, versioned with schema; one source compiled to GraphQL, MCP, audit | Less expressive than general frameworks; new grammar to maintain | **Selected: testable, explainable, surface-portable by construction** |

## Consequences

- Policy authoring is more constrained than general-purpose authorization
  frameworks, but it is testable, explainable, and portable across GraphQL and
  MCP.
- GraphQL policy enforcement becomes a make-or-break V1 proof point. It must be
  validated with relationship-heavy schemas before broad feature expansion.
- REST parity is explicitly not a launch criterion for policy workflows.
- Policy language changes require schema versioning discipline because clients
  may observe GraphQL nullability and operation capability changes.

## Non-Goals

- A Turing-complete policy DSL.
- User-defined GraphQL resolvers as a policy mechanism.
- A durable long-running workflow engine.
- Broad REST parity for policy authoring, preview, or approval.
- Global cross-tenant policy joins.

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Closed grammar proves too weak for real authorization needs | M | H | Grammar designed to evolve (attributes, target_policy); compile reports surface gaps early |
| Row-policy filtering degrades GraphQL pagination performance | M | M | Indexability requirements emitted by the compiler; relationship-heavy schema validation before broad expansion |
| Intent staleness checks too strict, causing approval churn | M | L | Stale-fail with re-preview is deliberate; monitor stale-rate |
| Policy/GraphQL nullability changes break clients on policy edits | M | M | Schema versioning discipline; compile report names nullability changes |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| GraphQL traversal/pagination/counts leak no hidden entities under row policy | Any leak found in relationship-heavy schema tests |
| Intent execution rejects on any hash/version/pre-image drift | Any TOCTOU bypass |
| Identical decisions for the same operation across GraphQL and MCP | Contract-test divergence (CONTRACT-002 vs CONTRACT-003/004) |
| Policy compile reports actionable for fixture-test loop | Authoring friction reports |

## Supersession

- **Supersedes**: None (layers on FEAT-012 grants; amended ADR-012 §4a and
  ADR-013 core tools in place — see those ADRs' Amendment sub-headings)
- **Superseded by**: None

## Concern Impact

- **Concern selection**: Establishes governed-writes as the core safety
  concern: closed grammar, fixed evaluation order, intent-bound approval.
  Constrains all write surfaces to one policy semantics.
- **Practice override**: None.

## References

- [ADR-012: GraphQL Query Layer](ADR-012-graphql-query-layer.md)
- [ADR-013: MCP Server](ADR-013-mcp-server.md)
- [ADR-018: Tenant, User, and Credential Model](ADR-018-tenant-user-credential-model.md)
- [CONTRACT-002: GraphQL Surface](../contracts/CONTRACT-002-graphql-surface.md)
- [CONTRACT-004: Policy Grammar](../contracts/CONTRACT-004-policy-grammar.md)
- [FEAT-012: Authorization](../../01-frame/features/FEAT-012-authorization.md)
- [FEAT-029: Access Control (Policy)](../../01-frame/features/FEAT-029-access-control.md)
- [FEAT-030: Mutation Intents and Approval](../../01-frame/features/FEAT-030-mutation-intents-approval.md)

## Follow-Up Work

- FEAT-029 implements data-layer policy enforcement.
- FEAT-030 implements mutation preview, approval, and intent execution.
- FEAT-015 must expose GraphQL policy-safe pagination, redaction, and
  relationship traversal.
- FEAT-016 must expose policy envelopes and intent outcomes through MCP.
