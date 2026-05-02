---
ddx:
  id: FEAT-031
  depends_on:
    - helix.prd
    - FEAT-011
    - FEAT-015
    - FEAT-016
    - FEAT-029
    - FEAT-030
    - ADR-006
    - ADR-012
    - ADR-013
    - ADR-019
---
# Feature Specification: FEAT-031 - Policy and Intents Admin UI

**Feature ID**: FEAT-031
**Status**: Specified
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-22
**Updated**: 2026-04-22

## Overview

Axon ships dedicated web UI coverage for the policy and mutation-intent
workflows defined by FEAT-029 and FEAT-030. The UI is not the policy security
boundary; GraphQL and MCP enforce policy below every client. The UI is the
operator and developer surface that makes those decisions inspectable,
reviewable, and testable.

FEAT-031 extends the FEAT-011 admin UI. It adds database-scoped policy,
intent, approval, and audit-lineage workflows that use GraphQL as the primary
surface. MCP-originated policy and intent outcomes are visible in the UI so a
human operator can understand what an agent saw and why Axon allowed, denied,
or routed a mutation for approval. REST remains a fallback only for health,
static assets, operational compatibility, and cases where GraphQL is
intractable.

## Problem Statement

FEAT-029 and FEAT-030 define governed reads, redaction, policy explanation,
mutation preview, approval routing, and stale-intent protection. Without a
first-party UI, operators must use raw GraphQL calls to inspect policy results
or approve risky writes. That is not a baseline product experience for the V1
policy proof: every human-facing user story represented by the policy and
intent slice needs a corresponding Axon web UI workflow.

## Requirements

### Functional Requirements

#### Policy Workspace

- **Policy authoring**: The Schemas workspace exposes the raw `access_control`
  policy block, validates it with the FEAT-029 compiler, and shows compile
  errors with rule IDs, field paths, and remediation hints.
- **Dry-run evaluation**: Operators can choose a subject, operation, and
  sample row to evaluate the effective policy before activation.
- **Effective policy inspection**: The database workspace exposes
  `effectivePolicy` and `explainPolicy` results through GraphQL, including
  row visibility, field redaction, field write denial, and approval envelope
  summaries.
- **Policy version visibility**: The UI shows the active schema/policy version
  on policy, entity, intent, and audit views.

#### Impact Matrix and Explain Panel — operation scope

The policy workspace surfaces effective-policy results on two distinct
UX surfaces with deliberately different operation coverage:

- **Impact Matrix** (grid view of subject × entity × operation cells)
  surfaces the **five entity-CRUD operations**: `read`, `create`, `update`,
  `patch`, `delete`. These are the operations whose semantics fit a single
  (subject, entity, op) tuple — they need no further fixture context to
  evaluate. The matrix is optimized for at-a-glance policy review across
  many subjects and entities.
- **Explain Panel** (drill-in view for a single subject/entity/op) covers
  **all eight policy operations**: the five above, plus `transition`,
  `rollback`, and `transaction`. These three require additional fixture
  context — a `transition` needs a `(from_state, to_state)` pair, a
  `rollback` needs a target audit_id or version, and a `transaction`
  needs an operation list — which the explain panel collects via fixture
  selectors before evaluating.

This split means US-113 acceptance criteria are evaluated on two surfaces:
matrix-coverage assertions enumerate the 5 entity-CRUD ops; explain-panel
coverage assertions enumerate all 8 ops with appropriate fixtures.

#### Policy-Safe Entity Browsing

- **Hidden rows**: Collection lists, relationship tabs, search results,
  pagination, and counts are rendered from policy-filtered GraphQL results.
- **Redacted fields**: Entity detail, list previews, relationship views, and
  audit views render redacted fields as an explicit redacted state without
  leaking original values into the DOM.
- **Denied writes**: Entity edit, lifecycle, rollback, and transaction flows
  display stable denial codes and policy explanations and do not optimistically
  mutate UI state after a denied response.

#### Mutation Intent Review

- **Preview before commit**: UI write flows that can require approval show a
  diff modal backed by `previewMutation`, including affected rows, field-level
  JSON merge-patch diff, pre-image versions, operation hash, policy decision,
  explanation, expiry, and intent token/ID.
- **Pending intent list**: A database-scoped intent inbox lists pending,
  approved, rejected, expired, committed, and stale intents with filters for
  status, requester, subject, approver role, risk reason, age, collection, and
  MCP/tool origin.
- **Intent detail**: The intent detail view shows the canonical operation,
  policy version, schema version, grant version, pre-images, diff, approval
  route, required reason, requester, agent/tool identity when present, and
  current commit eligibility.
- **Approve and reject**: Authorized approvers can approve or reject through
  GraphQL with a reason when policy requires it. The UI blocks self-approval
  when the policy requires separation of duties and surfaces stable structured
  errors when the caller lacks the approver role.
- **Stale and mismatched intents**: If entity, schema, policy, grant, or
  operation hash bindings change, the UI marks the intent stale or mismatched,
  disables commit, and offers a re-preview/resubmit path.

#### Audit Lineage And MCP Visibility

- **Audit chain**: Entity audit history and database audit detail link
  preview, approval, rejection, expiry, and committed mutation events by
  intent ID.
- **Redacted audit reads**: Audit views apply the same FEAT-029 field
  redaction as entity reads for the viewer.
- **MCP envelope visibility**: Operators can inspect the policy envelope an
  MCP-capable agent received for a tool call, plus the structured
  `allowed`, `needs_approval`, `denied`, or `conflict` outcome.
- **Deep links**: Intent detail links to the originating entity, audit entry,
  policy explanation, and GraphQL operation where applicable.

### Non-Functional Requirements

- **GraphQL-primary**: All policy and intent UI workflows use GraphQL
  operations from FEAT-029 and FEAT-030. REST tests or REST-only routes do not
  satisfy FEAT-031 acceptance criteria.
- **No client-side security boundary**: UI filtering is an affordance only.
  Hidden rows, redaction, denial, and approval routing must be enforced by the
  backend before the UI receives data.
- **No secret leakage**: Redacted field values, intent tokens, and credential
  metadata are not written to logs, browser storage, or hidden DOM attributes
  outside the visible/copyable values intentionally exposed for operators.
- **Keyboard and dense-table usability**: The approval inbox supports
  keyboard selection, filtering, and approve/reject actions without requiring
  route changes for every row.

## User Stories

### Story US-113: Inspect Effective Policy In The Web UI [FEAT-031]

**As an** operator
**I want** to inspect the effective policy for a subject, row, and operation
**So that** I can understand why Axon allows, denies, redacts, or routes a
write for approval

**Acceptance Criteria:**
- [ ] The database Policy route renders `effectivePolicy` for the selected
  subject and collection. E2E: `policy-authoring.spec.ts`
- [ ] The explain panel can evaluate a selected entity, sample row, or JSON
  fixture for read, create, update, patch, delete, transition, rollback, and
  transaction operations. E2E: `policy-authoring.spec.ts`
- [ ] Policy explanations include stable rule IDs, decision, reason code,
  field paths, policy version, and required approver role when applicable.
  E2E: `policy-authoring.spec.ts`
- [ ] The GraphQL console can reproduce the same `effectivePolicy` and
  `explainPolicy` result visible in the policy panel. E2E:
  `graphql-policy-console.spec.ts`

### Story US-114: Author And Dry-Run Policies Before Activation [FEAT-031]

**As a** developer defining collection schemas
**I want** to edit and test policy blocks in the web UI before activation
**So that** policy mistakes are caught before agents or users rely on them

**Acceptance Criteria:**
- [ ] The Schemas route exposes the collection `access_control` block beside
  the raw schema editor. E2E: `policy-authoring.spec.ts`
- [ ] Previewing a schema/policy change runs the FEAT-029 compiler and returns
  a compile report with errors, warnings, affected GraphQL nullability, and
  MCP envelope changes. E2E: `policy-authoring.spec.ts`
- [ ] A failed policy compile blocks activation and leaves the active policy
  version unchanged. E2E: `policy-authoring.spec.ts`
- [ ] A successful dry-run can evaluate fixture subjects and rows before the
  operator applies the new schema/policy version. E2E:
  `policy-authoring.spec.ts`

### Story US-115: Browse Entities With Policy-Safe UI Semantics [FEAT-031]

**As an** operator or developer
**I want** entity lists, relationship tabs, and audit views to reflect the same
policy results as GraphQL
**So that** the web UI cannot mislead me or leak hidden data

**Acceptance Criteria:**
- [ ] Entity list rows, relationship traversal, cursors, and `totalCount`
  match policy-filtered GraphQL results. E2E: `policy-enforcement.spec.ts`
- [ ] Redacted fields render as a redacted state in list, detail,
  relationship, and audit views, and the original value is absent from the DOM.
  E2E: `policy-enforcement.spec.ts`
- [ ] Denied writes display the GraphQL error code, field path, and policy
  explanation without applying an optimistic UI update. E2E:
  `policy-enforcement.spec.ts`
- [ ] Audit reads apply the same redaction rules as entity reads for the
  current viewer. E2E: `intent-audit-lineage.spec.ts`

### Story US-116: Preview And Commit Mutation Intents From The Web UI [FEAT-031]

**As a** human user performing a risky write
**I want** the UI to preview the mutation and show the policy decision before
commit
**So that** I know exactly what Axon will write and why

**Acceptance Criteria:**
- [ ] UI write flows call `previewMutation` and show affected entities,
  field-level diff, pre-image versions, policy decision, explanation, expiry,
  and intent ID before commit. E2E: `mutation-intents.spec.ts`
- [ ] An under-threshold invoice update can be previewed and committed through
  GraphQL from the UI without approval. E2E: `mutation-intents.spec.ts`
- [ ] A denied preview shows the denial reason and does not expose an
  executable intent token. E2E: `mutation-intents.spec.ts`
- [ ] Successful commit links to the resulting audit entry and updated entity.
  E2E: `mutation-intents.spec.ts`, `intent-audit-lineage.spec.ts`

### Story US-117: Review, Approve, And Reject Pending Intents [FEAT-031]

**As a** finance approver
**I want** an approval inbox and intent detail view
**So that** I can approve or reject high-risk agent writes with enough context

**Acceptance Criteria:**
- [ ] The Intent Inbox route lists pending intents with status, requester,
  subject, collection, operation, policy reason, required role, age, expiry,
  and MCP/tool origin when present. E2E: `approval-inbox.spec.ts`
- [ ] The intent detail view shows the canonical operation, diff, policy
  explanation, pre-images, version bindings, approval route, and audit links.
  E2E: `approval-inbox.spec.ts`
- [ ] Approving an over-threshold invoice intent requires the configured role
  and reason, writes an approval audit entry, and makes the intent eligible for
  commit. E2E: `approval-inbox.spec.ts`
- [ ] Rejecting an intent records actor, reason, policy version, and intent ID;
  rejected intents cannot commit. E2E: `approval-inbox.spec.ts`
- [ ] Self-approval is blocked when the policy requires separation of duties.
  E2E: `approval-inbox.spec.ts`

### Story US-118: Handle Stale And Mismatched Intents Safely [FEAT-031]

**As an** approver
**I want** stale or mismatched intents to be obvious and uncommittable
**So that** I cannot approve a different write than the one previewed

**Acceptance Criteria:**
- [ ] If an entity version changes after preview, the intent detail view shows
  `intent_stale`, disables commit, and offers re-preview. E2E:
  `mutation-intents.spec.ts`
- [ ] If policy, schema, grant, or operation hash changes after preview, the UI
  shows the specific stale or mismatch code and no partial commit occurs. E2E:
  `mutation-intents.spec.ts`
- [ ] Expired intents are visible in history but cannot be approved or
  committed. E2E: `approval-inbox.spec.ts`
- [ ] Re-preview creates a new intent ID and preserves lineage to the prior
  stale intent. E2E: `intent-audit-lineage.spec.ts`

### Story US-119: Inspect MCP-Originated Policy And Intent Outcomes [FEAT-031]

**As an** operator supervising agents
**I want** to see what an MCP-capable agent saw and submitted
**So that** I can debug agent behavior without guessing from raw logs

**Acceptance Criteria:**
- [ ] The policy workspace can show the MCP tool envelope for the current
  subject, collection, and operation. E2E: `mcp-envelope-preview.spec.ts`
- [ ] MCP-originated intents show agent identity, delegated authority,
  credential/grant version, tool name, tool arguments summary, and structured
  outcome. E2E: `intent-audit-lineage.spec.ts`
- [ ] A denied MCP tool result and the corresponding UI policy explanation use
  the same stable reason code. E2E: `mcp-envelope-preview.spec.ts`
- [ ] `needs_approval`, `denied`, and `conflict` MCP outcomes are visible from
  the intent inbox or audit lineage view. E2E:
  `intent-audit-lineage.spec.ts`

## Story Coverage Map

Every policy/intent story represented by FEAT-015, FEAT-016, FEAT-029, and
FEAT-030 has an Axon web UI acceptance surface:

| Source story | Web UI coverage | Target E2E |
|---|---|---|
| US-048 GraphQL relationships | Entity relationship tabs use policy-filtered GraphQL traversal and counts | `policy-enforcement.spec.ts` |
| US-049 GraphQL introspection | GraphQL console and policy workspace expose policy-aware schema shape | `graphql-policy-console.spec.ts` |
| US-050 GraphQL subscriptions | Live UI updates must not reveal hidden rows or redacted fields | `policy-enforcement.spec.ts` |
| US-057 GraphQL mutations | UI write flows use GraphQL preview/commit and policy error handling | `mutation-intents.spec.ts` |
| US-110 Policy traversal | Relationship tabs, lists, and counts prove hidden target omission | `policy-enforcement.spec.ts` |
| US-111 Mutation intents | UI preview, approval, stale, and commit flows | `mutation-intents.spec.ts`, `approval-inbox.spec.ts` |
| US-051 Admin GraphQL | GraphQL console reproduces policy and intent operations | `graphql-policy-console.spec.ts` |
| US-052 MCP discovery | Policy workspace shows generated agent tool envelopes | `mcp-envelope-preview.spec.ts` |
| US-053 MCP CRUD | Audit and intent views identify MCP-originated writes | `intent-audit-lineage.spec.ts` |
| US-054 MCP GraphQL bridge | UI can compare `axon.query` behavior with GraphQL console results | `mcp-envelope-preview.spec.ts` |
| US-055 MCP subscriptions | Audit/lineage views show policy-safe agent notification effects | `intent-audit-lineage.spec.ts` |
| US-056 MCP stdio | Operator setup/status surface links stdio config to agent-originated audit | `mcp-envelope-preview.spec.ts` |
| US-112 MCP policy envelopes | UI previews the envelope an agent role sees | `mcp-envelope-preview.spec.ts` |
| US-101 Hidden entities | Lists, search, relationships, and counts omit hidden rows | `policy-enforcement.spec.ts` |
| US-102 Redaction | Entity and audit views render redacted state with no DOM leakage | `policy-enforcement.spec.ts` |
| US-103 Denied writes | Forms surface stable denial code and explanation | `policy-enforcement.spec.ts` |
| US-104 Explain policy | Policy workspace renders `effectivePolicy` and `explainPolicy` | `policy-authoring.spec.ts` |
| US-109 Policy authoring | Schema route supports compile report and fixture dry-run | `policy-authoring.spec.ts` |
| US-105 Preview mutation | Diff modal shows pre-image versions and policy decision | `mutation-intents.spec.ts` |
| US-106 Approval routing | Approval inbox and detail support approve/reject with reason | `approval-inbox.spec.ts` |
| US-107 Stale execution | UI disables stale/mismatched commits and supports re-preview | `mutation-intents.spec.ts` |
| US-108 MCP intents | UI exposes MCP-originated intent outcomes and audit lineage | `intent-audit-lineage.spec.ts` |

## Routes And Views

| Route or tab | Expected workflows | E2E coverage |
|---|---|---|
| `/ui/tenants/:tenant/databases/:database/policies` | Effective-policy inspection, explain/dry-run, MCP envelope preview | `policy-authoring.spec.ts`, `mcp-envelope-preview.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/schemas` policy tab | Edit `access_control`, compile, dry-run, activate | `policy-authoring.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/collections/:name` Data tab | Policy-safe list/detail, redaction, denied write handling | `policy-enforcement.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/collections/:name` Relationships tab | Policy-filtered relationship traversal and counts | `policy-enforcement.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/intents` | Pending/approved/rejected/expired/committed intent inbox | `approval-inbox.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/intents/:intent` | Diff, bindings, policy explanation, approve/reject/commit, stale handling | `approval-inbox.spec.ts`, `mutation-intents.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/audit` | Approval lineage, redacted audit reads, intent/audit deep links | `intent-audit-lineage.spec.ts` |
| `/ui/tenants/:tenant/databases/:database/graphql` | Policy and intent GraphQL reproduction | `graphql-policy-console.spec.ts` |

## Verification

Implementation beads must add:

- `ui/tests/e2e/policy-authoring.spec.ts`
- `ui/tests/e2e/policy-enforcement.spec.ts`
- `ui/tests/e2e/graphql-policy-console.spec.ts`
- `ui/tests/e2e/mutation-intents.spec.ts`
- `ui/tests/e2e/approval-inbox.spec.ts`
- `ui/tests/e2e/intent-audit-lineage.spec.ts`
- `ui/tests/e2e/mcp-envelope-preview.spec.ts`

These Playwright tests run against a live `axon-server` seeded with the
SCN-017 procurement fixture. Passing UI tests require the corresponding
GraphQL and MCP contract tests to pass first.

## Dependencies

- **FEAT-011 / ADR-006**: FEAT-031 extends the existing SvelteKit admin UI.
- **FEAT-015 / ADR-012**: GraphQL is the primary policy and intent UI API.
- **FEAT-016 / ADR-013**: MCP policy envelopes and intent outcomes are visible
  to operators.
- **FEAT-029 / ADR-019**: Data-layer policy compiler, enforcement,
  introspection, and explanations.
- **FEAT-030 / ADR-019**: Mutation preview, approval routing, stale-intent
  safety, and audit lineage.

## Out Of Scope

- A drag-and-drop or natural-language policy builder.
- Long-running business workflow orchestration beyond short-lived mutation
  intents.
- Custom per-application approval form builders.
- REST-only policy or approval workflows.
- Mobile-first approval experience.

## Traceability

### Related Artifacts

- **Parent PRD Sections**: Requirements Overview > GraphQL-primary
  application surface, Agent-native MCP surface, Governed mutation intents.
- **User Stories**: US-113, US-114, US-115, US-116, US-117, US-118, US-119.
- **Architecture**: ADR-006, ADR-012, ADR-013, ADR-019.
- **E2E coverage**: Target Playwright specs listed in Verification.

### Feature Dependencies

- **Depends On**: FEAT-011, FEAT-015, FEAT-016, FEAT-029, FEAT-030,
  ADR-006, ADR-012, ADR-013, ADR-019.
- **Depended By**: Policy-intents UI implementation beads.
