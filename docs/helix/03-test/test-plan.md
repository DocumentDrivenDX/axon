---
dun:
  id: helix.test-plan
  depends_on:
    - helix.prd
    - helix.principles
    - helix.technical-requirements
---
# Axon Test Plan

**Version**: 0.1.0
**Date**: 2026-04-04
**Status**: Draft

---

## 1. Philosophy

> "Test suite first, implementation second." — Axon Principle P1

Following FoundationDB's approach: the test suite defines the system. Correctness properties are encoded as executable invariants. The implementation exists to pass them. If a property isn't tested, it isn't guaranteed.

This test plan specifies **what must be true** about Axon. Each section defines invariants, workloads, and acceptance criteria that must pass before the corresponding feature is considered implemented. The test plan is the governing artifact — code that doesn't satisfy these specifications is incomplete regardless of how many unit tests it has.

---

## 2. Test Architecture

### Layers

| Layer | Purpose | Framework | Runs |
|-------|---------|-----------|------|
| **L1: Correctness invariants** | Prove ACID, audit completeness, schema enforcement, link integrity under fault injection | `axon-sim` DST framework | CI: every commit. Nightly: extended seeds |
| **L2: Business scenario tests** | Validate real-world workflows from use case research (AP/AR, CRM, CDP, etc.) end-to-end | Integration tests against embedded Axon | CI: every commit |
| **L3: Property-based tests** | Generate random entities, links, schemas, transactions and verify invariants hold | `proptest` or `bolero` | CI: every commit (bounded iterations). Nightly: extended |
| **L4: Backend conformance** | Verify every StorageAdapter implementation passes the identical test suite | Parameterized tests across SQLite, Postgres, FoundationDB | CI: SQLite. Pre-merge: all backends |
| **L5: Performance benchmarks** | Verify latency and throughput targets from technical requirements | `criterion` benchmarks | Nightly. Ratcheted — can only improve |
| **L6: API contract tests** | Verify gRPC and HTTP gateway conform to protobuf contract | Generated client tests | CI: every commit |

### Deterministic Simulation Framework (`axon-sim`)

Built on the FoundationDB model (see [research](../00-discover/foundationdb-dst-research.md)):

| Component | Description |
|-----------|-------------|
| `SimRng` | Xorshift64 PRNG. Same seed → identical execution. Every random decision flows through this |
| `Buggify` | Fault injector activated probabilistically during simulation. Injects: transaction aborts, storage errors, write delays |
| Workloads | Composable test workloads with 4 phases: SETUP → EXECUTION → CHECK → METRICS |
| Runtime abstraction | `StorageAdapter` trait enables identical test code against memory, SQLite, Postgres backends |
| Seed exploration | CI runs N seeds per workload. Nightly runs M >> N seeds. Any failure seed is recorded and replayed in CI forever |

### Ratchets (HELIX Quality Gates)

| Ratchet | Direction | Metric | Enforced |
|---------|-----------|--------|----------|
| Correctness seeds passing | Increasing | Count of seeds with 0 invariant violations | CI gate |
| Invariant count | Increasing | Number of distinct invariants under test | Per-release review |
| Simulation hours | Increasing | Cumulative simulated hours of fault injection | Tracked in CI metrics |
| Test coverage (line) | Increasing | % lines exercised by L1-L4 tests | CI gate (ratchet file) |
| Performance p99 | Improving | Latency must not regress beyond threshold | Nightly gate |
| Audit gap count | Decreasing → 0 | Mutations without corresponding audit entries | CI gate |

---

## 3. L1: Correctness Invariants

These are the properties that must hold under all circumstances, including concurrent access and fault injection. Each invariant has a formal statement, a verification method, and a simulation workload.

### INV-001: No Lost Updates

**Statement**: If two transactions concurrently update the same entity, exactly one succeeds and the other receives a version conflict error with the current state. No write is silently overwritten.

**Verification**: Two concurrent simulated agents repeatedly read-modify-write the same entity. After N rounds, the entity's value must reflect exactly the writes that were acknowledged as successful.

**Workload**: `ConcurrentWriterWorkload`
- 2-10 simulated agents
- Single shared entity
- Each agent: read → compute → write with expected version
- CHECK: final value == sum of acknowledged writes
- BUGGIFY: inject version conflicts, storage errors, delays

### INV-002: Snapshot Isolation (Cycle Test)

**Statement**: Concurrent transactions produce results equivalent to some serial execution order. No phantom reads, no dirty reads.

**Verification**: FoundationDB's cycle test. N entities form a ring via typed links. Transactions atomically update pairs of adjacent nodes. The CHECK phase walks the ring — exactly N hops means isolation held.

**Workload**: `CycleWorkload`
- Ring of 5-100 entities connected by `next` links
- 100-10,000 swap transactions under concurrent execution
- CHECK: ring walk returns to start in exactly N hops
- BUGGIFY: inject transaction aborts, delays, storage errors
- Failure detection: fewer or more hops = isolation violation

### INV-002b (P1): Write Skew Prevention

**Statement**: Write skew is prevented under serializable isolation.

### INV-003: Audit Completeness

**Statement**: Every committed mutation (entity create, update, delete; link create, delete) that survives to a readable state has a corresponding audit entry. If the mutation is visible and the process has not crashed between the storage commit and the audit flush, the audit entry exists.

**V1 scope**: The V1 in-memory implementation uses a post-commit audit strategy
(see ADR-003 audit write path, ADR-004 audit integration). Audit entries are
written immediately after `commit_tx()` succeeds. INV-003 holds for all
non-crash scenarios. Crash-safety between commit and audit flush is deferred to
the durable storage adapter implementation (SQLite/PostgreSQL), which must ensure
both writes use the same backing store transaction.

**Verification**: After any workload (non-crash), count committed mutations vs audit entries. They must match exactly. Every entity version in storage must have a corresponding audit entry with matching before/after state.

**Workload**: `AuditCompletenessWorkload`
- Mixed CRUD operations across multiple collections
- CHECK: `count(mutations) == count(audit_entries)`
- CHECK: for each entity, walk audit entries and reconstruct current state — must match stored state
- BUGGIFY: inject `audit.append()` failures — the mutation is committed but the
  audit entry is missing; this is a detectable gap that the durable backend
  recovery mechanism must close
- NOTE: for durable backends, BUGGIFY must also inject process crashes between
  `commit_tx()` and audit flush and verify the startup recovery mechanism
  closes any gaps

### INV-004: Audit Immutability

**Statement**: No audit entry is ever modified or deleted through any API path. The audit log is strictly append-only.

**Verification**: Record audit entry IDs and hashes before and after a workload. All pre-existing entries must be unchanged.

**Workload**: `AuditImmutabilityWorkload`
- Create entities, record audit entries and content hashes
- Execute more operations
- CHECK: all previously recorded audit entries still exist with identical content
- Attempt to update/delete audit entries via API — must fail

### INV-005: Schema Enforcement

**Statement**: No entity in storage violates its collection's schema. Every write is validated; invalid writes are rejected with structured errors.

**Verification**: After any workload, read every entity from storage and validate against its collection schema. All must pass.

**Workload**: `SchemaEnforcementWorkload`
- Define collections with schemas (required fields, types, enums, nesting)
- Attempt valid and invalid writes in random order
- CHECK: all invalid writes rejected; all entities in storage pass schema validation
- CHECK: error responses include field path, expected type, actual value

### INV-006: Link Integrity

**Statement**: No link references a non-existent entity (unless force-deleted with cascade). Link-type constraints (cardinality, required target collection) are enforced.

**Verification**: After any workload, for every link in storage, both source and target entities must exist. Link-type constraints must hold.

**Workload**: `LinkIntegrityWorkload`
- Create entities and links across collections
- Delete entities (should fail if inbound links exist, or cascade)
- CHECK: no dangling links; all link-type constraints satisfied

### INV-007: Version Monotonicity

**Statement**: Entity versions strictly increase. No version is reused or skipped. Version 1 is always the creation version.

**Verification**: For each entity, audit entries must show version 1, 2, 3, ... with no gaps or repeats.

**Workload**: Verified as a CHECK in every workload that modifies entities.

### INV-008: Transaction Atomicity

**Statement**: Multi-operation transactions either fully commit (all operations visible, all audit entries present with shared transaction ID) or fully abort (no operations visible, no audit entries).

**Verification**: Execute transactions that include intentional failures mid-transaction. Verify partial state never leaks.

**Workload**: `TransactionAtomicityWorkload`
- Transactions with 2-10 operations
- BUGGIFY: inject failures after some operations but before commit
- CHECK: for each transaction ID in audit log, all operations are present; for failed transactions, no operations are present
- AP/AR scenario: debit account A, credit account B, create ledger entry — must be all-or-nothing

### INV-017: Tenant Path Isolation (ADR-018, FEAT-014)

**Statement**: An entity written at URL path `/tenants/A/databases/X/...` is never readable at any URL path with a different `{tenant}` segment, regardless of the authenticating credential. Cross-tenant reads MUST return 404 or 403, never data from the wrong tenant.

**Verification**: For every `(tenant_a, tenant_b, entity_id)` triple in the workload, attempt to read `entity_id` via every `(tenant_c/database_d)` path. The only path that returns data MUST be the one the entity was written at.

**Workload**: `TenantPathIsolationWorkload` — creates N tenants each with M databases, writes entities to random `(tenant, database, collection, id)` tuples, issues read requests for every tuple at every path, asserts the tuple→path mapping is injective.

### INV-018: Grant Enforcement (ADR-018 §4, FEAT-012)

**Statement**: A request with a JWT whose `grants.databases[].ops` does not include the op required by the HTTP method MUST be rejected with 403 `op_not_granted`, and the underlying storage MUST NOT be touched (no partial side effects, no audit entry for the attempted op).

**Verification**: For every combination in ADR-018's op-to-HTTP-method mapping table, issue a JWT with grants deliberately missing the required op and assert: (a) response is 403 with `error.code = "op_not_granted"`, (b) storage state is unchanged pre/post, (c) audit log has zero new entries, (d) `axon_auth_rejections_total{error_code="op_not_granted"}` incremented by exactly 1.

**Workload**: `GrantEnforcementWorkload` — iterates the HTTP method × op matrix, BUGGIFY inserts random deletions from the grants array.

### INV-019: JWT Rejection Determinism (ADR-018 §4 failure mode table)

**Statement**: Every row of ADR-018's JWT failure mode table maps a specific JWT defect to a specific `(status, error.code)` pair. That mapping MUST be deterministic: the same defect MUST produce the same pair across all calls, backends, and runs.

**Verification**: Enumerate each failure mode from the table, construct the minimal defective JWT, submit it, and assert `(response.status, response.body.error.code)` matches the table row. Run the same enumeration against every storage backend. Run the enumeration under `axon-sim` with three fixed seeds — the output MUST be byte-identical across seeds.

**Workload**: `JwtRejectionDeterminismWorkload` — table-driven test case per row. No randomness; failure is fatal.

### INV-020: Auto-Bootstrap Uniqueness (ADR-018 §6, FEAT-014)

**Statement**: Under N concurrent first-requests to a fresh deployment, exactly one `tenants` row is created with `name = "default"`, exactly one `tenant_users` row joins the authenticating user as admin, and all N requests observe the bootstrapped state after their respective commits. No duplicate tenant, no duplicate membership, no lost bootstrap.

**Verification**: Spawn N parallel requests against a fresh empty DB. Post-run, `SELECT COUNT(*) FROM tenants WHERE name='default'` MUST return 1; `SELECT COUNT(*) FROM tenant_users` MUST return 1 per distinct user; every request MUST have observed `tenant_id` equal to the single row's id.

**Workload**: `AutoBootstrapConcurrencyWorkload` — N ∈ {2, 8, 64, 256} with BUGGIFY-injected commit delays to widen the race window.

### INV-021: Federation Consistency (ADR-018 §2, FEAT-012)

**Statement**: A `user_identities` row with `(provider, external_id)` resolves to exactly one `user_id`, and that `user_id` is stable across all subsequent resolutions for the same `(provider, external_id)` pair — even under concurrent first-seen resolutions.

**Verification**: Issue N parallel whois resolutions for the same tailnet identity on a fresh DB. Assert exactly one `users` row, exactly one `user_identities` row, all N requests return the same `user_id`. Then add a second provider (`oidc`) for the same human and assert the `users` row is preserved while two `user_identities` rows now point to it.

**Workload**: `FederationRaceWorkload`.

### INV-022: Audit Attribution Stability (ADR-018 Implementation Notes, FEAT-003)

**Statement**: An audit entry's `{user_id, tenant_id, jti}` triple remains resolvable to the original identity even after: (a) the user's display name / email changes, (b) the user is suspended, (c) the credential is revoked, (d) the tenant is deleted (subject to retention policy). No audit entry's attribution fields may be mutated post-write.

**Verification**: Write audit entries, then mutate each attribution dimension (rename user, suspend user, revoke jti, delete tenant within retention). Re-query the audit entries and assert the original `{user_id, tenant_id, jti}` values are preserved byte-for-byte.

**Workload**: `AuditAttributionStabilityWorkload`.

---

## 4. L2: Business Scenario Tests

Real-world workflows from the use case research, encoded as deterministic integration tests. Each scenario tests a complete business process end-to-end against embedded Axon.

### SCN-001: AP/AR — Payment Application with Partial Payment

**Source**: Use case research, AP/AR domain, workflow #2

**Setup**:
- Create `invoices` and `payments` collections with schemas from use case research
- Create 3 invoices: INV-030 ($5,000), INV-035 ($5,000), INV-040 ($3,000)
- Create customer with balance $13,000

**Execution**:
1. Create payment PMT-107 for $7,500
2. In one transaction: apply $5,000 to INV-030 (→ paid), $2,500 to INV-035 (→ partially_paid), create ledger entries, update customer balance

**Check**:
- [ ] INV-030 status is `paid`, INV-035 status is `partially_paid`
- [ ] `paid-by` links carry correct `amount_applied` metadata
- [ ] Ledger entries balance: debit cash $7,500 == credit AR $7,500
- [ ] Customer balance reduced by $7,500
- [ ] Audit log shows all changes under one transaction ID
- [ ] Reverting the transaction restores all entities to pre-payment state

**Failure scenario**: Version conflict on customer balance mid-transaction → entire payment rolls back, no partial application.

### SCN-002: CRM — Contact Merge (Duplicate Resolution)

**Source**: Use case research, CRM domain, workflow #2

**Setup**:
- Create `contacts` and `deals` collections
- Contact-A and Contact-B represent the same person
- Contact-A has links: `works-at` → Company-X, `owns-deal` → Deal-1
- Contact-B has links: `works-at` → Company-X, `owns-deal` → Deal-2

**Execution**:
1. In one transaction: re-link Deal-1 from Contact-A to Contact-B, merge Contact-A fields into Contact-B, delete Contact-A

**Check**:
- [ ] Contact-A no longer exists
- [ ] Contact-B has both deals linked
- [ ] No orphaned links (no link references Contact-A)
- [ ] Audit log shows the merge as one transaction with before/after for all entities
- [ ] Contact-B's version incremented; merged fields present

### SCN-003: CDP — Identity Resolution and Profile Merge

**Source**: Use case research, CDP domain, workflow #1

**Setup**:
- Create `profiles` and `source-records` collections
- Two source records from different channels with matching email
- No unified profile exists yet

**Execution**:
1. Identity resolution creates a new unified profile
2. Links both source records to the profile via `resolved-from` with confidence scores
3. Profile gets canonical fields derived from highest-confidence sources

**Check**:
- [ ] Unified profile exists with canonical fields
- [ ] Both source records linked via `resolved-from` with metadata (confidence, match_rule)
- [ ] Audit trail shows profile creation and link creation
- [ ] Schema validation passes on the unified profile

### SCN-004: ERP — BOM Explosion via Recursive Traversal

**Source**: Use case research, ERP domain, workflow #2

**Setup**:
- Create `products` collection
- Widget-A contains Sub-Assembly-B (qty: 2) and Component-C (qty: 4)
- Sub-Assembly-B contains Component-C (qty: 1) and Component-D (qty: 3)

**Execution**:
1. Traverse from Widget-A via `contains` links to depth 3

**Check**:
- [ ] Traversal returns Sub-Assembly-B, Component-C (twice — direct and via B), Component-D
- [ ] Link metadata (`quantity`) is included at each hop
- [ ] Total Component-C needed: 4 (direct) + 2×1 (via B) = 6
- [ ] Leaf nodes identified (entities with no outgoing `contains` links)

### SCN-005: Workflow — Invoice Approval Chain

**Source**: Use case research, Workflow Automation + AP/AR domains

**Setup**:
- Create `invoices` collection with state machine: draft → submitted → approved → paid
- Invoice requires `approved-by` link before transition to `approved`

**Execution**:
1. Create invoice in `draft` state
2. Attempt to move directly to `approved` — must fail (invalid transition)
3. Move to `submitted`
4. Attempt to move to `approved` without `approved-by` link — must fail (guard condition)
5. Create `approved-by` link to approver contact
6. Move to `approved` — succeeds

**Check**:
- [ ] Invalid transition rejected with error listing valid transitions
- [ ] Guard condition failure rejected with specific reason
- [ ] Valid transition succeeds after guard is satisfied
- [ ] Audit trail captures each transition attempt (successful and failed)

### SCN-006: Issue Tracking — Dependency DAG and Ready Queue

**Source**: Use case research, Issue Tracking + Agentic Applications domains

**Setup**:
- Create `issues` collection
- Issue-A depends on Issue-B and Issue-C
- Issue-B depends on Issue-D
- Issue-C has no dependencies

**Execution**:
1. Query "ready" issues (status=open, all dependencies in status=done)
2. Close Issue-D, re-query
3. Close Issue-B, re-query
4. Close Issue-C, re-query

**Check**:
- [ ] Initially: only Issue-C and Issue-D are ready (no deps or deps done)
- [ ] After closing D: Issue-B becomes ready
- [ ] After closing B: no new ready issues (Issue-A still blocked by C)
- [ ] After closing C: Issue-A becomes ready
- [ ] Dependency traversal correctly identifies transitive blockers

### SCN-007: Agentic — Bead Lifecycle with Concurrent Agents

**Source**: Use case research, Agentic Applications domain

**Setup**:
- Create `beads` collection with lifecycle: draft → pending → ready → in_progress → review → done
- 5 beads with dependency DAG
- 3 simulated agents claiming and completing beads

**Execution**:
1. Agents concurrently query ready queue, claim beads (update status to in_progress)
2. OCC ensures no two agents claim the same bead
3. Agents complete beads, unblocking dependents

**Check**:
- [ ] No bead processed by more than one agent (OCC prevents double-claim)
- [ ] Dependency DAG respected — bead not started until deps done
- [ ] Audit log shows which agent processed each bead
- [ ] All beads reach `done` state

### SCN-008: MDM — Golden Record Merge with Survivorship

**Source**: Use case research, MDM domain

**Setup**:
- Create `golden-records` and `source-records` collections
- Two source records with overlapping but conflicting data
- Match rule triggers merge

**Execution**:
1. In one transaction: create golden record, link both sources via `sourced-from`, apply survivorship rules (highest-confidence field wins)

**Check**:
- [ ] Golden record has correct field values per survivorship rules
- [ ] Both source records linked with `sourced-from` metadata (confidence, match_rule, source_system)
- [ ] Audit trail records the merge with before/after state
- [ ] Schema validation passes on golden record

### SCN-009: Document Management — Version Chain

**Source**: Use case research, Document Management domain

**Setup**:
- Create `documents` collection
- Document with 3 versions, linked via `supersedes` links

**Execution**:
1. Create v1, then v2 (linked `supersedes` v1), then v3 (linked `supersedes` v2)
2. Traverse `supersedes` chain to find version history

**Check**:
- [ ] Traversal from v3 via `supersedes` returns [v2, v1] in order
- [ ] Each version has correct metadata and audit trail
- [ ] Latest version queryable without traversal (field-based query)

### SCN-010: Time Tracking — Approval and Billing

**Source**: Use case research, Time Tracking domain

**Setup**:
- Create `time-entries` and `projects` collections
- Time entries linked to projects via `logged-for`

**Execution**:
1. Create time entries for a project
2. Submit for approval (status transition)
3. Approve (requires `approved-by` link)
4. Bill (aggregation query: sum hours by project where status=approved)

**Check**:
- [ ] State machine enforces: draft → submitted → approved → billed
- [ ] Aggregation returns correct total hours
- [ ] Audit trail shows approval chain

### SCN-011: Cross-Tenant Isolation via Path Routing (FEAT-014, ADR-018)

**Source**: ADR-018 — tenant as global account boundary with path-based wire protocol

**Setup**:
- Two tenants: `acme` (admin: `alice`) and `globex` (admin: `alice`, with role `read`)
- Each tenant has a database named `orders` (same name, different tenants)
- Alice has a `users` row; two `tenant_users` rows link her to both tenants
- Two JWT credentials for Alice: one with `aud=acme` granting read/write on `acme.orders`; one with `aud=globex` granting read on `globex.orders`

**Execution**:
1. With the acme credential, POST an entity to `/tenants/acme/databases/orders/entities/invoices/inv-001`
2. With the acme credential, attempt the same POST against `/tenants/globex/databases/orders/entities/invoices/inv-001`
3. With the globex credential, GET `/tenants/globex/databases/orders/entities/invoices/inv-001`
4. With the globex credential, attempt a POST to `/tenants/globex/databases/orders/entities/invoices/inv-002`
5. List collections under each tenant's path

**Check**:
- [ ] Step 1 succeeds (200) and creates the invoice in acme's orders database
- [ ] Step 2 returns 403 with "aud mismatch" — credential is bound to acme, URL tenant is globex
- [ ] Step 3 returns 404 — inv-001 exists in acme's orders, not globex's orders (same name, different database)
- [ ] Step 4 returns 403 with "op not granted" — globex credential has read-only grants
- [ ] Step 5 returns completely disjoint collection lists — no cross-tenant visibility
- [ ] Audit entries in acme carry Alice's user_id; audit entries in globex also carry Alice's user_id but are isolated to globex
- [ ] There is no HTTP path, header, or body field that can leak data across the tenant boundary

### SCN-012: User in Two Tenants with Different Roles (FEAT-012, FEAT-014)

**Source**: ADR-018 — M:N users via `tenant_users` join table

**Setup**:
- User `bob` with a single `users` row and two `user_identities` rows — one for tailscale, one for a future OIDC provider (federated to the same user_id)
- `tenant_users(acme, bob, admin)` and `tenant_users(globex, bob, read)`
- Both tenants have a database named `config`

**Execution**:
1. Bob authenticates via Tailscale and calls `GET /control/tenants` — expects to see both acme and globex
2. Bob attempts `POST /tenants/acme/databases/config/entities/settings/s-001` — admin in acme, should succeed
3. Bob attempts the same POST against `/tenants/globex/databases/config/entities/settings/s-001` — read-only in globex, should 403
4. Bob creates a JWT credential scoped to acme with write grants on `config`
5. Bob attempts to create a JWT credential scoped to globex with write grants on `config` — should fail because his role in globex is `read`
6. Remove Bob from globex (`DELETE /control/tenants/globex/users/bob`) and re-run step 1

**Check**:
- [ ] Step 1 lists both tenants, with Bob's role per tenant visible in the response
- [ ] Step 2 succeeds with a 200 and an audit entry attributed to `bob`
- [ ] Step 3 returns 403 with "role insufficient for op" (or similar structured error)
- [ ] Step 4 succeeds and returns a signed JWT with `aud=acme`, `grants.databases[0].ops=["read","write"]`
- [ ] Step 5 returns 403 with "grants exceed role"
- [ ] After step 6, step 1 lists only acme; Bob's access to acme is unchanged; attempting any operation on globex paths returns 403

### SCN-013: JWT Credential Grant Enforcement and Revocation (FEAT-012)

**Source**: ADR-018 — JWT credentials with `grants` claim; revocation via `jti`

**Setup**:
- Tenant `acme` with databases `orders`, `analytics`, `internal`
- User `svc-ci` with role `write` in `acme`
- Credential issued to `svc-ci` with `grants: { databases: [{name: "orders", ops: ["read","write"]}, {name: "analytics", ops: ["read"]}] }`, TTL 1 hour

**Execution**:
1. POST an entity to `/tenants/acme/databases/orders/entities/invoices/inv-001` with the credential
2. POST an entity to `/tenants/acme/databases/analytics/...` with the same credential (analytics is grant-read-only)
3. GET an entity from `/tenants/acme/databases/analytics/...` with the same credential
4. GET an entity from `/tenants/acme/databases/internal/...` — internal is not in grants at all
5. Revoke the credential via `DELETE /control/tenants/acme/credentials/{jti}`
6. Re-attempt step 1 with the revoked credential

**Check**:
- [ ] Step 1 succeeds (200) — orders grants write
- [ ] Step 2 returns 403 with "op not granted" — analytics is read-only
- [ ] Step 3 succeeds (200) — analytics grants read
- [ ] Step 4 returns 403 with "database not in grants" — internal is outside the credential's scope
- [ ] Step 5 returns 204 (or similar) and the `jti` is added to the revocation table
- [ ] Step 6 returns 401 within 1 second of step 5 (LRU cache propagation window)
- [ ] An audit entry exists for the credential revocation, attributed to the admin who revoked it
- [ ] Clock skew: a credential with `exp` 10 minutes in the future is accepted; a credential with `nbf` 10 minutes in the future is rejected

### SCN-014: Authentication Rejection Matrix (ADR-018 §4)

**Source**: ADR-018 Section 4 JWT failure mode table — each row is a distinct negative test.

**Setup**:
- A tenant `acme` with one admin user `alice`, a `write` member `bob`, and one database `orders`.
- A valid baseline JWT for `alice` against `acme` with `grants: [{name: "orders", ops: [read, write, admin]}]`.

**Execution**: for every row in ADR-018's failure mode table (14 rows as of this writing), construct the minimal defective JWT or URL, submit a representative request, and assert the response status, `error.code`, and observability counter.

Representative rows:
1. No Authorization header → 401 `unauthenticated`
2. Bearer with non-JWT payload → 401 `credential_malformed`
3. `aud` is a JSON array `["acme"]` → 401 `credential_malformed`
4. Signature computed with a wrong key → 401 `credential_invalid`
5. `exp` = now − 60s → 401 `credential_expired`
6. `nbf` = now + 3600s → 401 `credential_not_yet_valid`
7. `jti` added to revocation table before request → 401 `credential_revoked`
8. `iss` = "other-deployment" → 401 `credential_foreign_issuer`
9. Valid JWT for tenant `beta` submitted to `/tenants/acme/...` → 403 `credential_wrong_tenant`
10. Valid JWT whose `sub` user was marked `suspended_at_ms` → 401 `user_suspended`
11. Valid JWT whose `sub` was removed from `tenant_users` → 403 `not_a_tenant_member`
12. JWT with grants for `orders` submitted to `/tenants/acme/databases/customers/...` → 403 `database_not_granted`
13. JWT with `ops: [read]` submitted via POST → 403 `op_not_granted`
14. Successful baseline request → 200 (ratchet: baseline must still succeed)

**Check**:
- [ ] Every row returns the exact `(status, error.code)` pair from ADR-018's table
- [ ] Every row increments `axon_auth_rejections_total{error_code="..."}` by exactly 1 (the baseline row increments nothing)
- [ ] Every row produces a structured log event with full envelope fields
- [ ] No row touches storage (no new entities, no new audit rows) — verified by a pre/post snapshot of the backing store

### SCN-015: Default-Tenant Bootstrap Under Concurrency (FEAT-014, ADR-018 §6)

**Setup**: A fresh deployment with zero tenants, zero users, a single storage backend.

**Execution**:
1. Spawn 64 parallel requests, each using a distinct Tailscale identity, each hitting `/tenants/default/databases/default/collections/items`.
2. Wait for all 64 to complete.

**Check**:
- [ ] Exactly 1 row in `tenants` with `name = "default"`
- [ ] 64 rows in `users` (one per distinct identity) and 64 rows in `tenant_users` with role `admin` pointing at the one default tenant
- [ ] Exactly 1 row in `databases` with `name = "default"` bound to the default tenant
- [ ] All 64 requests return the same `tenant_id` and `database_id` in their responses
- [ ] Zero `credential_malformed` / `unauthenticated` rejections in the counter
- [ ] Audit log contains `tenant.created`, `database.created`, and 64 `user.provisioned` events — no duplicates

### SCN-016: BYOC Deployment Boundary (FEAT-025, ADR-017)

**Source**: ADR-017 + FEAT-025 + ADR-018's clarification that FEAT-025 is the BYOC control plane above deployments, not an embedded per-deployment control plane.

**Setup**:
- A local "control plane" process owning a registry of deployments.
- Two `axon-server` processes on different ports, each registered as a managed deployment (`dep-alpha`, `dep-beta`), each with its own tenant set.

**Execution**:
1. `dep-alpha` has tenant `acme`; `dep-beta` has tenant `acme` (same name, different deployment).
2. A data-plane write to `dep-alpha`'s `acme` tenant.
3. A data-plane read to `dep-beta`'s `acme` tenant for the same entity id.
4. Control-plane query via the BYOC control plane: `GET /control/deployments/dep-alpha/tenants`.

**Check**:
- [ ] Step 3 returns 404 — the two `acme` tenants are fully independent
- [ ] The BYOC control plane can enumerate tenants per deployment but does not aggregate data across deployments
- [ ] The BYOC control plane has no data-plane surface (attempts to read an entity via the control plane return 404)
- [ ] Each deployment's audit log is self-contained (no BYOC-level cross-deployment audit)

---

## 5. L3: Property-Based Tests

### PROP-001: Schema Round-Trip

**Property**: For any valid ESF schema definition, generating a random valid entity and validating it against the schema always succeeds. Generating a random invalid entity (type mismatch, missing required field) always fails with a structured error.

### PROP-002: Audit Reconstruction

**Property**: For any sequence of CRUD operations on an entity, replaying the audit log from the beginning produces the current state of the entity.

### PROP-003: OCC Linearizability

**Property**: For any sequence of concurrent read-then-write operations on a single entity, the final state is consistent with some serial ordering of the successful writes. No acknowledged write is lost.

### PROP-004: Transaction Serializability

**Property**: For any set of concurrent multi-entity transactions, the final database state is consistent with some serial ordering of the committed transactions.

### PROP-005: Link Graph Consistency

**Property**: After any sequence of entity/link operations, the link graph is consistent: no dangling references, all link-type constraints hold, traversal from any entity follows only valid links.

### PROP-009: Credential Grant Subset Invariant (ADR-018 §4, FEAT-012)

**Property**: For every credential issued through `POST /control/tenants/{id}/credentials`, the credential's `grants` object is a subset of the issuer's role capabilities per ADR-018's grants rule table. Formally: let `R(i)` = the set of `(database, op)` pairs the issuer `i` may delegate given their tenant role, and `G(c)` = the set expressed by credential `c`. Then `G(c) ⊆ R(i)` for every successfully-issued credential, and any attempt to issue a credential with `G(c) ⊄ R(i)` returns 403 `grants_exceed_role`.

**Generators**: random issuer roles, random requested grants, random tenant/database graphs. Rejection path and accept path are both exercised.

### PROP-010: Path Routing Determinism (ADR-018, FEAT-014)

**Property**: For any URL `/tenants/{t}/databases/{d}/collections/{c}/entities/{id}`, the `(tenant, database, collection, entity_id)` tuple extracted by the routing middleware is identical across all backends, all protocols (REST and GraphQL), and all sessions. The extraction is a pure function of the URL; no hidden state affects the result.

**Generators**: random URL segments (including Unicode, URL-encoded bytes, max-length identifiers). The same URL is fed through REST and through GraphQL's equivalent `entity(tenant, database, collection, id)` query; the resolved tuple must match.

### PROP-011: Tailscale ↔ JWT Equivalence (ADR-018 §4, FEAT-012)

**Property**: For any given `(user, tenant, database, op)` combination, the request handler produces the identical response whether authenticated via a Tailscale whois synthetic claim or via a JWT credential granting the same scope. Response bodies, audit log entries (modulo the `auth_method` field), and error codes are byte-identical.

**Generators**: random (user, tenant, database, op) combinations. Each combination is run twice — once with `Authorization: Bearer <jwt>`, once via the tailnet sock — and the outputs are diffed. A diff that isn't explicitly on the `auth_method` field is a bug.

---

## 6. L4: Backend Conformance

Every StorageAdapter implementation must pass the **identical** test suite. Tests are written against the trait, parameterized by backend.

| Test Suite | SQLite | PostgreSQL | FoundationDB | Memory |
|-----------|:------:|:----------:|:------------:|:------:|
| INV-001 through INV-008 | Required | Required | Required | Required |
| INV-017 through INV-022 | Required | Required | Required | Required |
| SCN-001 through SCN-010 | Required | Required | Required | Required |
| SCN-011 through SCN-016 | Required | Required | Required | Required |
| PROP-001 through PROP-005 | Required | Required | Required | Required |
| PROP-009 through PROP-011 | Required | Required | Required | Required |
| BM-001 through BM-010 | Required | Required | Required | N/A (memory not benchmarked) |

If a backend cannot pass any invariant, it is not shipped.

---

## 7. L5: Performance Benchmarks

From technical requirements. All benchmarks use `criterion` and are ratcheted.

| Benchmark | Target (p99) | Workload |
|-----------|-------------|----------|
| BM-001: Single entity read | < 5 ms | 10,000 random point lookups |
| BM-002: Single entity write | < 10 ms | 10,000 creates with schema validation + audit |
| BM-003: Multi-entity transaction (5 ops) | < 20 ms | 1,000 transactions (debit/credit pattern from SCN-001) |
| BM-004: Collection scan (1,000 entities) | < 100 ms | Filter + sort + paginate |
| BM-005: Audit log append overhead | < 2 ms | Measured as delta: write-with-audit minus write-without |
| BM-006: Link traversal (3 hops) | < 50 ms | BOM explosion pattern from SCN-004 |
| BM-007: Aggregation (10K entities) | < 500 ms | COUNT/SUM/GROUP BY over invoice collection |
| BM-008: Concurrent writers (100) | Linear throughput scaling | 100 agents writing to different entities |
| BM-009: Schema validation | < 1 ms | Validate typical entity (20 fields, 2 levels nesting) |
| BM-010: Audit query (single entity) | < 100 ms | Retrieve all audit entries for one entity (100 mutations) |

---

## 8. L6: API Contract Tests

- gRPC service methods match protobuf definitions exactly
- HTTP gateway produces identical results to gRPC for all operations
- Error responses conform to structured error format (code, detail, field path)
- Embedded API (Rust trait) produces identical results to network API

### L6 Path-Based Route Contract (ADR-018)

Every data-plane route is path-prefixed with `/tenants/{t}/databases/{d}/`.
The contract suite generates a golden route inventory from the axum router
and asserts the following properties on every build:

- [ ] Every data-plane route starts with the literal prefix `/tenants/{tenant}/databases/{database}/`. Routes under `/health`, `/ui`, and `/control` are the only exceptions.
- [ ] No route reads an `X-Axon-Database` header; the contract test greps the crate source for `X-Axon-Database` and fails if any match is found.
- [ ] No `/db/{name}/...` prefix survives; fail on any router builder using the legacy prefix.
- [ ] Every data-plane route, on unauthenticated access, returns 401 or 403 per ADR-018's failure mode table — not 200 and not 500. Enumerate the route table and hit each one with no credentials; record the status.
- [ ] The generated OpenAPI schema lists every `/tenants/{tenant}/databases/{database}/...` route and names `tenant` + `database` as required path parameters; the GraphQL SDL lists matching top-level types. The two surfaces are cross-checked by a golden test.
- [ ] The grants rule table in ADR-018 §4 is enforced on every control-plane endpoint that issues credentials: the contract test submits deliberately-over-scoped issuance requests and asserts 403 `grants_exceed_role`.

### L8 SDK and Golden Client Contract

Each supported SDK (Rust, TypeScript, Python when they land) has a matching L8 golden-client test that exercises the full `(tenant, database)` fluent API:

- [ ] A `.tenant(t).database(d).entity(c, id).get()` call against a live server resolves to the expected entity.
- [ ] The SDK's error enum has a stable `error_code` discriminant that matches the ADR-018 failure mode table one-for-one — adding a code to the table without adding it to the SDK enum is a compile error via a shared codegen manifest.
- [ ] A credential with revoked `jti` surfaces as `CredentialRevoked` — not a generic `Unauthorized`.

---

## 9. Test Execution Schedule

| Trigger | Test Layers | Seed Count | Timeout |
|---------|------------|-----------|---------|
| Every commit (CI) | L1 (100 seeds), L2, L3 (1K iterations), L4 (SQLite + memory), L6 | 100 | 10 min |
| Pre-merge | L1 (1K seeds), L2, L3 (10K iterations), L4 (all backends), L5, L6 | 1,000 | 30 min |
| Nightly | L1 (100K seeds), L2, L3 (100K iterations), L4 (all backends), L5 | 100,000 | 4 hours |
| Weekly | L1 (1M seeds), L3 (1M iterations) | 1,000,000 | 24 hours |

### Seed Management

- Every CI run records the seeds used and their pass/fail status
- Failed seeds are added to a **regression seed file** that runs on every CI build forever
- The regression seed file is a ratchet — seeds are added, never removed

---

## 10. Traceability

| Invariant / Scenario | Feature | Principle | Use Case Domain |
|----------------------|---------|-----------|-----------------|
| INV-001 No Lost Updates | FEAT-008 | P4 (Transactions) | All |
| INV-002 Cycle Test | FEAT-008 | P4 (Transactions) | All |
| INV-003 Audit Completeness | FEAT-003 | P2 (Audit) | AP/AR, CDP, MDM |
| INV-004 Audit Immutability | FEAT-003 | P2 (Audit) | AP/AR, Compliance |
| INV-005 Schema Enforcement | FEAT-002 | P5 (Schema) | All |
| INV-006 Link Integrity | FEAT-007 | P3 (Entities/Links) | CRM, ERP, CDP |
| INV-007 Version Monotonicity | FEAT-008 | P4 (Transactions) | All |
| INV-008 Transaction Atomicity | FEAT-008 | P4 (Transactions) | AP/AR, CRM, MDM |
| SCN-001 Payment Application | FEAT-008, FEAT-003 | P2, P4 | AP/AR |
| SCN-002 Contact Merge | FEAT-007, FEAT-008 | P3, P4 | CRM |
| SCN-003 Identity Resolution | FEAT-007, FEAT-003 | P3, P2 | CDP |
| SCN-004 BOM Explosion | FEAT-009 | P3 | ERP |
| SCN-005 Approval Chain | FEAT-010, FEAT-003 | P2 | Workflow, AP/AR |
| SCN-006 Dependency DAG | FEAT-009, FEAT-006 | P3 | Issue Tracking, Agentic |
| SCN-007 Bead Lifecycle | FEAT-006, FEAT-008 | P1, P4 | Agentic |
| SCN-008 Golden Record Merge | FEAT-007, FEAT-008 | P3, P4 | MDM |
| SCN-009 Version Chain | FEAT-007, FEAT-009 | P3 | Document Mgmt |
| SCN-010 Time Approval | FEAT-010, FEAT-003 | P2 | Time Tracking |
| SCN-011 Cross-Tenant Isolation via Path Routing | FEAT-014, FEAT-012, ADR-018 | P1 | Multi-tenant SaaS |
| SCN-012 User in Two Tenants with Different Roles | FEAT-012, FEAT-014 | P1 | Multi-tenant SaaS |
| SCN-013 JWT Credential Grant Enforcement and Revocation | FEAT-012 | P1 | Security, integrations |
| SCN-014 Authentication Rejection Matrix | FEAT-012, ADR-018 | P1 (Security) | All |
| SCN-015 Default-Tenant Bootstrap Under Concurrency | FEAT-014, ADR-018 | P1 | Multi-tenant SaaS |
| SCN-016 BYOC Deployment Boundary | FEAT-025, ADR-017 | P1 | BYOC operators |
| INV-017 Tenant Path Isolation | FEAT-014, ADR-018 | P1 | All |
| INV-018 Grant Enforcement | FEAT-012, ADR-018 | P1 (Security) | All |
| INV-019 JWT Rejection Determinism | FEAT-012, ADR-018 | P1 (Security) | All |
| INV-020 Auto-Bootstrap Uniqueness | FEAT-014, ADR-018 | P1 | All |
| INV-021 Federation Consistency | FEAT-012, ADR-018 | P1 | All |
| INV-022 Audit Attribution Stability | FEAT-003, ADR-018 | P2 (Audit) | Compliance, Security |
| PROP-009 Credential Grant Subset | FEAT-012, ADR-018 | P1 | All |
| PROP-010 Path Routing Determinism | FEAT-014, ADR-018 | P1 | All |
| PROP-011 Tailscale ↔ JWT Equivalence | FEAT-012, ADR-018 | P1 | All |

---

*This test plan is the governing artifact. Implementation is complete when all invariants, scenarios, properties, and benchmarks pass across all backends.*
