---
ddx:
  id: US-091
  review:
    self_hash: bf9480d0bd2235a3decc49f451c6f2a520d83dc6a69432add436092bc006a056
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-091: User in Multiple Tenants

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-06, AUZ-08, AUZ-09, AUZ-13
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As a** developer belonging to two customer tenants (Ava, Agent Application Developer persona)
**I want** my single user identity to have different roles in each
**So that** I can admin one tenant while having read-only access to another

## Context

Membership is many-to-many with per-tenant role independence (ADR-018). One
human is one user; what changes per tenant is the membership role and the
credentials issued under it. This story proves tenant isolation of authority
for a multi-tenant user.

## Walkthrough

1. An operator adds the same user to tenant `acme` as admin and tenant
   `globex` as read.
2. The user lists tenants and sees both memberships.
3. The user works in `acme` with admin authority and in `globex` with
   read-only authority.
4. Credentials issued for `acme` are rejected on `globex` paths.

## Acceptance Criteria

- [ ] **US-091-AC1** — Given one user, when memberships are created in two
  tenants with different roles, then both coexist on the same user identity.
- [ ] **US-091-AC2** — Given the multi-tenant user, when they list tenants,
  then both tenants appear.
- [ ] **US-091-AC3** — Given a credential issued for one tenant, when it is
  presented against the other tenant's path, then it is rejected for
  tenant-binding mismatch.
- [ ] **US-091-AC4** — Given membership in two tenants, when one membership
  is removed, then the other is unaffected.
- [ ] **US-091-AC5** — Given requests in each tenant, when authorization is
  evaluated, then each tenant honors only its own membership role.
- [ ] **US-091-AC6** — Given mutations in both tenants, when audit entries
  are written, then both carry the same user identity.
- [ ] **US-091-AC7** — Given credential issuance in one tenant, when the
  requested grants reference databases of the other tenant, then issuance is
  rejected.

## Edge Cases

- **Deployment admin listing**: a deployment admin sees all tenants, not just
  their memberships.
- **Role change in one tenant**: takes effect within the identity cache TTL
  and never alters the other tenant's role.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Dual membership | US-091-AC1 | User `dana` | Add admin@acme, read@globex | Both memberships on one user |
| Tenant listing | US-091-AC2 | `dana` authenticated | List tenants | `acme` and `globex` returned |
| Cross-tenant token | US-091-AC3 | Credential bound to `acme` | Use on `globex` path | Rejected |
| Independent removal | US-091-AC4 | Both memberships | Remove globex membership | acme admin unaffected |
| Per-tenant role | US-091-AC5 | Same session | Admin op in `acme`; write in `globex` | acme succeeds; globex write forbidden |
| Shared identity in audit | US-091-AC6 | Mutations in both | Read audit in both | Same user identity recorded |
| Cross-tenant grants | US-091-AC7 | Issue in `acme` | Grants name globex database | Issuance rejected |

## Dependencies

- **Stories**: US-089, US-090
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-06, AUZ-08, AUZ-09, AUZ-13
- **PRD Requirements**: FR-25
- **External**: ADR-018 (membership model), CONTRACT-001 (membership routes)

## Out of Scope

- Cross-tenant data sharing or policy joins (explicitly excluded by
  FEAT-029).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
