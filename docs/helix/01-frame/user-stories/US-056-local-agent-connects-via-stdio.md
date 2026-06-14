---
ddx:
  id: US-056
  review:
    self_hash: ff05aa4c6fe051720ca82459a11b4a3bcacfa8a9ac8a7574d6a7e84dd9219417
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-056: Local Agent Connects via Stdio

**Feature**: FEAT-016 — MCP Server
**Feature Requirements**: MCP-12
**PRD Requirements**: FR-21, FR-23
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer using Claude Code with Axon
**I want** to add Axon as an MCP server over stdio
**So that** my local agent can directly query and modify my Axon data

## Context

Local development is the first touch of the agent-native experience: a
developer should wire Axon into an MCP-capable coding agent in one
configuration step with no network or auth setup. This story exercises
MCP-12. The stdio invocation command and trust-boundary semantics are
normative in CONTRACT-003 (with CONTRACT-008 for the CLI entry point).

## Walkthrough

1. Ava adds Axon to her agent's MCP configuration using the stdio invocation
   (per CONTRACT-003/CONTRACT-008).
2. The agent starts the MCP server over stdin/stdout and performs discovery.
3. All tools, resources, and prompts are available; the agent reads and
   writes Ava's local Axon data.
4. No credential setup is required: stdio shares the local development trust
   boundary.

## Acceptance Criteria

- [ ] **US-056-AC1** — Given the documented stdio invocation, when the
  process starts, then it serves MCP over stdin/stdout.
- [ ] **US-056-AC2** — Given a standard MCP client configuration (such as
  Claude Code), when Axon is registered with the stdio invocation, then the
  client connects and completes discovery.
- [ ] **US-056-AC3** — Given a stdio session, when the agent lists tools,
  resources, and prompts, then the full surface is available — identical to
  the HTTP transport's surface for the same data.
- [ ] **US-056-AC4** — Given a stdio session, when the agent operates, then
  no authentication is required (local trust boundary per CONTRACT-003).

## Edge Cases

- **Tenant/database omitted in resource URIs on stdio**: Defaults expand per
  CONTRACT-003's stdio convenience rule.
- **Server process killed mid-session**: The client surfaces a transport
  error; restarting reconnects cleanly with no corrupted state.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Stdio handshake | US-056-AC1 | Local Axon data dir | Start stdio invocation, send initialize | Valid MCP handshake response |
| Client config | US-056-AC2 | Claude Code MCP config entry | Start a session | Discovery completes; tools listed |
| Surface parity | US-056-AC3 | Same data via stdio and HTTP | List tools/resources/prompts on both | Identical surface |
| No auth | US-056-AC4 | No credentials configured | Run a read and a write tool | Both succeed in the local trust boundary |

## Dependencies

- **Stories**: US-052 (discovery flow)
- **Feature Spec**: FEAT-016
- **Feature Requirements**: MCP-12
- **PRD Requirements**: FR-21, FR-23
- **External**: CONTRACT-003 (transports), CONTRACT-008 (CLI invocation),
  FEAT-028 (unified binary)

## Out of Scope

- Remote HTTP transport authentication (CONTRACT-001/CONTRACT-003).
- Multi-tenant stdio sessions beyond default expansion.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
