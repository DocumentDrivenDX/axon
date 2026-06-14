---
ddx:
  id: US-111
  review:
    self_hash: e25cc8d0ca761912d2f352e283d3ca71d727404df4afc7f0cf6a27f34fabe14d
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-111: Preview And Commit Mutation Intents

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-15, GQL-17
**PRD Requirements**: FR-7, FR-8, FR-20, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As** an agent or UI client built by Ava (agent application developer)
**I want** GraphQL to preview, approve, and commit mutation intents
**So that** governed writes use one primary API surface

## Context

The mutation-intent workflow (FEAT-030) is Axon's safe public write path;
GraphQL is its primary surface. This story exercises GQL-15 and GQL-17:
preview with diff and policy explanation, the approval workflow, stale-intent
rejection, and machine-readable outcome fields that SDKs and MCP must
preserve. Field shapes are normative in CONTRACT-002.

## Walkthrough

1. The client previews a proposed mutation and receives a diff, policy
   decision, pre-image versions, and — when allowed or approval-routed — an
   intent token.
2. A reviewer approves (or rejects) the intent; the action is audited.
3. The client commits the approved intent and receives the committed result
   with an audit entry linking back to the intent.
4. If the entity, policy, or operation changed since preview, the commit is
   rejected as stale and the client previews again.

## Acceptance Criteria

- [ ] **US-111-AC1** — Given a proposed mutation, when the client runs the
  preview operation, then the response includes a diff, policy decision,
  pre-image versions, and an intent token when applicable (shapes per
  CONTRACT-002).
- [ ] **US-111-AC2** — Given a pending intent, when an operator approves or
  rejects it, then the operator action is recorded in the audit log.
- [ ] **US-111-AC3** — Given an intent whose entity version, policy version,
  or operation hash changed since preview, when the client commits it, then
  the commit is rejected as stale naming the stale dimension.
- [ ] **US-111-AC4** — Given a committed intent, when the resulting audit
  entry is inspected, then it links to the approved intent.
- [ ] **US-111-AC5** — Given preview, stale, conflict, approval-required, and
  committed responses, when any client inspects them, then each exposes
  stable machine-readable fields that SDKs and MCP tools preserve.

## Edge Cases

- **Double commit of the same intent**: The second commit does not apply the
  mutation twice; it returns a structured already-committed/invalid-intent
  outcome.
- **Reject then commit**: Committing a rejected intent fails with a
  structured outcome; no mutation applies.
- **Approval by an unauthorized subject**: The approval is denied by policy
  and audited as a denied action.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Preview | US-111-AC1 | Invoice at version 5, amount 9000→12000 | Preview mutation | Diff + needs-approval decision + token bound to version 5 |
| Audited approval | US-111-AC2 | Pending intent | Approve as reviewer | Approval recorded in audit log with actor |
| Stale rejection | US-111-AC3 | Entity advances to version 6 after preview | Commit the version-5 intent | Stale rejection naming the pre-image dimension; no mutation |
| Linked audit | US-111-AC4 | Approved intent committed | Read mutation audit entry | Entry references the intent ID |
| Field stability | US-111-AC5 | All five outcome kinds | Compare GraphQL fields with SDK/MCP fixtures | Identical machine-readable fields |

## Dependencies

- **Stories**: US-057 (generated mutations), US-110 (policy enforcement)
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-15, GQL-17
- **PRD Requirements**: FR-7, FR-8, FR-20, FR-28
- **External**: CONTRACT-002 (intent field shapes), FEAT-030 (intent
  lifecycle semantics)

## Out of Scope

- Intent lifecycle internals, expiry, and binding rules (FEAT-030 stories).
- MCP intent mirroring (US-112, FEAT-030 US-108).
- Approval routing configuration and reviewer UI (FEAT-031 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
