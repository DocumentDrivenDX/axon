---
ddx:
  id: US-090
  review:
    self_hash: 1301fd6157a1f7e50558ab30cf05f340388389dafe6406fc75712b9f113ac73a
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-090: JWT Credential for Integration Access

**Feature**: FEAT-012 — Authentication, Identity, and Authorization
**Feature Requirements**: AUZ-12, AUZ-13, AUZ-14, AUZ-15, AUZ-16
**PRD Requirements**: FR-25
**Priority**: P1
**Status**: Approved

## Story

**As an** operator issuing access to a CI job (Wei, Business Workflow Builder persona)
**I want** to mint a tenant-scoped credential with narrow grants
**So that** the CI job can only read one database and cannot escalate

## Context

Credentials are the machine authentication path: tenant-bound, grant-scoped,
revocable tokens issued by the control plane. The credential format, claims,
and verification order are governed by ADR-018; the issuance, listing, and
revocation endpoints by CONTRACT-001. This story proves the full credential
lifecycle for a narrowly scoped integration.

## Walkthrough

1. Operator requests a credential for a target user with read-only grants on
   one database and a TTL.
2. System validates the issuer's authority, the target's membership, and the
   grant ceiling, then returns the signed token once.
3. The CI job presents the token: reads in the granted database succeed;
   writes, other databases, and other tenants are rejected.
4. Operator revokes the credential; subsequent use fails within one second.

## Acceptance Criteria

- [ ] **US-090-AC1** — Given a tenant admin, when they request a credential
  for a member with read-only grants on one database and a TTL, then a signed
  token bound to that tenant, user, grants, and expiry is returned
  (claim shape per ADR-018).
- [ ] **US-090-AC2** — Given the issued credential, when its holder performs
  a read in the granted database, then the request succeeds.
- [ ] **US-090-AC3** — Given the issued credential, when its holder attempts
  a write in the granted database, then the request is rejected for operation
  mismatch.
- [ ] **US-090-AC4** — Given the issued credential, when its holder addresses
  a database not in its grants, then the request is rejected.
- [ ] **US-090-AC5** — Given the issued credential, when its holder addresses
  a different tenant's path, then the request is rejected for tenant-binding
  mismatch.
- [ ] **US-090-AC6** — Given the credential is revoked, when it is presented
  afterwards, then verification fails within 1 second of revocation.
- [ ] **US-090-AC7** — Given an issuance request with grants exceeding the
  issuer's or target's role ceiling, when it is submitted, then it is
  rejected and no credential is minted.

## Edge Cases

- **Expired credential**: rejected at verification with the stable
  expiry-class error code.
- **Listing**: credential listings return metadata only; the signed token is
  never retrievable after issuance.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Mint scoped token | US-090-AC1 | Admin of `acme`; member `ci-bot` | Issue read-only grant on db `ci` | Signed tenant-bound token returned once |
| Granted read | US-090-AC2 | Token from AC1 | Read in `ci` | 2xx success |
| Op mismatch | US-090-AC3 | Same token | Write in `ci` | Forbidden |
| Database mismatch | US-090-AC4 | Same token | Read in `prod` | Forbidden |
| Tenant mismatch | US-090-AC5 | Same token | Read in tenant `globex` | Rejected |
| Fast revocation | US-090-AC6 | Token revoked | Present token after 1 s | Unauthenticated rejection |
| Ceiling enforcement | US-090-AC7 | Target role `read` | Request write grants | Rejected; nothing minted |

## Dependencies

- **Stories**: US-089 (users exist), US-091 (membership)
- **Feature Spec**: FEAT-012
- **Feature Requirements**: AUZ-12 through AUZ-16
- **PRD Requirements**: FR-25
- **External**: ADR-018 (credential claims, grants rules, verification
  order), CONTRACT-001 (credential endpoints, error envelope)

## Out of Scope

- Data-layer policy refinement on top of grants (FEAT-029).
- Automatic credential rotation schedules.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
