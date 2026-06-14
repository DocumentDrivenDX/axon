---
ddx:
  id: US-116
  review:
    self_hash: 16cf95fa023fe0c076904028c354428f0041d48fc25da4e056d12b87cf0965b5
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-116: Preview And Commit Mutation Intents From The Web UI

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-11
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As a** human user performing a risky write (Wei, Business Workflow Builder persona)
**I want** the UI to preview the mutation and show the policy decision before commit
**So that** I know exactly what Axon will write and why

## Context

FEAT-030 defines mutation preview and intent binding; this story puts the
preview in front of every UI write flow that can require approval: a diff
modal with affected entities, pre-image versions, policy decision,
explanation, expiry, and intent identifier.

## Walkthrough

1. User edits an entity through a UI write flow that can require approval.
2. System calls mutation preview and shows the diff modal with the policy
   decision and intent details.
3. For an allowed under-threshold change, the user commits from the modal.
4. System commits through GraphQL and links to the resulting audit entry and
   updated entity.

## Acceptance Criteria

- [ ] **US-116-AC1** — Given a UI write flow that can require approval, when
  the user submits, then the UI shows a preview with affected entities,
  field-level diff, pre-image versions, policy decision, explanation, expiry,
  and intent identifier before commit.
- [ ] **US-116-AC2** — Given an under-threshold allowed change, when the user
  commits from the preview, then the mutation commits through GraphQL without
  approval.
- [ ] **US-116-AC3** — Given a denied preview, when the modal renders, then
  it shows the denial reason and exposes no executable intent token.
- [ ] **US-116-AC4** — Given a successful commit, when the UI confirms, then
  it links to the resulting audit entry and the updated entity.

## Edge Cases

- **Preview expiry**: an expired preview cannot be committed; the UI offers
  re-preview.
- **Approval-routed preview**: a needs-approval decision routes to the intent
  inbox (US-117) instead of offering direct commit.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Preview modal | US-116-AC1 | Invoice edit | Submit | Diff, decision, pre-images, expiry, intent ID shown |
| Direct commit | US-116-AC2 | Under-threshold change | Commit from modal | Mutation applied, no approval |
| Denied preview | US-116-AC3 | Policy-denied change | Preview | Reason shown; no executable token |
| Audit linkage | US-116-AC4 | Committed change | Follow links | Audit entry and updated entity open |

## Dependencies

- **Stories**: US-105 (FEAT-030 preview), US-113 (policy explanations)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-11
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-002 (mutation-intent GraphQL fields), CONTRACT-004
  (decision/reason codes)

## Out of Scope

- Approver workflows (US-117) and stale handling detail (US-118).
- Intent lifecycle semantics (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
