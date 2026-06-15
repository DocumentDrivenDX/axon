---
ddx:
  id: FEAT-030
  depends_on:
    - helix.prd
  review:
    self_hash: 81a89ddb42efe517ddde6ea7481c104b3600481a32072e31bd9d94cd7294922d
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Feature Specification: FEAT-030 — Mutation Intents and Approval

**Feature ID**: FEAT-030
**Status**: in_review
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-22
**Updated**: 2026-06-10
**Requirement Prefix**: INT
**Covered PRD Subsystem(s)**: Guardrailed Transactions and Mutation Intents; API and Deployment Surfaces
**Covered PRD Requirements**: FR-7, FR-8; FR-28 (governed-path-by-default behavior of the generated surfaces)
**Cross-Subsystem Rationale**: The preview → intent → approval → commit workflow IS the feature: it binds the transaction-safety subsystem (FR-7/FR-8) to the generated GraphQL/MCP/SDK/CLI surfaces that must expose it as the default write path (FR-28). The workflow is incoherent if either half ships alone.

## Overview

Axon supports previewable, explainable, approval-routable mutations through
GraphQL and MCP, implementing PRD FR-7 and FR-8. An agent or human can ask
Axon what a write would change, which policy rules would fire, whether the
write can execute autonomously, and whether a human approval is required
before commit.

GraphQL is the primary client surface for mutation intents. MCP mirrors the
same workflow for agents. REST is a fallback only for operational cases where
GraphQL is intractable.

The intent workflow is Axon's safe public write path (FR-28). Generated
GraphQL, MCP, SDK, CLI, and operator surfaces must make preview, intent,
approval, and commit discoverable without requiring application developers to
invent wrapper conventions.

## Ideal Future State

An agent proposes a write and immediately sees the diff, the affected
records, the policy rules that fired, and whether a human must approve. A
human approver reviews exactly the diff that will commit — never a different
one — because the approval is bound to the reviewed pre-image, schema
version, policy version, subject, and operation hash. If anything moves
between review and commit, the commit fails with a named stale dimension and
the caller previews again. Every surface (GraphQL, MCP, SDK, CLI, operator
UI) speaks the same machine-readable decision vocabulary, so coordinating a
governed write requires no custom Axon logic anywhere.

## Problem Statement

- **Current situation**: Agents can submit structurally valid writes that
  are too risky to commit without human review. A simple allow/deny policy
  is not enough for business workflows such as invoice approval, vendor
  changes, access grants, or contract edits.
- **Pain points**: Without a "valid, but requires approval" middle state,
  teams either over-block agents or let risky writes through. Naive
  approval flows are stale-prone: a human approves a diff for version 5
  and accidentally commits a different diff against version 7.
- **Desired outcome**: A preview/intent/approval/commit workflow in which
  the approval artifact is bound to the exact pre-image, policy version,
  and operation that was reviewed, and stale bindings always fail closed.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Mutation preview | What would this write change, and would it be allowed? | Side-effect-free preview with diff, policy explanation, and approval route for every governed mutation shape |
| Intent token and record | How is the reviewed state bound to the eventual commit? | Server-side intent records and opaque tokens binding subject, versions, pre-images, and operation hash |
| Approval routing | Who must approve this, and how do they act on it? | Needs-approval routing, approve/reject operations, pending-intent review queries |
| TOCTOU-safe execution | Can an approval be reused for a different write? | Commit-time revalidation of every bound dimension; stale/mismatch failures that never partially commit |
| Audit and observability | Who proposed, approved, and committed what? | Intent lifecycle threading into audit with preview kept distinct from data mutations |

## Requirements

### Functional Requirements by Area

#### Mutation Preview

- **INT-01**. Axon must preview generated entity mutations, generic
  transaction mutations, lifecycle transitions, link mutations, and
  rollback/revert requests without mutating entity or link state. The
  GraphQL preview/commit/approve/reject field surface is defined in
  [CONTRACT-002 — GraphQL surface](../../02-design/contracts/CONTRACT-002-graphql-surface.md)
  (Policy and mutation-intent fields); MCP mirrors it per CONTRACT-003;
  SDK wrappers per
  [CONTRACT-009 — SDK surface](../../02-design/contracts/CONTRACT-009-sdk-surface.md).
- **INT-02**. Preview must return the canonical operation, computed diff,
  affected fields, affected entity/link IDs, pre-image versions, policy
  decision, rule explanation, approval route if required, schema version,
  policy version, and the audit references that will connect the eventual
  commit to the preview.
- **INT-03**. Preview must apply the same validation, transition, and
  policy rules as commit, so a preview decision is predictive of the
  commit decision against unchanged state.

#### Intent Token and Record

- **INT-04**. A preview decided `allow` or `needs_approval` must return
  an opaque intent token referencing a server-side intent record as
  defined by ADR-019. The record must capture the machine-readable
  decision context — scope binding, subject and delegation, version
  bindings, operation hash, reviewed pre-image versions, decision,
  approval state, expiry, and a reviewer-safe summary. The normative
  record and field surface are defined by ADR-019 and CONTRACT-002.
- **INT-05**. The token must be bound to tenant, database, subject,
  credential ID, grant version, schema version, policy version, operation
  hash, and all affected pre-image versions (TOCTOU safety).
- **INT-06**. Tokens must expire; expired tokens can be neither executed
  nor approved. Tokens must be single-use once committed.
- **INT-07**. Tokens are not self-authorizing bearer grants: executing a
  token must re-check the caller's current authorization envelope.
- **INT-08**. Pending intents are short-lived review records, not
  workflow instances: they must not schedule retries, timers,
  notifications, or long-running steps.

#### Approval Routing

- **INT-09**. Policy envelopes (ADR-019) decide whether a valid mutation
  is autonomous, approval-routed, or denied. A denied preview must return
  the matching policy explanation and no executable intent token.
- **INT-10**. `needs_approval` intents must record the required approver
  role, reason requirements, approval deadline, and a summary safe to
  show approvers.
- **INT-11**. Operators must be able to approve, reject, and query
  pending intents; agents must receive the same approval state in
  structured tool results so they can pause, report, or request human
  review. Surfaces per CONTRACT-002 (GraphQL), CONTRACT-003 (MCP),
  CONTRACT-008 (CLI), and CONTRACT-009 (SDK).

#### TOCTOU-Safe Execution

- **INT-12**. Committing an intent must revalidate the operation hash,
  schema version, policy version, grant version, subject scope, approval
  state, and every pre-image version.
- **INT-13**. If any bound value changed, execution must fail with a
  stale-intent outcome naming the stale dimension so the caller can
  preview again; a mismatched operation hash must fail as a mismatch. A
  stale or mismatched intent must never partially commit.
- **INT-14**. In multi-entity transactions, all affected pre-image
  versions are bound; one stale entity invalidates the whole intent.
- **INT-15**. Generated direct write surfaces that receive
  `needs_approval` from policy must return an approval-required outcome
  and must not mutate entity/link state (FR-28: the governed path is the
  only path through the approval envelope).
- **INT-16**. All surfaces must preserve the machine-readable decision
  fields (decision, reason, policy, approval route, intent ID, version
  bindings, stale dimension, operation hash, transaction ID, audit
  reference) without requiring clients to parse human-readable text; the
  exact field vocabulary is defined in CONTRACT-002 and CONTRACT-009.

#### Audit and Observability

- **INT-17**. Preview must be recorded as an operational event distinct
  from data-mutation audit entries; preview must never produce an
  entity/link mutation audit entry. Preview-audit threading semantics are
  governed by
  [ADR-023 — preview audit threading](../../02-design/adr/ADR-023-preview-audit-threading.md),
  with the normative event shape in
  [CONTRACT-005 — audit record](../../02-design/contracts/CONTRACT-005-audit-record.md)
  (Mutation-intent lifecycle threading).
- **INT-18**. Approval, rejection, expiration, and execution must be
  audited. A successful commit's mutation audit entry must include the
  intent ID, policy decision trace, approval ID if present, and pre/post
  images with caller-appropriate redaction.
- **INT-19**. Audit queries must answer: who proposed the change, which
  agent/tool proposed it, who approved or rejected it, which
  policy/version required approval, and what diff was reviewed.

### Non-Functional Requirements

- **Performance**: preview latency < 50 ms p99 for a single-entity write
  with indexed policy predicates; intent execution overhead < 10 ms above
  the underlying mutation path.
- **Determinism**: intent token validation must be deterministic and
  independent of wall-clock ordering except for expiration.
- **Isolation**: intent storage must be tenant/database scoped.

## User Stories

- [US-105 — Preview A GraphQL Mutation](../user-stories/US-105-preview-a-graphql-mutation.md)
- [US-106 — Route Risky Writes For Approval](../user-stories/US-106-route-risky-writes-for-approval.md)
- [US-107 — Prevent Stale Approval Execution](../user-stories/US-107-prevent-stale-approval-execution.md)
- [US-108 — Use Mutation Intents From MCP](../user-stories/US-108-use-mutation-intents-from-mcp.md)

## Edge Cases and Error Handling

- **Policy change during approval**: the intent becomes stale and must be
  previewed again.
- **Entity change during approval**: the intent becomes stale and must be
  previewed again.
- **Approver loses role**: approval or commit fails authorization.
- **Token replay**: committed tokens are rejected on reuse.
- **Multi-entity transaction**: all affected pre-image versions are
  bound; one stale entity invalidates the whole intent.
- **Rollback preview**: rollback/revert requests produce the same diff
  and approval flow as ordinary writes (FEAT-023).
- **Expiry during review**: an intent that expires while pending can no
  longer be approved or committed; the expiration is audited.

## Success Metrics

- 100% stale-intent rejection for changed pre-image, schema, policy,
  grant, subject binding, or operation hash (PRD approval-safety metric).
- 0 entity/link mutations from previews, denied intents, or
  approval-required direct writes.
- 100% of committed intents traceable in audit to proposer, approver (if
  any), governing policy version, and reviewed diff.
- GraphQL, MCP, SDK, and CLI return identical decision vocabulary for
  the same intent outcome in the shared parity fixture suite.

## Constraints and Assumptions

- Policy envelopes and the intent record model are governed by ADR-019;
  this feature does not define its own policy grammar.
- Intent validity is bound to schema and policy versions, so FEAT-017
  schema versioning and FEAT-029 policy versioning must expose stable
  version identifiers.
- The intent store is short-lived review state, not a durable workflow
  queue; long-running orchestration stays outside Axon (PRD Non-Goals).
- GraphQL remains canonical: MCP, SDK, and CLI mirror its semantics
  rather than defining divergent intent workflows.

## Dependencies

- **Other features**: FEAT-003 (audit records approvals, rejections, and
  committed intent lineage), FEAT-005 (shared API, CLI, and SDK surfaces
  expose the governed write contract), FEAT-012 / ADR-018 (identity,
  credentials, grants, and tenant/database scopes), FEAT-015 (GraphQL is
  the primary intent workflow surface), FEAT-016 (MCP mirrors GraphQL
  semantics for agents), FEAT-017 (schema and policy versions bind intent
  validity), FEAT-029 / ADR-019 (data policies and envelopes produce
  allow, needs-approval, or deny decisions).
- **External services**: None. Normative surfaces live in CONTRACT-002
  (GraphQL intent fields), CONTRACT-003 (MCP), CONTRACT-005 (intent
  lifecycle audit threading), CONTRACT-008 (CLI), and CONTRACT-009 (SDK);
  preview-audit semantics in ADR-023.
- **PRD requirements**: FR-7 (P0), FR-8 (P0), FR-28 (P0); contributes to
  FR-29's governed-workflow SDK operations via CONTRACT-009.

FEAT-022 agent guardrails integrate with mutation intents later by adding
rate/scope risk signals, but FEAT-030 does not depend on FEAT-022 to define
the baseline preview, approval, and stale-intent safety model.

## Out of Scope

- Durable long-running workflow orchestration.
- Arbitrary external semantic validation hooks (see FEAT-022's
  parking-lot entry).
- Broad REST parity for preview and approval.
- Custom application-specific approval workflow builders beyond FEAT-031's
  baseline Axon web UI intent inbox and intent detail flows.
- Graph-wide arbitrary point-in-time rollback (see FEAT-023 Out of
  Scope).
