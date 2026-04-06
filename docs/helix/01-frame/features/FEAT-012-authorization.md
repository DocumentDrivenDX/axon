---
dun:
  id: FEAT-012
  depends_on:
    - helix.prd
    - FEAT-005
    - ADR-005
---
# Feature Specification: FEAT-012 - Authorization

**Feature ID**: FEAT-012
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Axon requires an authentication and authorization layer to control who can
access data and what operations they can perform. The auth model is built
on **OIDC (OpenID Connect)** for identity, with **Tailscale** as the
default (and first) identity provider via its LocalAPI whois mechanism.

The design separates identity (who you are) from authorization (what you
can do), allowing other OIDC providers (Auth0, Okta, Google, etc.) to be
added without changing the authorization model.

## Problem Statement

Axon currently has no authentication — all endpoints are open. In production,
agents, operators, and the admin UI all need distinct identities with
appropriate access levels. Agents should be able to read and write their
designated collections but not drop them. Operators should have full access.
Audit log entries should carry real actor identities, not "anonymous".

## Requirements

### Functional Requirements

#### Identity (Authentication)

- **OIDC-based**: Identity is established via an OIDC-compatible provider
  that supplies a verified user/node identity on each request
- **Tailscale as default provider**: When running on a tailnet, identity
  comes from Tailscale's LocalAPI whois endpoint (see ADR-005). The
  whois response provides user email, node name, and ACL tags
- **Provider-agnostic authorization**: The authorization layer consumes
  a normalized identity (email, roles/tags) regardless of which OIDC
  provider supplied it
- **No-auth mode**: `--no-auth` flag or `AXON_NO_AUTH=1` disables
  authentication entirely. All requests get actor `"anonymous"` with
  admin privileges. Required for local development and embedded mode
- **Identity propagation**: The authenticated identity is injected into
  the request context and used as the `actor` field in audit log entries

#### Authorization (What You Can Do)

- **Role-based access control (RBAC)**: Four built-in roles:

  | Role | Permissions |
  |------|-------------|
  | `admin` | All operations on all collections, including drop and schema changes |
  | `write` | Create, update, delete entities and links in any collection |
  | `read` | Read entities, query, traverse, browse audit log |
  | `none` | No access (explicitly denied) |

- **Role assignment**: Roles are derived from the identity provider:
  - **Tailscale**: Mapped from ACL tags (`tag:axon-admin` → admin,
    `tag:axon-write` → write, `tag:axon-read` → read)
  - **Generic OIDC**: Mapped from JWT claims (configurable claim name,
    e.g., `axon_role` in the ID token)
  - **Default role**: Configurable. Default is `read` for authenticated
    users with no explicit role assignment

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

| Phase | Capability |
|-------|-----------|
| V1 | Global RBAC roles (admin/write/read/none) + --no-auth mode |
| V2 | Per-collection policies, field masking, field immutability |
| V3 | Per-entity attribute conditions, policy inheritance, policy UI |

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
- [ ] Agent connects via Tailscale IP; Axon resolves its identity via whois
- [ ] Audit entries show the agent's Tailscale node name as actor
- [ ] Connections from outside the tailnet are rejected with 401
- [ ] Agent with no recognized Tailscale tags receives the configured `default_role` (default: `read`)

### Story US-044: Role-Based Access Control [FEAT-012]

**As an** operator managing Axon
**I want** to restrict what agents can do based on their role
**So that** agents can't accidentally drop collections or modify schemas

**Acceptance Criteria:**
- [ ] An agent with `tag:axon-write` can create/update/delete entities
- [ ] An agent with `tag:axon-write` cannot drop collections or change schemas
- [ ] An agent with `tag:axon-read` gets 403 on any write operation
- [ ] An admin with `tag:axon-admin` can perform all operations
- [ ] When a node has multiple role-granting tags, the highest-privilege role wins (admin > write > read)

### Story US-045: Development Without Auth [FEAT-012]

**As a** developer running Axon locally
**I want** to disable auth for development
**So that** I don't need a Tailscale connection during development

**Acceptance Criteria:**
- [ ] `axon-server --no-auth` starts without requiring tailscaled
- [ ] All requests succeed as admin in no-auth mode
- [ ] Audit entries show actor as `"anonymous"` in no-auth mode

## Technical Design

### Identity Resolution Flow

```
Request arrives
    │
    ▼
┌─────────────────┐
│  --no-auth set?  │──yes──▶ Identity = { actor: "anonymous", role: admin }
└────────┬────────┘
         │ no
         ▼
┌─────────────────┐
│ Extract peer IP  │
│ from connection  │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Check cache      │──hit──▶ Use cached identity
│ (IP → Identity)  │
└────────┬────────┘
         │ miss
         ▼
┌─────────────────┐
│ Call provider    │
│ (Tailscale whois │
│  or OIDC verify) │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Map to Role      │
│ (tags → role)    │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Cache + inject   │
│ into request ctx │
└─────────────────┘
```

### Middleware Architecture

```rust
// axum middleware (HTTP)
async fn auth_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(auth): State<AuthProvider>,
    mut request: Request,
    next: Next,
) -> Response {
    match auth.resolve_identity(addr).await {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(AuthError::Unauthorized) => StatusCode::UNAUTHORIZED.into_response(),
        Err(AuthError::Forbidden) => StatusCode::FORBIDDEN.into_response(),
        Err(AuthError::Unavailable) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

// Same pattern for tonic interceptor (gRPC)
```

### Provider Trait

```rust
#[async_trait]
trait IdentityProvider: Send + Sync {
    async fn resolve(&self, peer_addr: SocketAddr) -> Result<Identity, AuthError>;
}

struct Identity {
    actor: String,       // email, node name, or "anonymous"
    role: Role,          // admin, write, read, none
    provider: String,    // "tailscale", "oidc", "none"
    raw_claims: Value,   // provider-specific metadata
}
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
- **ADR-005**: Architecture decision for Tailscale LocalAPI whois
- `tailscale-localapi` crate (v0.5.0) for whois calls
- `jsonwebtoken` crate for generic OIDC JWT validation (Phase 2)

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
- **User Stories**: US-043, US-044, US-045
- **Architecture**: ADR-005 (Tailscale LocalAPI whois)
- **Implementation**: `crates/axon-server/src/auth/` (planned)

### Feature Dependencies
- **Depends On**: FEAT-005
- **Depended By**: FEAT-011 (Admin UI will inherit auth when enabled)
