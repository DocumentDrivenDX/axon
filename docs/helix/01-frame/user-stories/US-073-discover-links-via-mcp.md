---
ddx:
  id: US-073
  review:
    self_hash: 292545d8e525edc27ec32914b4ad6cb684c1e20b71cf9a9d5fcb34cb65d831ca
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-073: Discover Links via MCP

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-08, QRY-10, QRY-15, QRY-16
**PRD Requirements**: FR-3, FR-21
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer whose AI agent works through MCP
**I want** MCP tools for link discovery and neighbor queries
**So that** my agent explores the graph through the standard agent protocol with the same guarantees as GraphQL

## Context

Inherited from the retired FEAT-020. Agents discover capabilities through
MCP tools; every named query must surface as a tool, and ad-hoc querying
must be available as a tool too — with policy and limits identical to the
GraphQL path (interface-parity, FR-21/FR-22). Tool shapes are normative in
CONTRACT-003.

## Walkthrough

1. Ava's agent lists MCP tools and finds one per activated named query, described from the query's declaration.
2. The agent invokes a link-discovery tool with parameters drawn from the query's parameter declarations.
3. The agent runs an ad-hoc query through the generic query tool for a one-off question.
4. Both calls pass the same policy enforcement and limits as the GraphQL surface.

## Acceptance Criteria

- [ ] **US-073-AC1** — Given an activated named query, when the agent lists MCP tools, then a corresponding tool exists with parameters drawn from the query's parameter declarations (per CONTRACT-003).
- [ ] **US-073-AC2** — Given the named query's description, when the tool is listed, then the tool description includes it.
- [ ] **US-073-AC3** — Given the generic ad-hoc query tool, when the agent invokes it with a query string and parameters, then execution follows the same parser/planner/policy path as GraphQL ad-hoc queries (QRY-10).
- [ ] **US-073-AC4** — Given the same subject, query, and data, when run via MCP and via GraphQL, then policy decisions, redactions, limits, and result content are identical.

## Edge Cases

- **Tool invocation with missing required parameter**: structured tool error naming the parameter.
- **Schema change deactivating a named query**: the tool disappears from listings; invoking a stale tool reference fails with a documented error.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Tool generation | US-073-AC1 | `link_candidates` named query active | List MCP tools | Tool present with declared parameters |
| Description | US-073-AC2 | Query has `description:` | List tools | Description included |
| Ad-hoc tool | US-073-AC3 | Generic query tool | Invoke with neighbor query | Rows returned, policy enforced |
| Parity | US-073-AC4 | Same query via MCP and GraphQL as same subject | Compare | Identical decisions and rows |

## Dependencies

- **Stories**: US-075 (named queries), US-076 (ad-hoc semantics)
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-08, QRY-10, QRY-15, QRY-16
- **PRD Requirements**: FR-3, FR-21
- **External**: CONTRACT-003 (MCP tool shapes), CONTRACT-007 (query semantics, limits, error codes), FEAT-016 (MCP server)

## Out of Scope

- MCP discovery of non-query tools (FEAT-016 stories).
- Subscriptions over MCP (FEAT-016 / V2).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
