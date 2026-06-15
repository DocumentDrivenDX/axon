---
ddx:
  id: US-044
  review:
    self_hash: 8a6e26beea783ee36a9a88ce6683e07a88b1033cb18533f05d8cc9e3ca882c0d
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-044: Role-Based Access Control

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-07, AUZ-13, AUZ-14, AUZ-22
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As an** operator managing Axon (Wei, Business Workflow Builder persona)
**I want** to restrict what agents can do based on their role
**So that** agents can't accidentally drop collections or modify schemas

## Context

Roles are the coarse authorization layer: admin, write, read, none. The
membership role is also the ceiling for any credential grants issued in that
tenant, so an agent's credential can never out-rank its owner. This story
exercises role enforcement on data-plane and control-plane operations.

## Walkthrough

1. Operator issues a write-scoped credential for an agent in a tenant.
2. Agent creates, updates, and deletes entities successfully.
3. Agent attempts an admin-only operation (for example, issuing itself a
   broader credential).
4. System rejects the escalation with a stable forbidden error.
5. A read-only credential holder attempts a write and is likewise rejected.

## Acceptance Criteria

- [ ] **US-044-AC1** — Given a credential with write grants, when its holder
  creates, updates, or deletes entities in the granted database, then the
  operations succeed.
- [ ] **US-044-AC2** — Given a write-level principal, when it attempts an
  admin-only operation such as credential escalation, then the operation is
  rejected.
- [ ] **US-044-AC3** — Given a read-only credential, when its holder attempts
  a write operation, then the request is rejected as forbidden.
- [ ] **US-044-AC4** — Given an admin principal, when it performs tenant
  control-plane operations (tenants, databases, users), then they succeed.
- [ ] **US-044-AC5** — Given a tenant member with a role ceiling, when a
  credential is requested with grants above that ceiling, then issuance is
  rejected and no credential is minted.

## Edge Cases

- **Multiple role sources**: when both a tag-derived and an explicitly
  assigned role exist, the explicit assignment wins (see US-124).
- **Role `none`**: the principal is explicitly denied all access even if
  network-reachable.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Write grants work | US-044-AC1 | Write credential for db `app` | Entity CRUD in `app` | Succeeds |
| Escalation blocked | US-044-AC2 | Write-level principal | Issue broader credential | Forbidden; nothing minted |
| Read-only write | US-044-AC3 | Read credential | POST-class write | Forbidden |
| Admin control plane | US-044-AC4 | Admin principal | Create tenant/database/user | Succeeds |
| Ceiling enforcement | US-044-AC5 | Member with `read` role | Request write grants | Issuance rejected |

## Dependencies

- **Stories**: US-090 (credential issuance)
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-07, AUZ-13, AUZ-14, AUZ-22
- **PRD Requirements**: FR-25
- **External**: ADR-018 (grants rule table, verification order),
  CONTRACT-001 (control-plane routes, error envelope)

## Out of Scope

- Schema-declared row/field policies (FEAT-029; see US-046, US-047 under
  that feature).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
