---
ddx:
  id: US-053
---

# US-053: Agent CRUDs Entities via MCP

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-02, MCP-06, MCP-07, MCP-13
**PRD Requirements**: FR-6, FR-21, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As** an AI agent (acting under Ava's delegated authority) managing beads
**I want** to create, read, update, and delete entities using MCP tools
**So that** I can manage work items through the standard agent protocol

## Context

Generated per-collection tools are the agent's primary write path; they must
carry the same OCC, lifecycle, policy, and governed-write semantics as
GraphQL mutations. This story exercises MCP-02, MCP-06, MCP-07, and MCP-13.
Tool signatures and outcome payloads are normative in CONTRACT-003.

## Walkthrough

1. The agent creates a bead through the generated create tool and receives
   it with ID and version.
2. It reads the bead, patches it with the expected version, and transitions
   its lifecycle state.
3. A conflicting write makes its next patch stale; the structured conflict
   result includes current state, and the agent retries successfully.
4. A write that policy routes for approval returns a needs-approval outcome
   with intent metadata instead of committing.
5. The agent deletes the bead.

## Acceptance Criteria

- [ ] **US-053-AC1** — Given the generated collection tools (signatures per
  CONTRACT-003), when the agent creates, gets, patches, and deletes an
  entity, then each operation succeeds with the documented result shape.
- [ ] **US-053-AC2** — Given a lifecycle-declared collection, when the agent
  uses the generated transition tool, then transitions are validated and
  invalid transitions fail with the valid target states.
- [ ] **US-053-AC3** — Given a stale expected version, when the agent
  writes, then the conflict result includes the current entity state so the
  agent can retry.
- [ ] **US-053-AC4** — Given any tool failure, when the agent inspects the
  error, then it carries a stable structured code and detail fields the
  agent can act on programmatically (codes per CONTRACT-003).
- [ ] **US-053-AC5** — Given a write that policy routes for approval, when
  the agent calls the direct write tool, then a needs-approval outcome with
  intent metadata returns and no entity or link state mutates.

## Edge Cases

- **Patch on a deleted entity**: Structured not-found outcome, not a
  protocol error.
- **Concurrent tool calls on the same entity**: Each is independent; OCC
  resolves the conflict deterministically.
- **Schema validation failure**: Field-level structured detail returns; no
  partial write.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| CRUD round-trip | US-053-AC1 | Empty `beads` collection | create → get → patch → delete | All succeed; version increments on patch |
| Lifecycle validation | US-053-AC2 | Bead in `draft`; draft→done invalid | Transition to `done` | Error listing valid target states |
| Conflict retry | US-053-AC3 | Bead advanced by another writer | Patch with stale expected version | Conflict outcome with current state; retry succeeds |
| Structured codes | US-053-AC4 | Get a missing ID | Call get tool | Stable not-found code + detail |
| Governed write | US-053-AC5 | Policy routes amount > 10000 | Direct update to 12000 | needs-approval + intent metadata; entity unchanged |

## Dependencies

- **Stories**: US-052 (tool discovery)
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-02, MCP-06, MCP-07, MCP-13
- **PRD Requirements**: FR-6, FR-21, FR-28
- **External**: CONTRACT-003 (tool signatures and outcomes), FEAT-030
  (intent semantics)

## Out of Scope

- The full preview/approve/commit intent journey via MCP (FEAT-030 US-108).
- GraphQL-bridge queries (US-054).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
