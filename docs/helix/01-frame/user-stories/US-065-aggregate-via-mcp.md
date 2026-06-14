---
ddx:
  id: US-065
  review:
    self_hash: 19e5a9296cb22aa060ef03e602589e4dc03bad2eaeb5f38a8da0e1c92f7182f6
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-065: Aggregate via MCP

**Feature**: FEAT-018 — Aggregation Queries
**Feature Requirements**: AGG-06, AGG-07
**PRD Requirements**: FR-3, FR-21
**Priority**: P1
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority)
**I want** to compute aggregations via an MCP tool
**So that** I can understand data distributions through the standard agent
protocol

## Context

Agents reach summaries through the same generated tool surface they use for
everything else. This story exercises FEAT-018's MCP projection (AGG-06) and
cross-surface parity (AGG-07). The tool signature is normative in
CONTRACT-003; the semantics are the unified planner's (CONTRACT-007).

## Walkthrough

1. The agent discovers the per-collection aggregation tool in the tool list.
2. It reads the tool description, which explains available functions and
   valid field types.
3. It invokes the tool with filter, aggregation, and grouping parameters.
4. It receives structured JSON with groups and aggregated values matching
   what GraphQL would return for the same request.

## Acceptance Criteria

- [ ] **US-065-AC1** — Given a registered collection, when the agent lists
  tools, then a per-collection aggregation tool is auto-generated (signature
  per CONTRACT-003).
- [ ] **US-065-AC2** — Given the aggregation tool, when the agent invokes it
  with filter, aggregation, and grouping parameters, then the parameters are
  accepted as described.
- [ ] **US-065-AC3** — Given a grouped aggregation invocation, when it
  executes, then the response is structured JSON with groups and aggregated
  values.
- [ ] **US-065-AC4** — Given the tool description, when the agent reads it,
  then it explains the available functions and valid field types without
  external documentation.
- [ ] **US-065-AC5** — Given the same subject, filter, grouping, and
  functions, when the aggregation runs via MCP and via GraphQL, then the
  results are identical.

## Edge Cases

- **Type-invalid aggregation**: The tool returns the same structured type
  error code the other surfaces return.
- **Policy-hidden rows**: Excluded identically to GraphQL — totals never
  diverge between surfaces.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Generated tool | US-065-AC1 | `beads` registered | List tools | Aggregation tool present for beads |
| Parameterized call | US-065-AC2 | 20 beads | Invoke with filter + count + group_by status | Accepted; correct grouped counts |
| Structured result | US-065-AC3 | Grouped invocation | Inspect response | JSON with groups and values |
| Self-documenting | US-065-AC4 | Tool description | Read description | Functions and field-type rules explained |
| Surface parity | US-065-AC5 | Same aggregation via MCP and GraphQL | Compare results | Identical groups, values, totals |

## Dependencies

- **Stories**: US-052 (tool discovery), US-062, US-063 (aggregation
  semantics)
- **Feature Spec**: FEAT-018
- **Feature Requirements**: AGG-06, AGG-07
- **PRD Requirements**: FR-3, FR-21
- **External**: CONTRACT-003 (MCP aggregation tool), CONTRACT-007 (planner),
  CONTRACT-002 (parity reference)

## Out of Scope

- GraphQL aggregation projection details (US-064).
- Ad-hoc Cypher aggregation via the bridge/query tools (FEAT-009 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
