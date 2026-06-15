---
ddx:
  id: US-105
  review:
    self_hash: 42a149a9dd97579d04548cba7dd65f8ba7c829bcbb7fa25313453e454fce3037
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-105: Preview A GraphQL Mutation

**Feature**: FEAT-030 — Mutation Intents and Approval
**Feature Requirements**: INT-01, INT-02, INT-03, INT-04, INT-16, INT-17
**PRD Requirements**: FR-7, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As an** agent developer (Ava, agent application developer)
**I want** to preview a GraphQL mutation before commit
**So that** the agent can show the operator what will change and why

## Context

Trustworthy agent writes start with a faithful preview: the same validation,
transition, and policy rules as commit, a field-level diff, and an explicit
decision — without touching state. This story exercises preview behavior
(INT-01..INT-03), intent record creation (INT-04), machine-readable decision
fields (INT-16), and the preview/audit separation (INT-17). The GraphQL
field surface is normative in CONTRACT-002.

## Walkthrough

1. Ava's agent submits a preview of an invoice update through GraphQL.
2. Axon returns the affected entity ID, pre-image version, field-level
   diff, and the policy decision with rule explanation.
3. Because the decision is `allow`, the response carries an executable
   intent token backed by a server-side intent record binding schema
   version, policy version, operation hash, and pre-image versions.
4. The agent shows the diff to the operator; nothing has mutated, and no
   data-mutation audit entry exists for the preview.

## Acceptance Criteria

- [ ] **US-105-AC1** — Given an invoice update preview, when it executes,
  then the response includes the affected entity ID, pre-image version,
  field-level diff, and policy decision.
- [ ] **US-105-AC2** — Given a mutation that policy denies, when it is
  previewed, then the response is `deny` with the matching policy rule and
  no executable intent token.
- [ ] **US-105-AC3** — Given any preview, when it completes, then no
  entity/link mutation audit entry is created and no entity/link state
  changes (preview-audit semantics per ADR-023 and CONTRACT-005).
- [ ] **US-105-AC4** — Given identical state, when the same operation is
  previewed and committed, then preview applied the same validation,
  transition, and policy rules as commit.
- [ ] **US-105-AC5** — Given a preview that returns an executable token,
  when the intent record is inspected, then it stores schema version,
  policy version, operation hash, and all pre-image versions.
- [ ] **US-105-AC6** — Given preview responses on any surface, when
  consumed by SDK, CLI, MCP, or operator UI clients, then the decision
  fields are stable and machine-readable without parsing human text
  (vocabulary per CONTRACT-002/CONTRACT-009).

## Edge Cases

- **Preview of an invalid payload**: validation failures surface in the
  preview result exactly as commit would report them; no token is issued.
- **Preview of a lifecycle transition or link mutation**: same preview
  shape applies to all governed mutation shapes, including rollback/revert
  requests.
- **Repeated previews**: each preview creates its own intent record; old
  un-committed intents simply expire.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Allowed preview | US-105-AC1 | Invoice v5; subject allowed | Preview amount change | Diff, entity ID, pre-image v5, decision `allow`, token |
| Denied preview | US-105-AC2 | Policy denies vendor edits for subject | Preview vendor edit | `deny` with rule name; no token |
| No side effects | US-105-AC3 | Any preview | Query audit + reread entity | No mutation audit entry; entity unchanged |
| Parity with commit | US-105-AC4 | Same state, same operation | Preview, then commit | Same decision and validation outcomes |
| Record binding | US-105-AC5 | Allowed preview | Inspect intent record | Schema/policy versions, operation hash, pre-image versions present |

## Dependencies

- **Stories**: None (entry point for US-106, US-107).
- **Feature Spec**: FEAT-030
- **Feature Requirements**: INT-01, INT-02, INT-03, INT-04, INT-16, INT-17
- **PRD Requirements**: FR-7, FR-28
- **External**: CONTRACT-002 (GraphQL intent fields), CONTRACT-009 (SDK),
  CONTRACT-005 (intent lifecycle audit threading), ADR-019 (intent record),
  ADR-023 (preview-audit threading)

## Out of Scope

- Approval routing and approver actions (US-106).
- Stale/mismatch rejection at commit (US-107).
- MCP mirroring (US-108).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
