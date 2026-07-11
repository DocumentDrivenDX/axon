---
ddx:
  id: FEAT-022
  depends_on:
    - helix.prd
  review:
    self_hash: 63ecd2aff32e4cc0aa516c6cc8632ffb5ed3a004a6b633edf60dfc0b038f0fc6
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T04:03:36Z"
---
# Feature Specification: FEAT-022 — Agent Guardrails

**Feature ID**: FEAT-022
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-06-10
**Requirement Prefix**: GRD
**Covered PRD Subsystem(s)**: Guardrailed Transactions and Mutation Intents
**Covered PRD Requirements**: FR-9
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Preventive controls for agent interactions with Axon, beyond the reactive
audit trail. Agents operating through MCP, GraphQL, or the JSON API are
subject to scope constraints and rate limits that bound misuse before data
is committed. This implements PRD FR-9 (policy-bounded rate limits,
delegated authority, and scope constraints).

Guardrails do not replace data-layer policies (FEAT-029) or mutation intents
(FEAT-030). Guardrails bound an agent's operational blast radius; policies
decide what data the subject may see or mutate; intents route valid but risky
writes for approval.

## Ideal Future State

An operator delegates work to an autonomous agent with confidence that a
confused or compromised agent cannot mutate unrelated business state: every
agent credential carries an explicit scope (tenant, database, collection,
optional entity filter) and explicit mutation limits. When an agent exceeds
its bounds, the rejection is structured, attributable to the exact boundary
violated, retryable where appropriate, and visible in audit alongside the
delegating user and applied guardrail policy. Operators tune guardrails per
agent identity without restarting anything, and can always answer "what is
this agent currently allowed to do, and why was this request rejected?"

## Problem Statement

- **Current situation**: Audit trails (FEAT-003) are reactive — they
  record what happened after the fact. Policies (FEAT-029) decide
  per-record visibility and mutation rights, but nothing bounds the
  aggregate behavior of an agent identity.
- **Pain points**: An agent in a tight loop can bulk-mutate far more state
  than its task warrants; a misconfigured or compromised agent can write
  outside the records its task concerns; operators cannot bound blast
  radius per automation.
- **Desired outcome**: Per-agent scope constraints and rate limits reject
  out-of-bounds operations before commit, with structured, audited,
  retryable-where-appropriate errors.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Scope constraints | Can this agent only touch the records its task concerns? | Tenant/database/collection scope plus optional entity-filter narrowing, enforced on every mutation |
| Rate limiting and blast-radius caps | Can a runaway agent bulk-mutate everything it can see? | Per-actor mutation limits and per-transaction affected-entity caps with retryable rejection signals |
| Guardrail administration | How do I configure different limits for different automations? | Per-agent-identity guardrail policy lifecycle: create, inspect, update, disable, delete; effective-policy resolution |
| Delegated identity attribution | Who was actually behind this rejected write? | Guardrail decisions distinguish delegating user from delegated agent and carry full identity context into audit |

## Requirements

### Functional Requirements by Area

#### Scope Constraints

- **GRD-01**. An agent operating on a task must only be able to mutate
  entities within its assigned scope. Scope is defined at the tenant,
  database, or collection level (integrating with FEAT-012 authorization
  and FEAT-014 multi-tenancy) and may be narrowed by an entity filter
  (for example, "only entities where `assignee = agent-id`").
- **GRD-02**. Mutations outside scope must be rejected before commit with
  a structured error that identifies the violated scope boundary and
  carries a stable error code. Entity-filter scope must be enforced on
  update and delete, not just create.
- **GRD-03**. Scope rejections must be recorded in the audit log with the
  rejected operation, actor identity context, tenant, database, and
  reason, per
  [CONTRACT-005 — audit record](../../02-design/contracts/CONTRACT-005-audit-record.md).

#### Rate Limiting and Blast-Radius Caps

- **GRD-04**. Axon must enforce per-actor mutation rate limits that bound
  the blast radius of a misbehaving agent: an agent must not be able to
  bulk-mutate entities in a tight loop without bound.
- **GRD-05**. A rate-limited mutation must return a retryable signal with
  backoff guidance, distinguishable from authorization denial. The
  normative rejection envelope, status semantics, and limiter behavior
  (per-server, per-actor sliding window) are defined in
  [CONTRACT-001 — HTTP API surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md)
  (Rate limiting).
- **GRD-06**. Rate-limit accounting must be scoped so that one tenant's
  agents cannot starve another tenant's agents of write capacity.
- **GRD-07**. A transaction whose staged operations exceed the configured
  affected-entity cap must be rejected before commit, so a single
  transaction cannot silently mutate an unbounded set of records.
- **GRD-08**. Rate-limit and cap rejections must be visible in audit with
  the applied guardrail policy and retry hint.

#### Guardrail Administration

- **GRD-09**. Operators must be able to create, inspect, update, disable,
  and delete guardrail policies per agent identity through the control
  surface, restricted to tenant administrators.
- **GRD-10**. Reading an agent's effective guardrail policy must show
  global defaults merged with identity-specific overrides.
- **GRD-11**. Guardrail policy changes must take effect for subsequent
  requests without a server restart, must be audited, and invalid
  configurations must be rejected with structured validation errors.

#### Delegated Identity Attribution

- **GRD-12**. Guardrail decisions must distinguish the delegating
  user/service from the delegated agent identity. Rejections and
  allowances must carry `user_id`, `agent_id`, `delegated_by`, credential
  ID, grant version, tenant, database, and the applied guardrail policy.
- **GRD-13**. Credential rotation and revocation must take effect for
  subsequent requests and must be visible in audit.

### Non-Functional Requirements

- **Performance**: rate-limit evaluation must add < 1 ms overhead to
  mutation latency.
- **Efficiency**: scope checks must be evaluable without additional
  storage reads beyond what the mutation already requires.
- **Reliability**: guardrail rejection must be fail-closed — if guardrail
  state cannot be evaluated, the mutation is rejected, not silently
  allowed.

## User Stories

- [US-092 — Keep Agent Writes Inside Assigned Scope](../user-stories/US-092-keep-agent-writes-inside-assigned-scope.md)
- [US-093 — Throttle Agent Mutation Bursts](../user-stories/US-093-throttle-agent-mutation-bursts.md)
- [US-094 — Configure Guardrails Per Agent Identity](../user-stories/US-094-configure-guardrails-per-agent-identity.md)

## Edge Cases and Error Handling

- **Scope and policy both deny**: guardrail scope rejection and FEAT-029
  policy denial are distinct outcomes; the response must identify which
  layer rejected the operation so operators debug the right configuration.
- **Same collection name, different database/tenant**: scope is bound to
  tenant and database identity, not names; a matching collection name in
  another database must still be rejected.
- **Approval-routed writes**: writes that policy routes for approval are
  represented as FEAT-030 mutation intents; approval is not a guardrail
  bypass, and intent commits are still subject to scope and rate
  guardrails.
- **Rate limit hit mid-transaction**: the transaction is rejected as a
  unit; no partial commit occurs.
- **Guardrail policy deleted while agent is active**: subsequent requests
  fall back to global defaults; the change is audited.
- **Clock skew / retry storms**: retry guidance is server-relative
  (retry-after semantics per CONTRACT-001), so client clock skew cannot
  produce premature retries that re-trip the limiter.

## Success Metrics

- 100% of mutations outside an agent's assigned scope are rejected before
  commit and attributed in audit to the violated boundary.
- 100% of guardrail rejections (scope, rate, cap) carry the delegating
  user, delegated agent, and applied policy in audit.
- A runaway-agent simulation (tight mutation loop) is bounded by the
  configured rate and affected-entity caps with zero out-of-bound commits.
- Guardrail policy changes propagate to enforcement without restart,
  observable on the next request.

## Constraints and Assumptions

- Guardrails are preventive operational bounds, not data-visibility
  controls; they assume FEAT-029 policies remain the authority on what a
  subject may see or mutate per record.
- The V1 rate limiter is intentionally coarse (per-server, per-actor
  sliding window per CONTRACT-001); per-route or distributed
  fleet-coordinated limiting is not assumed.
- ADR-016 deferred the per-transaction affected-entity cap in its first
  implementation pass; this spec retains the cap (GRD-07) as desired
  future state.
- Agent identity, delegation, credentials, and grant versions are modeled
  by FEAT-012/ADR-018 and are available to guardrail evaluation.

## Dependencies

- **Other features**: FEAT-012 (Authorization — scope constraints build on
  RBAC/ABAC grants and credential identity), FEAT-014 (Multi-Tenancy —
  scope is expressed against tenant/database boundaries), FEAT-029
  (Access Control — guardrails run alongside data-layer policies),
  FEAT-030 (Mutation Intents — approval-routed writes are mutation
  intents, not guardrail bypasses).
- **External services**: None. Normative surfaces live in CONTRACT-001
  (rate-limit envelope and semantics) and CONTRACT-005 (audit record).
- **PRD requirements**: FR-9 (P1) — policy-bounded rate limits, delegated
  authority, and scope constraints after mutation intents are proven.

## Out of Scope

- **Semantic validation hooks**: content-aware validation of proposed
  mutations (for example, detecting that an invoice amount of $0.01 is
  semantically wrong) by external validators before commit. Deferred —
  full semantic validation is an open research problem, and the hook
  interface will be designed only after FEAT-029 policy enforcement and
  FEAT-030 mutation intents are proven in production. Tracked in
  `docs/helix/parking-lot.md` (Semantic Validation Hooks for Agent
  Guardrails).
- **Per-record authorization decisions**: what a subject may see or
  mutate is FEAT-029 policy, not a guardrail.
- **Approval routing**: deciding that a valid write needs human review is
  FEAT-030 mutation intents.
- **Distributed or per-tenant-budgeted rate limiting**: V1 limiter scope
  is governed by CONTRACT-001; fleet-coordinated quotas are future work.
