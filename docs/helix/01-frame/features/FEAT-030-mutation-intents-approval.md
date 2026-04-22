---
ddx:
  id: FEAT-030
  depends_on:
    - helix.prd
    - FEAT-003
    - FEAT-012
    - FEAT-015
    - FEAT-016
    - FEAT-017
    - FEAT-029
    - ADR-019
---
# Feature Specification: FEAT-030 - Mutation Intents and Approval

**Feature ID**: FEAT-030
**Status**: Specified
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-22
**Updated**: 2026-04-22

## Overview

Axon supports previewable, explainable, approval-routable mutations through
GraphQL and MCP. An agent or human can ask Axon what a write would change,
which policy rules would fire, whether the write can execute autonomously, and
whether a human approval is required before commit.

GraphQL is the primary client surface for mutation intents. MCP mirrors the
same workflow for agents. REST is a fallback only for operational cases where
GraphQL is intractable.

## Problem Statement

Agents can submit structurally valid writes that are too risky to commit
without human review. A simple allow/deny policy is not enough for business
workflows such as invoice approval, vendor changes, access grants, or contract
edits. Axon needs a middle state: "valid, but requires approval."

Preview and approval must also avoid stale approvals. A human must not approve
a diff for version 5 and accidentally commit a different diff against version
7. The approval artifact must be bound to the exact pre-image, policy version,
and operation that was reviewed.

## Requirements

### Functional Requirements

#### Mutation Preview

- GraphQL exposes `previewMutation(input)` for generated entity mutations,
  generic transaction mutations, lifecycle transitions, link mutations, and
  rollback/revert requests.
- MCP tools expose `preview: true` or paired preview tools for the same
  operations.
- Preview returns the canonical operation, computed diff, affected fields,
  affected entity/link IDs, pre-image versions, policy decision, rule
  explanation, and approval route if required.
- Preview never mutates entity or link state.

#### Mutation Intent Token

- A preview that is `allow` or `needs_approval` returns an opaque intent token.
- The token is bound to tenant, database, subject, credential ID, grant
  version, schema version, policy version, operation hash, and all affected
  pre-image versions.
- The token references a server-side intent record as defined by ADR-019. It is
  not a self-authorizing bearer grant.
- Tokens expire. Expired tokens cannot be executed or approved.
- Tokens are single-use once committed.
- Tokens are not bearer authorization by themselves. Executing a token still
  re-checks the caller's current authorization envelope.

Intent record fields:

| Field | Purpose |
|---|---|
| `intent_id` | Stable ID for approval, audit, and token lookup |
| `tenant_id`, `database_id` | Scope binding |
| `subject` | `user_id`, `agent_id`, `delegated_by`, `credential_id`, `grant_version` |
| `schema_version`, `policy_version` | Version binding for TOCTOU safety |
| `operation_hash` | Hash of canonical mutation/transaction input |
| `pre_images[]` | Entity/link IDs and versions reviewed in preview |
| `decision` | `allow`, `needs_approval`, or `deny` |
| `approval_state` | `none`, `pending`, `approved`, `rejected`, `expired`, `committed` |
| `expires_at` | TTL for review and execution |
| `review_summary` | Diff/risk summary safe to show to approvers |

Pending intents are short-lived review records, not workflow instances. They do
not schedule retries, timers, notifications, or long-running steps.

#### Approval Routing

- Policy envelopes from ADR-019 decide whether a valid mutation is
  autonomous, approval-routed, or denied.
- `needs_approval` intents record the required approver role, reason
  requirements, approval deadline, and summary visible to operators.
- GraphQL exposes approval, rejection, and pending-intent queries for operator
  workflows.
- MCP exposes the same approval state in structured tool results so agents can
  pause, report, or ask a human to review.

#### Intent Execution

- `commitMutationIntent(input)` executes an allowed or approved intent.
- Execution revalidates the operation hash, schema version, policy version,
  grant version, subject scope, and every pre-image version.
- If any bound value changed, execution fails with `intent_stale` and returns
  the stale dimension so the caller can preview again.
- If execution succeeds, the normal mutation audit entry includes the intent ID,
  policy decision trace, approval ID if present, and pre/post images with
  caller-appropriate redaction.

#### Audit And Observability

- Preview may be recorded as an operational event, but it is not a data
  mutation audit entry.
- Approval, rejection, expiration, and execution are audited.
- Audit queries can answer: who proposed the change, which agent/tool proposed
  it, who approved or rejected it, which policy/version required approval, and
  what diff was reviewed.

### Non-Functional Requirements

- Preview latency target: <50ms p99 for a single-entity write with indexed
  policy predicates.
- Intent execution overhead target: <10ms above the underlying mutation path.
- Intent token validation must be deterministic and independent of wall-clock
  ordering except for expiration.
- Intent storage must be tenant/database scoped.

## GraphQL Shape

Conceptual fields:

```graphql
type Mutation {
  previewMutation(input: MutationPreviewInput!): MutationPreviewResult!
  approveMutationIntent(input: ApproveIntentInput!): MutationIntent!
  rejectMutationIntent(input: RejectIntentInput!): MutationIntent!
  commitMutationIntent(input: CommitIntentInput!): CommitIntentResult!
}

type Query {
  pendingMutationIntents(filter: MutationIntentFilter): MutationIntentConnection!
  mutationIntent(id: ID!): MutationIntent
}
```

Generated collection mutations may also accept `mode: PREVIEW | COMMIT`.
The generic shape remains canonical so agents and SDKs can implement one
intent workflow across collections.

## User Stories

### Story US-105: Preview A GraphQL Mutation [FEAT-030]

**As an** agent developer
**I want** to preview a GraphQL mutation before commit
**So that** the agent can show the operator what will change and why

**Acceptance Criteria:**
- [ ] Preview of an invoice update returns affected entity ID, pre-image
  version, field-level diff, and policy decision.
- [ ] Preview of a denied mutation returns `deny` with the matching policy rule
  and no executable intent token.
- [ ] Preview does not create an entity/link mutation audit entry.
- [ ] Preview applies the same validation, transition, and policy rules as
  commit.
- [ ] Preview stores an intent record with schema version, policy version,
  operation hash, and pre-image versions when it returns an executable token.

### Story US-106: Route Risky Writes For Approval [FEAT-030]

**As an** operator
**I want** high-risk agent writes to require approval
**So that** low-risk work can proceed autonomously while sensitive changes stay
under human control

**Acceptance Criteria:**
- [ ] A policy envelope can allow invoice changes under a threshold and require
  approval above it.
- [ ] A `needs_approval` result includes approval role, reason requirement, and
  intent ID.
- [ ] An approver can approve or reject through GraphQL.
- [ ] Approval and rejection are audited with actor, reason, policy version,
  and intent ID.

### Story US-107: Prevent Stale Approval Execution [FEAT-030]

**As a** compliance reviewer
**I want** approved mutations to execute only against the reviewed state
**So that** approval cannot be reused for a different write

**Acceptance Criteria:**
- [ ] Executing an intent after the target entity version changes returns
  `intent_stale`.
- [ ] Executing an intent after the policy version changes returns
  `intent_stale`.
- [ ] Executing an intent with a different operation hash returns
  `intent_mismatch`.
- [ ] A stale or mismatched intent cannot partially commit.

### Story US-108: Use Mutation Intents From MCP [FEAT-030]

**As an** MCP-capable agent
**I want** tool results to expose preview, approval, and conflict states
**So that** I can coordinate governed writes without custom Axon logic

**Acceptance Criteria:**
- [ ] MCP tool descriptions include policy envelope summaries.
- [ ] A tool call that needs approval returns structured `needs_approval`
  output with intent token and approval summary.
- [ ] A denied tool call returns structured policy explanation.
- [ ] `axon.query` follows the same GraphQL intent semantics.

## Edge Cases

- **Policy change during approval**: the intent becomes stale and must be
  previewed again.
- **Entity change during approval**: the intent becomes stale and must be
  previewed again.
- **Approver loses role**: approval or commit fails authorization.
- **Token replay**: committed tokens are rejected on reuse.
- **Multi-entity transaction**: all affected pre-image versions are bound; one
  stale entity invalidates the whole intent.
- **Rollback preview**: rollback/revert requests produce the same diff and
  approval flow as ordinary writes.

## Dependencies

- **FEAT-003**: Audit records approvals, rejections, and committed intent
  lineage.
- **FEAT-012 / ADR-018**: Identity, credentials, grants, and tenant/database
  scopes.
- **FEAT-015**: GraphQL is the primary intent workflow surface.
- **FEAT-016**: MCP mirrors GraphQL semantics for agents.
- **FEAT-017**: Schema and policy versions bind intent validity.
- **FEAT-029 / ADR-019**: Data policies and envelopes produce allow,
  needs-approval, or deny decisions.

FEAT-022 agent guardrails integrate with mutation intents later by adding
rate/scope risk signals, but FEAT-030 does not depend on FEAT-022 to define the
baseline preview, approval, and stale-intent safety model.

## Out Of Scope

- Durable long-running workflow orchestration.
- Arbitrary external semantic validation hooks.
- Broad REST parity for preview and approval.
- Approval UI design beyond the GraphQL/MCP contract.
- Graph-wide arbitrary point-in-time rollback.

## Verification

Implementation beads must add tests for:

- GraphQL preview/commit happy path.
- GraphQL `needs_approval` path and approval audit.
- Intent stale detection for entity version, policy version, and schema version.
- Operation hash mismatch rejection.
- MCP structured `needs_approval`, `denied`, and `conflict` outputs.
- Multi-entity transaction intent binding.
