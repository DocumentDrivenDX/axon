---
ddx:
  id: US-041
  review:
    self_hash: 6d2121397b8b0fb9941986f3772321e26b5450e9d02f6ebf76ff64fa1532c97b
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-041: Administer Users, Members, and Credentials

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-04, UI-05, UI-06
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As an** operator (Wei, Business Workflow Builder persona)
**I want** user and tenant access controls in the UI
**So that** I can administer access without direct API calls

## Context

FEAT-012/ADR-018 define users, tenant memberships, and tenant-bound
credentials; this story gives operators a console for those control-plane
objects so routine access administration never requires curl. It exercises
FEAT-011 requirements UI-04 through UI-06.

## Walkthrough

1. Operator opens the Users screen and provisions a user record.
2. System shows the user in the deployment-wide user list with its ACL row.
3. Operator opens a tenant's Members screen and adds the provisioned user
   with a role.
4. System lists the membership; the operator can change the role or remove
   the member.
5. Operator opens the tenant's Credentials screen and issues a credential for
   the member.
6. System shows the issued token exactly once; afterwards only credential
   metadata appears in the table, where the operator can revoke it.

## Acceptance Criteria

- [ ] **US-041-AC1** — Given the Users screen, when the operator adds,
  changes, or removes a deployment-wide ACL row, then the change is applied
  and reflected in the list.
- [ ] **US-041-AC2** — Given the Users screen, when the operator provisions
  or suspends a user record, then the user's state updates accordingly.
- [ ] **US-041-AC3** — Given a tenant Members screen and a provisioned user,
  when the operator adds the user as a member, changes the member's role, and
  removes the member, then each step succeeds and is reflected in the member
  list.
- [ ] **US-041-AC4** — Given a tenant member, when the operator issues a
  credential for that member, then the signed token is shown exactly once in
  the issue flow and is never displayed again afterwards.
- [ ] **US-041-AC5** — Given an issued credential, when the operator views
  the credentials table, then the credential's metadata appears and the
  operator can revoke it, after which it is marked revoked.

## Edge Cases

- **Issuing for a non-member**: the issue flow surfaces the structured
  control-plane error; no credential is minted.
- **Revoking an already-revoked credential**: the UI reflects the terminal
  revoked state without error noise.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| ACL row lifecycle | US-041-AC1 | Users screen | Add, edit, remove an ACL row | Each change persisted and listed |
| Provision and suspend | US-041-AC2 | Users screen | Provision user `dana`, then suspend | `dana` listed, then shown suspended |
| Membership lifecycle | US-041-AC3 | Tenant `acme`, user `dana` | Add as `read`, change to `write`, remove | Member list reflects each step |
| One-time token display | US-041-AC4 | `dana` is member of `acme` | Issue credential | Token visible once; metadata-only afterwards |
| Revocation | US-041-AC5 | Issued credential | Revoke from table | Row marked revoked |

## Dependencies

- **Stories**: US-040 (tenant navigation)
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-04, UI-05, UI-06
- **PRD Requirements**: FR-24
- **External**: CONTRACT-001 (control-plane routes), CONTRACT-002
  (control-plane GraphQL), ADR-018 (credential model)

## Out of Scope

- Credential verification and grant-ceiling enforcement semantics (FEAT-012,
  US-090).
- Self-service credential flows for non-admin users.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
