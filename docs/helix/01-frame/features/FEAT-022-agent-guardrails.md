---
dun:
  id: FEAT-022
  depends_on:
    - helix.prd
    - FEAT-012
    - FEAT-019
---
# Feature Specification: FEAT-022 - Agent Guardrails

**Feature ID**: FEAT-022
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-06
**Updated**: 2026-04-06

## Overview

Preventive controls for agent interactions with Axon, beyond the reactive
audit trail. Agents operating through MCP, GraphQL, or the JSON API are
subject to scope constraints, rate limits, and semantic validation hooks
that prevent misuse before data is committed.

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
desirable but deferred. Full semantic validation is an open research
problem. The hook interface will be designed when scope constraints and
rate limiting are proven in production.

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

## Acceptance Criteria

- [ ] Agent with scoped credentials cannot modify entities outside scope
- [ ] Rate-limited agent receives retryable error when exceeding limits
- [ ] ~~Semantic validation hook can reject a mutation~~ (deferred)
- [ ] Guardrails are configurable per agent identity, not just globally
- [ ] All guardrail rejections appear in the audit log with rejection
      reason
