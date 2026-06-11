---
ddx:
  id: US-112
---

# US-112: Agent Discovers Policy Envelopes

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-04, MCP-05, MCP-06, MCP-07
**PRD Requirements**: FR-12, FR-21, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority)
**I want** MCP tool metadata to describe allowed, approval-routed, and denied
write envelopes
**So that** I can choose safe actions before attempting a mutation

## Context

The safe path must be the discoverable path: an agent should know before
acting whether a write is autonomous, approval-routed, or denied, and should
observe exactly the same decision GraphQL would make. This story exercises
MCP-04 through MCP-07. Envelope generation derives from the compiled policy
plan (ADR-019/FEAT-029); outcome payloads are normative in CONTRACT-003.

## Walkthrough

1. The agent reads a write tool's description and sees the caller-visible
   policy envelope (for example, autonomous below a threshold, approval
   required above it).
2. It performs a write inside the autonomous envelope in commit mode; the
   write commits.
3. It attempts a write outside the envelope; it receives a needs-approval
   outcome with a mutation intent token.
4. It attempts a denied write; it receives the same policy explanation that
   GraphQL would return.
5. After a policy change, the agent is notified and refreshes tool metadata.

## Acceptance Criteria

- [ ] **US-112-AC1** — Given a write tool, when the agent reads its
  description, then it includes a policy envelope summary for the current
  subject where available.
- [ ] **US-112-AC2** — Given a write inside the autonomous envelope, when
  the agent calls the tool in commit mode, then the result is an allowed
  outcome and the write commits only in commit mode.
- [ ] **US-112-AC3** — Given a write outside the autonomous envelope, when
  the agent calls the tool, then the result is a needs-approval outcome with
  a mutation intent token and no state mutates.
- [ ] **US-112-AC4** — Given a denied write, when the agent calls the tool,
  then the policy explanation matches the explanation GraphQL returns for
  the same subject and operation.
- [ ] **US-112-AC5** — Given needs-approval, denied, stale, and conflict
  outcomes, when compared with GraphQL responses for the same operations,
  then the machine-readable fields are identical (payloads per
  CONTRACT-003/CONTRACT-002).
- [ ] **US-112-AC6** — Given a policy or schema change, when tool metadata
  regenerates, then connected agents are notified and refreshed metadata
  reflects the new envelopes.

## Edge Cases

- **Envelope unavailable for the subject**: The description omits the
  envelope rather than guessing; enforcement is unaffected.
- **Policy version changes between read and write**: The write returns a
  policy-version mismatch/stale outcome; the agent re-fetches metadata.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Envelope in description | US-112-AC1 | Threshold policy on invoices | Read write-tool description | Envelope summary present |
| Autonomous commit | US-112-AC2 | Amount change below threshold | Call tool in commit mode | allowed outcome; committed with audit ref |
| Routed for approval | US-112-AC3 | Amount change above threshold | Call write tool | needs-approval + intent token; entity unchanged |
| Denial parity | US-112-AC4 | Operation denied by rule R | Call via MCP and GraphQL | Identical explanation naming rule R |
| Metadata refresh | US-112-AC6 | Threshold raised by policy change | Await notification, re-read description | New threshold in envelope |

## Dependencies

- **Stories**: US-052 (discovery), US-053 (write tools)
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-04, MCP-05, MCP-06, MCP-07
- **PRD Requirements**: FR-12, FR-21, FR-28
- **External**: CONTRACT-003 (outcomes/metadata), CONTRACT-004 (policy
  grammar), FEAT-029, FEAT-030

## Out of Scope

- Policy authoring and activation (FEAT-029 stories).
- Approval review workflows (FEAT-030/FEAT-031 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
