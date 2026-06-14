---
ddx:
  id: US-106
  review:
    self_hash: 5bed0ef5288306d1a49b185ea56b523ec4ca6a213585f24210d7986cf6da99d6
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-106: Route Risky Writes For Approval

**Feature**: FEAT-030 — Mutation Intents and Approval
**Feature Requirements**: INT-09, INT-10, INT-11, INT-15, INT-18
**PRD Requirements**: FR-7, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As an** operator (Wei, business workflow builder)
**I want** high-risk agent writes to require approval
**So that** low-risk work proceeds autonomously while sensitive changes stay
under human control

## Context

Allow/deny is not enough for business workflows: an invoice update under a
threshold should flow autonomously while one above it waits for a human.
This story exercises policy-envelope routing (INT-09), needs-approval intent
content (INT-10), approve/reject/pending operations (INT-11), the
approval-required behavior of direct writes (INT-15), and approval auditing
(INT-18).

## Walkthrough

1. Wei activates a policy envelope: invoice changes under $10,000 are
   autonomous; at or above, approval is required from a finance role.
2. Ava's agent previews a $12,000 invoice change; Axon returns
   `needs_approval` with the approver role, reason requirement, and intent
   ID.
3. Wei reviews the pending intent's diff and approves it with a reason.
4. The approval is audited with actor, reason, policy version, and intent
   ID; the agent may now commit the approved intent.

## Acceptance Criteria

- [ ] **US-106-AC1** — Given an approval policy envelope with a threshold,
  when changes below it are previewed, then they are `allow`; when at or
  above, then they are `needs_approval`.
- [ ] **US-106-AC2** — Given a `needs_approval` result, when it is
  returned, then it includes the required approver role, reason
  requirement, and intent ID.
- [ ] **US-106-AC3** — Given a generated direct write that hits the
  approval envelope, when it executes, then it returns an
  approval-required outcome, mutates nothing, and produces no entity/link
  mutation audit entry.
- [ ] **US-106-AC4** — Given a pending intent, when an approver with the
  required role approves or rejects it through GraphQL, then the intent's
  approval state changes accordingly (surface per CONTRACT-002).
- [ ] **US-106-AC5** — Given an approval or rejection, when audit is
  queried, then the event appears with actor, reason, policy version, and
  intent ID.

## Edge Cases

- **Approver lacks the required role**: approval fails authorization; the
  intent stays pending.
- **Intent expires while pending**: it can no longer be approved or
  committed; the expiration is audited.
- **Reason required but omitted**: the approval is rejected as invalid per
  the envelope's reason requirement.
- **State changes while pending**: the intent goes stale; approval of a
  stale intent cannot lead to commit (US-107).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Threshold routing | US-106-AC1 | Envelope: approval ≥ $10,000 | Preview $9,000 and $12,000 changes | `allow` and `needs_approval` respectively |
| Approval metadata | US-106-AC2 | $12,000 preview | Inspect result | Approver role `finance`, reason requirement, intent ID |
| Direct write blocked | US-106-AC3 | Same envelope | Direct generated mutation for $12,000 | Approval-required outcome; no mutation; no mutation audit entry |
| Approve flow | US-106-AC4 | Pending intent; finance approver | Approve with reason | Approval state `approved` |
| Audited decision | US-106-AC5 | Approved/rejected intent | Query audit | Actor, reason, policy version, intent ID present |

## Dependencies

- **Stories**: US-105 (preview creates the intent).
- **Feature Spec**: FEAT-030
- **Feature Requirements**: INT-09, INT-10, INT-11, INT-15, INT-18
- **PRD Requirements**: FR-7, FR-28
- **External**: ADR-019 (policy envelopes), CONTRACT-002 (GraphQL approve/
  reject/pending surfaces), CONTRACT-005 (intent lifecycle auditing)

## Out of Scope

- Stale-binding enforcement at commit (US-107).
- The admin UI intent inbox (FEAT-031).
- Notification or escalation workflows (FEAT-030 Out of Scope).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
