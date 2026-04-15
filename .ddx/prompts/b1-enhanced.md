<execute-bead>
  <bead id="axon-dccbdf73">
    <title>auth-jwt-pipeline: sign + verify with revocation cache + request extension</title>
    <description>
Implement the full JWT verification middleware in axon-server.

## What you're building

An axum middleware layer that runs on every data-plane request. It validates the JWT, installs a (user_id, tenant_id, grants) tuple into the request extension, and returns a typed AuthError on any failure. Downstream handlers read the extension and trust it completely.

## Scope — exactly what to add

### New file: `crates/axon-server/src/auth_pipeline.rs`

A module exposing two things:

1. A `JwtIssuer` helper that signs a `JwtClaims` into a compact JWT string. HS256 is the only algorithm required for this bead. The signing key is a `Vec<u8>` passed in at construction. Provide `issue(&self, claims: &JwtClaims) -> Result<String, AuthError>` and a `verify(&self, token: &str) -> Result<JwtClaims, AuthError>` for round-tripping in tests.

2. An axum middleware `async fn jwt_verify_layer` that runs the 8-step verification order below. On success, installs a `ResolvedIdentity { user_id, tenant_id, grants }` into the request extension. On failure, returns a response built via `AuthError::into_response()` so the mapping to HTTP status + error.code is consistent.

The middleware also owns an `InMemoryRevocationCache` (a small `tokio::sync::RwLock<HashSet<Uuid>>` is fine; nothing fancy) and falls back to SQL (via the storage adapter's new `is_jti_revoked` method) on cache miss.

### Extend: `crates/axon-core/src/auth.rs`

- Add `ResolvedIdentity { pub user_id: UserId, pub tenant_id: TenantId, pub grants: Grants }`.
- Add `impl AuthError` with `pub fn status_code(&self) -> u16` returning 401 or 403 per ADR-018 §4 failure table, and `pub fn error_code(&self) -> &'static str` returning the stable code string. (An axum `IntoResponse` impl can live in axon-server.)
- Add `impl Grants` with `pub fn find_database(&self, name: &str) -> Option<&GrantedDatabase>` and `impl GrantedDatabase` with `pub fn has_op(&self, op: Op) -> bool`.

### Extend: `crates/axon-storage/src/adapter.rs` (trait)

Add one method to the `StorageAdapter` trait:

```rust
async fn is_jti_revoked(&self, jti: Uuid) -> Result<bool, AxonError>;
```

Implement it in `crates/axon-storage/src/memory.rs` (store a `HashSet<Uuid>`; default-empty), `crates/axon-storage/src/sqlite.rs`, and `crates/axon-storage/src/postgres.rs` (`SELECT 1 FROM credential_revocations WHERE jti = $1`). The A1 schema (`auth_schema.rs`) already has the `credential_revocations` table — just query it.

### Wire into: `crates/axon-server/src/lib.rs`

Re-export `auth_pipeline::{jwt_verify_layer, JwtIssuer, InMemoryRevocationCache}`. Don't wire it into the main router yet — that's D-phase work. For this bead, the middleware just needs to exist and be testable in isolation.

## Verification order (normative — ADR-018 §4)

1. Extract `Authorization: Bearer <jwt>` from the request. Missing header → `AuthError::Unauthenticated` (401). The `--no-auth` bypass is NOT part of this bead — that's E2.
2. Parse and verify the JWT signature with HS256 against the issuer's key. Signature invalid → `CredentialInvalid` (401). Malformed structure → `CredentialMalformed` (401). `aud` as a JSON array → `CredentialMalformed` (this falls out of the A2 `parse_claims` wrapper — use it).
3. Check `nbf` and `exp` against `SystemTime::now()` with a 30s skew budget. `exp` more than 30s in the past → `CredentialExpired` (401). `nbf` more than 30s in the future → `CredentialNotYetValid` (401).
4. Check `claims.jti` against the revocation cache, then SQL on miss. Hit → `CredentialRevoked` (401). Populate the cache on SQL hit to speed up subsequent checks.
5. Compare `claims.aud` to the `{tenant}` segment extracted from the URL path. Mismatch → `CredentialWrongTenant` (**403**, not 401 — see the table). The URL extraction path is: look at `request.uri().path()` and parse the leading `/tenants/{tenant}/databases/{database}/...` segments. For this bead, implement a small helper `fn extract_tenant_database(path: &str) -> Option<(&str, &str)>` that returns `None` for non-data-plane paths so they skip the middleware.
6. Look up the `sub` UUID in the `users` table via a new `StorageAdapter::get_user(UserId) -> Result<Option<User>, AxonError>` method. User not found or `user.suspended_at_ms.is_some()` → `UserSuspended` (401).
7. Check tenant membership: a new `StorageAdapter::get_tenant_member(TenantId, UserId) -> Result<Option<TenantMember>, AxonError>`. Missing → `NotATenantMember` (403).
8. Walk `claims.grants.databases[]` looking for an entry matching the URL `{database}`. None → `DatabaseNotGranted` (403). Found → compute the required op from the HTTP method (GET = Read, POST/PUT/PATCH/DELETE = Write) and intersect with the grant's ops. Empty intersection → `OpNotGranted` (403). Admin-requiring routes are out of scope for this bead — the middleware only handles Read/Write classification.
9. Build `ResolvedIdentity` and install it via `request.extensions_mut().insert(resolved)`.

## Error mapping — must exactly match ADR-018 §4

The mapping table is copied here verbatim. Do not invent variants; the 14 AuthError variants already exist in axon-core.

| Failure | Status | `error.code` |
|---|---|---|
| No Authorization header | 401 | `unauthenticated` |
| Header not `Bearer <token>` or base64 broken | 401 | `credential_malformed` |
| JWT structurally invalid / missing claims | 401 | `credential_malformed` |
| `aud` as JSON array | 401 | `credential_malformed` |
| Signature invalid | 401 | `credential_invalid` |
| `exp` in past (beyond skew) | 401 | `credential_expired` |
| `nbf` in future (beyond skew) | 401 | `credential_not_yet_valid` |
| `jti` revoked | 401 | `credential_revoked` |
| `iss` unknown | 401 | `credential_foreign_issuer` — for this bead, reject if `claims.iss != issuer.issuer_id` |
| `aud` ≠ URL tenant | **403** | `credential_wrong_tenant` |
| `sub` suspended/deleted | 401 | `user_suspended` |
| `sub` not a tenant member | **403** | `not_a_tenant_member` |
| URL database not in grants | **403** | `database_not_granted` |
| Op not in grant | **403** | `op_not_granted` |

Note the 401 vs 403 split: credential-level problems are 401, scope-level problems are 403.

## Testing — all must pass for this bead to be complete

### Unit tests in `crates/axon-server/tests/auth_pipeline_test.rs`

For every row of the error mapping table, build the minimal defective request and assert the middleware produces exactly that `(status, error_code)` pair. Table-driven with `#[test]` or a single `#[test]` that iterates an array of cases.

Additionally:

- `jwt_roundtrip_succeeds`: issue a valid JWT with `JwtIssuer::issue`, verify it with `JwtIssuer::verify`, assert claims match.
- `valid_jwt_populates_request_extension`: run the full middleware on a valid JWT, assert the response is 200 and the handler sees `ResolvedIdentity` in extensions.
- `revocation_takes_effect_within_one_second`: issue a valid JWT, verify it succeeds, insert the jti into the revocation cache, verify the next request returns 401 `credential_revoked`. Do NOT use sleep — invalidate the cache directly.
- `clock_skew_30s_accepted_31s_rejected`: construct a JWT with `exp` at `now - 29s` (should succeed) and one with `exp` at `now - 31s` (should fail with `credential_expired`). Do the same for `nbf`.
- `ops_matrix`: for each HTTP method in {GET, POST, PUT, PATCH, DELETE} and each op set in the grant, assert the expected 200/403 outcome.

### Integration test in `crates/axon-server/tests/auth_pipeline_integration_test.rs`

Build a minimal axum router with one dummy route `GET /tenants/acme/databases/orders/ping`. Install the middleware. Hit it with:
- No auth header → 401 `unauthenticated`
- Valid JWT with `grants: [{name: "orders", ops: [read]}]` → 200
- Valid JWT with the wrong tenant in aud → 403 `credential_wrong_tenant`
- Valid JWT with grants for a different database → 403 `database_not_granted`

### axon-sim workload `inv_018_grant_enforcement`

**If this is too large for one pass, skip the axon-sim workload and note it in the commit message for a follow-up bead.** The core verification must ship; the simulation workload can follow. Do not block the whole bead on the workload.

## Crate dependencies you will need

Add to `crates/axon-server/Cargo.toml` under `[dependencies]`:
```toml
jsonwebtoken = "9"
```

Everything else (axum, tower, tokio, uuid, serde_json, http) is already in the workspace.

## Context you must know about

### A1 landed: schema
- `crates/axon-storage/src/auth_schema.rs` has the SQL for `users`, `user_identities`, `tenant_users`, `credential_revocations`, etc.
- `user_identities.external_id` (NOT `subject`) — already corrected.
- Migration runs via `apply_auth_migrations_sqlite` / `_postgres`.

### A2 landed: types
- `crates/axon-core/src/auth.rs` has all the types: `TenantId`, `UserId`, `User`, `UserIdentity`, `Tenant`, `TenantMember`, `TenantRole`, `Op`, `GrantedDatabase`, `Grants`, `JwtClaims`, `AuthError` (14 variants).
- `parse_claims(&str) -> Result<JwtClaims, AuthError>` wraps serde_json and maps errors to `CredentialMalformed`. USE IT.
- `AUTH_ERROR_VARIANT_COUNT == 14` is enforced via compile-time exhaustive match.
- Delegation helper on `TenantRole`: `admin.can_delegate(op)` / etc.

### Files you should NOT touch
- Anything outside `crates/axon-server/src/auth_pipeline.rs`, `crates/axon-core/src/auth.rs` (additive), `crates/axon-storage/src/{adapter,memory,sqlite,postgres}.rs` (additive), and the two new test files.
- The existing `crates/axon-server/src/auth.rs` — that's the Tailscale whois path. Leave it alone. Your pipeline is a new module.

## Quality bar

You are the implementation step. A post-merge review with opus will verify the result against the AC. If your commit misses an AC item, the bead reopens and escalates to a higher tier. **Don't commit partial work hoping the review slides.** If you cannot complete the bead in this pass, write a `no_changes_rationale.txt` explaining exactly what's done and what's blocking, and do NOT commit — the bead will be re-queued.

Specifically: if you cannot make `cargo test -p axon-server` and `cargo clippy -p axon-server -- -D warnings` green, do not commit. Fix the issues first.
    </description>
    <acceptance>
- [ ] crates/axon-server/src/auth_pipeline.rs exists and compiles
- [ ] JwtIssuer can sign and verify a JwtClaims round-trip (HS256)
- [ ] Middleware implements the 8-step verification order
- [ ] Every row of the ADR-018 §4 failure table has a matching unit test asserting (status, error_code)
- [ ] Revocation cache invalidation test passes without sleep
- [ ] Clock skew test passes at 30s boundary (both directions)
- [ ] Integration test with a dummy protected route covers the 4 golden paths
- [ ] is_jti_revoked wired into memory/sqlite/postgres storage adapters (trait + 3 impls)
- [ ] get_user and get_tenant_member wired similarly
- [ ] ResolvedIdentity lives in axon-core and is installed into the axum request extension
- [ ] cargo test -p axon-server green
- [ ] cargo test -p axon-storage green (new trait methods added)
- [ ] cargo test -p axon-core green (new helpers added)
- [ ] cargo clippy --workspace -- -D warnings clean
    </acceptance>
    <labels>helix, area:auth, spec:ADR-018, phase:middleware</labels>
    <metadata parent="axon-b170a173" base-rev="HEAD" bundle=".ddx/executions/b1-enhanced-manual"/>
  </bead>

  <governing>
    <ref id="ADR-018" path="docs/helix/02-design/adr/ADR-018-tenant-user-credential-model.md">Sections 4 (Credentials) and 4 Verification Order — normative for everything in this bead</ref>
    <ref id="FEAT-012" path="docs/helix/01-frame/features/FEAT-012-authorization.md">Credentials (JWT, V5) and Identity (Authentication) sections</ref>
  </governing>

  <instructions>
You are a coding agent implementing a single bead inside an isolated git worktree. Work from the bead description above — it is complete and unambiguous. The AC list is the completion contract.

## How to work

1. **Read first, act second.** Read the governing ADR-018 sections and the existing crates/axon-core/src/auth.rs so you understand the AuthError variants and the parse_claims wrapper. Then read crates/axon-storage/src/auth_schema.rs to confirm the credential_revocations table shape. Then start coding.

2. **Cross-reference your work against the AC as you go.** Every AC checkbox must be provably satisfied by a piece of code you can point to. Before committing, re-read the AC and physically tick each one.

3. **Run the verification commands locally before committing.** cargo test and cargo clippy per the AC. If either fails, fix it. Do not commit red code.

4. **Commit once, when everything works.** Do not commit partial or exploratory changes. The bead is a single atomic unit of work.

5. **If you cannot complete the bead**, write `.ddx/executions/b1-enhanced-manual/no_changes_rationale.txt` with (a) what's done, (b) what's blocking, (c) why a follow-up bead is needed. Do not commit red code to "make progress".

## Constraints

- Work only in the files listed under "Scope" and "Files you should NOT touch" above.
- Do not modify CLAUDE.md or add new documentation.
- Do not run `ddx init`.
- Keep the `.ddx/executions/` directory intact — DDx uses it as execution evidence.
- Commit with `git add <specific files> && git commit -m 'feat(auth-pipeline): ... [axon-dccbdf73]'`. Do not `git add -A` — there may be other WIP in the worktree.

## Done definition

`cargo test -p axon-server -p axon-core -p axon-storage` green + `cargo clippy --workspace -- -D warnings` clean + every AC item ticked + one commit. That's done.
  </instructions>
</execute-bead>
