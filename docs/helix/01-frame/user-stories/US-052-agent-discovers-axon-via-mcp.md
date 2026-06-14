---
ddx:
  id: US-052
  review:
    self_hash: 1648dc74f0fec2ff5c0340cdf83e18bd5824db4f1b7b1af4f948686c70014813
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-052: Agent Discovers Axon via MCP

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-01, MCP-02, MCP-04, MCP-05, MCP-09
**PRD Requirements**: FR-21
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority) connecting to
Axon for the first time
**I want** to discover what collections and operations are available
**So that** I can interact with Axon without prior configuration

## Context

Discoverability is the heart of the agent-native value proposition: an
MCP-capable agent should learn everything it needs from generated tool
definitions and resource templates. This story exercises FEAT-016's tool
generation and metadata requirements (MCP-01, MCP-02, MCP-04, MCP-05,
MCP-09). Tool names and schemas are normative in CONTRACT-003.

## Walkthrough

1. The agent connects and requests the tool list.
2. It receives typed tool definitions for the core tools and for every
   registered collection, each with parameter schemas derived from ESF.
3. It reads tool descriptions — including parameter constraints, expected
   response shapes, and policy envelopes — and selects an operation without
   consulting external documentation.
4. A new collection is registered; the agent receives a list-changed
   notification and discovers the new tools.

## Acceptance Criteria

- [ ] **US-052-AC1** — Given registered collections, when the agent lists
  tools, then it receives typed tool definitions for all collections (names
  per CONTRACT-003).
- [ ] **US-052-AC2** — Given any collection tool, when the agent inspects
  its parameters, then the parameter schema derives from the collection's
  ESF.
- [ ] **US-052-AC3** — Given any tool description, when the agent reads it,
  then it is self-contained — parameter constraints and expected response
  shape included — sufficient to determine correct usage without external
  documentation.
- [ ] **US-052-AC4** — Given tool metadata, when the agent inspects it, then
  policy envelopes, redacted fields, approval requirements, schema/policy
  versions, and expected audit references are included.
- [ ] **US-052-AC5** — Given a connected agent, when a new collection is
  added, then the agent is notified and the new collection's tools become
  available.

## Edge Cases

- **No registered collections**: Core tools are still listed; the agent can
  create its first collection through them.
- **Stale tool list after schema change**: Calls with outdated parameters
  fail with structured validation errors prompting a re-list.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Full discovery | US-052-AC1 | 3 collections registered | List tools | Core tools + per-collection tool sets for all 3 |
| ESF-derived params | US-052-AC2 | `beads` with typed fields | Inspect create tool schema | Parameter schema matches ESF fields |
| Self-contained docs | US-052-AC3 | Any generated tool | LLM-readability fixture check | Constraints + response shape present |
| Envelope metadata | US-052-AC4 | Collection with approval policy | Inspect write tool metadata | Envelope, redactions, versions, audit refs present |
| Live regeneration | US-052-AC5 | Add `invoices` collection | Await notification, re-list | List-changed received; `invoices` tools present |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-01, MCP-02, MCP-04, MCP-05, MCP-09
- **PRD Requirements**: FR-21
- **External**: CONTRACT-003 (MCP surface), CONTRACT-010 (ESF), MCP protocol
  specification

## Out of Scope

- Executing operations (US-053, US-054).
- Policy envelope decision behavior (US-112).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
