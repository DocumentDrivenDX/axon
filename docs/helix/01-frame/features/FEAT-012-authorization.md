---
dun:
  id: FEAT-012
  depends_on:
    - helix.prd
    - FEAT-005
    - FEAT-014
    - ADR-005
    - ADR-018
---
# Feature Specification: FEAT-012 - Authentication, Identity, and Authorization

**Feature ID**: FEAT-012
**Status**: Active — V1 shipped; V2 scaffolded; V3 deferred; V5 (tenants + JWT) in evolution
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-14

## Implementation Status

| Phase | Capability | Status |
|-------|-----------|--------|
| V1 | Global RBAC (admin/write/read/none) + `--no-auth` + `--guest-role` | **Shipped** |
| V2 | `MaskPolicy` / `WritePolicy` structs and `GrantRegistry` defined | **Scaffolded** — types built and tested in `axon-core`, not yet wired into handlers |
| V3 | Per-entity attribute conditions, policy inheritance, policy UI | **Deferred** |
| V4 | Per-principal RBAC: Axon-owned user→role registry; `axon user grant/revoke/list`; `/control/users` REST API | **Superseded by V5** |
| V5 | First-class `User` type with federation; M:N tenant membership (`tenant_users`); JWT credentials with `grants` claim; path-based wire protocol | **In Evolution (ADR-018)** |

Key V1 implementation choices that differ from this spec:
- **No OIDC support** — Tailscale is the only external provider; OIDC deferred to V2+
- **No `tailscale-localapi` crate** — identity is resolved via direct HTTP/1.1 over the
  Tailscale Unix socket using `hyper` + `tokio::net::UnixStream`
- **`--guest-role` mode added** — unauthenticated requests receive a fixed role (not in
  original spec, added during implementation for edge deployments without Tailscale)
- **Actor = LoginName (email), with node-name fallback** — `Identity.actor` is set to
  the Tailscale `UserProfile.LoginName` (email) when a user profile is present; for
  tagged service nodes with no user profile the `ComputedName` (node name) is used as
  fallback

## Overview

Axon requires an authentication and authorization layer to control who can
access data and what operations they can perform. The V1 auth model uses
**Tailscale LocalAPI whois** for identity, with Tailscale ACL tags mapping
to built-in RBAC roles.

The design separates identity (who you are) from authorization (what you
can do). Future phases may add OIDC providers, but V1 is Tailscale-only.

## Problem Statement

Axon currently has no authentication — all endpoints are open. In production,
agents, operators, and the admin UI all need distinct identities with
appropriate access levels. Agents should be able to read and write their
designated collections but not drop them. Operators should have full access.
Audit log entries should carry real actor identities, not "anonymous".

## Requirements

### Functional Requirements

#### Users (First-Class Type, V5)

Users are a first-class concept in the control plane, independent of any
specific authentication provider. This supersedes the V4 per-principal
registry that treated Tailscale identities as the user (see ADR-018).

- **Stable identity**: Every user has a UUID that never changes. Display
  names, emails, and external identity mappings can all change; the
  `user_id` is the stable reference used by every other table that
  refers to users.
- **Global scope**: Users are global — not per-node, not per-tenant. A
  single `users` row represents a single human (or service account)
  across the entire deployment.
- **Minimal core fields**: `{ id, display_name, email?, created_at_ms,
  suspended_at_ms? }`. Additional profile fields (avatar, preferences,
  timezone) may be added later as optional columns or as entries in a
  user-attributes table. The core shape does not change.
- **Federation via `user_identities`**: External authentication providers
  map to a user through a separate `user_identities(provider, external_id,
  user_id)` table. Supported providers in V5:
  - `"tailscale"` — `external_id` = tailnet handle (e.g., `alice@tailnet`)
  - Future: `"oidc"` — `external_id` = OIDC `sub` claim; `"email"` for
    email+password; additional providers slot in the same way
- **Auto-provisioning from Tailscale (preserves V1 behavior)**: When a
  Tailscale-authenticated request arrives and no `user_identities` row
  matches the tailnet handle, the auth middleware creates a new `users`
  row and a `user_identities` row atomically. This preserves the
  implicit "whoever is on the tailnet is a user" behavior of V1 while
  upgrading the underlying data model.
- **Admin user management**: `GET /control/users`, `POST /control/users`,
  `GET /control/users/{id}`, `DELETE /control/users/{id}` (admin only).
  Users can be explicitly created before their first login (e.g., to
  pre-assign tenant membership), or created lazily on first login
  through auto-provisioning.

#### Tenant Membership (M:N, V5)

A user's relationship to a tenant is a row in `tenant_users`. The model
is explicitly many-to-many: one user can be a member of many tenants,
one tenant has many users.

- **`tenant_users(tenant_id, user_id, role, added_at_ms)`** — composite
  primary key on `(tenant_id, user_id)`. Role is `admin | write | read`.
- **Role ceiling**: The role on this row is the *ceiling* of what the
  user can do in that tenant. Credentials the user issues for themself
  (or that an admin issues on their behalf) have grants that are
  **always** a subset of the role's capabilities.
- **Per-tenant independence**: A user can be `admin` in tenant A and
  `read` in tenant B with no interaction between the two memberships.
- **Listing tenants visible to a user**: `GET /control/tenants` returns
  only the tenants in which the caller has a membership row (unless
  the caller is a deployment admin, who sees all).
- **M:N management routes**:
  - `GET /control/tenants/{id}/users` — list members
  - `POST /control/tenants/{id}/users/{user_id}` — add a member with a
    role (body: `{ "role": "admin" }`)
  - `PUT /control/tenants/{id}/users/{user_id}` — change a member's role
  - `DELETE /control/tenants/{id}/users/{user_id}` — remove membership
- **Tailscale → membership resolution**: When Tailscale auto-provisions
  a user, the middleware also checks for an auto-admin bootstrap rule
  (config flag). On a deployment with zero tenants, the first
  auto-provisioned user is added as admin of a fresh `default` tenant —
  matching FEAT-014's default-tenant bootstrap story.

#### Credentials (JWT, V5)

Credentials are signed JWTs issued by the control plane. Each credential
is bound to exactly one tenant via the `aud` claim and carries a
structured `grants` object that describes what the credential can do.
Credentials are the primary auth mechanism for non-Tailscale clients
(CLI from outside the tailnet, SDKs used by integrations, CI jobs,
machine-to-machine).

- **Claims shape** (see ADR-018 for the full JSON example):
  - `iss` — deployment identifier
  - `sub` — user's stable UUID
  - `aud` — tenant_id (singular — one credential per tenant)
  - `jti` — unique credential id for revocation
  - `iat` / `nbf` / `exp` — standard JWT timing claims; default TTL 24h
  - `grants` — opaque-to-JWT, parsed by axon-server. v1 shape:
    `{"databases": [{"name": "...", "ops": ["read"|"write"]}]}`
- **Issuance**: `POST /control/tenants/{id}/credentials` with a body
  describing the target user, grants, and TTL. The endpoint enforces:
  - The caller must be a tenant admin (or must be the target user
    themselves, issuing for their own use) — this is the self-issue
    case for CLI tokens and the admin-issue case for integration tokens.
  - The requested grants must be a subset of what the target user's
    role in the tenant permits (grants ≤ role). **See ADR-018 Section 4
    for the normative grants rule table (admin/write/read issuer
    capabilities) and op-to-HTTP-method mapping.** This feature spec
    defers to the ADR for the authoritative mapping to avoid drift.
  - The user must be a member of the tenant.
  - Returns the signed JWT string in the response body; the server
    does not persist the signed token (only the jti, for revocation
    tracking).
- **Verification** (on every data-plane request): extract
  `Authorization: Bearer <jwt>`, verify signature, check `exp` / `nbf` /
  `jti` revocation, compare `aud` to the URL path's `{tenant}` segment,
  resolve `sub` to a `users` row, check `grants.databases[]` against
  the URL path's `{database}` segment and the request method's required
  op. Install `(user_id, tenant_id, grants)` into the request extension.
- **Revocation**: `DELETE /control/tenants/{id}/credentials/{jti}` adds
  the jti to `credential_revocations`. An in-memory LRU cache in front
  of the SQL table keeps the verification path fast. Revoked credentials
  fail verification within ~1s of the DELETE call (LRU propagation time).
- **Listing**: `GET /control/tenants/{id}/credentials` returns metadata
  only — jti, user_id, issued_at, expires_at, revoked status, and grants.
  Never the signed JWT string (which is not persisted anyway).
- **Observability envelope** (cross-reference ADR-018 Section 4): every
  auth rejection emits a structured log event with fields `{error_code,
  tenant_path, database_path, op, user_id_if_known, jti_if_known,
  remote_addr}` and increments
  `axon_auth_rejections_total{error_code="..."}`. A rejection that cannot
  be counted is a bug. Operators use this for dashboard alerting on
  credential expiry waves and for post-incident forensics.
- **Error-code stability**: the `error.code` strings in ADR-018's failure
  mode table are a public SDK contract. They MUST NOT be renamed without
  a coordinated SDK release. Adding new codes is allowed; renaming
  existing ones is a breaking change.

#### Identity (Authentication)

Authentication establishes *who* the caller is by resolving an external
credential (a JWT, a Tailscale tailnet identity, etc.) to a `user_id`
in the global `users` table. Downstream authorization always operates
on the resolved user, never on the raw external identity.

- **Two auth paths in V5**:
  1. **JWT credential** (see "Credentials (JWT)" section above): the
     request carries `Authorization: Bearer <jwt>`. The server verifies
     the signature, extracts `sub` as the user_id, and skips the
     provider-specific resolution.
  2. **Tailscale whois** (preserved from V1): no `Authorization` header.
     The server resolves the tailnet identity via ADR-005's LocalAPI
     whois, looks up `user_identities(provider="tailscale",
     external_id=<tailnet handle>)`, and uses the resulting user_id
     (auto-provisioning on first seen). Auto-provisioning MUST use an
     `INSERT ... ON CONFLICT DO NOTHING` transaction on
     `(provider, external_id)` — never check-then-insert — so that parallel
     first-seen requests for the same tailnet identity converge on a
     single `users` row. See ADR-018 Section 6 for the normative SQL
     pattern and the required concurrency invariant.
- **Both paths converge** on the same request extension:
  `(user_id, tenant_id, grants)`. Handlers do not know which
  authentication path was taken.
- **Future providers (OIDC, email+password)**: additional rows in
  `user_identities` with new `provider` values. The `users` table and
  the request extension shape do not change. OIDC adds a validation
  path (verify ID token signature, extract `sub`, look up federation
  row).
- **`--no-auth` mode**: disables authentication entirely. The request
  extension is synthesized with a synthetic anonymous user, a default
  tenant context, and admin grants. Required for local development
  and embedded mode. Does not persist users or tenants — a pure
  in-memory convenience.
- **`--guest-role` mode** (preserved from V1): edge deployments without
  Tailscale can map unauthenticated requests to a fixed role on the
  default tenant. Implementation is identical to `--no-auth` modulo
  the role ceiling.
- **Identity propagation into audit**: the resolved `user.display_name`
  (or `user.email`, configurable) is the `actor` field in audit log
  entries. Audit never records the external id (tailnet handle, OIDC
  sub) — that's a federation-table detail.

#### Authorization (What You Can Do)

- **Role-based access control (RBAC)**: Four built-in roles:

  | Role | Permissions |
  |------|-------------|
  | `admin` | All operations on all collections, including drop and schema changes |
  | `write` | Create, update, delete entities and links in any collection |
  | `read` | Read entities, query, traverse, browse audit log |
  | `none` | No access (explicitly denied) |

- **Role assignment**: Roles are derived from Tailscale ACL tags.  When a
  node carries multiple role-granting tags the highest-privilege role wins:

  | Tag | Role |
  |-----|------|
  | `tag:axon-admin` / `tag:admin` | `admin` |
  | `tag:axon-write` / `tag:axon-agent` / `tag:write` | `write` |
  | `tag:axon-read` / `tag:read` | `read` |
  | *(no matching tag)* | `--tailscale-default-role` (default `read`) |

  `tag:axon-agent` is the conventional tag for automated agent workloads
  that need read/write but not admin access.

- **Default role**: Configurable via `--tailscale-default-role` (default `read`)
  for authenticated nodes that carry no recognized ACL tag

#### Attribute-Based Access Control (ABAC)

Beyond global roles, Axon supports fine-grained access policies based on
attributes of the **user**, the **resource** (entity/collection), and the
**action**. Policies are expressed as rules that combine these attributes.

- **Per-collection permissions**: Rules that scope a role to specific
  collections. Example: "erik has `write` on `technical-designs` but
  `read` on `prds`; mike has `write` on `prds` but `read` on
  `technical-designs`"

- **Per-entity policies**: Rules based on entity data attributes.
  Example: "agents with role `write` can only update entities where
  `status != 'approved'`" — preventing mutation of finalized records

- **Field-level visibility (masking)**: Certain fields in entity data
  can be hidden from users who lack a required attribute. Example:
  `salary` field in `employees` collection is visible only to users
  with `tag:hr-admin`. Other users see the entity but the masked
  fields are omitted from the response

- **Field-level immutability**: Certain fields can be made read-only
  for specific roles. Example: `approved_by` field can only be set by
  users with `admin` role; `write` users can update other fields but
  `approved_by` is silently preserved (not overwritten) or rejected

- **Policy storage**: ABAC policies are stored as entities in a
  system collection (`__axon_policies__`) with a defined schema.
  Policies are themselves audited — every policy change produces an
  audit entry

- **Policy evaluation order**: Deny rules take precedence over allow
  rules. More-specific rules (per-entity) override less-specific
  (per-collection). Explicit rules override the default role.

##### Policy Rule Schema (Conceptual)

```json
{
  "id": "pol-001",
  "effect": "allow",
  "principal": { "email": "erik@example.com" },
  "action": ["write"],
  "resource": {
    "collection": "technical-designs"
  }
}
```

```json
{
  "id": "pol-002",
  "effect": "deny",
  "principal": { "tag": "tag:axon-agent" },
  "action": ["update"],
  "resource": {
    "collection": "invoices",
    "condition": { "field": "status", "eq": "approved" }
  }
}
```

```json
{
  "id": "pol-003",
  "effect": "mask",
  "principal": { "role": "read" },
  "resource": {
    "collection": "employees",
    "fields": ["salary", "ssn"]
  }
}
```

```json
{
  "id": "pol-004",
  "effect": "immutable",
  "principal": { "role": "write" },
  "resource": {
    "collection": "contracts",
    "fields": ["approved_by", "approval_date"]
  }
}
```

##### Implementation Phases

| Phase | Capability | Status |
|-------|-----------|--------|
| V1 | Global RBAC roles (admin/write/read/none) + `--no-auth` + `--guest-role` | **Shipped** |
| V2 | Per-collection policies, field masking, field immutability | Scaffolded (structs + tests, not wired) |
| V3 | Per-entity attribute conditions, policy inheritance, policy UI | Deferred |

#### Network-Layer Security (Tailscale-Specific)

- **Tailscale ACLs**: When using Tailscale, network-layer ACLs are
  enforced before traffic reaches Axon. This provides defense-in-depth:
  unauthorized nodes can't even establish a connection
- **Bind to Tailscale interface**: Option to bind HTTP/gRPC to the
  Tailscale interface (100.x.x.x) only, rejecting non-tailnet traffic

### Non-Functional Requirements

- **Latency**: Auth overhead < 2ms per request (cached identity lookup)
- **Caching**: Identity lookups cached per source IP with configurable
  TTL (default: 60s)
- **Graceful degradation**: If the identity provider is unavailable
  (e.g., tailscaled down), requests fail with 503 Service Unavailable,
  not silent bypass
- **Audit integration**: Every audit entry's `actor` field reflects the
  authenticated identity (email for Tailscale, subject claim for OIDC)

## User Stories

### Story US-043: Authenticate via Tailscale [FEAT-012]

**As an** agent running on a tailnet
**I want** Axon to recognize my Tailscale identity
**So that** my operations are attributed to me in the audit log

**Acceptance Criteria:**
- [x] Agent connects via Tailscale IP; Axon resolves its identity via
  whois. Test: `crates/axon-server/tests/federation_test.rs`
- [x] Audit entries show the resolved identity as actor. Test:
  `crates/axon-server/tests/api_contract.rs`,
  `crates/axon-server/tests/cutover_jwt_test.rs`
- [x] Connections without valid authentication are rejected with 401.
  Test: `crates/axon-server/tests/auth_pipeline_test.rs`,
  `crates/axon-server/tests/auth_pipeline_integration_test.rs`
- [x] Agent with no recognized Tailscale tags is still resolved to a
  stable user identity for later role assignment. Test:
  `crates/axon-server/tests/federation_test.rs`

### Story US-044: Role-Based Access Control [FEAT-012]

**As an** operator managing Axon
**I want** to restrict what agents can do based on their role
**So that** agents can't accidentally drop collections or modify schemas

**Acceptance Criteria:**
- [x] A JWT credential with write grants can create/update/delete
  entities. Test: `crates/axon-server/tests/cutover_jwt_test.rs`,
  `crates/axon-server/tests/control_credentials_test.rs`
- [x] A write-level principal cannot perform admin-only operations such
  as credential escalation. Test:
  `crates/axon-server/tests/control_credentials_test.rs`
- [x] A read-only credential gets 403 on write operations. Test:
  `crates/axon-server/tests/cutover_jwt_test.rs`,
  `crates/axon-server/tests/auth_pipeline_test.rs`
- [x] An admin principal can perform tenant control-plane operations.
  Test: `crates/axon-server/tests/control_tenants_test.rs`,
  `crates/axon-server/tests/control_databases_test.rs`,
  `crates/axon-server/tests/control_users_provision_test.rs`
- [x] Grant validation enforces the highest allowed privilege ceiling for
  a tenant member and rejects grants above that ceiling. Test:
  `crates/axon-server/tests/control_credentials_test.rs`

### Story US-045: Development Without Auth [FEAT-012]

**As a** developer running Axon locally
**I want** to disable auth for development
**So that** I don't need a Tailscale connection during development

**Acceptance Criteria:**
- [x] `axon-server --no-auth` starts without requiring tailscaled. Test:
  `crates/axon-server/tests/no_auth_test.rs`,
  `crates/axon-server/tests/cutover_jwt_test.rs`
- [x] Requests receive a synthetic admin identity in no-auth mode. Test:
  `crates/axon-server/tests/no_auth_test.rs`,
  `crates/axon-server/tests/control_tenants_test.rs`
- [x] Audit entries show actor as `"anonymous"` in no-auth mode. Test:
  `crates/axon-server/tests/api_contract.rs`

## Technical Design

### Identity Resolution Flow

```
Request arrives
    │
    ▼
┌─────────────────┐
│  --no-auth set?  │──yes──▶ Identity = { actor: "anonymous", role: Admin }
└────────┬────────┘
         │ no
         ▼
┌─────────────────┐
│  --guest-role?   │──yes──▶ Identity = { actor: "guest", role: <configured> }
└────────┬────────┘
         │ no (Tailscale mode)
         ▼
┌─────────────────┐
│ Extract peer IP  │
│ from connection  │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Check IP cache   │──hit──▶ Use cached identity (TTL: 60 s default)
│ (RwLock map)     │
└────────┬────────┘
         │ miss
         ▼
┌──────────────────────────────────┐
│ GET /localapi/v0/whois?addr=peer  │
│ via Unix socket (tailscaled.sock) │
└────────┬─────────────────────────┘
         │ 200 OK                  └─ 422 → 401 Unauthorized
         ▼                         └─ socket error → 503 Unavailable
┌─────────────────────────────────┐
│ Axon user-role registry lookup   │
│ by UserProfile.LoginName         │──found──▶ Use assigned role
└────────────┬────────────────────┘
             │ not found
             ▼
┌─────────────────┐
│ Map ACL tags     │
│ to Role          │
└────────┬────────┘
         │ no matching tags
         ▼
    default_role
         │
         ▼
┌─────────────────┐
│ Cache + inject   │
│ Identity into    │
│ request context  │
└─────────────────┘
```

### Middleware Architecture

HTTP auth runs as an axum middleware layer (`authenticate_http_request` in
`crates/axon-server/src/gateway.rs`). gRPC uses `AxonServiceImpl::authorize`
in `crates/axon-server/src/service.rs`. Both delegate to `AuthContext::resolve_peer`.

```rust
// Actual implementation (axon-server/src/gateway.rs)
pub(crate) async fn authenticate_http_request(
    State(auth): State<AuthContext>,
    mut request: Request,
    next: Next,
) -> Response {
    match auth.resolve_peer(request_peer_address(&request)).await {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(error) => auth_error_response(error),
    }
}
```

Route handlers extract identity with `Extension<Identity>` and enforce
role requirements:

```rust
async fn my_handler(Extension(identity): Extension<Identity>, ...) {
    identity.require_write()?;  // or require_read() / require_admin()
    // ... handler logic
}
```

### Auth Types

```rust
// crates/axon-server/src/auth.rs
pub struct Identity {
    pub actor: String,  // Tailscale LoginName (email), node name fallback, "anonymous", or "guest"
    pub role: Role,     // Admin | Write | Read
}

pub enum AuthMode {
    NoAuth,
    Tailscale { default_role: Role },
    Guest { role: Role },
}

/// In-memory, write-through cache of principal→role assignments.
/// Backed by the control-plane SQLite database.
#[derive(Clone, Default)]
pub struct UserRoleStore(Arc<RwLock<HashMap<String, Role>>>);
```

### Configuration

```toml
# axon-server.toml (or CLI flags / env vars)

[auth]
# "tailscale" | "oidc" | "none"
provider = "tailscale"
# Default role for authenticated users without explicit role assignment
default_role = "read"
# Cache TTL for identity lookups (seconds)
cache_ttl = 60

[auth.tailscale]
# Path to tailscaled socket (auto-detected on Linux/macOS)
socket = "/run/tailscale/tailscaled.sock"
# Role mapping: Tailscale ACL tag → Axon role
[auth.tailscale.role_map]
"tag:axon-admin" = "admin"
"tag:axon-write" = "write"
"tag:axon-read"  = "read"

[auth.oidc]
# Standard OIDC configuration (for non-Tailscale deployments)
issuer = "https://accounts.google.com"
audience = "axon-server"
role_claim = "axon_role"
```

## Edge Cases

- **Multiple tags**: If a Tailscale node has both `tag:axon-admin` and
  `tag:axon-read`, the highest-privilege role wins (admin)
- **Unknown tag**: Nodes with no recognized Axon tags get the
  `default_role` (configurable, default: `read`)
- **tailscaled down**: If the Tailscale socket is unreachable, return
  503 on all requests. Do not fail open
- **Cache poisoning**: Cache is keyed by source IP + port. Tailscale
  IPs are stable per-node, so the cache is safe
- **Embedded mode**: The CLI always runs with no-auth (in-process, no
  network boundary to authenticate)

## Dependencies

- **FEAT-005** (API Surface): Auth middleware wraps HTTP and gRPC endpoints
- **FEAT-014** (Multi-Tenancy): Tenant as global account boundary; path-
  based wire protocol `/tenants/{tenant}/databases/{database}/...`.
  FEAT-012's auth middleware resolves tenant from the URL path.
- **FEAT-025** (Control Plane): Hosts the SQL tables (`users`,
  `user_identities`, `tenant_users`, `credential_revocations`) and the
  control-plane CRUD routes for users, memberships, and credentials.
- **ADR-005**: Architecture decision for Tailscale LocalAPI whois (still
  the default auth provider in V5; becomes one of several federations)
- **ADR-018**: Governing decision for tenant/user/credential model, JWT
  claim shape, verification order, and the walk-back of the pre-V5
  "database = tenant" model
- `hyper` + `tokio::net::UnixStream` — direct HTTP/1.1 over the Tailscale Unix socket
  (`tailscale-localapi` crate was evaluated but not used; direct socket calls chosen)
- `jsonwebtoken` — **required for V5** (JWT signing + verification)

### Story US-048: Per-Principal Role Assignment [FEAT-012]

**As an** operator
**I want** to assign roles directly to user principals (by email/login)
**So that** I control authorization in Axon without needing to configure Tailscale ACL tags

**Acceptance Criteria:**
- [ ] `axon user grant erik@example.com admin` assigns the admin role to that principal
- [ ] `axon user revoke erik@example.com` removes the explicit assignment; falls back to tag-based role then default
- [ ] `axon user list` shows all explicit role assignments
- [ ] `GET /control/users` returns all user-role mappings (admin only)
- [ ] `PUT /control/users/{login}` sets the role for a principal (admin only)
- [ ] `DELETE /control/users/{login}` removes the explicit role for a principal (admin only)
- [ ] Role assignments survive server restarts (persisted in control-plane SQLite)
- [ ] Changes take effect within the identity cache TTL (default 60s)
- [ ] Axon-owned role assignment takes priority over Tailscale ACL tag mapping

### Story US-046: Field-Level Masking [FEAT-012]

**As a** data steward
**I want** sensitive fields hidden from unauthorized users
**So that** PII and confidential data is only visible to those who need it

**Acceptance Criteria:**
- [ ] A `mask` policy on `employees.salary` hides the field from `read`-role users
- [ ] `admin` users see the full entity including masked fields
- [ ] Masked fields are omitted from the response (not replaced with null or redacted)
- [ ] Masking applies to query results, entity detail, and audit log data_after

### Story US-047: Attribute-Based Write Control [FEAT-012]

**As an** operator
**I want** to control who can edit which collections and fields
**So that** PRD authors can't edit technical designs and vice versa

**Acceptance Criteria:**
- [ ] A policy grants erik `write` on `technical-designs` and `read` on `prds`
- [ ] erik's attempt to update an entity in `prds` returns 403
- [ ] mike's complementary policy allows the reverse
- [ ] An `immutable` policy on `contracts.approved_by` prevents `write`-role users from changing that field

### Story US-089: First-Class User with Tailscale Auto-Provisioning [FEAT-012]

**As a** developer logging in via Tailscale
**I want** my Tailscale identity to resolve to a stable user row
**So that** my audit entries are attributed to a consistent user even if
  my tailnet handle changes

**Acceptance Criteria:**
- [ ] First request from a previously-unseen tailnet identity creates a
  `users` row (display_name from tailnet) and a
  `user_identities(provider="tailscale", external_id=<handle>)` row
- [ ] Subsequent requests resolve to the same user_id via the federation
  table lookup
- [ ] Audit entries carry the user's display_name or email, not the
  raw tailnet handle
- [ ] If the operator renames the user (`PUT /control/users/{id}`) the
  next audit entry uses the new display name
- [ ] If the operator changes the tailnet handle mapping, auto-provision
  does not create a duplicate user — the existing row is reused
- [ ] `--no-auth` mode still works without persisting any user rows
  (synthesizes an anonymous context in memory)

### Story US-090: JWT Credential for Integration Access [FEAT-012]

**As an** operator issuing access to a CI job
**I want** to mint a tenant-scoped JWT with narrow grants
**So that** the CI job can only read one database and cannot escalate

**Acceptance Criteria:**
- [ ] `POST /control/tenants/{id}/credentials` with a body specifying
  the target user, grants `{databases: [{name: "ci", ops: ["read"]}]}`,
  and TTL returns a signed JWT
- [ ] The returned JWT has `aud=tenant_id`, `sub=user_id`, `jti=<uuid>`,
  `exp=<ttl>`, and the specified `grants` claim
- [ ] Presenting the JWT on `GET /tenants/{id}/databases/ci/entities/...`
  succeeds when the op is `read`
- [ ] Presenting the JWT on `POST /tenants/{id}/databases/ci/entities/...`
  returns 403 (op mismatch)
- [ ] Presenting the JWT on `GET /tenants/{id}/databases/prod/...`
  returns 403 (database not in grants)
- [ ] Presenting the JWT against a different tenant's path returns 403
  (aud mismatch)
- [ ] `DELETE /control/tenants/{id}/credentials/{jti}` revokes the JWT;
  subsequent requests using it return 401 within 1 second
- [ ] Issuing a credential with grants exceeding the issuer's role
  returns 403 and nothing is minted

### Story US-091: User in Multiple Tenants [FEAT-012]

**As a** developer belonging to two customer tenants
**I want** my single user identity to have different roles in each
**So that** I can admin `acme` while having read-only access to `globex`

**Acceptance Criteria:**
- [ ] A single `users` row has two `tenant_users` rows with different
  roles — `admin` in acme, `read` in globex
- [ ] `GET /control/tenants` from my account lists both acme and globex
- [ ] A JWT credential issued for acme (`aud=acme`) cannot be presented
  against globex's path (different `aud`)
- [ ] Removing my membership from one tenant does not affect the other
- [ ] Auth middleware on `/tenants/acme/...` honors the acme role
  regardless of any globex membership
- [ ] Audit entries on both tenants carry the same user_id
- [ ] Credentials I issue for acme cannot contain grants that reference
  databases in globex (verified at issuance time)

## Out of Scope

- API key authentication (use OIDC instead)
- User management UI (roles come from the identity provider, not Axon)
- Token refresh/rotation (handled by the identity provider)
- Multi-provider federation (one provider per deployment in V1)
- Row-level security with SQL-like predicates (ABAC conditions are simpler
  field equality checks, not arbitrary expressions)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #6 (Authentication/authorization)
- **User Stories**: US-043, US-044, US-045 (shipped); US-046, US-047 (scaffolded); US-048 (in progress)
- **Architecture**: ADR-005 (Tailscale LocalAPI whois)
- **Implementation**:
  - `crates/axon-server/src/auth.rs` — `AuthMode`, `AuthContext`, `Identity`, `LocalApiWhoisProvider`
  - `crates/axon-core/src/auth.rs` — `Role`, `CallerIdentity`, `MaskPolicy`, `WritePolicy`, `GrantRegistry`
  - `crates/axon-server/src/gateway.rs` — `authenticate_http_request` middleware, `/auth/me` endpoint
  - `crates/axon-server/src/service.rs` — gRPC `authorize` + `auth_to_status`
  - `crates/axon-server/src/serve.rs` — `auth_context_from_serve_args`, CLI flags
  - `crates/axon-server/src/user_roles.rs` — `UserRoleStore` (in-memory + SQLite backing)
  - `crates/axon-server/src/control_plane_routes.rs` — `/control/users` REST endpoints
  - `crates/axon-cli/src/main.rs` — `axon user grant/revoke/list` CLI commands

### Feature Dependencies
- **Depends On**: FEAT-005
- **Depended By**: FEAT-011 (Admin UI will inherit auth when enabled)
