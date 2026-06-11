---
ddx:
  id: US-043
---

# US-043: Authenticate via Tailscale

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-03, AUZ-04, AUZ-18, AUZ-21
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As an** agent running on a tailnet (operated by Ava, Agent Application Developer persona)
**I want** Axon to recognize my Tailscale identity
**So that** my operations are attributed to me in the audit log

## Context

Tailscale is Axon's default external authentication provider (ADR-005).
A request arriving over the tailnet must resolve to a stable Axon user via
the federation mapping — auto-provisioning on first contact — so that audit
attribution survives handle changes and requires no manual onboarding.

## Walkthrough

1. Agent connects to Axon over its Tailscale address with no bearer
   credential.
2. System resolves the tailnet identity through the provider, looks up the
   federation mapping, and auto-provisions a user on first contact.
3. Agent performs a mutation.
4. System records the resolved user's display identity as the audit actor.

## Acceptance Criteria

- [ ] **US-043-AC1** — Given an agent connecting via a Tailscale address,
  when the request arrives without a bearer credential, then Axon resolves
  its identity through the Tailscale provider to a stable user.
- [ ] **US-043-AC2** — Given a resolved Tailscale identity, when the agent
  mutates data, then the audit entry's actor is the resolved user's display
  identity, not the raw tailnet handle.
- [ ] **US-043-AC3** — Given a connection that cannot be authenticated, when
  the request is processed, then it is rejected as unauthenticated with the
  stable error envelope (CONTRACT-001).
- [ ] **US-043-AC4** — Given an agent with no recognized role tags, when it
  authenticates, then it still resolves to a stable user identity available
  for later role assignment.

## Edge Cases

- **Identity provider unreachable**: requests fail closed with service
  unavailable; Axon never silently bypasses authentication.
- **Concurrent first contact**: parallel first-seen requests for the same
  tailnet identity converge on a single user record.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Whois resolution | US-043-AC1 | Unseen tailnet identity | First request | User auto-provisioned; request attributed |
| Audit attribution | US-043-AC2 | Resolved user `dana` | Update an entity | Audit actor is `dana`'s display identity |
| Unauthenticated rejection | US-043-AC3 | Non-tailnet, no credential | Any request | Unauthenticated rejection with stable code |
| Untagged identity | US-043-AC4 | Node without role tags | Authenticate | Stable user resolved; default role applies |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-03, AUZ-04, AUZ-18, AUZ-21
- **PRD Requirements**: FR-25
- **External**: ADR-005 (Tailscale provider), ADR-018 (identity model),
  CONTRACT-001 (error envelope)

## Out of Scope

- Role derivation and enforcement (US-044).
- Credential-based authentication (US-090).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
