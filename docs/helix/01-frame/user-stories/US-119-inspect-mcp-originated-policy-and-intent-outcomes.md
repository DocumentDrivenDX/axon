---
ddx:
  id: US-119
  review:
    self_hash: a49d41a2ce0d5214277cc1ecfe9942b9b8ba2979055c8a47687bed4268b40773
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-119: Inspect MCP-Originated Policy And Intent Outcomes

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-16, PUI-18
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As an** operator supervising agents (Ava, Agent Application Developer persona)
**I want** to see what an MCP-capable agent saw and submitted
**So that** I can debug agent behavior without guessing from raw logs

## Context

Agents act through MCP envelopes generated from the same compiled policy
plan. When an agent's write is allowed, denied, or routed for approval, the
operator needs the envelope the agent received and the structured outcome —
in the same console, with the same reason codes the UI uses elsewhere.

## Walkthrough

1. Operator opens the policy workspace and previews the MCP tool envelope for
   a subject, collection, and operation.
2. System renders the envelope an agent with that subject would receive.
3. Operator opens an MCP-originated intent from the inbox or audit lineage.
4. System shows agent identity, delegated authority, credential/grant
   version, tool name, argument summary, and the structured outcome.

## Acceptance Criteria

- [ ] **US-119-AC1** — Given the policy workspace, when the operator selects
  a subject, collection, and operation, then the MCP tool envelope for that
  selection is shown (envelope semantics per CONTRACT-003).
- [ ] **US-119-AC2** — Given an MCP-originated intent, when its detail
  renders, then agent identity, delegated authority, credential/grant
  version, tool name, tool argument summary, and structured outcome are
  shown.
- [ ] **US-119-AC3** — Given a denied MCP tool result, when the operator
  views the corresponding UI policy explanation, then both use the same
  stable reason code.
- [ ] **US-119-AC4** — Given needs-approval, denied, and conflict MCP
  outcomes, when the operator works the intent inbox or audit lineage view,
  then each outcome is visible there.

## Edge Cases

- **Revoked credential**: envelope inspection for a since-revoked credential
  renders as historical context tied to its grant version, not current
  capability.
- **Tool arguments containing redacted data**: argument summaries apply the
  viewer's redaction rules.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Envelope preview | US-119-AC1 | Agent-role subject, invoices, update | Preview envelope | Envelope rendered for that tuple |
| MCP intent detail | US-119-AC2 | Intent from MCP tool call | Open detail | Agent identity, grants, tool, outcome shown |
| Shared reason codes | US-119-AC3 | Denied MCP write | Compare tool result and UI explanation | Same reason code |
| Outcome visibility | US-119-AC4 | Outcomes of each type | Browse inbox/lineage | needs-approval, denied, conflict all visible |

## Dependencies

- **Stories**: US-108 (MCP intents), US-112 (MCP policy envelopes), US-117
  (inbox)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-16, PUI-18
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-003 (MCP envelopes and outcomes), CONTRACT-004
  (reason codes), CONTRACT-002 (lineage queries)

## Out of Scope

- MCP server behavior itself (FEAT-016).
- Agent guardrail configuration (FEAT-022).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
