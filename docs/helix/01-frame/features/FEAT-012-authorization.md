---
ddx:
  id: FEAT-012
  depends_on:
    - helix.prd
  review:
    self_hash: d37c0b05aaef5e6da2c11ad0f7433660198cf96113dec4bf07fee4e095521eea
    deps:
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Specification: FEAT-012 — Authentication, Identity, and Authorization

**Feature ID**: FEAT-012
**Status**: approved
**Priority**: P1
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Identity, Tenancy, and Storage Portability
**Covered PRD Requirements**: FR-25
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: AUZ

## Overview

Axon controls who can access data and what operations they can perform. This
feature implements the identity portion of PRD FR-25: stable users, tenant
membership, credentials, and grants that participate in every policy and
audit decision. The design separates identity (who you are) from
authorization (what you can do): external authentication providers resolve to
a stable Axon user, and authorization operates on that user — never on the
raw external identity.

ADR-018 is the governing decision for the tenant/user/credential model,
credential claim shape, and verification order. ADR-005 governs Tailscale as
an authentication provider. This spec defines the required behavior; it does
not restate those decisions.

## Ideal Future State

Every request to Axon — from an agent, an operator, the admin UI, a CI job,
or the CLI — carries a resolved identity with an explicit tenant scope and an
explicit grant set. Operators can provision users ahead of first login, add
them to tenants with role ceilings, and mint narrow, revocable, tenant-bound
credentials for integrations. Audit entries always name a real, stable actor.
Developers run locally with auth disabled and deploy to production with no
behavioral surprises, because both modes converge on the same resolved
identity context.

## Problem Statement

- **Current situation**: Without this feature, all endpoints are open and
  audit entries carry "anonymous" actors.
- **Pain points**: Agents, operators, and the admin UI need distinct
  identities with appropriate access levels. Agents must be able to read and
  write their designated databases but not escalate or drop collections.
  Integrations need scoped, revocable credentials rather than shared secrets.
- **Desired outcome**: Every operation is attributable to a stable user, every
  credential is tenant-bound with grants no broader than the holder's role,
  and unauthorized operations fail closed with stable, observable errors.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| User identity and federation | Who is this caller, across providers and time? | Stable global users; provider-to-user federation; auto-provisioning |
| Tenant membership | What may this user do in this tenant? | M:N membership with per-tenant role ceilings |
| Credentials | How do non-interactive clients authenticate? | Tenant-bound credential issuance, verification, revocation, listing |
| Request authentication | How does a request become a resolved identity? | Bearer-credential and Tailscale paths; dev modes; uniform identity context |
| Role-based authorization | What operation classes does a role permit? | Built-in roles; tag- and assignment-derived roles |
| Network-layer security | What traffic can reach Axon at all? | Tailnet binding and defense-in-depth posture |

## Requirements

### Functional Requirements by Area

#### User Identity and Federation

- **AUZ-01**. Every user must have a stable identifier that never changes.
  Display names, emails, and external identity mappings may all change; the
  user ID is the stable reference used everywhere else.
- **AUZ-02**. Users must be global — not per-node, not per-tenant. One user
  record represents one human or service account across the deployment. A
  user record carries a display name, an optional email, a creation time, and
  a suspension state.
- **AUZ-03**. External authentication providers must map to users through
  provider/external-ID federation mappings. Tailscale is the first provider;
  additional providers (OIDC, email+password) must be addable as new mappings
  without changing user identity or the resolved identity context.
- **AUZ-04**. When a Tailscale-authenticated request arrives for a
  previously-unseen tailnet identity, Axon must auto-provision a user and its
  federation mapping atomically. Concurrent first-seen requests for the same
  external identity must converge on a single user record (concurrency
  invariant per ADR-018).
- **AUZ-05**. Admins must be able to create, list, inspect, update, and
  delete users through the control-plane API (route surface per
  CONTRACT-001). Users can be created before their first login (for example,
  to pre-assign tenant membership) or lazily via auto-provisioning.

#### Tenant Membership

- **AUZ-06**. A user's relationship to a tenant must be an explicit
  membership with a role of `admin`, `write`, or `read`. Membership is
  many-to-many: one user in many tenants, one tenant with many users.
- **AUZ-07**. The membership role is the ceiling of what the user can do in
  that tenant. Grants on any credential issued for that user in that tenant
  must always be a subset of the role's capabilities.
- **AUZ-08**. Memberships must be independent per tenant: a user can be
  `admin` in one tenant and `read` in another with no interaction between the
  two.
- **AUZ-09**. Listing tenants must return only the tenants where the caller
  holds a membership, except for deployment admins, who see all.
- **AUZ-10**. Tenant members must be manageable (list, add with role, change
  role, remove) through the control-plane API (route surface per
  CONTRACT-001).
- **AUZ-11**. On a deployment with zero tenants, a configuration-gated
  bootstrap rule must allow the first auto-provisioned user to become admin
  of a fresh default tenant, matching FEAT-014's default-tenant bootstrap
  behavior.

#### Credentials

- **AUZ-12**. Credentials must be signed tokens issued by the control plane,
  each bound to exactly one tenant and carrying a structured grants object
  describing what the credential can do (databases and permitted operation
  classes). The claim shape, grants rule table, operation-class mapping, and
  default TTL are governed by ADR-018; issuance/revocation/listing endpoints
  are defined in CONTRACT-001.
- **AUZ-13**. Issuance must enforce: the caller is a tenant admin or is the
  target user self-issuing; the requested grants are a subset of the target
  user's role ceiling in that tenant (per the ADR-018 grants rule table); and
  the target user is a member of the tenant. The signed token is returned
  exactly once and is never persisted by the server.
- **AUZ-14**. Every data-plane request presenting a credential must be
  verified in the order governed by ADR-018: signature, validity window,
  revocation status, tenant binding against the addressed tenant, and grant
  coverage of the addressed database and the operation class of the request.
- **AUZ-15**. Credentials must be revocable by credential ID. A revoked
  credential must fail verification within 1 second of revocation.
- **AUZ-16**. Credential listing must return metadata only (credential ID,
  user, issuance and expiry times, revocation status, grants) — never the
  signed token.
- **AUZ-17**. Every authentication or authorization rejection must emit a
  structured log event and increment a rejection counter labeled by error
  code. A rejection that cannot be counted is a bug. Rejection error codes
  are a public SDK contract: they may be added but must never be renamed
  (error envelope and codes per CONTRACT-001 and ADR-018).

#### Request Authentication

- **AUZ-18**. Axon must support two authentication paths that converge on the
  same resolved identity context (user, tenant, grants): bearer-credential
  verification, and Tailscale identity resolution per ADR-005 with federation
  lookup and auto-provisioning. Handlers must not be able to observe which
  path was taken.
- **AUZ-19**. A no-auth development mode must disable authentication and
  synthesize an anonymous identity with a default tenant context and admin
  grants, without persisting any user or tenant records. Embedded/in-process
  use (the CLI's embedded mode) always runs in this mode.
- **AUZ-20**. A guest-role mode must map unauthenticated requests to a fixed,
  configured role on the default tenant, for edge deployments without
  Tailscale.
- **AUZ-21**. The audit `actor` must be the resolved user's display identity
  (display name or email, configurable) — never the raw external identity
  (tailnet handle, OIDC subject).

#### Role-Based Authorization

- **AUZ-22**. Axon must provide four built-in roles with these operation
  classes:

  | Role | Permissions |
  |------|-------------|
  | `admin` | All operations, including drop, schema changes, and control-plane administration |
  | `write` | Create, update, delete entities and links |
  | `read` | Read entities, query, traverse, browse audit log |
  | `none` | No access (explicitly denied) |

- **AUZ-23**. For Tailscale-authenticated callers, roles must be derivable
  from Tailscale ACL tags via a configurable tag-to-role mapping; when a node
  carries multiple role-granting tags, the highest-privilege role wins;
  authenticated callers with no recognized tag receive a configurable default
  role (default `read`). The mapping and flag surface are defined in
  CONTRACT-008 (CLI and configuration).
- **AUZ-24**. Operators must be able to assign roles directly to user
  principals, independent of provider tags. Explicit assignments take
  priority over tag-derived roles, persist across server restarts, and take
  effect within the identity cache TTL. Management is available via CLI and
  control-plane API (surfaces per CONTRACT-008 and CONTRACT-001).

#### Network-Layer Security

- **AUZ-25**. When deployed on a tailnet, Axon must support binding its
  listeners to the Tailscale interface only, so non-tailnet traffic cannot
  connect; network ACLs then provide defense-in-depth ahead of application
  auth.

### Non-Functional Requirements

- **Performance**: Authentication overhead under 2 ms per request with a warm
  identity cache.
- **Caching**: Identity lookups cached with a configurable TTL (default
  60 s).
- **Reliability (fail closed)**: If the identity provider is unavailable
  (e.g., the Tailscale daemon is down), requests fail with a service
  unavailable error — never a silent bypass.
- **Revocation propagation**: Revoked credentials are rejected within 1
  second of revocation.
- **Observability**: 100% of auth rejections produce a structured event and a
  counter increment with a stable error code.
- **Audit integration**: Every audit entry's `actor` reflects the resolved
  authenticated identity.

## Relationship to FEAT-029

FEAT-012 owns identity-level authorization: who the caller is, which tenant
they operate in, the role ceiling, and the database/operation grants on
credentials. Schema-declared data policies — entity-level visibility,
row-level filtering, field-level redaction, and field write control — are
governed by FEAT-029.

The boundary rule is: **FEAT-029 refines access; it never grants access that
FEAT-012/ADR-018 denied.** Identity, tenant membership, credential grants,
and operation class are checked first; FEAT-029 policies apply afterward and
can only narrow the result. The policy grammar, evaluation order, denial
envelopes, and reason codes — including the legacy FEAT-012 policy-rule
schema — are normatively defined in CONTRACT-004. The field-masking and
attribute-based write-control user stories formerly listed here (US-046,
US-047) are owned by FEAT-029.

## User Stories

- [US-043 — Authenticate via Tailscale](../user-stories/US-043-authenticate-via-tailscale.md)
- [US-044 — Role-Based Access Control](../user-stories/US-044-role-based-access-control.md)
- [US-089 — First-Class User with Tailscale Auto-Provisioning](../user-stories/US-089-first-class-user-with-tailscale-auto-provisioning.md)
- [US-090 — JWT Credential for Integration Access](../user-stories/US-090-jwt-credential-for-integration-access.md)
- [US-091 — User in Multiple Tenants](../user-stories/US-091-user-in-multiple-tenants.md)
- [US-123 — Development Without Auth](../user-stories/US-123-development-without-auth.md)
- [US-124 — Per-Principal Role Assignment](../user-stories/US-124-per-principal-role-assignment.md)

## Edge Cases and Error Handling

- **Multiple tags**: A node carrying multiple role-granting tags resolves to
  the highest-privilege role.
- **Unknown tag**: Authenticated nodes with no recognized tag receive the
  configured default role.
- **Identity provider down**: All requests fail closed with service
  unavailable; Axon never falls back to an open posture.
- **Renamed user**: Renaming a user changes the audit actor for subsequent
  entries; the user ID and history attribution remain stable.
- **Remapped external identity**: Changing a federation mapping must reuse
  the existing user record — auto-provisioning must not create a duplicate
  user.
- **Cross-tenant credential use**: A credential presented against a tenant
  other than the one it is bound to is rejected at verification, regardless
  of the holder's memberships.
- **Grant escalation attempt**: Issuance requests with grants above the
  target's role ceiling are rejected and no credential is minted.

## Success Metrics

- Zero data-plane operations execute without a resolved identity outside the
  explicitly configured no-auth/guest modes.
- 100% of audit entries carry a stable resolved actor (no raw external IDs).
- An operator can provision a user, grant tenant membership, and mint a
  scoped integration credential in a single console session.
- Credential revocation takes effect within its 1-second target in
  verification tests.

## Constraints and Assumptions

- ADR-018 is authoritative for the credential claim shape, grants rule table,
  verification order, and concurrency invariants; this spec intentionally
  does not duplicate them.
- Tailscale (per ADR-005) is the default external provider; deployments
  without Tailscale rely on credentials, guest-role, or no-auth modes.
- The control plane (FEAT-025) hosts the persistent stores for users,
  federation mappings, memberships, and revocations.
- Latency targets assume an in-process identity cache; cold lookups may
  exceed the 2 ms budget.

## Dependencies

- **Other features**: FEAT-005 / FEAT-015 / FEAT-016 (auth wraps the native,
  GraphQL, and MCP surfaces); FEAT-014 (tenant model and path-scoped wire
  protocol; auth resolves tenant from the addressed path); FEAT-025 (control
  plane hosts identity/membership/credential storage and routes); FEAT-029
  (consumes the resolved subject for data-layer policies).
- **External services**: Tailscale LocalAPI (per ADR-005). Normative
  surfaces: CONTRACT-001 (control-plane routes, error envelope), CONTRACT-008
  (CLI flags and configuration), CONTRACT-004 (policy grammar boundary),
  ADR-018 (credential model).
- **PRD requirements**: FR-25 (P1).

## Out of Scope

- Schema-declared entity, row, and field-level data policies — governed by
  FEAT-029 (grammar in CONTRACT-004).
- External IdP federation beyond Tailscale (OIDC, email+password) — the
  federation model accommodates it, but no additional provider is required.
  Axon issues, rotates, and revokes its own credentials per ADR-018; token
  lifecycle is not delegated to an external IdP.
- User and credential management *UI* — the screens are owned by FEAT-011;
  this feature owns the control-plane behavior they call.
- Opaque API keys as a separate mechanism — Axon-issued credentials are the
  supported machine authentication path.
- Row-level security with arbitrary SQL-like predicates — FEAT-029's closed
  policy grammar governs what predicates exist.
