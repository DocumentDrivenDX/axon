---
ddx:
  id: US-054
  review:
    self_hash: 24005576169d63f2edd1428b4eb6b2017ced47944f6125c903ab867471423a55
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-054: Agent Queries via GraphQL through MCP

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-03, MCP-13
**PRD Requirements**: FR-21
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority) needing complex
data
**I want** to execute GraphQL queries through an MCP tool
**So that** I can fetch entities with relationships in one request

## Context

The GraphQL bridge tool gives agents the full expressive read surface —
relationship traversal, field selection, filters — through one tool instead
of hundreds of specialized ones. This story exercises MCP-03 and MCP-13; the
bridged semantics are CONTRACT-002's, and the tool envelope is
CONTRACT-003's.

## Walkthrough

1. The agent composes a GraphQL query from the schema knowledge it gained at
   discovery.
2. It invokes the bridge tool with the query document.
3. The response returns entity data with nested relationships, identical to
   a direct GraphQL call.
4. When the document is malformed or fails during execution, the agent
   receives a structured tool error it can act on.

## Acceptance Criteria

- [ ] **US-054-AC1** — Given the bridge tool (per CONTRACT-003), when the
  agent submits a GraphQL query document, then the response carries the full
  CONTRACT-002 response including nested relationships.
- [ ] **US-054-AC2** — Given schema knowledge from discovery, when the agent
  composes queries dynamically, then valid documents execute without
  Axon-specific client code.
- [ ] **US-054-AC3** — Given invalid GraphQL syntax, when the tool is
  invoked, then a structured MCP error returns before the execution engine
  is reached.
- [ ] **US-054-AC4** — Given a query that fails during execution (unknown
  field, policy denial, depth limit), when the tool returns, then the
  GraphQL errors surface as structured MCP tool errors with stable codes.

## Edge Cases

- **Mutation document via the bridge**: Executes with full governed-write
  semantics — approval-routed operations return approval-required outcomes,
  never silent commits.
- **Oversized response**: Standard pagination semantics apply; the tool does
  not truncate silently.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Nested fetch | US-054-AC1 | Bead with 2 dependencies | Bridge query with relationship selection | Bead + dependencies in one response |
| Dynamic composition | US-054-AC2 | Discovery metadata only | Agent-generated query | Executes successfully |
| Syntax error | US-054-AC3 | `query {` (unterminated) | Invoke bridge tool | Structured parse error, no execution |
| Execution error | US-054-AC4 | Query exceeding depth limit | Invoke bridge tool | Structured tool error with stable code |

## Dependencies

- **Stories**: US-052 (discovery)
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-03, MCP-13
- **PRD Requirements**: FR-21
- **External**: CONTRACT-003 (tool envelope), CONTRACT-002 (GraphQL
  semantics)

## Out of Scope

- Ad-hoc Cypher through MCP (FEAT-009 US-076 surface; CONTRACT-007).
- Bridge-tool streaming of large results (FEAT-016 out of scope).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
