---
ddx:
  id: US-092
  review:
    self_hash: 397e99044efc729a26566c750c542c9e3dd3a8d8db094e7b17a2d46c6483738b
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-092: Keep Agent Writes Inside Assigned Scope

**Feature**: FEAT-022 — Agent Guardrails
**Feature Requirements**: GRD-01, GRD-02, GRD-03, GRD-12
**PRD Requirements**: FR-9
**Priority**: P1
**Status**: Draft

## Story

**As an** operator delegating work to autonomous agents (Wei, business
workflow builder)
**I want** each agent constrained to its assigned tenant, database,
collection, and optional entity filter
**So that** a confused or compromised agent cannot mutate unrelated business
state

## Context

Agents act with delegated authority, and a prompt-injected or misconfigured
agent will happily write wherever its credential reaches. Scope constraints
bound the credential itself to the records the task concerns. This story
exercises scope definition and narrowing (GRD-01), structured pre-commit
rejection (GRD-02), audited rejections (GRD-03), and delegated identity
attribution (GRD-12).

## Walkthrough

1. Wei issues an agent credential scoped to one tenant, one database, the
   `invoices` collection, and the entity filter `assignee = agent-id`.
2. The agent creates and updates invoices assigned to it; the writes succeed.
3. The agent attempts to update a `vendors` record; Axon rejects the write
   before commit with a structured error naming the violated collection
   scope.
4. Wei finds the rejection in the audit log with the agent, delegating user,
   and the boundary that was violated.

## Acceptance Criteria

- [ ] **US-092-AC1** — Given a credential scoped to a tenant, database, and
  collection, when the agent creates, updates, and deletes entities inside
  that scope, then the operations succeed.
- [ ] **US-092-AC2** — Given the same credential, when it writes to a
  collection outside its scope, then the write is rejected before commit with
  a structured guardrail error and a stable error code.
- [ ] **US-092-AC3** — Given the same credential, when it writes to a
  different database or tenant, then the write is rejected even if the
  collection name matches.
- [ ] **US-092-AC4** — Given an entity-filter scope, when the agent updates
  or deletes an entity outside the filter, then the operation is rejected —
  filter scope applies to update and delete, not just create.
- [ ] **US-092-AC5** — Given a scope rejection, when the error is returned,
  then it identifies the violated scope boundary.
- [ ] **US-092-AC6** — Given a scope rejection, when audit is queried, then
  the rejection appears with the rejected operation, actor identity context
  (`user_id`, `agent_id`, `delegated_by`), tenant, database, and reason (per
  CONTRACT-005).

## Edge Cases

- **Scope vs policy denial**: when both guardrail scope and FEAT-029 policy
  would reject, the response identifies which layer rejected so the operator
  debugs the right configuration.
- **Filter references a mutable field**: an update that would move an entity
  out of the agent's filter scope is evaluated against the guardrail
  configuration deterministically — no state where the agent strands records
  it can no longer touch without an audited decision.
- **Unscoped (human) credentials**: credentials without guardrail scope are
  unaffected by this enforcement.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| In-scope CRUD | US-092-AC1 | Credential scoped to `tenant-a/db-1/invoices` | Create, update, delete an invoice assigned to the agent | All succeed |
| Wrong collection | US-092-AC2 | Same credential | Update a `vendors` entity | Rejected pre-commit; stable error code; collection boundary named |
| Same name, other tenant | US-092-AC3 | `tenant-b/db-1/invoices` exists | Write to tenant-b invoices | Rejected; tenant boundary named |
| Filter on delete | US-092-AC4 | Filter `assignee = agent-1` | Delete invoice assigned to someone else | Rejected; filter boundary named |
| Audited rejection | US-092-AC6 | Any rejection above | Query audit | Entry with operation, actor, delegated_by, tenant, database, reason |

## Dependencies

- **Stories**: None.
- **Feature Spec**: FEAT-022
- **Feature Requirements**: GRD-01, GRD-02, GRD-03, GRD-12
- **PRD Requirements**: FR-9
- **External**: CONTRACT-005 (audit record); FEAT-012/ADR-018 identity and
  grants; FEAT-014 tenant/database model

## Out of Scope

- Rate limits and affected-entity caps (US-093).
- Guardrail policy administration (US-094).
- Per-record visibility and field redaction (FEAT-029).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
