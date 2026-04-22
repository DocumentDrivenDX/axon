---
ddx:
  id: FEAT-022
  depends_on:
    - helix.prd
    - FEAT-012
    - FEAT-019
    - FEAT-029
    - FEAT-030
---
# Feature Specification: FEAT-022 - Agent Guardrails

**Feature ID**: FEAT-022
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-04-22

## Overview

Preventive controls for agent interactions with Axon, beyond the reactive
audit trail. Agents operating through MCP, GraphQL, or the JSON API are
subject to scope constraints, rate limits, and semantic validation hooks
that prevent misuse before data is committed.

Guardrails do not replace data-layer policies (FEAT-029) or mutation intents
(FEAT-030). Guardrails bound an agent's operational blast radius; policies
decide what data the subject may see or mutate; intents route valid but risky
writes for approval.

## Problem Statement

Audit trails (FEAT-003) are reactive — they record what happened after the
fact. In agentic workflows, agents can submit data that is structurally
valid (passes schema validation) but semantically wrong (e.g., setting an
invoice amount to $0.01 instead of $10,000). Preventive guardrails reduce
the blast radius of misbehaving agents.

## Requirements

### Functional Requirements

#### Scope Constraints

- An agent operating on a task (e.g., a bead) should only be able to
  modify entities within its assigned scope.
- Mutations outside scope are rejected with a clear error identifying the
  scope boundary.
- Scope is defined at the collection, schema, or database level
  (integrates with FEAT-012 authorization, FEAT-014 multi-tenancy).
- Scope can be narrowed by entity filter (e.g., "only entities where
  `assignee = agent-id`").

#### Rate Limiting

- Prevent an agent from bulk-mutating entities in a tight loop without
  explicit approval.
- Configurable rate limits per agent identity: mutations per second,
  mutations per minute, entities affected per transaction.
- Rate limit exceeded produces a retryable error with backoff guidance.
- Burst allowance configurable separately from sustained rate.

#### Semantic Validation Hooks (Deferred)

Semantic validation hooks — allowing external validators to examine
proposed mutations in context before commit — are architecturally
desirable but deferred. Full semantic validation is an open research problem.
The hook interface will be designed only after FEAT-029 policy enforcement and
FEAT-030 mutation intents are proven in production.

#### Delegated Agent Identity

- Guardrail decisions must distinguish the delegating user/service from the
  delegated agent identity.
- Rejections and approvals include `user_id`, `agent_id`, `delegated_by`,
  credential ID, grant version, tenant, database, and applied guardrail policy.
- Credential rotation and revocation take effect for subsequent requests and
  must be visible in audit.

### Non-Functional Requirements

- Rate limiting must add <1ms overhead to mutation latency.
- Scope checks must be evaluable without additional storage reads beyond
  what the mutation already requires.
- Semantic hooks have a configurable timeout (default 5s). Timeout =
  reject.

### Dependencies

- FEAT-012 (Authorization) — scope constraints build on RBAC/ABAC grants.
- FEAT-019 (Validation Rules) — semantic hooks extend the validation
  pipeline.
- FEAT-029 (Access Control) — guardrails run alongside data-layer policies.
- FEAT-030 (Mutation Intents) — approval-routed writes are represented as
  mutation intents, not guardrail bypasses.

## User Stories

### Story US-092: Keep Agent Writes Inside Assigned Scope [FEAT-022]

**As an** operator delegating work to autonomous agents
**I want** each agent constrained to its assigned tenant, database,
collection, and optional entity filter
**So that** a confused or compromised agent cannot mutate unrelated
business state

**Acceptance Criteria:**
- [ ] A scoped credential can create, update, and delete entities inside
  its allowed tenant/database/collection scope. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] The same credential receives 403 with a structured guardrail error
  when it writes outside the allowed collection. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] The same credential receives 403 when it writes to a different
  database or tenant, even if collection names match. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Entity-filter scope is enforced on update and delete, not just
  create. Planned E2E: `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Scope rejection responses identify the violated scope boundary and
  include a stable error code. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Scope rejections are written to the audit log with the rejected
  operation, actor, tenant, database, and reason. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`

### Story US-093: Throttle Agent Mutation Bursts [FEAT-022]

**As an** operator running many agents
**I want** per-agent rate limits and affected-entity caps
**So that** an agent cannot accidentally perform a runaway bulk mutation

**Acceptance Criteria:**
- [ ] Per-agent sustained and burst mutation limits are enforced
  independently. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] A rate-limited mutation returns a retryable error with
  `retry_after_ms`. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Rate-limit counters are scoped by agent identity and tenant/database
  so one tenant cannot starve another. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] A transaction exceeding the configured affected-entity cap is
  rejected before commit. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Rate-limit rejections appear in the audit log with the applied
  policy id and retry hint. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`

### Story US-094: Configure Guardrails Per Agent Identity [FEAT-022]

**As an** operator
**I want** to create, inspect, update, disable, and delete guardrail
policies per agent identity
**So that** different automations can receive appropriately narrow
permissions and limits

**Acceptance Criteria:**
- [ ] Guardrail policy CRUD is available through the control API and is
  restricted to tenant admins. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Reading an agent's effective policy shows global defaults merged
  with identity-specific overrides. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Updating a policy takes effect for subsequent requests without
  server restart. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Invalid guardrail configuration is rejected with structured
  validation errors. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Guardrail policy changes are audited. Planned E2E:
  `crates/axon-server/tests/agent_guardrails_test.rs`
- [ ] Semantic validation hooks remain deferred; no acceptance criterion
  may be marked implemented until the hook interface is specified and
  covered by executable tests.
