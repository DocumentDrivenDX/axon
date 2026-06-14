---
ddx:
  id: US-093
  review:
    self_hash: 0be03705ee5317eaf3650cc1270c9f3e7f26cd4aac21b48e6e2e7c8a0e52bb66
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-093: Throttle Agent Mutation Bursts

**Feature**: FEAT-022 — Agent Guardrails
**Feature Requirements**: GRD-04, GRD-05, GRD-06, GRD-07, GRD-08
**PRD Requirements**: FR-9
**Priority**: P1
**Status**: Draft

## Story

**As an** operator running many agents (Ava, agent application developer)
**I want** per-agent rate limits and affected-entity caps
**So that** an agent cannot accidentally perform a runaway bulk mutation

## Context

A retry loop or a misunderstood instruction can turn one agent into a bulk
mutation engine. Rate limits bound mutation velocity per actor (GRD-04..06)
and the affected-entity cap bounds the size of any single transaction
(GRD-07); both produce audited, retry-friendly rejections (GRD-05, GRD-08).
The normative rejection envelope and limiter semantics live in CONTRACT-001
(rate limiting).

## Walkthrough

1. Ava's agent enters a tight loop, submitting mutations far faster than its
   configured limit.
2. Axon rejects the excess writes with a retryable rate-limit signal carrying
   backoff guidance.
3. The agent backs off per the guidance and resumes within its limit.
4. Later, the agent stages one transaction touching more entities than its
   affected-entity cap; Axon rejects the transaction before commit.
5. Ava reviews the audit log and sees both rejections with the applied
   guardrail policy.

## Acceptance Criteria

- [ ] **US-093-AC1** — Given a configured per-actor mutation limit, when an
  agent exceeds it, then excess mutations are rejected while compliant
  traffic proceeds.
- [ ] **US-093-AC2** — Given a rate-limited mutation, when the rejection is
  returned, then it is a retryable signal with backoff guidance,
  distinguishable from authorization denial (envelope per CONTRACT-001).
- [ ] **US-093-AC3** — Given agents in two tenants, when one tenant's agent
  saturates its own limit, then the other tenant's agents are not starved —
  accounting is per actor.
- [ ] **US-093-AC4** — Given a configured affected-entity cap, when a
  transaction stages more entities than the cap, then it is rejected before
  commit and no partial mutation occurs.
- [ ] **US-093-AC5** — Given rate-limit or cap rejections, when audit is
  queried, then the rejections appear with the applied guardrail policy and
  retry hint.

## Edge Cases

- **Burst at window boundary**: limiter behavior at window edges follows the
  sliding-window semantics in CONTRACT-001 — no double-allowance at
  boundaries.
- **Mixed surfaces**: mutations through GraphQL, MCP, and the JSON API count
  against the same per-actor accounting; an agent cannot escape its limit by
  switching surfaces.
- **Reads unaffected**: queries and previews are not write-rate-limited.
- **Limit hit mid-transaction**: the transaction rejects as a unit; no
  partial commit.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Loop throttled | US-093-AC1 | Limit allows N writes/window | Submit 3N writes in one window | ~2N rejected, N committed |
| Retryable signal | US-093-AC2 | Rate-limited request | Inspect rejection | Retryable signal + backoff guidance per CONTRACT-001 |
| Tenant isolation | US-093-AC3 | Agent A (tenant-a) saturated; agent B (tenant-b) idle | Agent B writes | Agent B unaffected |
| Entity cap | US-093-AC4 | Cap = 100 entities/transaction | Stage transaction touching 250 entities | Rejected pre-commit; nothing applied |
| Audited rejection | US-093-AC5 | Any rejection above | Query audit | Entry with applied policy id and retry hint |

## Dependencies

- **Stories**: US-094 (limits are configured per agent identity).
- **Feature Spec**: FEAT-022
- **Feature Requirements**: GRD-04, GRD-05, GRD-06, GRD-07, GRD-08
- **PRD Requirements**: FR-9
- **External**: CONTRACT-001 (rate-limit envelope and sliding-window
  semantics); CONTRACT-005 (audit record); FEAT-008 (transaction staging)

## Out of Scope

- Scope boundaries (US-092).
- Distributed, fleet-coordinated, or per-tenant-budget quotas (parked per
  FEAT-022 Out of Scope).
- Deciding that a large-but-legitimate write needs approval (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
