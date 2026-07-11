---
ddx:
  id: FEAT-031
  depends_on:
    - helix.prd
  review:
    self_hash: 6949273cd3f6b1e8f7fe71591cad9457da82bb6dd9fcea05f20228dd5a1ef0b8
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:37Z"
---
# Feature Specification: FEAT-031 — Policy and Intents Admin UI

**Feature ID**: FEAT-031
**Status**: approved
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-24 (policy-testing and approval admin-UI
flows; the base administration console is FEAT-011), FR-30 (operator UI
portion of diff/log/blame-style audit and repair views)
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: PUI

## Overview

Axon ships dedicated web UI coverage for the policy and mutation-intent
workflows defined by FEAT-029 and FEAT-030, implementing the policy-testing,
approval, and audit-explanation portions of PRD FR-24 and FR-30. The UI is
not the policy security boundary; GraphQL and MCP enforce policy below every
client. The UI is the operator and developer surface that makes those
decisions inspectable, reviewable, and testable.

FEAT-031 extends the FEAT-011 admin UI with database-scoped policy, intent,
approval, and audit-lineage workflows that use GraphQL as the primary surface
(policy and intent fields per CONTRACT-002). MCP-originated policy and intent
outcomes are visible in the UI so a human operator can understand what an
agent saw and why Axon allowed, denied, or routed a mutation for approval.

## Ideal Future State

An operator never needs a raw GraphQL call to answer "why did Axon allow,
deny, redact, or route that write?". They open the policy workspace, pick a
subject and operation, and read the same explanation the engine produced. A
developer edits a policy beside the schema, sees compile errors with rule IDs
and hints, dry-runs fixtures, and activates with confidence. An approver
works a keyboard-friendly inbox, sees exactly the diff that was previewed —
guaranteed by stale-intent bindings — and approves or rejects with a recorded
reason. When an agent misbehaves, the operator inspects the envelope the
agent received and the structured outcome of its tool call, instead of
guessing from logs.

## Problem Statement

- **Current situation**: FEAT-029 and FEAT-030 define governed reads,
  redaction, policy explanation, mutation preview, approval routing, and
  stale-intent protection — reachable only through raw GraphQL or MCP calls.
- **Pain points**: Operators must hand-write GraphQL to inspect policy
  results or approve risky writes. That is not a baseline product experience
  for the V1 policy proof, and it makes approvals slow and error-prone.
- **Desired outcome**: Every human-facing user story in the policy and intent
  slice has a corresponding Axon web UI workflow.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Policy workspace | What does the active policy do, and what would my change do? | Policy authoring beside the schema, compile reports, dry-run, effective-policy inspection, impact matrix, explain panel |
| Policy-safe entity browsing | Does the console itself respect policy? | Policy-filtered lists/relationships/counts, explicit redaction states, honest denied-write handling |
| Mutation intent review | What is about to change, and who signs off? | Preview diffs, intent inbox, intent detail, approve/reject, stale/mismatch safety |
| Audit lineage and MCP visibility | What happened, in what order, and what did the agent see? | Intent-linked audit chains, redacted audit reads, MCP envelope and outcome inspection |

## Requirements

### Functional Requirements by Area

#### Policy Workspace

- **PUI-01**. The schema workspace must expose the collection's
  access-control policy block beside the schema editor, validate it with the
  FEAT-029 compiler, and show compile errors with rule IDs, field paths, and
  remediation hints.
- **PUI-02**. Operators must be able to dry-run a policy change before
  activation: previewing a schema/policy change returns a compile report
  (errors, warnings, affected GraphQL nullability, MCP envelope changes); a
  failed compile blocks activation and leaves the active policy version
  unchanged; a successful dry-run can evaluate fixture subjects and rows
  before the new version is applied.
- **PUI-03**. The database workspace must render effective-policy and
  policy-explanation results — row visibility, field redaction, field write
  denial, and approval envelope summaries — for a selected subject,
  collection, and operation, using the GraphQL policy fields defined in
  CONTRACT-002. The GraphQL console must be able to reproduce the same
  results the policy panel shows.
- **PUI-04**. The UI must show the active schema/policy version on policy,
  entity, intent, and audit views.
- **PUI-05**. An impact matrix (subject × entity × operation grid) must
  surface the five entity-CRUD operations — read, create, update, patch,
  delete — whose semantics fit a single (subject, entity, operation) tuple
  with no further fixture context, optimized for at-a-glance policy review
  across many subjects and entities.
- **PUI-06**. An explain panel (drill-in for a single subject/entity/
  operation) must cover all eight policy operations: the five entity-CRUD
  operations plus transition, rollback, and transaction. The panel collects
  the extra fixture context these need — a from/to state pair for
  transitions, a target audit entry or version for rollbacks, and an
  operation list for transactions — before evaluating.
- **PUI-07**. Explaining a transaction fixture must return per-step outcomes
  in addition to the aggregate decision, with entity state threaded forward
  across steps: the subject context stays constant for the whole transaction,
  while a step that creates or updates an entity makes that resulting state
  visible to later steps' evaluation and a delete step removes it. Multi-step
  intents therefore preview accurately without writing any data.

#### Policy-Safe Entity Browsing

- **PUI-08**. Collection lists, relationship tabs, search results,
  pagination cursors, and counts must be rendered from policy-filtered
  GraphQL results, so hidden rows never appear in the console.
- **PUI-09**. Entity detail, list previews, relationship views, and audit
  views must render redacted fields as an explicit redacted state without
  leaking original values into the DOM.
- **PUI-10**. Entity edit, lifecycle, rollback, and transaction flows must
  display stable denial codes and policy explanations on denied writes and
  must not optimistically mutate UI state after a denied response.

#### Mutation Intent Review

- **PUI-11**. UI write flows that can require approval must show a
  preview-backed diff modal before commit, including affected rows,
  field-level diff, pre-image versions, operation hash, policy decision,
  explanation, expiry, and intent identifier (mutation-preview semantics per
  FEAT-030 and CONTRACT-002).
- **PUI-12**. A database-scoped intent inbox must list pending, approved,
  rejected, expired, committed, and stale intents with filters for status,
  requester, subject, approver role, risk reason, age, collection, and
  MCP/tool origin.
- **PUI-13**. The intent detail view must show the canonical operation,
  policy version, schema version, grant version, pre-images, diff, approval
  route, required reason, requester, agent/tool identity when present, and
  current commit eligibility.
- **PUI-14**. Authorized approvers must be able to approve or reject with a
  reason when policy requires one. The UI must block self-approval when the
  policy requires separation of duties and must surface stable structured
  errors when the caller lacks the approver role.
- **PUI-15**. If entity, schema, policy, grant, or operation-hash bindings
  change after preview, the UI must mark the intent stale or mismatched with
  the specific code, disable commit, and offer a re-preview/resubmit path
  that creates a new intent while preserving lineage to the prior one.

#### Audit Lineage and MCP Visibility

- **PUI-16**. Entity audit history and database audit detail must link
  preview, approval, rejection, expiry, and committed mutation events by
  intent identifier, with deep links from intent detail to the originating
  entity, audit entry, policy explanation, and GraphQL operation where
  applicable.
- **PUI-17**. Audit views must apply the same FEAT-029 field redaction as
  entity reads for the current viewer.
- **PUI-18**. Operators must be able to inspect the policy envelope an
  MCP-capable agent received for a tool call, plus the structured allowed /
  needs-approval / denied / conflict outcome; denied MCP results and the
  corresponding UI policy explanation must use the same stable reason codes
  (envelope semantics per CONTRACT-003 and CONTRACT-004).

### Non-Functional Requirements

- **GraphQL-primary**: All policy and intent UI workflows use the GraphQL
  operations defined by FEAT-029/FEAT-030 (CONTRACT-002). REST usage is
  limited to the documented exception classes in CONTRACT-001.
- **No client-side security boundary**: UI filtering is an affordance only;
  hidden rows, redaction, denial, and approval routing are enforced by the
  backend before the UI receives data.
- **No secret leakage**: Redacted field values, intent tokens, and credential
  metadata are not written to logs, browser storage, or hidden DOM attributes
  beyond the visible/copyable values intentionally exposed to operators.
- **Keyboard and dense-table usability**: The approval inbox supports
  keyboard selection, filtering, and approve/reject actions without requiring
  a route change per row.
- **Inbox responsiveness**: The intent inbox remains interactive with at
  least 500 pending intents; applying a filter renders in under 200 ms p95
  (target, assumption to validate).

## Screens

All screens live inside FEAT-011's tenant/database-scoped navigation model;
HTTP route surface per CONTRACT-001, GraphQL operations per CONTRACT-002.

| Screen | Operator capability |
|---|---|
| Policy workspace (database-scoped) | Effective-policy inspection, impact matrix, explain panel, MCP envelope preview |
| Schema policy tab | Edit the access-control block, compile, dry-run, activate |
| Collection data view | Policy-safe list/detail, redaction states, denied-write handling |
| Collection relationships view | Policy-filtered relationship traversal and counts |
| Intent inbox (database-scoped) | Filterable pending/approved/rejected/expired/committed/stale intent list |
| Intent detail | Diff, version bindings, policy explanation, approve/reject/commit, stale handling |
| Audit views | Approval lineage, redacted audit reads, intent/audit deep links |
| GraphQL console | Reproduce policy and intent operations shown elsewhere in the UI |

## User Stories

- [US-113 — Inspect Effective Policy In The Web UI](../user-stories/US-113-inspect-effective-policy-in-the-web-ui.md)
- [US-114 — Author And Dry-Run Policies Before Activation](../user-stories/US-114-author-and-dry-run-policies-before-activation.md)
- [US-115 — Browse Entities With Policy-Safe UI Semantics](../user-stories/US-115-browse-entities-with-policy-safe-ui-semantics.md)
- [US-116 — Preview And Commit Mutation Intents From The Web UI](../user-stories/US-116-preview-and-commit-mutation-intents-from-the-web-ui.md)
- [US-117 — Review, Approve, And Reject Pending Intents](../user-stories/US-117-review-approve-and-reject-pending-intents.md)
- [US-118 — Handle Stale And Mismatched Intents Safely](../user-stories/US-118-handle-stale-and-mismatched-intents-safely.md)
- [US-119 — Inspect MCP-Originated Policy And Intent Outcomes](../user-stories/US-119-inspect-mcp-originated-policy-and-intent-outcomes.md)

## Story Coverage Map

Every policy/intent story represented by FEAT-015, FEAT-016, FEAT-029, and
FEAT-030 has an Axon web UI acceptance surface (story IDs per the
user-stories registry ledger):

| Source story | Web UI coverage |
|---|---|
| US-048 GraphQL relationships | Entity relationship tabs use policy-filtered GraphQL traversal and counts |
| US-049 GraphQL introspection | GraphQL console and policy workspace expose policy-aware schema shape |
| US-050 GraphQL subscriptions | Live UI updates must not reveal hidden rows or redacted fields |
| US-057 GraphQL mutations | UI write flows use GraphQL preview/commit and policy error handling |
| US-110 Policy traversal | Relationship tabs, lists, and counts prove hidden target omission |
| US-111 Mutation intents | UI preview, approval, stale, and commit flows |
| US-051 Admin GraphQL | GraphQL console reproduces policy and intent operations |
| US-052 MCP discovery | Policy workspace shows generated agent tool envelopes |
| US-053 MCP CRUD | Audit and intent views identify MCP-originated writes |
| US-054 MCP GraphQL bridge | UI can compare agent query behavior with GraphQL console results |
| US-055 MCP subscriptions | Audit/lineage views show policy-safe agent notification effects |
| US-056 MCP stdio | Operator setup/status surface links stdio config to agent-originated audit |
| US-112 MCP policy envelopes | UI previews the envelope an agent role sees |
| US-101 Hidden entities | Lists, search, relationships, and counts omit hidden rows |
| US-102 Redaction | Entity and audit views render redacted state with no DOM leakage |
| US-103 Denied writes | Forms surface stable denial code and explanation |
| US-104 Explain policy | Policy workspace renders effective-policy and explanation results |
| US-109 Policy authoring | Schema route supports compile report and fixture dry-run |
| US-105 Preview mutation | Diff modal shows pre-image versions and policy decision |
| US-106 Approval routing | Approval inbox and detail support approve/reject with reason |
| US-107 Stale execution | UI disables stale/mismatched commits and supports re-preview |
| US-108 MCP intents | UI exposes MCP-originated intent outcomes and audit lineage |

## Edge Cases and Error Handling

- **Intent goes stale while open**: If bindings change while an approver has
  the detail view open, the next interaction surfaces the stale state and
  disables commit rather than submitting against changed state.
- **Approver loses the role mid-session**: Approve/reject actions surface the
  backend's structured authorization error; the UI does not pre-authorize
  from cached role state.
- **Expired intents**: Visible in history with their lineage, but cannot be
  approved or committed.
- **Redacted values and clipboard/export**: Redacted states are not
  copyable as original values anywhere in the UI, including exports and
  tooltips.
- **Envelope for revoked credentials**: MCP envelope inspection for a
  credential that has since been revoked shows the envelope as historical
  context tied to its grant version, not as current capability.

## Success Metrics

- Every human-facing FEAT-029/FEAT-030 user story has a working UI workflow
  (the Story Coverage Map is fully satisfiable in the console).
- An approver can triage, inspect, and resolve a pending intent end-to-end
  without writing a GraphQL operation by hand.
- Policy/UI parity checks find zero divergence between panel-rendered
  explanations and the same operations issued from the GraphQL console.
- Zero redacted-value leaks into the DOM, logs, or browser storage in
  enforcement test suites.

## Constraints and Assumptions

- FEAT-031 extends the FEAT-011 console and inherits its stack (ADR-006),
  navigation model, and deployment constraints.
- The backend policy engine (FEAT-029) and intent lifecycle (FEAT-030) exist
  below the UI; this feature renders and operates them, it does not
  re-implement decisions client-side.
- The inbox-responsiveness target is an assumption to validate against
  realistic intent volumes.
- Operator fixtures (sample subjects, rows, transactions) used by the explain
  panel are evaluated server-side; the UI only collects fixture context.

## Dependencies

- **Other features**: FEAT-011 (base admin console this extends), FEAT-015
  (GraphQL is the primary policy/intent UI API), FEAT-016 (MCP envelopes and
  outcomes surfaced to operators), FEAT-029 (policy compiler, enforcement,
  introspection, explanations), FEAT-030 (mutation preview, approval routing,
  stale-intent safety, audit lineage).
- **External services**: None. Normative surfaces: CONTRACT-001 (HTTP
  routes), CONTRACT-002 (GraphQL policy/intent fields, control-plane
  GraphQL), CONTRACT-003 (MCP envelopes), CONTRACT-004 (reason codes and
  denial envelopes). Governing decisions: ADR-006, ADR-012, ADR-013, ADR-019.
- **PRD requirements**: FR-24 (P1), FR-30 (P1) — as the operator-UI surface
  of the P0 policy/intent subsystems.

## Out of Scope

- A drag-and-drop or natural-language policy builder.
- Long-running business workflow orchestration beyond short-lived mutation
  intents.
- Custom per-application approval form builders.
- REST-only policy or approval workflows.
- Mobile-first approval experience.
