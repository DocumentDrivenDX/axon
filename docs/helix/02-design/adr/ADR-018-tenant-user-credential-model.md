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
- `aud` — the tenant this credential is valid against. Must match the
  `{tenant}` segment of the URL path on every request.
- `jti` — unique credential ID. Used for revocation.
- `iat` / `nbf` / `exp` — standard JWT timing claims. Default TTL is 24
  hours; credentials issued for longer-lived use cases (CI, integrations)
  can request longer TTLs subject to policy.
- `grants.databases[]` — the list of databases (within the tenant) the
  credential can touch, each with an `ops` list. `ops` values in v1 are
  `read` and `write`; future ops (`delete`, `admin`) are reserved.

**The `grants` field is designed to evolve.** v1 has database-level ops.
Future iterations may add `collections`, `fields` for field-level ABAC,
`filters` for row-level filters, `rate_class` for throttling category, etc.
The JWT claim is opaque to the JWT spec but parsed by axon-server's auth
middleware.

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

### 5. REST and GraphQL division of labor

Both surfaces nest under the same `/tenants/{t}/databases/{d}/` path prefix
so they share the same routing, authentication, and `(user_id, tenant_id,
grants)` extension. A single middleware layer handles tenant/database
extraction; the REST handlers and the GraphQL resolver context both read
from the request extension.

| Operation class | Surface |
|-----------------|---------|
| Single-entity CRUD addressable by canonical URL | REST |
| Multi-entity queries, filters, joins, traversal | GraphQL |
| Subscriptions (live change feeds per-collection) | GraphQL |
| Batch transactions | REST `POST /transactions` AND GraphQL `commitTransaction` mutation |
| Schema management (`PUT /schema`) | REST — it's a declarative config operation |
| Audit query — point lookup | REST |
| Audit query — filtered/paginated | GraphQL |
| Admin / control plane | REST only — self-documenting, HTTP verb semantics |
| Health, metrics | REST only |

Rationale:

- **REST is unbeatable for resource identity.** A canonical URL that is
  simultaneously the routing key, the cache key, and the identifier you
  put in audit logs and webhooks is too valuable to give up.
- **GraphQL is unbeatable for queries.** Batching, field selection,
  relationship resolution, typed schemas, subscriptions — these are
  GraphQL's core strengths and trying to replicate them in REST is
  exactly what spawned GraphQL in the first place.
- **Neither is strictly worse.** This is the same division GitHub, Stripe,
  and Shopify converged on. It is not a transitional compromise; it is
  the intended long-term surface.

This section amends ADR-012 (GraphQL Query Layer) to clarify that GraphQL
is not expected to subsume REST. It supersedes any prior statement
suggesting one would replace the other.

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

`--no-auth` mode skips the bootstrap and synthesizes the same context
in-memory on every request — there's no persistent user, no persistent
tenant, just an anonymous admin claim that lets everything succeed.

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
| **Negative** | Non-trivial implementation scope: SQL migration, auth middleware redesign, router restructure, UI two-level picker, SDK rewrite, test matrix rewrite — roughly 4–6 weeks of focused work; JWT introduces a new runtime dependency and a signing key management surface; per-tenant credentials mean users with access to N tenants carry N credentials in their config (but N is small for humans, and SDKs can manage the list) |
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
