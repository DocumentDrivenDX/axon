---
ddx:
  id: US-108
  review:
    self_hash: 13e6e08564aef59816b7386372762247a27e4754b3f1984e7a1bb98b429f4dbb
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-108: Use Mutation Intents From MCP

**Feature**: FEAT-030 — Mutation Intents and Approval
**Feature Requirements**: INT-01, INT-11, INT-16
**PRD Requirements**: FR-7, FR-21, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As an** agent application developer (Ava) whose MCP-capable agent writes
governed data
**I want** tool results to expose preview, approval, and conflict states
**So that** the agent can coordinate governed writes without custom Axon
logic

## Context

Agents reach Axon through MCP, and PRD FR-21 requires MCP to mirror GraphQL
semantics. If tool results carry the same machine-readable decision
vocabulary — preview, needs-approval, denial, stale, conflict — an agent can
implement one intent workflow across every collection. This story exercises
the MCP mirror of preview (INT-01), approval-state visibility (INT-11), and
field parity (INT-16). The MCP tool surface is normative in CONTRACT-003.

## Walkthrough

1. Ava's agent discovers Axon's MCP tools; tool descriptions include the
   policy envelope summaries so the agent knows which writes may need
   approval.
2. The agent previews and then submits an invoice change through an MCP
   tool; the write is approval-routed.
3. The tool result is structured `needs_approval` output with the intent
   token and approval summary; the agent pauses and asks a human to review.
4. After approval, the agent commits the intent through MCP and receives
   the same decision fields a GraphQL client would.

## Acceptance Criteria

- [ ] **US-108-AC1** — Given generated MCP tools, when their descriptions
  are read, then they include policy envelope summaries (surface per
  CONTRACT-003).
- [ ] **US-108-AC2** — Given a tool call that policy routes for approval,
  when it returns, then the output is structured `needs_approval` with the
  intent token and approval summary.
- [ ] **US-108-AC3** — Given a tool call that policy denies, when it
  returns, then the output is a structured policy explanation.
- [ ] **US-108-AC4** — Given the MCP query/mutation path, when intents are
  previewed, approved, and committed through it, then the workflow follows
  the same intent semantics as GraphQL.
- [ ] **US-108-AC5** — Given `needs_approval`, denied, stale, and conflict
  outcomes via MCP, when compared to GraphQL outcomes for the same
  operations, then the machine-readable fields are preserved identically.

## Edge Cases

- **Agent retries a stale intent via MCP**: the structured stale outcome
  names the stale dimension, so the agent previews again instead of
  retrying blindly.
- **Approval happens on another surface**: an intent previewed via MCP and
  approved via GraphQL (or the admin UI) commits via MCP — intent state is
  surface-independent.
- **Tool result truncation**: decision fields are structured output, not
  prose, so summarization or truncation of human text cannot destroy the
  decision context.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Discoverable envelopes | US-108-AC1 | Collection with approval envelope | List MCP tools | Tool description includes envelope summary |
| Structured needs_approval | US-108-AC2 | $12,000 invoice change via MCP tool | Call tool | `needs_approval` + intent token + approval summary |
| Structured denial | US-108-AC3 | Subject denied vendor edits | Call tool | Structured policy explanation; no token |
| Cross-surface parity | US-108-AC5 | Same operations via MCP and GraphQL | Compare outcomes | Identical machine-readable decision fields |

## Dependencies

- **Stories**: US-105, US-106, US-107 (the workflow MCP mirrors).
- **Feature Spec**: FEAT-030
- **Feature Requirements**: INT-01, INT-11, INT-16
- **PRD Requirements**: FR-7, FR-21, FR-28
- **External**: CONTRACT-003 (MCP tool surface), CONTRACT-002 (canonical
  GraphQL semantics), CONTRACT-005 (audit references)

## Out of Scope

- MCP transport, discovery, and non-intent tooling (FEAT-016).
- Agent-side orchestration of approval waits (client concern).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
