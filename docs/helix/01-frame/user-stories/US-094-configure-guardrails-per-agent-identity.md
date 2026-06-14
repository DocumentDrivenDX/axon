---
ddx:
  id: US-094
  review:
    self_hash: 1186848257c549debd43898db1157fa3572672d33953fc42dad588ec0d07074f
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-094: Configure Guardrails Per Agent Identity

**Feature**: FEAT-022 — Agent Guardrails
**Feature Requirements**: GRD-09, GRD-10, GRD-11, GRD-13
**PRD Requirements**: FR-9
**Priority**: P1
**Status**: Draft

## Story

**As an** operator (Wei, business workflow builder)
**I want** to create, inspect, update, disable, and delete guardrail policies
per agent identity
**So that** different automations receive appropriately narrow permissions
and limits

## Context

One automation reconciles invoices nightly; another drafts vendor records on
demand. They need different scopes and limits, tunable without redeploying
anything. This story exercises guardrail policy lifecycle (GRD-09),
effective-policy resolution (GRD-10), live propagation, validation, and
auditing of changes (GRD-11), and credential rotation effects (GRD-13).

## Walkthrough

1. Wei creates a guardrail policy for agent identity `invoice-bot`: invoice
   collection scope and a conservative mutation limit.
2. Wei reads the agent's effective policy and sees global defaults merged
   with the identity-specific overrides.
3. Wei raises the agent's limit; the next request from the agent is governed
   by the new limit with no server restart.
4. Wei submits an invalid configuration; Axon rejects it with structured
   validation errors and the active policy is unchanged.
5. Every policy change appears in the audit log.

## Acceptance Criteria

- [ ] **US-094-AC1** — Given a tenant administrator, when they create,
  inspect, update, disable, or delete a guardrail policy for an agent
  identity through the control surface, then the operation succeeds; given a
  non-admin caller, then the same operations are denied.
- [ ] **US-094-AC2** — Given global defaults and identity-specific overrides,
  when an agent's effective policy is read, then the response shows the
  merged result.
- [ ] **US-094-AC3** — Given a policy update, when the agent makes its next
  request, then the updated policy governs it without a server restart.
- [ ] **US-094-AC4** — Given an invalid guardrail configuration, when it is
  submitted, then it is rejected with structured validation errors and the
  previously active policy remains in effect.
- [ ] **US-094-AC5** — Given any guardrail policy change, when audit is
  queried, then the change appears with actor and before/after context.
- [ ] **US-094-AC6** — Given a rotated or revoked agent credential, when the
  agent makes subsequent requests, then the rotation/revocation is in effect
  and visible in audit.

## Edge Cases

- **Policy deleted while agent is mid-task**: subsequent requests fall back
  to global defaults; the deletion is audited.
- **Disabled vs deleted**: a disabled policy is retained for re-enablement
  and inspection; a deleted one falls back to defaults.
- **Conflicting overrides**: effective-policy resolution is deterministic;
  the read surface shows which layer contributed each value.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Admin CRUD | US-094-AC1 | Tenant admin credential | Create/update/disable/delete policy for `invoice-bot` | All succeed; non-admin gets denied |
| Effective merge | US-094-AC2 | Default limit 10/min; override 60/min for `invoice-bot` | Read effective policy | Shows 60/min from override, other values from defaults |
| Live update | US-094-AC3 | Agent throttled at 10/min | Raise to 60/min; agent retries | Next requests governed by 60/min, no restart |
| Invalid config | US-094-AC4 | Negative rate limit | Submit update | Structured validation error; old policy still active |
| Rotation | US-094-AC6 | Credential rotated | Agent uses old credential | Rejected; audit shows rotation taking effect |

## Dependencies

- **Stories**: US-092, US-093 (the enforcement these policies configure).
- **Feature Spec**: FEAT-022
- **Feature Requirements**: GRD-09, GRD-10, GRD-11, GRD-13
- **PRD Requirements**: FR-9
- **External**: FEAT-012/ADR-018 (admin roles, credentials, grants);
  CONTRACT-005 (audit record)

## Out of Scope

- Semantic validation hooks — deferred; tracked in
  `docs/helix/parking-lot.md` (Semantic Validation Hooks for Agent
  Guardrails).
- Authoring data-layer access policies (FEAT-029/FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
