---
ddx:
  id: US-117
  review:
    self_hash: 8d1598d1d40e5980b2bb7aac0ff2e86f778b98b9ea45b5ce0dfc62dd9f3d8a2f
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-117: Review, Approve, And Reject Pending Intents

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-12, PUI-13, PUI-14
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As a** finance approver (Wei, Business Workflow Builder persona)
**I want** an approval inbox and intent detail view
**So that** I can approve or reject high-risk agent writes with enough context

## Context

Approval routing only works if approvers have a fast, complete review
surface: a filterable inbox of intents and a detail view carrying the
canonical operation, diff, bindings, and approval route. Separation of duties
must be enforced visibly.

## Walkthrough

1. Approver opens the database intent inbox and filters to pending intents.
2. System lists intents with status, requester, subject, collection,
   operation, policy reason, required role, age, expiry, and MCP/tool origin.
3. Approver opens an intent's detail view and reviews the canonical
   operation, diff, pre-images, version bindings, and approval route.
4. Approver approves with a reason; the system writes an approval audit entry
   and marks the intent commit-eligible. Rejection records actor, reason,
   policy version, and intent ID.

## Acceptance Criteria

- [ ] **US-117-AC1** — Given pending intents, when the approver opens the
  inbox, then intents are listed with status, requester, subject, collection,
  operation, policy reason, required role, age, expiry, and MCP/tool origin
  when present.
- [ ] **US-117-AC2** — Given an intent, when the approver opens its detail
  view, then it shows the canonical operation, diff, policy explanation,
  pre-images, version bindings, approval route, and audit links.
- [ ] **US-117-AC3** — Given an over-threshold intent, when an approver with
  the configured role approves with a reason, then an approval audit entry is
  written and the intent becomes commit-eligible.
- [ ] **US-117-AC4** — Given an intent, when it is rejected, then actor,
  reason, policy version, and intent ID are recorded and the intent can never
  commit.
- [ ] **US-117-AC5** — Given a policy requiring separation of duties, when
  the requester attempts to approve their own intent, then the UI blocks
  self-approval and surfaces the structured error.

## Edge Cases

- **Caller lacks approver role**: approve/reject actions surface the stable
  structured error from the backend.
- **Keyboard operation**: triage, filter, and approve/reject are operable by
  keyboard without a route change per row.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Inbox listing | US-117-AC1 | 5 intents, mixed status | Open inbox, filter pending | Pending rows with full columns |
| Detail review | US-117-AC2 | Pending intent | Open detail | Operation, diff, bindings, route shown |
| Approve with reason | US-117-AC3 | Over-threshold invoice intent | Approve as finance role | Audit entry; commit-eligible |
| Reject permanently | US-117-AC4 | Pending intent | Reject with reason | Recorded; commit impossible |
| Self-approval block | US-117-AC5 | Requester = approver | Attempt approve | Blocked with structured error |

## Dependencies

- **Stories**: US-106 (FEAT-030 approval routing), US-116 (intents created
  from UI)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-12, PUI-13, PUI-14
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-002 (intent GraphQL operations), CONTRACT-005
  (approval audit records)

## Out of Scope

- Stale/mismatch handling (US-118).
- Custom approval form builders.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
