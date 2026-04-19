---
dun:
  id: ADR-018
  depends_on:
    - helix.prd
    - ADR-005
    - ADR-011
    - ADR-012
    - ADR-017
    - FEAT-012
    - FEAT-014
    - FEAT-025
---
# ADR-018: Tenant as Global Account Boundary, M:N Users, JWT Credentials, and Path-Based Wire Protocol

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-14 | Accepted | Erik LaBianca | ADR-011, ADR-017, FEAT-012, FEAT-014, FEAT-025 | High |

## Context

Axon is pre-release. Two earlier decisions — captured in ADR-011 and in commit
`efe4aa1 refactor(tenancy): simplify tenant model — one tenant, one database` —
modeled **database as the tenant isolation boundary**, with grants scoped per
database, users treated as global identities, and a connection-level
`X-Axon-Database` header (or `/db/{name}/...` prefix) for database routing.

That model cannot express the real multi-tenancy needs we want Axon to support:

| Gap | What the current model can't do |
|-----|---------------------------------|
| One customer owning many databases | A billing/access boundary that groups multiple databases under one account — required for organizations that run `billing`, `analytics`, `events` databases under one SaaS account |
| Users in multiple tenants | A single human (or agent) being a member of multiple organizations, each with different roles — the common SaaS "switch workspace" flow |
| Scoped machine credentials | Short-lived, narrow tokens with grants < their issuer's role, for CI jobs and integrations — blast-radius isolation when a credential leaks |
| Federated identity | Users authenticated via Tailscale today, but OIDC/email+password/other providers later, all mapping to the same stable user identity |
| Path-identifiable entities | A canonical URL for every entity that lets edge routers resolve tenant+database to a node without parsing request bodies — the property of true REST resource identity |

Commit `efe4aa1`'s rationale ("simplify tenant model") was reasonable when the
assumption was single-tenant-per-deployment dev mode. For the SaaS, team, and
multi-tenant deployment scenarios from ADR-011's own problem statement, it is
incorrect. This ADR walks it back explicitly.

We are pre-release. No deployments are in production. This change is a clean
break: no backward-compatible routes, no deprecation period, no header
migration. Existing tests, SDKs, and CLI commands are rewritten in the same
commits that change the routing.

| Aspect | Description |
|--------|-------------|
| Problem | Tenant/database confusion prevents multi-database-per-tenant, multi-tenant-per-user, and path-based resource identity |
| Current State (origin/master) | `tenants` table has a `db_name` column (efe4aa1), grants are scoped per database, `X-Axon-Database` header routes by database, users are whatever Tailscale whois resolves to |
| Requirements | Tenant as first-class global account boundary above database; users M:N with tenants; JWT credentials with grants; no headers on data-plane routes; every entity has a canonical URL |

## Decision

### 1. Four-level conceptual hierarchy

```
tenant  (global account boundary — owns everything below)
├── users            (M:N membership via tenant_users table)
├── credentials      (tenant-scoped JWTs issued on behalf of a member user)
└── databases        (N per tenant, each placed on a node)
     └── schemas     (ADR-011, unchanged)
          └── collections
               └── entities

node  (physical placement; invisible from the data path)
```

**What is a tenant**: a tenant is a top-level account boundary. It owns
databases, it has members (users), and it issues credentials. SaaS customers
are tenants. An organization with several product databases is one tenant.
A solo developer is a tenant with one member and a few databases. Tenants are
**global** — they are not tied to any specific node, and a tenant's databases
can be placed on different nodes without the tenant caring.

**What a tenant owns (authoritatively)**:

- **Databases**: a `tenant_databases(tenant_id, database_name)` join entry
  declares that a database belongs to a tenant. Database names are unique
  within a tenant but not globally — tenants `acme` and `globex` can both
  have a database named `orders`.
- **Users**: members are linked via `tenant_users(tenant_id, user_id, role)`.
  Membership is M:N — a user can be a member of multiple tenants, each with
  an independent role.
- **Credentials**: issued by the tenant (through a tenant admin) to grant a
  member user access to a subset of the tenant's databases.

**What a tenant does not own**:

- Users themselves. The `users` table is global. Tenant-membership is a
  relationship, not ownership.
- Nodes. Placement is handled by the existing node topology from ADR-011.
- The default/fallback tenant for un-authenticated requests. `--no-auth` mode
  synthesizes a synthetic tenant context in memory but does not persist a
  tenant row.

### 2. Wire protocol: pure path-based nesting, no headers

All data-plane routes nest under `/tenants/{tenant}/databases/{database}/...`.
There is no `X-Axon-Database` header, no `X-Axon-Tenant` header, no
un-prefixed fallback route.

```
# Data plane — REST
GET    /tenants/{tenant}/databases                             list databases in tenant
POST   /tenants/{tenant}/databases                             create database under tenant
GET    /tenants/{tenant}/databases/{db}
DELETE /tenants/{tenant}/databases/{db}

GET    /tenants/{tenant}/databases/{db}/collections
POST   /tenants/{tenant}/databases/{db}/collections/{name}
GET    /tenants/{tenant}/databases/{db}/collections/{name}
PUT    /tenants/{tenant}/databases/{db}/collections/{name}/schema
DELETE /tenants/{tenant}/databases/{db}/collections/{name}

POST   /tenants/{tenant}/databases/{db}/entities/{collection}/{id}   create
GET    /tenants/{tenant}/databases/{db}/entities/{collection}/{id}   read
PUT    /tenants/{tenant}/databases/{db}/entities/{collection}/{id}   update
PATCH  /tenants/{tenant}/databases/{db}/entities/{collection}/{id}   patch
DELETE /tenants/{tenant}/databases/{db}/entities/{collection}/{id}

POST   /tenants/{tenant}/databases/{db}/transactions
POST   /tenants/{tenant}/databases/{db}/snapshot
GET    /tenants/{tenant}/databases/{db}/audit/query
POST   /tenants/{tenant}/databases/{db}/entities/{collection}/{id}/rollback
POST   /tenants/{tenant}/databases/{db}/transactions/{tx_id}/rollback
POST   /tenants/{tenant}/databases/{db}/collections/{name}/rollback
POST   /tenants/{tenant}/databases/{db}/lifecycle/{collection}/{id}/transition

# Data plane — GraphQL
POST   /tenants/{tenant}/databases/{db}/graphql                HTTP queries/mutations
WS     /tenants/{tenant}/databases/{db}/graphql/ws             subscriptions

# Control plane — above any specific tenant
GET    /control/tenants                                        list (admin only)
POST   /control/tenants                                        create tenant
GET    /control/tenants/{id}
DELETE /control/tenants/{id}

GET    /control/tenants/{id}/users                             M:N list
POST   /control/tenants/{id}/users/{user_id}                   add user with role
PUT    /control/tenants/{id}/users/{user_id}                   update role
DELETE /control/tenants/{id}/users/{user_id}                   remove membership

POST   /control/tenants/{id}/credentials                       issue JWT
GET    /control/tenants/{id}/credentials                       list (metadata only)
DELETE /control/tenants/{id}/credentials/{jti}                 revoke

GET    /control/users                                          global user list (admin only)
POST   /control/users                                          create user (admin only)
GET    /control/users/{id}
DELETE /control/users/{id}

# Non-tenant-scoped
GET    /health                                                 deployment health
GET    /ui/...                                                 embedded admin UI
```

**No un-prefixed routes exist.** Any request to `/entities/...`,
`/collections/...`, `/transactions`, or any other legacy path returns 404.
The SDK, CLI, UI, and test fixtures are rewritten as part of the
implementation. This is a pre-release clean break.

**Why path-based, not header-based**: because every entity having a canonical
URL is a first-class property, not a nicety. A URL carries:

- **Resource identity** — you can reference an entity by its URL in audit
  logs, webhooks, link metadata, notifications, client bookmarks. The URL
  *is* the identifier.
- **Routing** — an edge gateway can parse `(tenant, database)` from the path
  in constant time and proxy to the right node without touching the body.
  Headers require reading and parsing every request.
- **Cache keys** — HTTP caches key on URL. `GET /tenants/acme/.../entities/t-001`
  is a coherent cache entry; `GET /entities/t-001` with a `X-Axon-Database`
  header is not (caches don't key on custom headers).
- **Observability** — logs, metrics, tracing spans naturally bucket by URL
  path. Per-tenant and per-database dashboards fall out for free.
- **gRPC parity** — gRPC methods are also tenant+database scoped via a path
  prefix (`/tenants/{t}/databases/{d}/axon.EntityService/CreateEntity`) so
  the two surfaces share the same routing logic.

### 3. Users: first-class global type with federation

```rust
pub struct User {
    pub id: UserId,                      // stable UUID, never changes
    pub display_name: String,
    pub email: Option<String>,
    pub created_at_ms: u64,
    pub suspended_at_ms: Option<u64>,
}
```

Storage lives in the control plane, not in any tenant's database:

```sql
CREATE TABLE users (
    id             TEXT PRIMARY KEY,
    display_name   TEXT NOT NULL,
    email          TEXT,
    created_at_ms  INTEGER NOT NULL,
    suspended_at_ms INTEGER
);

-- External identities federate to the global user_id
CREATE TABLE user_identities (
    provider     TEXT NOT NULL,          -- "tailscale", "oidc", "email", etc.
    external_id  TEXT NOT NULL,          -- tailnet handle, oidc sub, email, ...
    user_id      TEXT NOT NULL REFERENCES users(id),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (provider, external_id)
);

-- M:N tenant membership with a role per (user, tenant)
CREATE TABLE tenant_users (
    tenant_id    TEXT NOT NULL REFERENCES tenants(id),
    user_id      TEXT NOT NULL REFERENCES users(id),
    role         TEXT NOT NULL,          -- admin | write | read
    added_at_ms  INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, user_id)
);
```

**Auto-provisioning from Tailscale (current implicit behavior, preserved)**:

When a Tailscale-authenticated request arrives:

1. The `tsnet` whois middleware resolves the tailnet identity to an
   `external_id` (typically the tailnet handle `alice@tailnet`).
2. The auth layer looks up `user_identities(provider="tailscale", external_id=...)`.
3. On hit: the row's `user_id` is the acting user.
4. On miss: a new row is inserted into `users` (display name from the tailnet
   identity) and a matching row into `user_identities`. This matches the
   current implicit-provisioning behavior that existed before this ADR and
   is intentionally preserved — Tailscale users don't have to be explicitly
   provisioned to use the system.
5. The layer then consults `tenant_users` for membership in the tenant named
   in the URL path. On miss: if the deployment has been bootstrapped with an
   auto-admin tailnet identity (config flag), the user is auto-added as admin
   of a fresh default tenant; otherwise the request returns 403.

Future providers (OIDC, email+password, API keys external to the JWT system)
are added as rows in `user_identities` with a different `provider` string.
The `users` table shape never changes.

### 4. Credentials: tenant-scoped JWTs carrying grants

Credentials are signed JWTs. Each credential is bound to exactly one tenant
via the `aud` claim. A user who is a member of N tenants has N credentials
(one per tenant), because the `aud` claim is a single value in our schema.

**Claim shape**:

```json
{
  "iss": "axon://eitri.example",
  "sub": "user_01HZ...",
  "aud": "tenant_acme",
  "jti": "cred_01HZ...",
  "iat": 1760000000,
  "nbf": 1760000000,
  "exp": 1760086400,
  "grants": {
    "databases": [
      { "name": "orders",    "ops": ["read", "write"] },
      { "name": "analytics", "ops": ["read"] }
    ]
  }
}
```

- `iss` — deployment identifier (hostname or configured URN). Prevents
  cross-deployment credential confusion.
- `sub` — the user's stable UUID from the `users` table.
- `aud` — the tenant this credential is valid against. **Must be a
  single string, not an array.** Must match the `{tenant}` segment of
  the URL path on every request. Verification rejects JWTs whose `aud`
  claim is a JSON array with `credential_malformed` (see failure table
  below). Multi-audience credentials are not supported in v5 and are
  not intended to be supported in a future version — a user who is a
  member of N tenants uses N distinct credentials.
- `jti` — unique credential ID. Used for revocation.
- `iat` / `nbf` / `exp` — standard JWT timing claims. Default TTL is 24
  hours; credentials issued for longer-lived use cases (CI, integrations)
  can request longer TTLs subject to policy.
- `grants.databases[]` — the list of databases (within the tenant) the
  credential can touch, each with an `ops` list. v5 ops are `read`,
  `write`, and `admin` — see "Grants rule table" below for what each
  op covers and who can mint which ops.

**The `grants` field is designed to evolve.** The v5 `grants` shape is
locked to `{ databases: [{ name, ops }] }`. Future iterations may add
`collections`, `fields` for field-level ABAC, `filters` for row-level
filters, or `rate_class` for throttling category. The verification
middleware must reject unknown top-level keys in `grants` with
`credential_malformed` (strict mode) so a forward-incompatible
deployment fails closed on older verifiers.

#### Grants JSON Schema (v5)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://axon.example/schemas/credential-grants-v5.json",
  "type": "object",
  "additionalProperties": false,
  "required": ["databases"],
  "properties": {
    "databases": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "ops"],
        "properties": {
          "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 255,
            "pattern": "^[a-zA-Z][a-zA-Z0-9_-]*$"
          },
          "ops": {
            "type": "array",
            "items": { "enum": ["read", "write", "admin"] },
            "minItems": 1,
            "maxItems": 3,
            "uniqueItems": true
          }
        }
      },
      "minItems": 0,
      "maxItems": 1024
    }
  }
}
```

Verifiers MUST reject any `grants` payload that doesn't match this
schema with the `credential_malformed` rejection (see failure table).
The 1024-database cap is a safety bound to prevent pathological
credentials; real-world credentials are expected to list 1–10 databases.

#### Grants rule table — who can mint what (v5)

The issuer's `tenant_users.role` at issuance time sets a ceiling on the
grants any credential they mint can contain. The rule is:

| Issuer role | Can mint credentials with ops in... | For databases in... |
|---|---|---|
| `admin` | `{read, write, admin}` | Any database in the tenant, including databases created *after* the credential was minted (because the credential identifies the tenant, not individual databases) |
| `write` | `{read, write}` — **not `admin`** | Any database in the tenant (because role is tenant-wide in v5; future per-database roles will narrow this) |
| `read`  | `{read}` only | Any database in the tenant |

**Self-issuance**: a tenant admin can mint credentials for any member of
the tenant (admin issue-on-behalf). A non-admin can self-issue their own
credentials as long as the grants are ≤ their role. Cross-member
non-admin issuance (write-role user mints a credential for another user)
is rejected.

**Enforcement at issuance**: `POST /control/tenants/{id}/credentials`
validates the above rule before signing. Rejection returns
`403 forbidden` with code `grants_exceed_issuer_role` and a structured
list of which requested ops/databases exceeded the ceiling.

**Enforcement at verification**: the verification middleware trusts
that the claim was valid at issuance time. It does NOT re-validate
grants against the user's current role. This means a credential issued
when the user was `admin` still carries admin grants even if the user
has since been demoted to `write`. Mitigation: short TTL (24h default)
limits the divergence window. If operators need immediate revocation on
role change, they must explicitly revoke the credential's `jti`.

#### Op-to-HTTP-method mapping (v5)

The grant op required for each data-plane route is fixed per route, not
derived from the HTTP method. This is more explicit than a generic
"GET=read / else=write" rule and prevents surprises at schema or
database boundaries.

| Route (under `/tenants/{t}/databases/{d}`) | Method | Required op | Notes |
|---|---|---|---|
| `/entities/{c}/{id}` | GET | `read` on `{d}` | |
| `/entities/{c}/{id}` | POST/PUT/PATCH/DELETE | `write` on `{d}` | |
| `/entities/{c}/{id}/rollback` | POST | `admin` on `{d}` | destructive history rewrite |
| `/collections` | GET | `read` on `{d}` | |
| `/collections/{c}` | POST | `admin` on `{d}` | schema-shape change |
| `/collections/{c}` | DELETE | `admin` on `{d}` | |
| `/collections/{c}/schema` | GET | `read` on `{d}` | |
| `/collections/{c}/schema` | PUT | `admin` on `{d}` | schema evolution |
| `/collections/{c}/rollback` | POST | `admin` on `{d}` | |
| `/transactions` | POST | `write` on **every** database touched | fail-closed: if the tx touches dbs `{x, y}`, credential needs write on both |
| `/transactions/{tx_id}/rollback` | POST | `admin` on **every** database touched | |
| `/snapshot` | POST | `read` on `{d}` | read-only bulk operation |
| `/audit/query` | GET | `read` on `{d}` | |
| `/audit/tail` | GET | `read` on `{d}` | |
| `/lifecycle/{c}/{id}/transition` | POST | `write` on `{d}` | |
| `/graphql` | POST | per-operation: each GraphQL query/mutation resolved to its op at execute time | query → `read`, mutation → per-mutation-type (most are `write`, schema mutations are `admin`) |
| `/graphql/ws` | WS | `read` on `{d}` at connect; subscriptions inherit | |

Control-plane routes (`/control/tenants/...`, `/control/users/...`) are
authorized differently — they check the caller's **role** (not grants)
and require tenant-admin or deployment-admin membership. The
op-to-method mapping above is for *data-plane* routes only.

| Control-plane route | Required privilege |
|---|---|
| `GET /control/tenants` | any authenticated user (results filtered to caller's memberships) |
| `POST /control/tenants` | deployment admin only |
| `GET /control/tenants/{id}` | member of that tenant, any role |
| `DELETE /control/tenants/{id}` | tenant admin OR deployment admin |
| `GET/POST/DELETE /control/tenants/{id}/databases[/{db}]` | tenant admin |
| `GET/POST/PUT/DELETE /control/tenants/{id}/users[/{user_id}]` | tenant admin |
| `POST /control/tenants/{id}/credentials` | tenant admin OR self-issue (sub == target user AND grants ≤ own role) |
| `GET /control/tenants/{id}/credentials` | tenant admin (lists all) OR self (lists own only) |
| `DELETE /control/tenants/{id}/credentials/{jti}` | tenant admin OR the jti's owner |
| `GET/POST/DELETE /control/users[/{id}]` | deployment admin only |
| `GET /health`, `GET /ui/*` | public — no authn required |

**Verification order on every data-plane request**:

1. Extract `Authorization: Bearer <jwt>`. If missing and the deployment is
   in `--no-auth` mode, synthesize an anonymous claim set with admin grants
   on the default tenant and skip to step 7.
2. Verify the signature against the server's signing key(s). HS256 for
   single-node dev; RS256 or EdDSA for multi-node. Signing key management
   lives in the control plane config.
3. Reject if `exp` has passed or `nbf` is in the future.
4. Check `jti` against the revocation list (`credential_revocations` table,
   with an in-memory LRU cache to avoid per-request SQL).
5. Extract `aud` and compare against the `{tenant}` segment parsed from the
   URL path. On mismatch, 403 (not 401) — the credential is valid, it's
   just being presented against the wrong tenant.
6. Resolve `sub` against the `users` table. If suspended or deleted, 401.
7. Walk `grants.databases[]` looking for an entry matching the URL's
   `{database}` segment. If none, 403. If found, intersect `ops` with the
   required op for the request method (`read` for GET, `write` for
   POST/PUT/PATCH/DELETE). If empty intersection, 403.
8. Install `(user_id, tenant_id, grants)` into the request's axum extension
   so handlers can enforce finer invariants (e.g., "user must be admin in
   the tenant to create a new database").

**JWT failure mode → HTTP status + error code**:

Every rejection along the verification order maps to a specific status and
a stable `error.code` string in the response body. This table is normative
— SDKs switch on `error.code`, not on the human-readable message.

| Failure | Status | `error.code` | Retryable | Notes |
|---------|--------|--------------|-----------|-------|
| No `Authorization` header (non-`--no-auth`) | 401 | `unauthenticated` | no | `WWW-Authenticate: Bearer` header required |
| Header present but not `Bearer <token>` | 401 | `credential_malformed` | no | Includes invalid base64, missing dots |
| JWT structurally invalid (bad JSON, missing claims) | 401 | `credential_malformed` | no | Missing `sub`, `aud`, `exp`, `jti`, or `iss` |
| `aud` is a JSON array instead of single string | 401 | `credential_malformed` | no | Normative: `aud` MUST be a single string |
| Signature invalid | 401 | `credential_invalid` | no | Wrong key, tampered payload |
| `exp` in the past | 401 | `credential_expired` | yes — after refresh | Client should re-issue |
| `nbf` in the future | 401 | `credential_not_yet_valid` | yes — after clock skew | Allow ≤30s skew before rejecting |
| `jti` present in `credential_revocations` | 401 | `credential_revoked` | no | Permanent |
| `iss` does not match this deployment's issuer | 401 | `credential_foreign_issuer` | no | Cross-deployment credential |
| `aud` ≠ URL `{tenant}` segment | 403 | `credential_wrong_tenant` | no | Credential is valid — wrong tenant |
| `sub` resolves to a suspended or deleted user | 401 | `user_suspended` | no | User re-activation required |
| `sub` is not a member of the URL `{tenant}` | 403 | `not_a_tenant_member` | no | Membership was revoked |
| URL `{database}` not in `grants.databases[]` | 403 | `database_not_granted` | no | Re-issue credential with broader grants |
| Required op (`read`/`write`/`admin`) not in matching grant's `ops[]` | 403 | `op_not_granted` | no | |

The 401/403 split is deliberate: 401 = "your credential is broken, get a
new one"; 403 = "your credential is fine, but it isn't for this tenant/
database/op." SDKs retry after refresh on 401 but surface 403 to the
caller as a permission error.

**Observability envelope**: every auth rejection emits a structured log
event with fields `{error_code, tenant_path, database_path, op,
user_id_if_known, jti_if_known, remote_addr}`. Metrics counter
`axon_auth_rejections_total{error_code}` gives operators a per-code
histogram for dashboarding and alerting. No rejection is silently
swallowed.

**Revocation**:

```sql
CREATE TABLE credential_revocations (
    jti            TEXT PRIMARY KEY,
    revoked_at_ms  INTEGER NOT NULL,
    revoked_by     TEXT                  -- user_id of the revoker
);
```

`DELETE /control/tenants/{id}/credentials/{jti}` adds the jti to this table.
In-memory LRU cache in front of it means most requests don't hit SQL.

**Tailscale auth vs JWT credentials**:

Tailscale-authenticated requests **do not carry a JWT** and are not
intended to. The whois middleware resolves the tailnet identity and
synthesizes the same `(user_id, tenant_id, grants)` extension struct
in memory for the life of the request, without minting or signing anything.
Handlers see the same shape regardless of how the caller authenticated.
This keeps dev-mode and single-tenant deployments from needing to deal
with credential issuance.

**Grants are always ≤ the issuer's role.** When a tenant admin issues a
credential, the control-plane endpoint validates that the requested grants
are a subset of what the admin's `tenant_users.role` permits. An admin can
issue a `read`-only credential; a `write` member cannot issue an `admin`
credential.

### 5. GraphQL-first interface policy

Both surfaces nest under the same `/tenants/{t}/databases/{d}/` path prefix
so they share the same routing, authentication, and `(user_id, tenant_id,
grants)` extension. A single middleware layer handles tenant/database
extraction; the REST handlers and the GraphQL resolver context both read
from the request extension.

GraphQL is the primary documented interface for developer and end-user
workflows. REST remains for operations where HTTP resource semantics,
streaming/file transfer, or operational break-glass behavior is demonstrably
better than GraphQL.

| Operation class | Surface |
|-----------------|---------|
| Single-entity CRUD | GraphQL primary; REST compatibility URLs may remain |
| Multi-entity queries, filters, joins, traversal | GraphQL |
| Subscriptions (live change feeds per-collection) | GraphQL |
| Batch transactions | GraphQL `commitTransaction`; REST compatibility endpoint may remain |
| Schema and collection management | GraphQL primary; REST compatibility/break-glass endpoints may remain |
| Audit query — point lookup | GraphQL primary; REST compatibility endpoint may remain |
| Audit query — filtered/paginated | GraphQL |
| Tenant/user/member/credential/database control plane | GraphQL primary for UI/SDK; REST compatibility/break-glass endpoints may remain |
| Entity-level rollback | GraphQL primary |
| Transaction and point-in-time rollback | REST break-glass until GraphQL recovery semantics are hardened |
| Health, metrics | REST only |

Rationale:

- **GraphQL is the best developer surface.** Introspection, generated
  schemas, typed inputs, field selection, batching, subscriptions, and
  self-documenting mutations are exactly the interface properties Axon wants
  application developers and the native UI to consume.
- **REST is still useful, but exceptional.** Health checks, metrics, static
  assets, streaming/file-oriented transports, compatibility URLs, and
  break-glass recovery operations retain REST where GraphQL is not the right
  shape.
- **The native Axon UI is the canary.** UI routes should exercise GraphQL for
  control-plane and data-plane workflows unless this ADR or the API contract
  names a specific REST-only exception.

This section amends prior ADR-018 language that made single-entity CRUD,
schema management, and the control plane REST-only. It also strengthens
ADR-012 by making GraphQL the primary write surface, not only the query layer.

### 6. Default tenant bootstrap

A brand-new deployment has no tenants, no databases, no users. On first
successful authenticated request, the server auto-bootstraps:

1. A `default` tenant with a single admin member (the authenticated user).
2. A `default` database inside that tenant.
3. A `default` schema inside that database (preserving ADR-011's schema
   level).

The authenticated user is added to `tenant_users(default, user_id, admin)`.
Subsequent requests from that user can operate against
`/tenants/default/databases/default/...` without further setup.

The auto-bootstrap behavior is idempotent. It runs only when there are zero
tenants. After any tenant has been created (auto or explicit), it does not
re-run.

**Concurrency**: if two requests arrive simultaneously against a fresh
deployment, both will observe zero tenants and both will attempt to
bootstrap. The implementation MUST use a unique constraint and idempotent
insert rather than a check-then-insert race:

```sql
INSERT INTO tenants (id, name, created_at_ms)
VALUES (?, 'default', ?)
ON CONFLICT (name) DO NOTHING;

INSERT INTO tenant_users (tenant_id, user_id, role, added_at_ms)
VALUES (
    (SELECT id FROM tenants WHERE name = 'default'),
    ?, 'admin', ?
) ON CONFLICT (tenant_id, user_id) DO NOTHING;
```

The `tenants.name` column has a `UNIQUE` index. The first transaction
wins; the second becomes a no-op. After both commit, both requests see
the bootstrapped state and proceed.

**Tailscale auto-provision race**: the whois middleware does an identical
`ON CONFLICT DO NOTHING` insert into `users` keyed on `(provider, external_id)`
in the `user_identities` table. The column is named `external_id` to match
the normative schema in Section 3 above. The flow is:

```sql
BEGIN;
INSERT INTO users (id, created_at_ms)
VALUES (?, ?) ON CONFLICT DO NOTHING;

INSERT INTO user_identities (provider, external_id, user_id)
VALUES ('tailscale', ?, ?)
ON CONFLICT (provider, external_id) DO NOTHING;

SELECT user_id FROM user_identities
WHERE provider = 'tailscale' AND external_id = ?;
COMMIT;
```

The final `SELECT` returns whichever `user_id` won the race. Both
concurrent first-seen requests for the same tailnet identity converge on
the same user row. Unit test: issue N parallel whois resolutions for the
same identity, assert exactly one `users` row and one `user_identities`
row result.

**`--no-auth` mode + tenant URLs**: `--no-auth` skips the persistent
bootstrap and synthesizes an anonymous admin claim per-request. The URL
path `{tenant}/{database}` is still honored — the anonymous claim is
generated with grants for whichever tenant/database the URL names, so any
URL routes successfully. The backing store is keyed by the URL's
`(tenant, database)` pair, so `--no-auth` gives you a per-URL in-memory
namespace with no persistent user identity. This is the dev-mode
contract: anything you name into existence via a URL works; nothing is
persisted beyond process lifetime unless the storage adapter says so.

### 7. Relationship to ADR-011 and commit efe4aa1

**ADR-011 is amended, not superseded.** ADR-011's contributions that stand:

- Schema level within a database (section 1, "Schema (Namespace)")
- Node topology — nodes as placement, not in the data path (section 2)
- Database placement and migration protocol (section 2, subsections
  "Node Registry", "Database Placement", "Request Routing",
  "Database Migration")
- Per-database storage isolation on PostgreSQL and KV stores (section 3)

**ADR-011 is walked back** on:

- Section 1 "Database" subsection's assertion that "A database is the
  fundamental unit of tenant isolation." The fundamental unit of tenant
  isolation is the **tenant**. A database is the unit of data isolation
  *within* a tenant.
- Section 1 "Fully Qualified Names" — three-part `database.schema.collection`
  names are still valid *within a tenant's scope* but the canonical wire
  form is the 4-segment URL path. The three-part name is for internal
  references (link types, schema cross-references).
- Section 4 "Access Control Integration" — grants are still per-database/
  schema/collection, but they're scoped *within a tenant*, and the
  tenant-level grant is expressed by tenant membership role, not a row in
  the grants table.
- Section 5 "API Surface" — the `X-Axon-Database` header and `/db/{name}/...`
  path prefix are **removed**. The new wire format is
  `/tenants/{tenant}/databases/{database}/...` everywhere.

**Commit `efe4aa1` is explicitly walked back.** The `tenants.db_name` column
is dropped and replaced by the `tenant_databases` join table (see schema in
section 3 above). The migration is a clean break per the pre-release policy;
no script is required because no production deployments exist.

## Alternatives Considered

### A. Keep `X-Axon-Database`, add `X-Axon-Tenant`

**Rejected.** Tenant in a header defeats the point of having canonical
entity URLs. Edge routing would require parsing headers. HTTP caches
wouldn't key by tenant. And we'd have two headers where the user said
"I prefer to avoid weird headers."

### B. Path: `/tenants/{tenant}/{database}/...` (no `databases/` segment)

**Rejected.** Ambiguous: is `foo` a database or a sub-resource? Not
self-documenting. The extra segment makes the URL longer by 9 characters
and is worth it for clarity.

### C. Single global credentials, user identity carries the tenant at request time

**Rejected.** If a credential is global (valid across all tenants the user
is a member of), then leaking it compromises every tenant the user
touches. Per-tenant credentials isolate blast radius. Industry practice
is split (GitHub PATs are global, AWS IAM access keys are per-account);
we chose the stricter model.

### D. Users are NOT first-class — keep using Tailscale identity directly

**Rejected.** Couples the entire auth model to Tailscale forever.
Federation via `user_identities` costs one table and gives us OIDC,
email+password, and any future provider for free. Tailscale becomes
one login provider among many, which is exactly what it should be.

### E. Backward-compatible un-prefixed routes

**Rejected explicitly.** Pre-release clean break. Dual-pathing doubles the
test matrix, doubles the SDK surface, and invites silent misrouting when
the strict fallback rule's edge cases are misunderstood. Rewriting the
existing call sites is a few hours of work; maintaining the dual path
forever is a permanent tax.

### F. GraphQL as the sole data-plane surface

**Rejected.** Loses resource identity, edge routing, HTTP caching, and
natural observability bucketing. GraphQL subscriptions cover the query
and push-notification use cases; REST covers the "every entity has a URL"
use case. See section 5.

## Consequences

| Type | Impact |
|------|--------|
| **Positive** | Clear tenant isolation; M:N users enable real SaaS and team-organization deployments; canonical entity URLs enable edge routing and HTTP caching; JWTs give us industry-standard credentials with structured grants; Tailscale stays the simple default for dev while OIDC arrives later as a clean provider addition; division of labor between REST and GraphQL is explicit; the whole stack (wire protocol, storage, auth, UI, SDK) becomes internally consistent about what a tenant is |
| **Negative** | Non-trivial implementation scope: SQL migration, auth middleware redesign, router restructure, UI two-level picker, SDK rewrite, test matrix rewrite; JWT introduces a new runtime dependency and a signing key management surface (see "Signing key rotation" below); per-tenant credentials mean users with access to N tenants carry N credentials in their config (but N is small for humans, and SDKs can manage the list) |
| **Neutral** | The 4-level hierarchy is one level deeper than ADR-011 intended; users have to understand what a tenant is, but this concept is well-established in SaaS tooling and doesn't need invention; GraphQL mutation resolvers gain a new context extraction path (the `(user_id, tenant_id, grants)` extension) but the resolvers themselves are unchanged |

## Validation

| Criterion | Test |
|-----------|------|
| Every data-plane route is path-prefixed with `/tenants/{t}/databases/{d}/` | Grep + route inventory test in `crates/axon-server/tests/api_contract.rs` |
| No `X-Axon-Database` header is read or written anywhere | `git grep "X-Axon-Database"` returns zero hits in `crates/` and `sdk/` after implementation |
| A request to an unprefixed route returns 404 | Integration test per method |
| JWT with `aud` mismatching URL tenant returns 403 | `crates/axon-server/tests/auth_path_test.rs` (new) |
| Revoked JWT returns 401 immediately (cache-hit path) and after server restart (SQL-hit path) | Integration test with credential revocation table |
| User in 2 tenants can access both via separate credentials; a single credential cannot access the other | L2 scenario `cross_tenant_isolation_with_m2n_users` in the test plan |
| Tailscale auto-provisioning creates exactly one `users` row and one `user_identities` row on first seen, no duplicates on second seen | Unit test on the auth middleware |
| Default tenant bootstrap runs once on zero-tenant deployment; not re-run after first tenant exists | Integration test |
| Grants ≤ role invariant is enforced at credential issuance | Unit test on `POST /control/tenants/{id}/credentials` |

## Implementation Notes

- JWT library: `jsonwebtoken` crate (already common in the Rust ecosystem)
- Signing key management: env var for dev; config file with rotation stub
  for prod. Full rotation protocol is deferred to a future ADR.
- **Signing key rotation vulnerability**: until the rotation ADR lands, a
  compromised signing key forces revocation of every outstanding credential
  in the affected deployment. Operators MUST treat the key as a
  tier-1 secret, store it only in a vault / KMS, and never commit it to
  version control. The rotation ADR MUST define: (a) key version identifier
  in JWT header `kid`, (b) overlap window where old and new keys both
  verify, (c) operator runbook for emergency rotation, (d) audit event
  emitted on rotation. This is a known gap and MUST be closed before v1.0.
- **Audit retention on tenant drop**: `DELETE /control/tenants/{id}`
  does NOT erase the tenant's audit log. Audit records are retained per
  the tenant's configured retention policy (default 7 years, archived
  to cold storage on tenant deletion, purged on policy expiry). The
  retention policy is a tenant-level setting editable by a tenant admin,
  with a deployment-level floor an operator can set. Audit attribution
  (`{user_id, tenant_id, jti}`) remains stable across tenant deletion
  so post-hoc forensic queries resolve correctly.
- The `TenantRouter` from FEAT-028 is renamed (conceptually) to
  `DatabaseRouter`: it resolves `(tenant, database)` → node/storage adapter.
  The tenant check happens at the auth middleware layer; the router only
  maps `(tenant, database)` to a backing store. This clean separation is
  part of the implementation bead stack.
- Grants enforcement lives in the auth middleware, not in individual
  handlers. Handlers trust the extension. This is important for defense in
  depth: adding a new route does not require adding a new authz check.
- The SDK (`sdk/typescript/packages/client`) gains a `.tenant(t).database(d)`
  fluent API; the legacy single-database SDK entrypoint is removed.

## References

- [ADR-005: Authentication via Tailscale tsnet](./ADR-005-authentication-tailscale-tsnet.md)
- [ADR-011: Multi-Tenancy, Namespace Hierarchy, and Node Topology](./ADR-011-multi-tenancy-and-namespace-hierarchy.md) (amended)
- [ADR-012: GraphQL Query Layer](./ADR-012-graphql-query-layer.md) (clarified)
- [ADR-017: Control Plane Topology and BYOC Deployment Model](./ADR-017-control-plane.md)
- [FEAT-012: Authorization](../../01-frame/features/FEAT-012-authorization.md) (rewrite)
- [FEAT-014: Multi-Tenancy and Namespace Hierarchy](../../01-frame/features/FEAT-014-multi-tenancy.md) (rewrite)
- [FEAT-025: Control Plane](../../01-frame/features/FEAT-025-control-plane.md) (updated)
- Commit `efe4aa1` — "refactor(tenancy): simplify tenant model — one tenant, one database" (walked back by this ADR)
- Industry prior art: GitHub REST+GraphQL split; Stripe API key scoping; AWS IAM access key model; Snowflake account → database hierarchy
