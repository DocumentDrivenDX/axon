---
ddx:
  id: US-123
  review:
    self_hash: 318ae3a28aed1013bc283e882cc944ea2effcf4fcbd0f86c4168bfc0d237db59
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-123: Development Without Auth

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-19
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As a** developer running Axon locally (Ava, Agent Application Developer persona)
**I want** to disable auth for development
**So that** I don't need a Tailscale connection during development

## Context

Renumbered from US-045 (collision with FEAT-011). Local development and
embedded mode must work with zero identity infrastructure: the no-auth mode
synthesizes an anonymous admin context in memory and persists nothing. The
flag surface is defined in CONTRACT-008.

## Walkthrough

1. Developer starts the server in no-auth mode without any identity provider
   running.
2. System starts and serves requests.
3. Developer makes requests; each receives a synthetic admin identity with a
   default tenant context.
4. Audit entries record the anonymous actor.

## Acceptance Criteria

- [ ] **US-123-AC1** — Given no identity provider is running, when the server
  starts in no-auth mode, then it starts successfully and serves requests.
- [ ] **US-123-AC2** — Given no-auth mode, when any request is served, then
  it receives a synthetic admin identity with a default tenant context.
- [ ] **US-123-AC3** — Given no-auth mode, when mutations occur, then audit
  entries record the anonymous actor.

## Edge Cases

- **No persistence**: no-auth mode persists no user or tenant records; it is
  a pure in-memory convenience.
- **Embedded mode**: in-process CLI use always behaves as no-auth — there is
  no network boundary to authenticate.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Startup without provider | US-123-AC1 | No tailscaled, no-auth flag | Start server | Server starts and serves |
| Synthetic identity | US-123-AC2 | No-auth server | Any data-plane request | Admin context, default tenant |
| Anonymous audit | US-123-AC3 | No-auth server | Create an entity | Audit actor is anonymous |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-19
- **PRD Requirements**: FR-25
- **External**: CONTRACT-008 (CLI flags and configuration)

## Out of Scope

- Guest-role mode for edge deployments (AUZ-20).
- Production deployments — no-auth is a development convenience only.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
