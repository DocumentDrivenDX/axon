# Axon Gap-Closure Plan — Final Reviewed Draft

> **Review instruction:** Act as an adversarial critic. Find any remaining
> ambiguity that can cause incompatible implementation, migration/data loss,
> public bypass, unaudited/partial mutation, backend divergence, security leak,
> or false readiness. Every finding must cite this plan or concrete repository
> evidence. Unsupported findings are invalid.

## Outcome and limits

Axon is not complete today. This plan closes the current product scope in two
measurable stages.

- **Finish line A — pilot-ready governed core:** every collection-addressable
  public or governed-system write is entity-schema-bound; typed internal
  auth/policy relational state is contract-bound and co-audited; links and mixed transactions are
  declared, referentially safe, atomic, and append-only audited; memory has
  equivalent process-lifetime logical semantics while SQLite and PostgreSQL
  16 are restart-durable; the documented read-only Cypher subset and policy
  rules are implemented; operations and required consumer evidence are
  archived.
- **Finish line B — current committed scope complete:** finish line A plus the
  governed local read replica in FR-32/FEAT-032.

The attainable finish-line-A verdicts are `pilot-ready` or `hold`. GA requires
a separate plan and criteria.

`TARGET_RELEASE` below is not hard-coded. Phase 0 derives and records it from
the fetched authoritative baseline; current local version text is evidence to
reconcile, not authority before fetch.

Non-goals: Cypher writes; shortest-path/centrality/graph analytics; offline
writes or reconciliation (FR-33); distributed transactions/multi-region
placement; cryptographic audit chaining; and new audit-retention/erasure
behavior. Audit claims are backend-qualified: memory is process-lifetime
append-only/co-committed only; SQLite/PostgreSQL are restart-durable
append-only/co-committed. None is cryptographically tamper proof.

## Governing semantics

- ADR-004/FEAT-008 select OCC with Snapshot Isolation by default and opt-in
  Serializable/SerializableStrict. PostgreSQL maps Snapshot to
  `REPEATABLE READ` and the serializable tiers to `SERIALIZABLE`; SQLite uses
  `BEGIN IMMEDIATE`. Keep ADR-026 read-set/signature guards on every backend
  for API parity even where PostgreSQL adds native serialization defense. Do
  not introduce application/entity `SELECT FOR UPDATE` or application
  pessimistic locking. Internal metadata coordination is allowed only through
  the typed, non-public catalog: tenant/database `audit_allocator`, tenant
  `auth_audit_allocator`, idempotency reservation owner+fence CAS/row check,
  `link_set_version` OCC predicate-guard rows, and schema/policy/migration
  generation or maintenance advisory locks. `link_set_version` is an explicit
  ADR-026 amendment: a write-conflict signature, not application state or a
  pessimistic entity/link lock; it is typed-manifested, capability-gated, and
  fault-tested under Phase 5. These
  cannot address, expose, or lock application entities/links/index rows. The
  two audit allocators serialize commit order only within their independent
  database-data and tenant-auth audit domains; no cross-domain total order is
  promised. The other locks serialize only their named metadata lifecycle.
  PostgreSQL currently uses plain `BEGIN`, so isolation mapping is
  implementation work.
- ADR-007 defines the active schema as the latest committed version. FEAT-017
  owns compatibility, dry-run, force, lazy reads, and revalidation.
- FEAT-007 defines source-owned unidirectional link declarations. The target
  need not declare a reverse link. V1 supports create/delete, not in-place
  link or metadata update.
- FEAT-004 defines deleted entities as not-found on live reads; history lives
  in audit, not tombstone reads.
- CONTRACT-006 uses monotonic database `audit_id` internally. Public replica
  cursors become random server-resolved opaque handles; PostgreSQL LSN/transaction IDs are not
  cursors.
- CONTRACT-007 keeps the V1 graph limits: path depth 10, unindexed threshold
  1,000, worst-case cardinality 1M rows, and timeout 30 seconds.

## 0. Pin the authoritative baseline

1. Fetch/prune `origin`; record remote URL, timestamp, and exact
   `origin/master` SHA. Stop if fetch is unavailable. In that tree reconcile the
   workspace package version, PRD version/status, active release notes, and
   release manifests; all must identify one release. Record it as
   `TARGET_RELEASE` (the currently visible baseline suggests 0.7.1, but fetch
   decides). Any mismatch is a blocking authority repair, not a guessed target.
2. Before any clean/switch/worktree creation, deterministically capture
   `git status --porcelain=v2 -z --untracked-files=all` and
   `git ls-files -z --others --ignored --exclude-standard`. Park every path in
   the union—modified, deleted, renamed, staged, untracked, and ignored—in a
   binary-safe manifest/archive with path bytes, file type/mode, symlink target,
   object ID where available, byte length, and SHA-256. The currently known
   `bead.rs`, `main.rs`, execution bundle, review artifacts, and harness sessions
   are examples, not an allowlist. Generated/cache exclusions are never
   inferred: list each exact path/prefix, reason, and size to the user and
   require explicit approval before omitting it; without approval it is parked.
   Manifest records preserve porcelain-v2 status and index/HEAD object IDs.
   Present files/symlinks have payload+hash; deleted paths have no worktree
   payload, record prior/index type+OID and `expected_absent=true`; renames are
   one paired record with old expected absent and new payload/OIDs. Verification
   checks present byte/hash/type, deleted/old-name absence, and every recorded
   object with `git cat-file`, then replays the archive into a temporary checkout
   and requires byte-for-byte identical porcelain-v2 `-z` status. Do this before
   any clean/switch operation. Compare
   parked work to `TARGET_RELEASE`, but do not port, discard, clean, or overwrite any path
   without a separate user decision.
3. Only after archive verification and explicit approval of every proposed
   exclusion, create a clean isolated branch/worktree from the pinned SHA. This
   operation must not switch, clean, reset, or alter the original checkout.
   Preserve DDx attempt history: no squash, rebase, amend, or filtering.
4. Derive tracker counts, readiness gates, and verdict beads from the fetched
   tracker; do not trust the current checkout or hard-coded IDs. Audit open and
   closed core/readiness beads against live behavior and rerun their ACs.
   Seed suspected false closures only from fetched evidence; do not assume the
   local audit-atomicity/link statuses are wrong. Reopen or supersede a closure
   only when its live AC fails, before filing parallel work.

Exit: pinned SHA/worktree, reconciled `TARGET_RELEASE`, parked-change manifest, requirements-to-live-
evidence matrix, and repaired tracker truth.

## 1. Freeze contracts, criteria, tests, and dependencies

Before implementation:

1. Reconcile the PRD's offline/bidirectional-sync wording directly with FR-31
   resumable streams, FR-32 read replica, and FR-33 offline-write deferral; do
   not cite a nonexistent numbered goal.
2. Refresh FEAT-032, ADR-025, ADR-003, ADR-004, architecture, implementation
   plan, and test plan. Describe `StorageCursorStore`, snapshot support, and
   TypeScript `LocalReplica` as partial/unwired. Correct ADR audit/isolation
   claims against live code.
3. Amend CONTRACT-010 and FEAT-001/002: `entity_schema` is required for every
   governed public/system collection. Typed physical/virtual internals follow
   their internal contract instead.
4. Add PRD success criteria for schema fail-closed/evolution, typed links and
   mixed transactions, payload limits, PostgreSQL qualification, graph policy
   and limits, and FR-32.
5. Freeze normative contracts for internal namespaces/audit exemptions,
   schema/link errors, Axon canonical JSON measurement, transaction-framed
   CDC, opaque tokens, and PostgreSQL support.
   Reconcile CONTRACT-006 with Accepted ADR-025/FEAT-032: producer restart and
   schema-compatible migrations remain resumable. Schema-incompatible changes
   and any policy/auth epoch change invalidate and require purge/rebootstrap.
   Amend CONTRACT-006's overly broad “all schema changes” wording accordingly.
   Also amend ADR-025's migration-risk section to select the pre-1.0 hard cut
   from raw resume IDs to opaque handles described in Phase 10.
   Amend CONTRACT-007 to make 1M a hard `TARGET_RELEASE` maximum and remove named-query
   cardinality/unindexed-scan opt-outs above the fixed thresholds; lower
   declaration limits remain allowed.
6. Make PostgreSQL 16 the sole `TARGET_RELEASE`-qualified PostgreSQL major,
   matching current CI/compose. Other majors are unsupported until qualified.
7. Author and freeze FEAT-032 stories/ACs before replica implementation.
8. Freeze graph benchmark dataset, hardware class, backend/configuration,
   warmup, sample count, percentile method, p99 thresholds, and ratchets. Run
   release-blocking p99 on a pinned dedicated runner/reference host; shared
   GitHub-hosted CI runs functional benchmarks only and cannot decide hold.
9. Correct the stale `__axon_policies__` claim and define one canonical policy
   substrate: adapter-owned `policy_catalog` state, not a generic entity
   pseudo-collection. It stores a monotonic `policy_epoch` keyed exactly by
   `(tenant_id, database_id)`
   and canonical policy hash covering active schema `access_control`, masks,
   and write policy. Separately, a durable per-tenant `auth_epoch` increments
   exactly once per committed single-tenant auth-state transaction containing
   one or more durable credential-record create/rotate/disable/revoke or grant/
   role changes. It never increments once per record inside that transaction.
   Minting or refreshing a
   bearer/JWT/authenticated session from an unchanged credential binds the
   current post-commit epoch but never increments it. Every new durable
   credential/grant/role record names exactly one tenant; runtime mutation with
   absent or multiple tenant scope fails `auth_scope_required` before writes.
   A legacy record with missing scope is handled only by the journaled migration
   protocol in Phase 3, never by a live multi-tenant auth transaction.
   Tokens bind their tenant epoch. This is deliberately tenant-wide in `TARGET_RELEASE`:
   every delta invalidates all bearer/JWT tokens, authenticated server sessions,
   query subscriptions, resume sessions, and idempotent replay authorization in
   that tenant, regardless of principal or database. “Relevant auth change”
   everywhere in the documents means exactly “the stored tenant auth epoch no
   longer equals the current epoch”; narrower principal/grant epochs are not a
   `TARGET_RELEASE` concept. Existing durable credentials can reauthenticate to obtain a
   token bound to the new epoch. Memory implements process-lifetime semantics;
   SQLite/PostgreSQL persist these values. Phase 1 freezes the contract; Phase
   4/7 implementation blocks graph and replica work.
   Database identity is tenant-qualified, so exactly one policy row exists per
   `(tenant_id, database_id)`; a semantic access-control/mask/write-policy
   change increments that one row,
   never rows for another tenant. Database creation atomically creates epoch 0;
   missing rows fail closed as `policy_catalog_missing`. New tenants receive
   rows only for their own databases—no policy fan-out is implied.
   Golden vectors cover one-record and multi-record transactions, rollback,
   retry, and fresh init's credential+grant transaction: init starts at 0 and
   commits both records plus auth audit at epoch 1.
10. Freeze `AXON-POLICY-HASH-1`: SHA-256 over AXON-CJSON-1 bytes of
    `{format_version,tenant_id,database_id,collections:[...]}`. Collections are
    sorted by qualified ID and contain only normalized policy semantics:
    `access_control`, masks, and write policy. Active schema version/lineage is
    separately bound and excluded, so compatible non-policy schema changes do
    not rotate the policy hash. Parse policy into one typed normalized AST,
    expand every grammar default, sort sets/maps by their normative keys, and
    erase syntax-only ordering/aliases; omit descriptions, timestamps, map
    insertion order, and epochs. Publish normalized-AST/hash/byte
    golden vectors shared by memory/SQLite/PostgreSQL and every surface.
    Semantic or canonical-format change requires a new hash version. The
    collections list contains every registered policy-addressable public or
    governed-system collection and excludes hidden physical/virtual state.
    Collection create adds an entry with every contract default expanded and
    increments policy epoch/hash once; collection delete removes it and does
    likewise. Materializing an implicit default as explicit but semantically
    identical syntax does not rotate. Existing-collection structural-only
    schema changes, schema history, and template changes do not rotate; any
    access-control/mask/write-policy semantic change does. Lifecycle activation
    co-audits the structural event then policy-catalog event and invalidates
    bound sessions when the hash rotates. Golden vectors cover empty catalog,
    create, delete, implicit→explicit default, structural-only, policy-only, and
    combined changes on every backend.
11. Freeze `AXON-SCHEMA-CATALOG-HASH-1`: SHA-256 over AXON-CJSON-1
    `{format_version,tenant_id,database_id,collections:[...]}` with every
    registered collection sorted by qualified ID and containing active version,
    SHA-256 of its normalized `StructuralSchemaV1` AST, and sorted normalized link
    declarations including target/cardinality/required/metadata/default
    semantics. `StructuralSchemaV1` explicitly removes `access_control`, masks,
    write policy, and every other policy-bearing section before hashing; those
    sections exist exclusively in the normalized policy AST/hash. A proposed
    document whose embedded policy projection disagrees with
    `proposed_database_policy` fails `policy_projection_mismatch`. A policy-only
    change therefore never rotates schema-catalog hash/lineage or emits a schema
    event; a structural change does. Any active version or structural semantic declaration change rotates it,
    including FEAT-017-compatible changes; inactive history, descriptions, and
    timestamps are excluded. Publish backend/surface golden vectors that vary
    policy-only, structural-only, and both projections, and bind all
    schema-policy activation OCC to this whole-catalog hash.

Tracker wiring:

- Every core corrective/evidence bead blocks the derived core evidence gate,
  which blocks the pilot verdict.
- Create a separate finish-line-B gate/review depending on the pilot verdict
  and every FR-32 bead; P2 work does not delay or contaminate finish line A.
- Each bead names requirement IDs, files, behavioral tests, exact commands,
  non-scope, and durable closure evidence.

Exit: `ddx doc validate`, stale-doc checks, traceability, PRD criteria, and
dependency graph agree; no implementation bead invents product semantics.

## 2. Seal internal namespaces and raw storage

### Typed namespace inventory

The initial fetched-tree inventory is:

| Name | Class | Rule |
|---|---|---|
| `__axon_links__` | hidden physical collection | forward link state; logical link audit covers it |
| `__axon_links_rev__` | hidden physical collection | reverse index; no separate audit |
| `_cdc_cursors` | hidden checkpoint collection | derived state; no actor audit |
| `link_set_version` | hidden OCC predicate-guard table | governed link mutations CAS it; no separate actor audit |
| `__axon_beads__` | governed system collection | full schema/policy/atomic-audit rules |
| `__mutation_intents` | virtual audit subject | audit namespace, not generic entity storage |
| `__axon_policies__` | stale documented-only name | reserved/rejected; correct docs, never implement as entity storage |
| adapter auth tables | governed internal relational state | non-`CollectionId`; credential/grant/role API, co-committed audit+tenant auth epoch |

Create typed `SystemCollection`/`AuditSubject` constructors plus an internal
storage manifest. Inventory every `CollectionId` construction, table/migration
construction, and raw `put/delete/put_link/delete_link` call—not merely names
with underscore prefixes. CI fails if a new collection-addressable namespace,
raw mutation call, or internal table is absent from the manifest/allowlist.
Secondary indexes, schemas, audit, idempotency state, and projections must be
classified or proven non-`CollectionId`-addressable.

The manifest also classifies every physical SQLite/PostgreSQL table, view,
trigger/function, and DML entrypoint as governed data, logical audit, auth,
derived checkpoint/index, or migration-only. Adapter production code receives
no raw governed connection; all INSERT/UPDATE/DELETE/MERGE/COPY/TRUNCATE and
dynamic builder writes touching governed tables, plus every mutating function/
procedure/trigger invocation through `SELECT`, `CALL`, or implicit trigger,
require a typed
`GovernedWriteTx` created only by the mutation-plan co-commit primitive.
`MigrationCapability`, `CheckpointCapability`, and allocator capabilities are
closed exceptions limited to their manifest-listed tables/functions; none can
address governed entity/link/schema/policy/auth rows outside its class.

CI runs `cargo xtask audit-dml-boundary`, which uses Rust AST plus SQL parsing to
inventory `sqlx` query macros/functions/`QueryBuilder`, SQLite execute/batch
calls, COPY APIs, migrations, triggers, stored functions, and raw-connection
access. Dynamic governed table identifiers and unparseable DML are build
failures, not allowlist escapes. Each allowed statement records source file,
function, tables, capability, mutation class, and co-commit/fault test; the
generated inventory is diffed against the checked-in manifest. External
PostgreSQL functions/procedures are declared `read_only` or `mutating`; mutating
ones have EXECUTE revoked from the ordinary runtime role and are callable only
through the capability wrapper. Trigger side effects are attributed to and
tested with their invoking governed statement. Migration-only functions use
the exclusive migration role and are unavailable after migration. External
compile-fail tests prove adapters/connections/capabilities cannot be obtained,
and backend fault tests execute every governed DML entrypoint at data/audit/
commit boundaries. A repository-wide negative fixture inserts direct data-only
SQL and proves the lint fails. No direct adapter-specific SQL write may satisfy
the plan merely because raw trait calls were sealed.

### Positive boundary enforcement

One positive collection-access guard runs on every generic handler operation:
entity/read/list/update/patch/delete, schema/template/lifecycle, links,
rollback/intents, query/traverse, and each staged transaction operation.

- Generic public paths reject hidden, virtual, and governed-system names.
- The bead module gets a sealed name capability only. Its built-in schema must
  declare the self-targeting `depends-on` many-to-many link and schema-owned
  lifecycle; all mutations still use the governed handler and atomic audit.
- Every embedded Rust `axon-api`, HTTP, gRPC/protobuf, GraphQL, MCP, CLI, and TypeScript SDK test
  attempts direct generic access to each reserved name and must fail.
- Generic access has one public error: `reserved_namespace` with reason
  `generic_access_forbidden` and details `{name,operation}`. Embedded Rust
  returns `AxonError::ReservedNamespace`; HTTP returns 400; gRPC
  `INVALID_ARGUMENT` with `axon.v1.ErrorDetails`; GraphQL returns an error with
  `extensions.code=reserved_namespace`; MCP returns JSON-RPC `-32602` with the
  same data code/details; CLI exits 2; and TypeScript throws
  `ReservedNamespaceError`. Missing internal system capability is a compile-time
  API boundary or internal `SystemCapabilityRequired` invariant failure and is
  never translated into a public capability oracle. Golden parity tests assert
  exact code/reason/details, not merely failure.
- Audit/history is part of the same boundary. Generic collection/link audit,
  history, rollback-preview, transaction lookup, and audit filter arguments run
  the lexical namespace guard before audit lookup; hidden, virtual, and
  governed-system names return the same `reserved_namespace` parity contract
  and reveal no existence/count. Generic application audit returns only
  policy-visible logical public entity/link/schema events. Its filtered cursor
  is opaque and exposes neither gaps nor totals from excluded subjects.
- Governed-system and policy/intent logical audit is available only through a
  dedicated tenant/database-admin `query_system_audit(SystemAuditSubject)`
  operation; after admin authorization the server supplies the sealed module
  capability. The request uses a closed typed subject enum, never an arbitrary
  collection name, and the response redacts internal physical keys. A fully
  authorized database-admin unfiltered audit inspection may use numeric
  `audit_id` and observe logical system-group ordering; that is an intentional
  admin disclosure, not a generic application API. It still never returns
  forward/reverse/index/checkpoint rows.
- Tenant auth audit is reachable only through the separate tenant-admin
  `query_auth_audit` substrate and `auth_audit_id`; it cannot be addressed via
  database audit IDs, collection filters, CDC, or history. Rollback never
  targets system/auth audit records directly. Embedded Rust, HTTP, gRPC,
  GraphQL, MCP, CLI, and TypeScript golden tests cover absent/reserved names,
  each typed admin subject, authorization denial, filtered-cursor secrecy, and
  physical-row non-observability.
- Move the raw adapter mutation SPI outside Axon's supported data-store API;
  embedded users receive the governed handler/transaction API. Treat this as
  an intentional pre-1.0 API tightening. Make
  `AxonHandler::storage_mut`/`storage_and_audit_mut` crate-private or
  `cfg(test)` and stop re-exporting a directly writable adapter as application
  API. Apply the same fate to `into_storage`, `audit_log_mut`, every concrete
  adapter/raw-trait mutation re-export, and
  `StorageCursorStore::storage_mut`/`into_inner` (internal or test-only).
  Read-only inspection gets explicit immutable methods. Add external
  compile-fail tests proving an application crate cannot obtain mutable raw
  storage/audit or invoke ungoverned mutations. This is compile-time sealing,
  not documentation-only convention.
- The published/supported Rust application surface is `axon-api` plus
  read-only/core value types and generated clients. `axon-storage` and concrete
  adapter packages become `publish = false` workspace-internal SPI with no
  application features; adapters are selected through `AxonBuilder`. Raw
  modules are sealed/not re-exported. Compile-fail fixtures depend on the exact
  published crates/features in release metadata; a local path dependency on an
  unpublished internal crate is explicitly outside the product contract.
- Before sealing, add governed handler methods for intent preview/commit,
  transaction execution, and audit queries so GraphQL/MCP replace every
  current `storage_mut`/`storage_and_audit_mut` use without regressing to raw
  access. Migrate all `axon-server` gateway/service call sites too. Production
  surface crates compile only against these methods.
  gRPC must also pass reserved-name, schema fail-closed, six-operation transaction
  grammar, and compile-time raw-access sealing tests.

Define a sealed governed-system lifecycle API distinct from hidden physical
storage: `ensure_system_collection`, `put_system_schema`, and governed system
entity/link mutations accept an unforgeable module capability, then run the
same schema/policy/OCC/audit co-commit path as public data. The bead module owns
only its specific capability. Capabilities cannot access hidden physical or
virtual names. Tests prove system bootstrap/schema evolution/audit work while
generic public and raw paths remain unavailable.

Replace core slash-delimited link IDs before Phase 5 with a typed,
length-prefixed/binary `LinkKey` and prefix representation that is injective for
all allowed collection/entity/type strings. Migrate forward/reverse keys in the
legacy tool and add delimiter/Unicode/property tests. Cardinality, duplicate,
prefix-scan, and audit logic may not operate on the old joined key. The
TypeScript local-replica key fix is separate and remains in Phase 10.

Audit classes:

- Governed collection/schema/template/entity/link/cascade/transaction and
  policy/intent state mutations co-commit logical audit.
- Durable credential-record create/rotate/disable/revoke and grant/role changes
  co-commit a redacted logical auth audit plus affected tenant `auth_epoch`
  increments. Bearer/JWT/session mint or refresh is audited as authentication
  telemetry, not a governed auth-state mutation, and does not change the epoch.
  Governed auth audit contains IDs,
  actor, scope, and before/after metadata but never credential/JWT secret
  material. Auth epoch is not exempt derived state.
- `AuthAuditRedactionV1` is a closed allowlist: event kind, tenant, actor/
  target principal IDs, credential/grant/role IDs, credential kind, resource
  scope IDs, enabled/revoked status transition, expiry timestamp, reason code,
  and sorted role/grant ID changes. It excludes credential/API-key/password/
  JWT/recovery-token bytes; hashes, salts, pepper IDs, KDF parameters, key
  fingerprints/prefixes, signing/private/public key material, session cookies,
  and free-form provider responses. Unknown metadata is dropped, never logged
  by default. Golden before/after vectors for password, API key, JWT/signing
  key, OAuth/provider credential, and recovery token prove excluded byte
  substrings never enter audit or error/log output.
- Auth audit is a separate typed tenant substrate, never fanned out into each
  database audit log. One auth mutation transaction locks the tenant's
  `auth_audit_allocator`, assigns contiguous `auth_audit_id` plus transaction
  index/size, and atomically commits credential/grant/role state, one
  `auth_epoch` increment, and redacted rows. It has an independent order from
  tenant/database `audit_id`; no combined cursor or ordering comparison exists.
  Only the tenant-admin auth-audit query can read it after policy checks. It is
  excluded from database change streams, graph subscriptions, CDC, and local
  replicas; those observe the epoch mismatch and revoke without an auth event.
  Memory provides process-lifetime co-commit; SQLite/PostgreSQL persist the auth
  catalog, allocator, and audit. Backup/restore signatures and migrations cover
  all three. Fault/concurrency tests cover every auth mutation, tenant-wide
  session invalidation, contiguous framing, independent simultaneous database writes,
  restart, and neither-without-the-other visibility.
- Every live auth-state mutation targets exactly one tenant and therefore one
  auth epoch/allocator/audit transaction. Cross-tenant credential/grant/role
  batch mutation does not exist in `TARGET_RELEASE`. During legacy migration only, an
  unscoped revocation is conservatively materialized as one deterministic
  `legacy_auth_scope_invalidation` transaction per enumerated tenant, ordered
  by tenant ID and journaled per item. The store-wide activation marker keeps
  all runtime access disabled until every tenant item verifies; crash/retry
  never duplicates its epoch increment or audit. Thus no public operation
  attempts a partially atomic multi-tenant auth commit.
- Idempotency has two classes. An `in_progress` reservation is durable but may
  be created before the governed transaction. It stores the complete namespace
  and request binding below plus `state`, a random 128-bit `owner_token`, a
  monotonically increasing `fence`, `lease_expires_at`, and `updated_at`.
  Acquisition is one compare-and-swap: absent creates fence 1 with a 30-second
  lease; an unexpired same-request row returns 409 `idempotency_in_progress`
  with `retry_after_ms`; an expired same-request row may be taken over by
  replacing the owner, incrementing the fence, and starting a new lease. A
  different request hash always returns `idempotency_key_conflict`, including
  while the row is in progress. The owner heartbeats every 10 seconds by CAS
  on owner+fence; failed heartbeat/fencing aborts its open governed transaction.
  Lease time is evaluated inside the reservation CAS, never from a caller-
  supplied timestamp. PostgreSQL uses `clock_timestamp()` from the database
  server. SQLite maintains one transactional `idempotency_lease_clock.last_ms`:
  `effective_now=max(os_utc_ms,last_ms+1)` is stored with each acquire/
  heartbeat; concurrent processes serialize on that row. If process start sees
  OS time behind `last_ms`, takeover is forbidden during 30 seconds of
  continuous local monotonic uptime, then one CAS may advance effective time to
  at least `last_ms+30001`; restart resets that grace rather than shortening it.
  Memory uses process-monotonic `Instant` and makes no restart claim. Lease rows
  store authoritative effective issue/heartbeat/expiry milliseconds plus clock
  source version. `retry_after_ms` derives from the same source. Multi-process,
  DB/client skew, backward/forward wall jumps, restart-before/after grace, and
  clock-row contention tests prove no takeover earlier than the durable
  30-second lease; forward jumps may conservatively fence the old owner but the
  commit fence still prevents duplicate success.
  A successful attempt transitions that exact owner+fence to `committed` in the
  same transaction as data and audit. The committed row stores
  `IdempotencyOutcomeV1`: contract version, public `surface_id`, operation kind,
  logical status, the exact AXON-CJSON-1 bytes of a transport-neutral
  `MutationOutcomeV1`, SHA-256 of those bytes, the committed audit transaction
  pointer, and a sorted allowlist of replayable semantic response metadata.
  Date, request ID, tracing, connection, and authentication headers are never
  stored. A same-surface replay verifies the bytes/hash and deterministically
  rerenders the original surface status/body/details; it may add only
  `idempotent_replay=true`. Because `surface_id` is request-hash-bound, using
  the key through another surface is a conflict rather than a cross-surface
  replay. No replay rereads mutable entity state. The transaction locks
  and rechecks the reservation immediately before commit; a stale owner rolls
  everything back. Only a committed row can serve replay. Denial or ordinary
  execution failure deletes the reservation only by matching owner+fence; a
  crash leaves it for lease takeover. Denials are never cached in `TARGET_RELEASE` and a
  retry reevaluates policy. Memory, SQLite, and PostgreSQL implement the same
  state machine; restart, clock-skew, heartbeat loss, stale-owner, and two-owner
  takeover tests prove one fenced outcome and no duplicate committed mutation.
  Phase 1 amends CONTRACT-001 to remove terminal-denial caching and freeze this
  protocol. Fault tests cover crashes before reservation, after reservation,
  after data/audit, and before outcome, proving no duplicate execution or false
  success.
  Namespace is `(tenant_id,database_id,principal_id,idempotency_key)`. The
  request binding is SHA-256 over AXON-CJSON-1 `IdempotencyRequestV1` with
  common fields format version, public `surface_id`, operation kind, isolation
  tier, user audit event, and response projection/return mode. For data
  mutation, its body is an ordered list of one or more of the six normalized
  operation variants with qualified targets, complete payload/patch/link
  metadata, expected-version/OCC preconditions, and force/cascade flags; a
  single operation is a one-element list. For the dedicated
  `schema_policy_activation` kind, its body is exactly the Phase 5 activation
  input and has no operation list. No other kind is valid in `TARGET_RELEASE`. Absent
  optional values are explicit nulls. Tenant/database/principal are supplied by
  the namespace and are not duplicated in the hash. Transport encoding,
  argument aliases, map insertion order, idempotency key, bearer credential,
  request/trace IDs, deadlines, retries, and non-semantic headers are excluded;
  a semantic conditional or response option cannot be excluded. The row also
  binds policy epoch/hash and tenant auth epoch outside the request hash. Phase
  1 publishes field-level golden vectors for all six variants, mixed/single-op
  forms, schema-policy activation, and every public surface; all adapters must construct this one core
  value before reserving. Same key with different hash returns 409
  `idempotency_key_conflict` and never the old response. Replay rechecks the
  same principal/scope and current authorization; changed policy/auth returns
  `idempotency_scope_changed` rather than leaking a cached response. Phase 1
  explicitly amends CONTRACT-001's prior same-key/different-payload behavior.
- Forward/reverse/index maintenance, `_cdc_cursors`, in-progress idempotency
  reservations, and
  in-memory projections are derived/checkpoint state reachable only from
  allowlisted internal calls. One logical link audit covers its physical rows.

## 2A. Implement migration and security prerequisites

Before Phase 3 mutates or classifies any store, implement and backend-test the
typed `policy_catalog`/`policy_epoch`, tenant `auth_epoch` and auth-audit
allocator, AXON-CJSON-1 plus policy/schema hash calculators, typed `LinkKey`,
physical table/DML manifest, migration journal/gate/anchor, maintenance-
generation primitive, and `BackupVerificationSignature` codecs. This phase
provides owner-local initialization/migration primitives and open-time fail-
closed checks only; Phase 4 later wires schema/policy lifecycle behavior and
all public surfaces. Phase 3 cannot start on stubs, mock hashes, or a backend
missing these persistence primitives. Cross-backend golden vectors and crash/
open tests are the exit gate.

## 3. Provide an exclusive, one-time legacy upgrade

Represent discovered legacy data as `LegacyUnbound`, but never expose it through
a persistent runtime. The state matrix is exact: gate `migration_required` makes
normal embedded `AxonBuilder::build`, server, worker, and read-only open fail at
store open with `StoreMigrationRequired`/503 `migration_required`; no network
listener starts and no collection-level request executes. Only local database-owner commands
`axon migrate schema-bindings --dry-run` and `axon migrate export` use the
tolerant reader, with no network listener. A `ready` store must have zero
`LegacyUnbound` registrations; the readiness scan/open fails
`storage_invariant_violation` if one appears through corruption/import. The 409
`schema_required` diagnostic is migration-tool/test-fixture-only and is removed
from the ordinary persistent public contract in the Phase 1 amendments.

Public validation order is lexical reserved-name guard before any catalog or
legacy lookup. Therefore generic access to a reserved name always returns the
Phase 2 `reserved_namespace` error, whether that name is absent, internal, or a
legacy user collection; it never reveals legacy existence. `schema_required`
is never returned by a persistent public runtime. The owner-local dry-run,
behind exclusive local ownership and
with no network listener, is the only path that reports
`legacy_reserved_name` and existence needed to rename/export. Golden tests
cover absent, internal, and legacy-reserved names on every public surface plus
the distinct local diagnostic.

The mapping must cover every discovered legacy public collection and link
type. Dry-run enumerates all unmapped discoveries and fails nonzero; apply
repeats discovery and refuses to change any catalog if even one collection or
link type is unmapped. Nothing becomes inaccessible through partial mapping.
Dry-run validates all mapped entities, targets, metadata, cardinality, and
required links and records catalog epoch/content signature. Apply requires a verified backup,
revalidates under lock, installs one database atomically, and journals an
idempotent mapping hash. It never invents schema, deletes, or quarantines data.

If a legacy user collection uses a newly reserved/system name, dry-run fails
with `legacy_reserved_name` and requires either owner-local export/abort or an
explicit `rename_to` valid public name in the mapping. Under the same backup,
maintenance generation, and atomic apply, rename moves registration, schemas,
entities, and all link endpoints/physical keys; immutable historical audit
keeps the old name and a migration audit records old→new. Collision or missing
rename aborts everything; no reserved legacy data becomes silently hidden.

The migration journal also stores a signed immutable audit subject alias
`old_reserved_name -> new_public_name` at the migration boundary. Stored
pre-migration audit bytes remain unchanged, but authorized generic audit/
history lookup by the new name projects matching old events as the new logical
subject, applies current policy/redaction, and marks only
`historical_name_migrated=true`; it never returns the old reserved string.
Direct filtering/lookup by the old name still fails the lexical
`reserved_namespace` guard. Database-admin `query_system_audit` may inspect the
actual alias and migration event as an intentional admin disclosure. Generic
rollback preview/commit of any pre-rename event fails unchanged with
`pre_migration_rollback_unsupported`; operators use verified export/manual
forward mutation instead. Transaction-ID and numeric-audit-ID application views
apply the same alias or omit unauthorized events, so no alternate path leaks the
old name. Golden surface tests cover history pagination across the boundary,
direct old/new filters, ID lookup, admin inspection, and rollback refusal.

The same atomic apply rewrites every legacy slash-delimited forward/reverse
link key to typed `LinkKey`, verifies endpoint equivalence and that no old key
remains, and rolls schema binding/rename/key rewrite back together on failure.

A separate mandatory storage migration covers every already schema-bound
database that predates `policy_catalog`, typed `LinkKey`, or both. It is one
store-wide, exclusive,
journaled migration unit, not independent best-effort database upgrades. Its
durable journal stores generation, full enumerated tenant/database manifest
and hash, phase (`migration_required`, `prepared`, `applying`,
`failed_retryable`, `restore_required`, `verified`, `activating`, `ready`, or
`aborted_restored`), and per-item checksums. Mirror
the generation/manifest/phase in the checksummed owner-local evidence bundle so
a store restore cannot erase the operator's recovery record. Under the
maintenance-generation protocol, derive AXON-POLICY-HASH-1
from each active normalized policy. Build the authoritative tenant manifest as
the sorted union of tenant registry, tenant-qualified databases/catalogs/
schemas/data/links/database audit, policy catalog, users/credentials/grants/
roles/revocations/auth audit, idempotency, resume/snapshot sessions, CDC/
checkpoint state, and migration journal. Every discovered tenant ID must resolve
to exactly one durable tenant registry record; missing/conflicting ownership
fails pre-write as `orphan_tenant_state` with the tables/keys listed for explicit
owner repair/export. A registry-only tenant is still included. The manifest and
per-source membership hashes enter `BackupVerificationSignature` and crash/
backend tests seed tenants visible in each source alone.

Dry-run separately enumerates every credential, grant, and role record lacking
tenant scope and emits only type, stable record ID, dependency IDs, and checksum
(never credential secret/hash material). Apply requires a signed owner mapping
that assigns each such record to exactly one existing tenant in the manifest;
credential principal, grant subject/resource, and role dependencies must all
resolve within that same tenant or the whole migration aborts
`legacy_auth_scope_conflict`. Unmapped records require owner-local verified
export and abort of the migration—there is no infer, delete, quarantine, or
multi-tenant copy option. Apply journals each mapping idempotently and emits one
redacted tenant-auth migration audit. Only an unscoped revocation uses the
explicit conservative per-tenant invalidation rule in Phase 2; it is never
generalized to credentials, grants, or roles. Crash/retry and mixed-dependency
fixtures cover every record class.

Derive each validated tenant auth catalog from durable grants/credentials/
revocations. For a missing catalog, idempotently create each database
`policy_epoch=0` and each tenant in this manifest `auth_epoch=1`; for an
existing catalog, verify its normalized hash/epochs and retain those epochs
rather than reset them. In the same per-
database journal item, rewrite every slash-delimited forward/reverse link key
in an already schema-bound store to typed `LinkKey`, verify endpoint and
metadata equivalence, rebuild affected prefixes/indexes, and prove no old key
remains. This key rewrite uses the same verified backup, exclusive maintenance
generation, manifest checksum, idempotent resume, and rollback rules as legacy
schema binding; it is mandatory even when `policy_catalog` already exists.
Epoch 1 is the single intentional invalidation barrier for legacy tenants that
lacked an auth catalog; retries never increment it. A store that already had a
valid auth catalog uses its retained epoch as the migration token binding.
Use one storage transaction where the adapter can span the catalog; otherwise
the journal resumes only the identical manifest and checksums. Network/runtime
open remains disabled for every non-`ready` phase, so partial rows are
never observable as a mixed store. After verifying the full enumeration and
row checksums, atomically write the global activation marker; only that marker
enables `policy_catalog_missing` fail-closed runtime behavior. Crash/retry at
every journal transition proves no partial guard, duplicate epoch bump, or
mixed migrated/unmigrated service; manifest drift requires restore/restart,
not a merged retry.

Existing credentials remain governed by durable state and may reauthenticate.
For a tenant that lacked an auth catalog, every pre-migration bearer/JWT/session
token lacks the epoch binding and fails closed as `reauthentication_required`
after activation. Clear that tenant's pre-migration authenticated server
sessions and resume sessions. Independently, exclusive migration enumerates and
classifies **every** `in_progress` idempotency row in the store for all tenants,
regardless of whether its auth catalog was already valid. It may journal-clear a
row only when durable V1 request binding plus audit transaction markers prove no
matching governed data/audit transaction committed. If that proof is absent—or
data/audit may have committed before the legacy outcome write—the namespace is
converted to immutable `legacy_non_replayable` with reason
`indeterminate_in_progress`; it returns 409 `idempotency_outcome_unavailable`
and requires a new key, never reexecutes. No worker can own a lease while the
migration lock is held. A row is retained in progress only if it has every V1
request/owner/fence/lease/policy/auth field and is proven post-activation—which
no pre-target-release row may claim. Crash/retry uses per-row checksums and never
clears committed state or an indeterminate tombstone. A tenant
with a previously valid catalog retains only tokens/sessions whose stored epoch
still matches. Any legacy committed idempotency row lacking verified
`IdempotencyRequestV1`, policy epoch/hash, tenant auth epoch, or
`IdempotencyOutcomeV1` is converted to an immutable
`legacy_non_replayable` tombstone under its original namespace; it never returns
cached data and never permits reexecution with that key. Authenticated reuse
returns 409 `idempotency_outcome_unavailable` with action `use_new_key`, even
when the presented request appears equal. Only a fully verifiable row with all
bindings may be retained as committed, and migration golden vectors prove each
binding and outcome hash before doing so. No inferred/backfilled authorization
binding is permitted.
Every newly minted token binds the tenant's current durable `auth_epoch`:
missing-catalog tenants start at 1 after migration, existing catalogs retain
their verified value, and tenants created after migration start at 0. Tests
present old/new tokens at embedded, HTTP, gRPC, GraphQL, MCP, CLI/SDK-backed,
stream, subscription, and replay paths before/after crash-resumed migration.
Do not invent empty policy defaults or enable the guard before backfill
activation completes.

Failure recovery is normative. A process/connection failure with no checksum
mismatch writes or is recovered as `failed_retryable`; the only forward action
is `axon migrate schema-bindings --resume <generation>`, which requires the
identical locked manifest and idempotently rechecks every completed item. A
content/checksum/verification mismatch after any item commit writes
`restore_required`; resume and runtime open are refused. The only action is
`axon migrate restore --generation <generation>` using that generation's
verified backup: restore into an isolated target, prove the pre-migration root
signature, then use SQLite atomic file replacement or PostgreSQL's recoverable
quarantine/restore database-rename state machine above under the exclusive
lock, and
write matching `aborted_restored` evidence to the restored journal and external
bundle. A new attempt uses a new generation. `--abort` without restore is
allowed only from `prepared` after proving zero item writes. A crash around
activation is resolved solely by the atomic global activation marker: marker
present means verify and finish the external/in-store `ready` transition;
absent means remain in maintenance
and resume verification/activation. Fault tests at every item/journal/restore/
activation boundary prove no runtime opens a partial store, no duplicate epoch
bump or key rewrite, no stranded maintenance generation after successful
restore, and no cleanup removes the only verified backup before terminal state.

The external record is an authoritative open gate, not evidence only.
`MigrationGateV1` stores store UUID, generation, manifest/root hashes, phase,
active database/file identity, PostgreSQL original/restore/quarantine names,
and last atomic transition. SQLite keeps it in an fsynced sidecar outside the
replaceable database file; PostgreSQL keeps it in a required control database/
DSN that is not the source or either rename target. Every persistent adapter
open, including read-only/server/worker/backup open, must read this configured
gate before the data store. The sole exception is one-time owner-local
`axon migrate gate init`, a distinct bootstrap mode that never starts a network
listener or constructs a normal application adapter. It takes the SQLite
exclusive file lock. For PostgreSQL it opens one privileged source connection,
then from the administration database records current CONNECT ACL/
`ALLOW_CONNECTIONS`, revokes CONNECT, sets `ALLOW_CONNECTIONS=false`, terminates
every other backend, and holds the administration advisory lock; concurrent
connection attempts must fail until terminalization. It uses the retained
privileged migration connection and a minimal migration-only reader to
read storage version/identity and compute the locked root signature. It then
generates the store UUID, writes+fsyncs external phase `initializing`, writes the
matching generation-0 anchor in one database transaction, verifies both, and
atomically advances the external gate to `ready` only if storage version,
required-migration manifest, root signature, policy/auth catalogs, typed-link
key scan, and global activation marker already prove the current finish-line-A
storage contract; otherwise it advances to owner-local-only
`migration_required`. This bootstrap reader may inspect every manifest-covered
table/index and application row read-only under the exclusive lock to compute
the full root and readiness proof; it exposes none through a public API and may
mutate only the anchor. It cannot initialize an already gated/mismatched store.
Only after external+anchor terminalize does the administration connection
restore the exact prior ACL/`ALLOW_CONNECTIONS`; crash recovery repeats the
block/terminate step before inspecting or changing either state. Integration
tests race normal and privileged-configured application connections throughout
signature/anchor computation and prove none enter.

Bootstrap crash recovery is exact: external `initializing` without an anchor
reruns with its stored UUID/signature; matching anchor plus `initializing`
repeats the same readiness classification and terminalizes `ready` or
`migration_required`; any mismatch refuses. An anchor whose external
gate is lost is never trusted or recreated automatically—owner recovery requires
the checksummed gate/evidence artifact through `axon migrate gate recover`.
No server/runtime open has an auto-init flag. `TARGET_RELEASE` runtime refuses a persistent
store with missing/unreachable gate or anchor as `migration_gate_missing`.

Brand-new persistent storage has a separate explicit owner flow:
`axon init store --tenant <id> --database <id> --admin-principal <id>` or the
embedded equivalent `AxonBuilder::init_store(OwnerInitCapability,
FreshStoreSpec)`. Normal `build()` never initializes. Init requires a nonexistent
SQLite target or nonexistent PostgreSQL database plus the same exclusive/admin
privilege preflight, starts no listener, writes external `initializing`, and
builds a temporary store with the current physical schema and typed namespace
manifest. In one initialization transaction it creates the store UUID/anchor,
tenant and database catalogs, governed-system schemas, empty public structural
set and the resulting complete structural catalog/hash, normalized default/
empty database policy at epoch 0, allocators,
migration journal, bootstrap database-audit group, admin credential/grant with
tenant auth epoch 1 and tenant-auth audit, and global activation marker. The
one-time credential secret is written before commit only to a mode-0600,
fsynced owner recovery artifact containing init UUID, target/spec checksum,
secret generation, secret, hash, and subphase; the store receives UUID+
generation+hash, never plaintext. A retry for the same UUID/spec must reuse that
exact artifact/secret and cannot mint a second credential. Mismatch refuses. An
explicit precommit abort records the generation revoked, unlinks+directory-
fsyncs the artifact, and no store accepting its hash exists. After ready,
`axon init credential consume` atomically reads once to the owner, marks
consumed, and unlinks+fsyncs; loss before consumption uses the same artifact,
while loss after consumption requires governed credential rotation.

Init verifies the finish-line-A root/readiness manifest, then atomically renames
the SQLite temp file or completes the PostgreSQL new-database identity, writes
matching in-store `ready`, and fsyncs external `ready`. Crash recovery uses the
same UUID and init subphases: before store commit delete/rebuild temp; after
commit but before promotion verify/promote it. Rebuild reuses the UUID-bound
credential artifact and hash above; it never regenerates. After promotion but before gate
terminalization verify and mark ready. Existing target, mismatched temp/gate,
missing recovery artifact, or partial verification fails closed and never
chooses a store. Fresh SQLite/PostgreSQL tests inject every boundary and prove
one usable ready store, one bootstrap credential, exact initial epochs/hashes/
audit, and no server auto-init.

Runtime opens only when external and in-store UUID/generation/root/active-store
identity match, both phases are `ready`, the storage version is current, every
required migration is recorded verified, and the matching global activation
marker is present. `migration_required` and `aborted_restored` are owner-local-
only; after restore, recovery reruns readiness classification and transitions
to `ready` or `migration_required`. Any non-ready phase, missing side, phase or
identity mismatch, unexpected PostgreSQL database-name combination, or an
external `ready` record paired with an unactivated restored store fails
closed as `migration_recovery_required`. The owner-local resume/restore command
is the only writer and updates+fsyncs the external gate before each destructive
database transition, then the in-store journal, then the terminal external
state. A restored pre-migration journal therefore cannot erase the gate. Crash
matrix tests delete/stale each side and stop before/after both PG renames,
SQLite replacement, activation, and terminalization; server open must either
select the one verified active store or refuse, never infer.

“Verified backup” means a proved restore, not file existence:

- After entering maintenance and before apply, SQLite uses its backup API to
  create the artifact; restore it to a separate temporary file, run
  `PRAGMA integrity_check`, open it through the adapter, and compare the
  catalog/content signature and row counts to the locked dry-run.
- PostgreSQL uses a custom-format `pg_dump`; restore with
  `pg_restore --exit-on-error` into an isolated verification database, open it
  through the adapter, and compare catalog/content signature and row counts.
  PostgreSQL apply is supported in `TARGET_RELEASE` only when a preflight administration
  connection can create databases, owns the source, terminate its sessions,
  and rename both source and restored databases. The preflight actually creates,
  renames, and drops a disposable database before maintenance; checking role
  flags is insufficient. On rollback, restore into `<source>__restore_<gen>`,
  verify it, terminate source connections from the administration database,
  rename source to `<source>__quarantine_<gen>`, then rename the verified restore
  to the original source name. Each rename is atomic; the external generation
  manifest makes a crash between them recoverable only through the closed
  transition table below; recovery never guesses or selects quarantine as
  active. Verify the original-name store
  before deleting quarantine. `--verify-dsn` may support dry-run backup proof
  on managed PostgreSQL but never authorizes apply: without the full tested
  create/terminate/rename capability, apply fails before maintenance or any
  mutation with `migration_restore_capability_missing`. Managed operators must
  export/migrate through a separately provisioned qualified store. Pin
  compatible pg_dump/pg_restore major versions to PostgreSQL 16.
- The verified backup is one `AxonRestoreBundleV1`, not a database artifact
  alone. Under the same quiescent generation it includes the SQLite external
  gate sidecar or a custom-format dump of the PostgreSQL control-DB gate row,
  its in-store anchor, owner evidence manifest, data backup, and a root manifest
  hashing every component/UUID/generation/phase. Verification restores data and
  gate into isolated locations and proves cross-binding; a missing/divergent
  external component fails the backup before apply.
- Record tool/database versions, command, artifact checksum, restore target,
  mapping hash, epoch, signatures, timestamp, and successful cleanup in the
  migration journal/evidence bundle.
- Apply refuses to mutate if restore verification is missing, mismatched, or
  older than the locked epoch. Memory is ephemeral and has no backup/apply
  migration claim.

PostgreSQL rename recovery uses this normative table. `O` is the configured
original database name, `R` the generation restore name, and `Q` quarantine.
The external gate writes+fsyncs the listed intent before each rename. “Partial”
means the failed/migrating source journal; “backup” means the verified
pre-migration root signature. No row below permits runtime service until the
last row says `ready`.

| External phase / last transition | Required in-store evidence | Observed names | Only legal next action | Runtime-active store |
|---|---|---|---|---|
| `restore_required` / `restore_building` | O has partial generation; verified backup artifact exists | O only, or O+incomplete R | Drop/recreate R, restore backup, verify R root, write `restore_verified` | none |
| `restore_verified` / `source_to_quarantine_pending` | O partial; R root equals backup; Q absent | O+R | Rename O→Q; fsync control record as `source_quarantined` | none |
| `source_to_quarantine_pending` after crash | Q partial; R root equals backup | Q+R, O absent | Record completed rename as `source_quarantined`; do not reverse | none |
| `source_quarantined` | Q partial; verified backup artifact exists | Q only | Recreate/verify R from the same backup, remaining `source_quarantined` | none |
| `source_quarantined` / `promote_restore_pending` | Q partial; R root equals backup | Q+R, O absent | Rename R→O; fsync `restore_promoted` | none |
| `promote_restore_pending` after crash | Q partial; O root equals backup | O+Q, R absent | Record completed rename as `restore_promoted` | none |
| `restore_promoted` | O root must equal backup; Q partial | O+Q | Reverify O, write matching restored anchor/journal, then `restored_verified`; mismatch fails closed and never promotes Q | none |
| `restored_verified` | O root/anchor verified; Q partial | O+Q | Classify O as `ready` or `migration_required`, write both terminal gates/evidence, then and only then delete Q after retention/evidence policy | O only if `ready` |
| terminal `ready` or `migration_required` | O matches terminal root/anchor | O, optionally Q awaiting cleanup | Open O only for `ready`; for either state, idempotently clean verified Q after evidence | O only if `ready` |

Any other tuple—including O+R+Q, R alone, no databases, a signature mismatch,
or a name not recorded for the generation—returns
`migration_recovery_required`, leaves all databases unavailable, and requires
owner recovery from the verified backup/evidence bundle. Quarantine is never
renamed back to O by automatic recovery. Fault tests stop before/after each gate
fsync and rename and assert this exact table.

`axon migrate gate recover --bundle <AxonRestoreBundleV1>` is the only missing-
gate recovery path. It takes the admin/exclusive lock, verifies the bundle root
and restored data anchor, writes an external `recovering` record for that exact
UUID/generation, then terminalizes `aborted_restored` and reruns readiness to
`ready` or `migration_required`; it never copies a stale `ready` phase directly.
If a newer external generation exists, or control DB/sidecar and restored anchor
disagree, recovery refuses. SQLite and PostgreSQL tests delete both live gate
forms, restore the bundle, and prove identical terminalization/open behavior.

`BackupVerificationSignature` is versioned by Axon storage-schema version and
finish line. Each storage migration declares its required table set; absent
future tables are explicitly `not_applicable`, never silently omitted. The
signature root includes the external gate/control component and owner evidence
manifest defined above, not only adapter tables. The
legacy-upgrade signature covers the source version; the finish-line-A manifest
includes policy/auth state and the complete public-stream `resume_sessions`
row: current/pending handle hashes, tenant/database/principal/grant,
surface+query/collection scope, current/pending schema lineage, policy epoch/
hash, tenant auth epoch, current/pending source boundaries, exact encoded
pending delivery plus digest, ACK state, issued/updated-at, and expiry. Finish
line B does not add or reinterpret `resume_sessions` columns; it adds only
separate snapshot-session/spool metadata and client-local replica lease/state,
and bumps the signature version. Each manifest
covers every persistent
adapter-owned table required for semantic restore: catalogs and namespaces,
all schema versions, entities/system metadata, forward/reverse links,
secondary/compound indexes, database and tenant-auth audit/allocators/
transaction framing, policy catalog and epochs/hashes, users/grants/credential
issuance+revocation, cursor
checkpoints, replica handles, idempotency state, and migration journal. Each
table contributes schema version, row count, and `AXON-BACKUP-ROW-1` hash to the
root. Its manifest declares a non-null unique stable key tuple for every table,
including tables without a SQL primary key; absent/nullable/non-unique or
collation-dependent keying is a verification failure until a surrogate stable
key migration exists. Rows sort lexicographically by the binary encoding of
that tuple, never database collation.

`AXON-BACKUP-ROW-1` encodes a table header (manifest version, ordered column
names and logical type tags), then length-prefixed rows and columns in manifest
ordinal order. Each value begins null/present plus type tag: signed/unsigned
integers are fixed-width big-endian; bool is 0/1; UTF-8 text is exact stored
bytes without normalization; blob is exact bytes; UUID is 16 bytes; timestamp
is signed UTC microseconds from Unix epoch; decimal is sign plus minimal
big-endian coefficient and signed scale; finite float is IEEE-754 big-endian
with `-0` normalized to `+0` and non-finite governed values rejected; JSON is
duplicate-free AXON-CJSON-1 bytes. Lengths are unsigned big-endian u64. Unknown
SQL affinity/coercion, invalid UTF-8, duplicate encoded key, column drift, or
unsupported type fails verification. SQLite/PostgreSQL vectors cover NULL,
empty/text Unicode, blobs with NUL, numeric boundaries, decimals, timestamps,
JSON, collation inversions, and a no-PK fixture. Explicit exclusions are only reconstructible
caches, connection/session state, expired snapshot spools, and process-memory
projections; rebuild/compare reconstructible indexes before acceptance.
SQLite/PostgreSQL golden restores prove omitting/changing an included table
fails verification.

Enforced quiescence:

- Memory: exclusive in-process migration guard (ephemeral/test only).
- SQLite: exclusive database transaction.
- PostgreSQL 16: cooperative database advisory lock plus the real safety
  boundary—one typed `store_generation` row
  `{store_uuid,generation,maintenance,writer_fence}` changed by every governed
  commit. Raw SQL writers are outside Axon's contract.
- A writer that does not request the advisory lock but uses the governed path
  must still fail on the epoch. Apply rejects changed epoch/signature or
  concurrent content, with no partial catalog.

The ordering is normative:

1. Every governed writer takes a shared generation lock and records epoch E.
   Immediately before audit allocation/commit, PostgreSQL executes a write-
   conflicting CAS:
   `UPDATE store_generation SET writer_fence=writer_fence+1 WHERE store_uuid=?
   AND generation=E AND maintenance=false RETURNING generation`. SQLite/memory
   perform the equivalent under their commit critical section. Zero rows or a
   stale-snapshot SQLSTATE `40001` cannot pass as a read of E.
2. Migration takes the exclusive generation lock (waiting for shared holders),
   locks/updates that row, atomically sets maintenance and increments E→E+1,
   commits the marker, and retains the session advisory lock. New writers fail;
   any deliberately non-cooperative governed writer that skipped the advisory
   lock still conflicts on the CAS: a pre-marker `REPEATABLE READ` snapshot gets
   `40001`, while a post-marker snapshot matches zero rows. On `40001` the
   handler rereads maintenance outside the aborted transaction; true maps to
   503 `migration_in_progress`, false uses the ordinary bounded transaction
   retry. Raw SQL is outside Axon's contract.
3. With all pre-E writers drained, migration captures the content signature,
   creates/restores/verifies backup, revalidates, and installs the full mapping
   under the same exclusive maintenance generation.
4. It commits the catalog, increments generation again, clears maintenance, and
   releases the exclusive lock. Only then may new writers start.

Test dry-run/apply/retry/rollback, a writer between dry-run/apply, a writer
during apply, and on PostgreSQL 16 a deliberately governed writer that skips
the advisory acquisition but reaches the CAS from a pre-marker
`REPEATABLE READ` snapshot. It must observe real `40001` then
`migration_in_progress`, with no data/audit commit. Also test backup restore and
proof that tolerant reads are unreachable from all public surfaces.

## 4. Make schemas fail closed and evolution complete

1. `create_collection`/`put_schema` reject null/invalid entity schema with 422
   `schema_validation`, reason `entity_schema_required`. This handler-level
   check is the single enforcement choke point for a present wrapper whose
   inner `entity_schema` is null; all surfaces delegate to it.
2. Unknown public collection is 404 `not_found`. A registered collection without
   active structural schema cannot exist in a `ready` persistent store; open
   fails `storage_invariant_violation`. The handler's internal
   `schema_required` branch remains only for memory migration fixtures and is
   compile/test-gated from published persistent surfaces.
3. Every entity/link/transaction mutation validates the latest schema and
   records its version. If activation changes before commit, abort everything
   with retryable 409 `schema_activation_changed` carrying
   `validated_version`/`active_version`. Existing manifest `schema_mismatch`
   retains its separate refresh semantics.
   `schema_mismatch` is raised only by a caller-supplied manifest/hash mismatch
   before staging and tells the caller to refresh. `schema_activation_changed`
   is raised only by commit-time schema-version recheck and tells the caller to
   rebuild/retry the transaction. Manifest validation runs first if both apply.
   Mixed-transaction detail is
   `{schema_changes:[{collection, validated_version, active_version,
   operation_indexes}]}` sorted by collection; link ops list source and target
   schema dependencies. There is no singular-version shortcut.
4. Implement/verify FEAT-017: classification/dry-run; one winner on concurrent
   schema update; explicit audited force; background revalidation; stored
   schema version; lazy reads/warnings; active-schema validation on next write.
   Do not invent V2 transform rules.
5. Extend FEAT-017 to link declarations. Required additions, optional→required,
   cardinality/target/metadata tightening, and removal with existing links are
   breaking; optional additions/loosening are compatible. Dry-run names
   affected entities/links.
6. `LegacyUnbound` is migration-tool-only and never public-readable. Separately,
   a schema-bound row/link made invalid by an explicitly forced FEAT-017 change
   remains live-readable with warning. A repair transaction is allowed only
   when its complete proposed final state is fully valid; bounded partial
   repairs are not supported in `TARGET_RELEASE`.
7. Reject a schema with more than 99 required link types: one entity plus its
   minimum required links cannot fit the 100-op atomic bootstrap. Other
   oversized bootstrap attempts get a stable diagnostic; no non-atomic
   exception exists.
8. Remove public schemaless creation: CLI collection creation currently permits
   omitted `--schema` and constructs `entity_schema: None`; make schema input
   required and reject null/omitted schema in handler, server, embedded Rust
   API, and TypeScript SDK.
   The existing evolution code's “schemaless→schema is compatible” branch is
   not a FEAT-017 public promise; move it behind the legacy upgrade path so
   FEAT-002's no-schemaless rule governs ordinary writes.
9. Wire the Phase 2A `policy_catalog` primitives into governed lifecycle and
   every public surface: schema access-control activation, mask/write-policy
   changes update the per-database policy epoch/hash; every
   credential/grant/role/revocation change updates each affected tenant auth
   epoch. Queries capture both at start; commit and long-lived streams detect
   change. The policy storage key is `(tenant_id, database_id)`; any active
   collection access-control/mask/write-policy change increments that owning
   database key only. Bearer/JWT/authenticated-session tokens bind only the
   tenant auth epoch. Database-scoped resume/snapshot sessions, graph contexts,
   and idempotency records separately bind both database policy epoch/hash and
   tenant auth epoch (plus schema lineage where specified). No bearer token
   embeds or fans out database policy epochs.

Tests span create/replace/patch/delete, lifecycle, rollback/intents,
transactions, every public surface, schema-change races, forced-state repair,
and all three backends.

## 5. Implement atomic declared links and mixed transactions

Link rules:

- Every link endpoint and every staged operation belongs to the transaction's
  one tenant-qualified database. Cross-tenant/cross-database links and
  transactions fail `cross_database_operation`; no distributed atomicity or
  policy/audit allocator crossing is attempted.
- Source and target are registered with active entity schemas.
- Source schema declares exact type; absent/empty catalogs fail closed.
- Target matches declaration; reverse target declaration is unnecessary.
- Metadata validates against active source declaration.
- `one-to-one` caps source+target; `one-to-many` caps target;
  specifically: one-to-one means source outbound ≤1 and target inbound ≤1;
  one-to-many means source outbound unlimited and target inbound ≤1;
  many-to-one means source outbound ≤1 and target inbound unlimited;
  many-to-many leaves both unlimited.
- Each surviving source with `required: true` has at least one outgoing link in
  the transaction's final state.

A required-link target may already exist or be created anywhere in the same
transaction; validation is order-independent and requires existence only in
the proposed final overlay. The 99-type
schema bound assumes reusable/pre-existing targets and is the absolute minimum
operation bound (source entity + one link per required type). When targets or
their own required graph are co-created, preflight counts every entity and
link exactly; any concrete bootstrap over 100 operations fails with
`required_link_bootstrap_exceeds_transaction_limit`. The schema remains usable
with pre-existing targets, so no incorrect lower global type cap is inferred.

Use one backend-neutral overlay evaluator: load affected snapshot entities and
inbound/outbound links; apply all staged operations in memory; validate
endpoints, schema/catalog epochs, type/target/metadata, duplicates,
cardinality, and required links; then pass the valid plan to one adapter
transaction. Commit rechecks OCC/epochs and co-commits data+audit. Single
mutations use a one-operation overlay. Decompose work into evaluator, storage
primitive, backend implementations, surfaces, and fault/concurrency suites.

`LinkReadSetV1` makes that validation safe under Snapshot Isolation. It records
typed length-prefixed keys and observed versions for: source and target entity
existence/version; source declaration version/hash; exact logical link
presence; every affected source/type outgoing cardinality set; every affected
target/declaration incoming cardinality set; every surviving source/required-
type set; and full inbound/outbound adjacency sets used by entity delete/
cascade. Each set has a durable manifest-classified `link_set_version` OCC row.
`LinkSetKeyV1` has a common length-prefixed tenant/database prefix and one of
these closed binary variants:

- `0x01 OutgoingCardinality(source_collection,source_entity,
  declaration_id,declaration_version_hash)`;
- `0x02 IncomingCardinality(source_collection,link_type,target_collection,
  target_entity,declaration_version_hash)`;
- `0x03 Required(source_collection,source_entity,declaration_id,
  declaration_version_hash)`;
- `0x04 OutboundAdjacency(source_collection,source_entity)`;
- `0x05 InboundAdjacency(target_collection,target_entity)`.

Each string/ID is UTF-8 byte-length-prefixed, hashes are fixed 32 bytes, variant
tag precedes fields, and unsigned lengths use big-endian u32; ordering is raw
encoded-byte ordering. Golden delimiter/Unicode/asymmetric one-to-one/one-to-
many/many-to-one vectors are shared by all backends. Exact link presence remains
the separate typed `LinkKey`; entity and declaration versions remain their own
read-set entries. Every link
create/delete/cascade and declaration activation CAS-increments all applicable
set rows in canonical key order in the same transaction. Memory uses identical
versioned map entries; SQLite/PostgreSQL use rows.

An absent set is recorded explicitly as `Absent`, not numeric version zero. At
commit, `Absent` executes `INSERT(key,version=1) ON CONFLICT DO NOTHING RETURNING
version`; one returned row wins, zero rows is retryable `transaction_conflict`.
An observed `Present(v)` executes `UPDATE ... SET version=v+1 WHERE key=? AND
version=v RETURNING version`; zero rows conflicts. PostgreSQL concurrent inserts
block at the unique key then yield one winner; SQLite performs this under its
write transaction; memory uses an atomic vacant-entry operation. A transaction
bumps each de-duplicated key once regardless of its internal link count. Guard
rows are never deleted or reset during ordinary entity/link/declaration
lifecycle, preventing ABA after delete/recreate; only an offline versioned
migration may compact them after proving no live/readable dependency. Overflow
fails closed as `link_set_version_exhausted`.

Commit compares entity/declaration versions and CASes every recorded set version
before physical link writes. Any changed set aborts unchanged as retryable
`transaction_conflict`, preventing max-N and required-link write skew. Backends
also enforce unique logical `LinkKey` and reverse key constraints; declarations
with max-one add manifest-managed unique constraints for the exact source/type
and/or target/declaration key as defense in depth. Constraints never replace set
versions for max-N, required, or adjacency absence. PostgreSQL 16/SQLite/memory
barrier tests overlap: two creates at the last cardinality slot, create vs
entity delete, delete-last-required vs unrelated update, two endpoint cascades,
and declaration activation vs link mutation. Exactly a valid serial outcome or
stable conflict is allowed; disjoint set keys must still commit concurrently.

The public multi-op transaction accepts exactly CONTRACT-001's six operations:
entity create/update/patch/delete and link create/delete. Schema, template,
lifecycle endpoint, policy, intent, credential, grant, and role mutations are
not stageable/mixable and return `unsupported_transaction_operation`. Their
single-operation handlers may reuse the internal one-operation co-commit
primitive, but that does not add them to the public multi-op grammar.

One dedicated governed admin operation, `activate_schema_policy`, supplies the
atomic path required by Phase 7 without extending that grammar. It is database-
scoped. Its input is exactly `{database, expected_policy_epoch,
expected_policy_hash, expected_schema_catalog_hash, schema_change,
proposed_database_policy, force, dry_run}`,
where `schema_change` is null for policy-only activation or exactly
`{collection,expected_active_schema_version,proposed_schema}` for one collection.
It compiles the proposed full database policy against the complete resulting
schema/catalog view, performs FEAT-017 compatibility/revalidation when a schema
change is present, and checks the expected policy values, whole pre-change
`AXON-SCHEMA-CATALOG-HASH-1`, plus that collection's expected version under the
database mutation transaction and again immediately before commit. Policy-only
activation therefore fails on any intervening compatible or incompatible
schema change. Dry-run returns the
deterministic diff and affected counts with no reservation/write/audit. An
idempotency key on `dry_run=true` is rejected before lookup as 400
`idempotency_not_supported_for_dry_run`; it is never ignored or consumed, so a
later apply may use that key normally. Cross-surface vectors cover dry-run with/
without a key followed by apply with the same key. Apply
atomically installs each changed half. A changed normalized policy increments
policy epoch exactly once. Audit is one transaction with a schema-activation
event first if `schema_change` is non-null and different, followed by a policy-
change event if normalized policy bytes differ. Policy-only change therefore
emits only the policy event; compatible schema-only change emits only the schema
event, preserves policy epoch/hash, and returns the new schema-catalog hash.
Schema-catalog mismatch is retryable 409 `schema_catalog_changed`. Apply with neither change returns 409
`activation_no_change` and writes/audits nothing. No reader observes one half.

The public bindings are embedded Rust `activate_schema_policy`, HTTP
`POST /v1/databases/{database}/schema-policy-activations`,
gRPC `ActivateSchemaPolicy`, GraphQL `activateSchemaPolicy`, MCP
`axon_schema_policy_activate`, CLI `axon schema activate --policy`, and the
generated TypeScript method of the same core operation. Apply accepts the
normal idempotency key. `IdempotencyRequestV1` encodes operation kind
`schema_policy_activation` and the exact input object instead of the six-op
list; golden vectors bind every field/surface. OCC/epoch mismatch is retryable
409 `schema_activation_changed`, `schema_catalog_changed`, or
`policy_epoch_changed`; invalid proposed
schema/policy is 422 `schema_validation` or `policy_schema_incompatible`;
unauthorized is the common policy denial. Error/status/details and activation
audit framing have cross-surface golden tests for zero-, one-, and two-event
outcomes. Separate schema-only and policy-only admin handlers delegate to this
database-scoped operation with the unchanged half,
so no competing two-step activation path exists.

`TARGET_RELEASE` has no atomic multi-collection schema activation. A change spanning
collections must be staged so the store is valid after every commit: add or
loosen backward-compatible fields/links collection-by-collection with unchanged
policy; use one `activate_schema_policy` call (with that collection's unchanged
current schema or `schema_change=null`) for the database-wide policy switch; then remove
or tighten now-unreferenced schema elements collection-by-collection. If no
such sequence preserves schema, links, required constraints, and compiled
policy at every step, the migration is unsupported in `TARGET_RELEASE` and dry-run returns
`multi_collection_atomic_activation_unsupported` without writes. Phase 1 docs
and tests include a valid staged example and an impossible rejected example.

V1 logical mutations are create link, delete link, and cascade link deletion.
No in-place update.

Transaction expansion and audit ordering are canonical for every mixed
transaction:

- Preserve caller staged-operation order.
- Reject overlapping staged mutations of the same entity/link with stable
  `duplicate_transaction_target` rather than depending on incidental order.
  This covers two caller-staged explicit mutations of the same target.
- Expand each staged op in place. Entity delete expands to de-duplicated link
  deletions sorted by canonical link key, then the entity event; other V1 ops
  produce one logical event.
- After all per-op expansion, globally de-duplicate link deletions by canonical
  link key. A link joining two deleted entities—or explicitly deleted before
  endpoint deletion—produces one physical deletion and logical audit, owned by
  the earliest staged operation that caused it; later expansions emit nothing.
  Generated cascade overlap with one explicit delete is therefore allowed and
  de-duplicated; it is not an explicit-target duplicate error.
- The fully expanded list receives consecutive audit IDs and zero-based
  `transaction_index` in that order; `transaction_size` is the expanded list
  length and is identical on all rows.
- External policy filtering preserves the relative source order of visible
  events. Define a new `ChangeBatch` contract/type; it may expose zero-based visible indexes but never the
  hidden source count/indexes.

Audit IDs are assigned as one contiguous range at commit. Memory uses its
transaction critical section; SQLite's `BEGIN IMMEDIATE` serializes the
database allocator. PostgreSQL, inside the data transaction immediately before
audit insert, executes `SELECT ... FOR UPDATE` on the one tenant-qualified
database `audit_allocator` row, reserves exactly `transaction_size` IDs,
inserts every framed row, advances the allocator, and commits before releasing.
The allocator acquisition is the commit-order serialization point. Under the
default `REPEATABLE READ`, a concurrent allocator-row change may produce SQLSTATE
`40001` even when application records do not conflict. The governed handler
therefore transparently retries the complete staged transaction, with a fresh
snapshot and revalidation, up to eight attempts using capped exponential jitter
(1, 2, 4, 8, 16, 32, 50 ms); it never retries external callbacks because none
run inside this boundary. The same idempotency owner+fence spans retries, is
heartbeated as needed, and only the successful attempt may commit its outcome.
Exhaustion returns stable `transaction_conflict` with reason
`audit_allocator_retry_exhausted` and attempt count. Serializable requests use
the same outer retry contract for `40001`; unique/deadlock/other errors are not
misclassified. Abort rolls allocator+rows back, so no gap/partial group is
visible. This allocator-row lock is permitted internal commit coordination, not
pessimistic locking of application entities. PostgreSQL 16 concurrency tests
force two and sustained many non-conflicting governed writes to overlap, observe
at least one real allocator `40001`, and prove both eventual commits within the
retry budget, commit-ordered non-interleaved contiguous ranges, stable
exhaustion behavior, and no partial replay.

Entity deletion:

- Collect/de-duplicate inbound+outbound logical links.
- `force=false` + any inbound link rejects everything unchanged.
- `force=false` + no inbound link deletes outbound links/reverse rows then
  entity atomically.
- `force=true` deletes the full union, forward/reverse rows, then entity.
- Validate required-link state for surviving affected sources.
- Audit logical links sorted by canonical link key, then entity, with one
  transaction ID and group framing. No live tombstone.

Before mutation, preflight the fully expanded logical plan—including cascade
deletions—and its canonical pre-image/audit payload. The root entity mutation
plus every expanded link mutation counts toward the 100-operation cap; their
canonical storage payload plus audit before/after payload counts toward a
40,000,000-byte expanded-commit
cap. Over-limit direct or transactional cascades fail unchanged with
`transaction_limit_exceeded` and `{actual_operations, operation_limit,
actual_bytes, byte_limit}`. Operators must delete/rewire links in bounded valid
transactions before retrying; force never bypasses limits or required links.

Pre-serialize the transaction's worst-case unredacted `AXON-CHANGE-1` source
frame—including before/after values, link/schema control events, and fixed
framing overhead—and require ≤32,000,000 bytes before commit. Policy filtering
may only remove/redact fields, never increase this bound. The public
`DeliveryBatch` limit is 40,000,000 bytes, guaranteeing every unsplittable
source transaction plus delivery envelope fits. Failure is
`change_event_limit_exceeded` with measured/limit details and no mutation.

Backend mechanism:

- Memory: one critical section/rollback snapshot.
- SQLite: `BEGIN IMMEDIATE` and uniqueness constraints.
- PostgreSQL 16: redesign the one-connection adapter so each live transaction
  owns a dedicated pooled connection and at least two governed transactions
  can overlap; map API Snapshot to `REPEATABLE READ` and Serializable/
  SerializableStrict to `SERIALIZABLE`; preserve ADR-026 guards; enforce
  uniqueness and SQLSTATE serialization/unique conflict mapping.

Use `axon-sim` deterministic schedules plus real SQLite/PostgreSQL concurrency.
Inject faults after forward, reverse/index, entity, before audit, after audit
before commit, and commit. Pre-commit failure leaves neither logical state nor
audit; success leaves both, including after durable restart.
The PostgreSQL gate asserts `repeatable read` for default Snapshot, then runs
two conflicting opt-in Serializable transactions, observes at least one real
serialization SQLSTATE, verifies its stable retry mapping, and asserts
`serializable` inside those transactions; a green non-conflicting suite is
insufficient.

## 6. Enforce one canonical payload/error contract

Create `AXON-CJSON-1` (explicitly **not** RFC 8785/JCS) in `axon-core`; every
surface passes parsed values to it.
It recursively sorts object keys, uses JSON escaping, and serializes `i64/u64`
exactly in decimal (including values above 2^53). Finite `f64` values that are
mathematically integral *and within i64/u64 range* canonicalize to the same
integer token (`2.0` → `2`), and negative zero canonicalizes to `0`, matching
JavaScript JSON semantics. Integral floats outside that range remain pinned
Ryu exponent/float form (for example the golden vector for `1e20`) and are not
reparsed as integer tokens. Other finite floats use the pinned `ryu::Buffer::format_finite` reference
algorithm; exponent spelling/sign/threshold are its frozen output. The contract
appendix freezes golden bytes; formatter/library
change requires `AXON-CJSON-2`, never silent drift. NaN/infinity are invalid
JSON. Axon's current parser does not preserve arbitrary-precision decimal
tokens; domains needing them use schema-declared decimal strings. A future
arbitrary-precision number feature must version this profile.

Pin `serde_json` configuration without `arbitrary_precision` or
`preserve_order`. Integer tokens without fraction/exponent parse as `i64` when
negative/in range or `u64` when nonnegative/in range; out-of-range integers
fail `number_out_of_range`. Fraction/exponent tokens parse as finite `f64` or
fail. Golden vectors freeze the 2^53, i64, u64, exponent, and float cutovers.

Object keys sort lexicographically by their unescaped Unicode scalar-value
sequence (equivalently valid UTF-8 code-point order), with no Unicode
normalization. Strings emit UTF-8 directly except JSON-mandated escapes for
quote, backslash, and U+0000..U+001F; use `\b\f\n\r\t` where defined and
lowercase `\u00xx` otherwise; never escape `/`. Reject unpaired surrogate
escapes as `invalid_unicode`. Golden vectors cover composed/decomposed keys,
astral scalars, controls, slash, quote/backslash, and invalid surrogates.

Reject duplicate object member names before conversion to `serde_json::Value`
at every raw JSON ingress: HTTP bodies, GraphQL variables, MCP JSON-RPC,
CLI/schema files, and SDK textual inputs. Custom streaming deserializers retain
an object-path key set and return 400 `duplicate_json_key` with JSON Pointer
and key. Already-constructed Rust `Value` inputs cannot contain duplicates.
Golden parity vectors cover root/nested duplicates, escaped-equivalent keys,
and duplicates inside arrays.
GraphQL validates duplicate fields in operation-document input objects before
coercion and uses the same duplicate-aware parser for JSON scalar literals;
payload-bearing literals are not normalized before this check. Tests cover
variables, ordinary input literals, and JSON scalar literals.

Measurement occurs server-side once after parsing, so SDK number rendering is
not a second authority. Shared golden vectors cover key order, Unicode,
escaping, i64/u64 boundaries and >2^53, floats/negative zero, arrays,
whitespace, and patch/default expansion. TypeScript sends vectors and asserts
the server's returned counts/errors; it does not reimplement authority.

The TypeScript SDK rejects any integer-valued `number` for which
`Number.isSafeInteger` is false. Its explicit `bigint`/`AxonInt64` serializer
emits exact signed i64 or unsigned u64 JSON number tokens after range checks;
raw textual JSON remains available for exact validated input. It never silently
rounds an unsafe integer. Boundary tests cover ±2^53, i64 min/max, and u64 max.

Limits:

- entity data: 1,000,000-byte default, per-collection up to 10,000,000;
- link metadata: 64,000;
- each normalized entity-schema or schema-template document: 2,000,000
  AXON-CJSON-1 bytes, 10,000 total AST fields/definitions/link declarations,
  and nesting depth 64;
- each normalized database-policy document: 2,000,000 AXON-CJSON-1 bytes,
  20,000 typed AST nodes, 10,000 rules/masks, nesting depth 64, and 64,000 bytes
  per string literal after canonical UTF-8 encoding;
- `activate_schema_policy` canonical logical input: 5,000,000 bytes including
  expected bindings, proposed schema, and full proposed policy;
- transaction: 100 ops, 10,000,000 `TransactionUserBytesV1` bytes;
- user audit event: 64,000 each and included in aggregate;
- HTTP/GraphQL mutation requests: identity `Content-Encoding` only in `TARGET_RELEASE`;
  reject compressed request bodies with 415. Enforce a streaming 12,000,000
  raw-byte cap before JSON parsing, without relaxing logical limits. A future
  compression feature must add streaming decompressed-size/ratio guards.

These schema/template/policy counts and canonical byte limits run before
compilation, diffing, idempotency reservation, or mutation for embedded Rust and
every transport; the 12 MB wire cap is not their substitute. Preflight the
complete generated schema/policy audit plus `SchemaControlBatch` source frame,
including canonical before/after documents and framing, against a 16,000,000-
byte admin-frame ceiling and the stricter overall 32 MB AXON-CHANGE-1 ceiling.
No force/dry-run/backend bypass exists. Structured limit kinds are
`schema_document`, `template_document`, `policy_document`,
`schema_policy_activation`, or `generated_admin_frame`, with exact
`{limit,actual}` and common surface parity tests at limit and limit+1.

The 12,000,000-byte pre-conversion ingress cap and no-compression rule apply to
every public mutation transport: HTTP/GraphQL/MCP JSON bytes, gRPC protobuf
message limits on client+server with compression disabled, CLI/schema files
checked before allocation/read, and TypeScript textual/raw JSON APIs checked
before parsing. Embedded Rust callers that pass an already-built `Value` have
no raw transport but still face all logical limits. The gRPC mapping is exact:
an inbound message with the compressed flag or any non-identity `grpc-encoding`
is rejected before decompression as `UNIMPLEMENTED`; a serialized request over
12,000,000 bytes is `RESOURCE_EXHAUSTED`. Both carry a protobuf
`axon.v1.ErrorDetails` in `Status.details`: respectively
`{code:"compressed_request_not_supported"}` or
`{code:"payload_limit_exceeded",kind:"request_bytes",limit:12000000,
actual:<count>}`. The streaming counter stops at 12,000,001, so `actual` is that
sentinel when the complete size is intentionally not consumed. Generated Axon
clients disable compression, preflight serialized bytes, and return the same
status/code/details for local oversize or attempted compression. Post-decode
logical count/payload failures remain `INVALID_ARGUMENT` with the structured
logical limit detail defined below. HTTP/GraphQL/MCP use 415
`compressed_request_not_supported` and 413 `payload_limit_exceeded` for these
wire failures. CLI/SDK local errors preserve the same codes. Golden client,
server, proxy-header, compressed-flag, exact-limit, and limit-plus-one tests
enforce parity.

The qualified Rust gRPC stack enforces this below Tonic decoding. A Tower
`AxonGrpcBodyGuard` wraps the raw HTTP/2 body before `tonic::codec::Streaming`:
it rejects non-identity `grpc-encoding`, parses each five-byte gRPC envelope,
rejects compressed flag `1` before forwarding any message bytes, and counts the
declared/streamed frame to the 12,000,000-byte limit. Tonic services advertise/
accept no compression and also set `max_decoding_message_size(12_000_000)` as
defense in depth; generated clients have an outbound serialization/body guard
and expose no `send_compressed` option. Handcrafted HTTP/2 integration tests send
a compressed flag with a tiny body, declared oversize, fragmented headers, and
a high-ratio compressed bomb while instrumenting the codec/decompressor; the
counter must show zero compressed bytes reached decoding or allocation. Tests
run against the real server/client stack, not handler mocks.

Generated cascade work has the same 100-op ceiling and a separate
40,000,000-byte expanded-commit ceiling over canonical storage payload plus
audit pre/post images,
as defined in Phase 5. Both are checked before writes.

`TransactionUserBytesV1` is one backend/surface-neutral measurement. Normalize
each staged operation after schema-default application and patch merge, then
AXON-CJSON-1 encode `TransactionUserOpV1` with operation tag, qualified target/
endpoint IDs, complete final entity payload, link type+metadata, force/cascade,
expected-version/OCC conditions, and semantic return options. Thus patch counts
the merged/default-expanded final value; create/update defaults count too. Add
8-byte big-endian length plus each op encoding in staged order, plus one
length-prefixed AXON-CJSON-1 common block containing isolation and user audit
event. Idempotency key, credential, trace/request IDs, transport aliases, and
generated cascade operations are excluded; cascade bytes remain in the 40 MB
expanded and 32 MB source-frame gates. `actual` is the complete sum. The error
`op_index` is the first op whose cumulative prefix exceeds the limit; if only
the common block crosses it, index is null and `component=common`. Golden
single/mixed/patch/default/link/audit vectors are identical on every surface.
Validate the complete batch before op 1. All duplicate op-count logic delegates
to the core validator.

Add structured `PayloadLimitExceeded` with kind, limit, actual, op index.
Payload/count maps to 400 `invalid_argument`; schema shape remains 422
`schema_validation`. Define exact HTTP, GraphQL extensions, MCP, CLI, embedded
Rust API, gRPC status/details, and TypeScript SDK mappings and golden parity
tests. gRPC receives the same payload/count limits and six-op transaction
grammar; protobuf size gates run before conversion to handler values.

## 7. Make backend/durability qualification truthful

Consolidate PostgreSQL fixture support:

- CI/release uses PostgreSQL 16 and `AXON_TEST_POSTGRES`; fail after bounded
  30-second readiness.
- Local mode may use testcontainers; unavailable runtime may skip only outside
  release and must report counts/reason.
- `AXON_REQUIRE_POSTGRES=1` turns any unavailable/skip into failure.
- Unique DB/schema per test, bounded pools, deterministic teardown, no
  poisonable global mutex; default parallelism.

Implement memory logical audit co-commit: adapter audit entries stage in the
same transaction snapshot and roll back with data. Handler audit is a derived
view updated before the commit lock is released; data and audit readers use
the same read boundary, so neither can observe data before its audit. It can
also be rebuilt from committed adapter audit. Memory remains
non-durable across process loss.

Route every governed single collection/schema/template/entity/link/policy
mutation through the same one-operation mutation-plan/co-commit primitive as
multi-op transactions. Remove direct data-then-audit call paths. Fault-test the
single-op memory path specifically for data-without-audit and audit-without-data.

Capability gates (marked complete only after tests):

| Capability | Memory | SQLite | PostgreSQL 16 |
|---|---|---|---|
| Governed semantics | required | required | required |
| Logical data+audit co-commit | process lifetime | durable | durable |
| Reopen/restart | no | implement/verify | implement/verify |
| Backup/restore | no | implement file backup/restore | implement dump/restore; document PITR as unqualified in `TARGET_RELEASE` unless separately tested |

Test every governed mutation class at before/after-data/before-audit/
after-audit/before-commit/commit/restart faults. Only neither or both is valid.
Derived/checkpoint exemptions are unreachable publicly.

Release requires three consecutive
`AXON_REQUIRE_POSTGRES=1 AXON_TEST_POSTGRES=... cargo test -p axon-storage`
runs plus PostgreSQL handler/graph/server E2E, with zero PG skips.

Before finish line A, replace raw resume IDs on release-supported GraphQL
subscriptions, MCP notifications, TypeScript change readers, and external CDC
with the generic durable server-resolved `resume_sessions` handle service.
Disable/reject raw resume IDs. This substrate provides opaque current/pending
handle state and restart/TTL cleanup; Phase 10 extends its framed replica
delivery semantics rather than postponing opacity. Public resume conformance is
a core evidence-gate dependency.

The common finish-line-A contract is complete: a 256-bit random client handle
maps in durable `resume_sessions` to handle hash, tenant-qualified database,
principal/grant, surface+query/collection scope, compatible schema lineage,
policy epoch/hash, tenant auth epoch, current boundary, optional pending
boundary/handle, exact pending delivery bytes/digest, ACK state, issued/updated-
at, and expiry. Unknown, expired, or tenant/database/principal/surface/scope-
mismatched handles all return the same `cursor_unavailable` shape/status/timing;
a mismatch never deletes or changes the owner's session. Incompatible
schema or policy/auth change revokes and requires fresh bootstrap; compatible
schema migration and producer restart preserve it. Current/pending promotion is
ACK-atomic, lost ACK replays pending, and TTL/restart cleanup is tested. Client
bytes reveal none of the stored fields. Phase 10 adds transaction-framed local
replica deliveries and leases, not basic handle security.

Unavailable-handle secrecy is measurable. After ordinary transport/syntax and
caller authentication (malformed token and invalid authentication keep their
own errors), every well-formed resume handle follows this order:
start a response timer; hash the token; perform one indexed lookup; substitute a
fixed synthetic row on miss; compute constant-time hashed comparisons for
tenant/database/principal/surface/scope and expiry; choose the owner-only epoch/
lineage checks only after scope matches. Unknown, expired, or scope mismatch
does the same cryptographic work, returns an identical fixed-length 410 body
with only `cursor_unavailable`, and defers expiry
cleanup until after response. Pad each negative response to a 20 ms floor plus
CSPRNG-uniform 0–5 ms jitter measured after authentication. On the pinned
security runner, 10,000 warmed samples per class must have identical status/
length, no response below the floor, pairwise p50 and p95 deltas at most 1 ms,
and two-sample KS statistic at most 0.08. Finish line A runs owner, other-
principal, other-tenant, other-scope/surface, expired, and random-unknown vectors
for resume handles only. Phase 10 requires snapshot handles to adopt this exact
algorithm/error envelope and vector list, but snapshot evidence belongs only to
finish line B. Production metrics aggregate these
classes and never label existence.

`SchemaLineageV1` is stored per session as a sorted dependency closure of every
collection and link declaration referenced by its scope/query:
`{collection_id,current_version,compatibility_chain_hash,referenced_fields,
referenced_link_types}`. A new active version is resumable only if FEAT-017
classifies the transition compatible *and* every referenced field/link retains
compatible type, target, metadata, cardinality, and read-default semantics.
All entries in a multi-collection scope must pass. A compatible schema-control
frame contains sorted collection/version/diff/default data. While delivery is
pending, store pending lineage separately; ACK atomically promotes boundary,
handle, and lineage. Incompatible or missing dependencies revoke the session.
Golden multi-collection/link tests cover compatible additions and every
invalidation class.

Policy and lineage validate jointly: a compatible schema change may preserve
AXON-POLICY-HASH-1 only when every policy/mask/write-rule referenced symbol and
type remains valid and normalized policy semantics are unchanged. Before schema
activation, compile every policy/mask/write rule against the proposed schema.
If any referenced field/link is missing, renamed, or type-incompatible, reject
the schema activation unchanged with `policy_schema_incompatible`. The only
way to activate that schema is one atomic schema+explicit-policy update whose
new normalized policy compiles; that update receives a new
AXON-POLICY-HASH-1 value and increments the database policy epoch exactly once.
A compatible schema-only activation with byte-identical normalized policy
changes lineage but not policy hash/epoch. There is no invalid-policy sentinel,
epoch-only repair, or post-activation recompilation window. Query/schema
compilation verifies this joint invariant before issuing/updating a handle.

Finish line A also implements the common public stream protocol: audit rows are
transaction-framed; each `ChangeBatch` is exactly one source transaction after
policy filtering; each `SchemaControlBatch` is exactly one relevant compatible
schema-only activation transaction; bounded `DeliveryBatch` envelopes carry
their ordered tagged-union frames plus one pending handle; fixed-cadence zero-frame checkpoints make no-activity and
hidden-only activity indistinguishable; and current/pending ACK promotion,
lost-ACK replay, TTL, restart, mismatch, and backpressure follow the exact
state machine/caps specified in Phase 10. GraphQL, MCP, TypeScript change
reader, and external CDC must pass the same no-leak/framing tests before pilot.
Phase 10 only adds immutable local snapshot materialization, local apply,
search/traversal, encryption, and freshness leases.

Kafka remains a required supported production sink under FEAT-021 even though
deployments may choose file or HTTP/SSE and the Cargo feature stays optional for
minimal embedded builds. Phase 1 pins Kafka/schema-registry image versions and
digests and reconciles CDC-15/16/17 with the common framed contract: Kafka, file,
and SSE emit the same `DeliveryFrameV1`/delete-event envelope; Kafka retries keep
stable `delivery_digest`, frame index, and event index for consumer de-dup; raw
source boundaries remain internal to `_cdc_cursors`; sink ACK advances that
checkpoint only after broker/file/HTTP acceptance. Kafka null-record compaction
tombstones are not Axon entity tombstones—the common logical delete event is
preserved identically on all sinks.

The release profile runs `cargo check -p axon-audit --features kafka`,
`cargo test -p axon-audit --features kafka`, and
`cargo clippy -p axon-audit --all-targets --features kafka -- -D warnings`.
With `AXON_REQUIRE_KAFKA=1`, a pinned real broker/registry fixture permits zero
skips and tests transaction/frame ordering, logical delete parity, schema IDs/
compatibility, at-least-once duplicate identity, producer restart from
`_cdc_cursors`, broker outage/backpressure and catch-up without blocking entity
writes, file/SSE envelope parity, and the frozen 10K events/s plus <1s p99
ratchets. Three consecutive release runs archive broker/client logs and exact
counts. Removing Kafka from `TARGET_RELEASE` instead requires an explicit
higher-authority PRD/FEAT scope change before criteria freeze; feature
optionality alone is not such a change.

## 8. Complete the declared V1 graph contract

Separate real implementation from citation work:

- Implement STP-075 index-threshold rejection, policy-bypass rejection, dry-run
  compile; STP-076 cardinality rejection; and any other UNTESTED behavior.
- Add `@covers`/citations such as STP-077 only after behavior passes.

Enforce CONTRACT-007 exactly: row policy at each match; redacted fields null at
projection and unusable in predicates/aggregates; hidden targets absent from
existence; visible-only counts/aggregates; link properties redacted; policy/
schema snapshot fixed at query start; fixed limits/errors.

Redacted entity/link fields are unusable in filters, ordering, grouping,
`DISTINCT`, aggregation, pagination/cursor-key construction, and named-query
sort/planning unless policy explicitly exposes them. Reject such plans before
execution; leak fixtures prove hidden values cannot change result order, group
shape, distinctness, or page boundaries.

Create one immutable `QueryExecutionContext` containing schema, policy
catalog epoch/hash, and tenant auth epoch and thread it through the production parser/planner/executor
and every hop. GraphQL and MCP must call that same path, not dead/test-only
planner code. The 1M worst-case cardinality budget is a hard `TARGET_RELEASE` ceiling for
ad-hoc and named queries; declarations may choose lower but never higher.
Amend CONTRACT-007 to remove or bound any named-query override accordingly.
Test observed index/cardinality rejection through real GraphQL/MCP.
The executor never hot-swaps an in-flight finite query's immutable context, but
authorization is not start-authorized through arbitrary emission. Immediately
before the first response write and before every page/chunk write, the handler
revalidates the database policy epoch/hash, tenant auth epoch, and exact schema
lineage. A mismatch discards all unsent buffered rows and returns/terminates
with `policy_changed`, `auth_changed`, or `schema_changed`; after a partial
stream it emits only the surface's terminal error when possible and closes,
never another data row. Independent pagination requests always build a fresh
context. Bytes whose transport write began before the revocation commit cannot
be retracted; the guarantee is no write authorized after observing the new
epoch. Embedded, HTTP, gRPC, GraphQL, MCP, CLI/SDK tests synchronize each
change between execution and first/page/chunk emission and assert parity.

A long-lived graph/query subscription is not a change-stream subscription: it
has no schema-control frames or compatible continuation. It terminates with
`policy_changed` on its database policy epoch/hash delta, `auth_changed` on its
tenant auth epoch delta, and `schema_changed` on any active-version delta in its
`SchemaLineageV1` dependency closure, compatible or not, then requires a fresh
query snapshot/plan. By contrast, the Phase 7/10 GraphQL change-stream
subscription follows the common delivery protocol: compatible lineage emits a
schema-control frame and continues after ACK; incompatible lineage terminates
with `schema_incompatible`. GraphQL operation type and MCP tool select exactly
one contract. Test narrowing, broadening, grant/revocation, compatible and
incompatible schema changes during finite query, graph subscription, and change
stream; neither stream can silently switch behavior.

Visible-link metadata receives field policy/redaction before graph projection
and replica materialization. Scalar redacted entity/link projections return
null on every surface; add explicit fixtures including replay/rebootstrap.

Run embedded handler, HTTP, gRPC, GraphQL, MCP, TypeScript SDK, SQLite, and PostgreSQL 16 fixtures for
filter/order/page/aggregate/existence/named query/subscription/bounded path.
Run the pre-frozen benchmarks and archive raw samples, environment, SHA, and
ratchet result. Exit with no in-scope UNTESTED/UNCITED_COVERAGE.

## 9. Close operations and required consumers

Complete installer/service, actionable doctor, health/tenant/auth/TLS,
SQLite/PostgreSQL backup/restore, monitoring, security/threat architecture,
and deployment checklists with executable evidence.
Installer fresh-store setup invokes the explicit Phase 3 owner-init flow only
after an operator supplies its spec and confirms the target is new; service
start itself never initializes, migrates, or repairs a store.

The existing private Svelte package `ui/` (`axon-admin-ui`) is the required
finish-line-A operator UI surface. From the pinned worktree run
`cd ui && bun install --frozen-lockfile`, then `bun run build`, `bun run
typecheck`, `bun run lint`, `bun run test`, `bun run check:covers`, `bun run
check:story-coverage`, `bun run test:e2e:sqlite`, and `bun run
test:e2e:postgres`. Both E2E backends are release-blocking; no UI requirement is
inferred beyond the frozen PRD/stories and these package-owned gates.

Nexiq, DDx, and Cayce are all required. Run release workloads from clean SHAs;
archive commands, native counts, skips, real Axon traffic, and postconditions.
Pin each consumer repository remote and commit SHA in the release matrix before
the first evidence run; reruns use those exact SHAs.
Before any evidence run, freeze one manifest per consumer with exact setup,
workload, and teardown commands; required native assertions and Axon
postconditions; allowed skips (none release-blocking); minimum successful Axon
request count; and required operation classes. The runner rejects missing or
changed manifests and traffic below threshold.
Any failure means `hold` absent a prior higher-authority scope change. Docs do
not waive core invariants or required consumers.

## 10. Complete the governed local read replica

Start only after frozen FEAT-032 ACs and durable audit/cursor substrate.
FR-32 sources are restricted to durable SQLite or qualified PostgreSQL 16.
Memory may support unit tests but cannot satisfy bootstrap/tail durability or
finish-line-B evidence.

### Transaction-framed tail

The audit framing, `ChangeBatch`/`DeliveryBatch`, cadence, caps, opaque
resume-session state machine, ACK/restart behavior, and no-leak tests are
finish-line-A common stream infrastructure implemented/gated in Phase 7.
Phase 10 reuses them unchanged for local application; it does not defer those
public stream guarantees beyond pilot.

Authority is singular by layer: durable audit is the source event log;
`StorageCursorStore` owns `_cdc_cursors` only for internal producer/sink
last-ACKed boundaries; `resume_sessions` owns each external client's current/
pending handle state. Sink ACK updates only `_cdc_cursors`; client ACK updates
only its resume session. Neither copies/overwrites the other, and restart/ACK
loss tests prove their expected independent boundaries cannot cause skips.

CONTRACT-005/006 requires audit rows in one source transaction to get
consecutive audit IDs plus internal transaction index/size/last-ID metadata.
The server reads complete groups. A single mutation is size one.

Define new wire types. `DeliveryFrameV1` is a tagged union:

- `Change(ChangeBatch)`: exactly one source transaction after policy filtering,
  with one or more visible/redacted data/link/control events other than schema
  compatibility control;
- `SchemaControl(SchemaControlBatch)`: exactly one committed, relevant,
  FEAT-017-compatible schema-only activation source transaction, containing
  sorted collection/version/diff/default/link-declaration entries and the
  resulting pending `SchemaLineageV1`. An activation that changes policy revokes
  on policy hash/epoch and never becomes this frame. An unrelated schema group
  produces no frame.

`DeliveryBatch` is an ordered list of zero or more `DeliveryFrameV1` values plus
one pending `next_cursor` handle and public `delivery_digest`. Frame order is
source-transaction order and a source group produces at most one tagged frame:

- A delivery is bounded to 64 total tagged frames and 1,000 visible entries
  (each change event counts one; each atomic `SchemaControlBatch` counts one
  regardless of its internal diff entries),
  and 40,000,000 AXON-CJSON-1 bytes, whichever is reached first. Cut watermark
  only between complete source transactions; never split either frame kind. A
  single valid source transaction is guaranteed by the 32 MB AXON-CHANGE-1
  precommit cap to fit with delivery framing. ACK gating is the backpressure mechanism; while pending, the server
  buffers no later delivery and source audit remains the durable queue. Compute
  public `delivery_digest = SHA-256(AXON-CJSON-1({format_version,frames,
  next_cursor}))`; only exact public delivery values are in its input and the
  digest field excludes itself. The client recomputes it without learning the
  source boundary or server-side lineage. Server state separately binds those
  exact canonical public bytes/digest to selected boundary and pending lineage
  before send, and replay is byte-for-byte.

- only visible/redacted events plus a server-resolved random `next_cursor`
  handle; never raw
  audit IDs or hidden op count;
- links whose endpoint is hidden are omitted;
- local events and cursor commit in one SQLite transaction; crash commits
  neither and replay is idempotent;
- a source group with zero visible events produces no `ChangeBatch` at all—no
  empty frame, source index, transaction marker, timestamp, or count. Only the
  fixed-cadence checkpoint delivery below may contain zero frames. Its opaque
  boundary may advance across hidden-only groups without exposing how many;
- each frame preserves source-transaction atomicity/order; a delivery may carry
  multiple frames but never merges them. Mixed transactions/cascades stay
  atomic in the authorized local view.

For liveness, emit a zero-frame checkpoint `DeliveryBatch` on a fixed five-second cadence
regardless of source activity. It always rotates a random opaque handle, so a
client cannot distinguish inactivity from hidden-only activity or infer a
count. Commit the checkpoint handle atomically; expose no timestamps, audit or
transaction IDs, or hidden metadata.

Cursor state transitions are exact. A session starts with durable `current`
handle/boundary and no `pending`. On each cadence tick, only when `pending` is
empty, scan complete source groups after `current` in source order and apply
policy. Translate a group to one non-empty `ChangeBatch`, one
`SchemaControlBatch`, no frame when fully hidden/unrelated, or immediate session
revocation when policy/auth/incompatible lineage changed. A compatible schema
activation is emitted exactly once because its audit transaction is crossed
once by the ACK-promoted source boundary; while an older delivery is pending it
waits in the durable source log and is not synthesized into that delivery.
Select the greatest boundary through the last complete scanned group for which
the resulting delivery is at most 64 frames, 1,000 visible entries, and
40,000,000 AXON-CJSON-1 bytes. Stop immediately before the first visible group
that would exceed any cap; never split it. Zero-visible groups encountered
before that stop advance only the opaque boundary and consume no frame/event/
byte count. Bound each cadence scan to 4,096 source groups; if that bound is
reached with only hidden groups, create the ordinary zero-frame checkpoint at
that boundary and continue after ACK, without exposing the scan count. A valid
single source group always fits because of the precommit source-frame invariant.
Persist exactly the selected boundary, ordered frames, encoded-byte digest, and
random handle as one pending `DeliveryBatch`; the boundary may equal `current`
when there was no source activity. Before writing any initial or replayed
pending bytes to a transport, reauthenticate and revalidate exact tenant/
database/principal/surface/scope, expiry, policy epoch/hash, tenant auth epoch,
and classify stored-versus-current schema lineage under the same authority
snapshot used to authorize the send. Equality succeeds unchanged. A FEAT-017-
compatible dependency-closure delta may resend the stored pending bytes; its
committed activation group remains after that pending boundary and the normal
post-ACK scan emits one `SchemaControlBatch` for each such source transaction in
order, subject to the ordinary caps/cadence/digest/ACK/replay rules. Pending
creation reads source boundary and lineage in one consistent snapshot, so it
cannot cross an activation group while storing pre-activation lineage. An
incompatible/missing dependency delta revokes. Policy/auth mismatch or
incompatible lineage atomically
revokes the session and deletes pending bytes/handle without serving them, then
returns the matching change code. Scope mismatch serves nothing but does not let an attacker
delete the owner's session. Tests revoke/rotate/narrow/broaden/change compatible
and incompatible schema after pending creation and immediately before initial
send, reconnect resend, and timeout resend; no unauthorized pending byte is
written.

Send visible frames or a zero-frame checkpoint only on the same fixed cadence—
never immediately because a hidden group arrived. While pending exists, do not
scan/advance; an authorized reconnect or timeout resends the identical pending
delivery+handle. ACK of that exact handle atomically
promotes pending→current and clears pending. Lost ACK and server restart retain
both states and replay; a pending visible delivery can never be skipped by a
checkpoint.

### Public ACK contract

The one core operation is
`ack_change_delivery(next_cursor) -> ChangeAckV1 { status, current_cursor }`.
The opaque cursor resolves the session, pending digest, and scope. With current
authentication, ACK of the exact pending handle atomically promotes it and
returns `status=promoted`; repeating the now-current handle is a read-only
success with `status=already_acked`, even when a later delivery is pending.
For `promoted`, `current_cursor` is exactly the supplied newly promoted handle;
for `already_acked`, it is exactly the supplied current handle. It never returns
the later pending handle, its digest, or any replacement. No other handle can
clear or replace pending state. Malformed handles are
`invalid_cursor`; unknown/expired handles are `cursor_unavailable`; a known
handle presented by another tenant/database/principal/surface/scope is
indistinguishable from unknown as `cursor_unavailable` and preserves owner
state. For the authenticated owner, policy or tenant-auth mismatch revokes and
returns `policy_changed` or `auth_changed`. Schema lineage uses the same three-
way classifier as send: equality promotes normally; a compatible dependency-
closure delta also permits ACK and promotes the pending boundary/handle plus
its stored pre-change lineage, leaving the later activation source groups for
the next scan's ordered `SchemaControlBatch`; only incompatible or missing
dependencies revoke as `schema_incompatible`. ACK retries are safe until session TTL and return no
raw boundary, digest, audit ID, or event count.

Freeze these surface operations in Phase 1:

- GraphQL mutation
  `ackChanges(input: { nextCursor: String! }): ChangeAckV1!`;
- MCP tool `axon_changes_ack` with exactly `{next_cursor}`;
- external CDC `POST /v1/change-streams/ack` with JSON
  `{ "next_cursor": "..." }`;
- TypeScript `ChangeReader.ack(nextCursor)`, which calls the reader's bound
  GraphQL or external-CDC transport and exposes the same typed result/error.

External CDC returns 200 for both success statuses, 400 `invalid_cursor`, 409
for the three epoch/lineage changes, and 410 `cursor_unavailable` for unknown,
expired, or any scope mismatch. GraphQL uses the same codes in
`extensions.code`. MCP
uses JSON-RPC `-32602` only for `invalid_cursor` and `-32010` for runtime ACK
errors with `data.code`; TypeScript preserves the code/status in typed errors.
Golden vectors cover every result on every surface, concurrent duplicate ACK,
ACK after a newer pending delivery, equality/compatible/incompatible lineage,
auth/scope mismatch, expiry, restart, and lost response.

Clients must validate and durably apply the complete delivery before ACK. The
local replica verifies and commits all frames plus the public
`delivery_digest` in one SQLite
transaction, then ACKs; after an ACK-response loss, replay/`already_acked` and
the stored digest prove the already-applied delivery rather than applying it
twice. No public API offers auto-ACK-on-receive.

### Immutable bootstrap snapshot

Memory clones under one authority/data read lock; SQLite/PostgreSQL capture
visible entities, links, max committed audit boundary, database policy epoch/
hash, tenant auth epoch, and exact schema catalog/`SchemaLineageV1` in one
consistent source read snapshot. The server policy-filters/redacts and fully
materializes that captured view into an unpublished temporary spool. Before
publishing page 1, it acquires the shared authority-generation guard, rechecks
captured policy/auth/schema against current values, and atomically persists the
snapshot row/spool pointer; a concurrent authority mutation either commits
first and makes this discard/retry, or commits after publication and makes the
per-page check revoke it. Data writes after the captured audit boundary do not
invalidate the immutable snapshot. No handle/page exists before this publish.

The public protocol has three core operations:

1. `begin_replica_bootstrap(ReplicaBootstrapRequestV1)` where the request is
   `{database,scope,page_size,bootstrap_idempotency_key}`. `scope` is the closed
   union `Collections{sorted_unique_ids,include_links}` or
   `SavedQuery{name,AXON_CJSON_params}`; page size is 1–1,000. The key is scoped
   by tenant/database/principal/scope hash for one hour; retry returns the same
   bootstrap state and a different request is `idempotency_key_conflict`.
   Response is `{snapshot_handle,first_page_cursor}` only.
2. `read_replica_snapshot_page({snapshot_handle,page_cursor})` returns
   `{items,next_page_cursor,complete,snapshot_digest?}`. Items are ordered tagged
   `Entity`/`Link` `ReplicaSnapshotItemV1` values after policy/redaction. Page
   cursors and handles are random server-resolved values and contain no boundary,
   count, scope, or ID. The final page includes
   `snapshot_digest=SHA-256(AXON-CJSON-1({format_version,scope_hash,
   schema_lineage,all_ordered_items}))`; earlier pages omit it.
3. `complete_replica_bootstrap({snapshot_handle,snapshot_digest})` is called
   only after the client durably applies all pages and verifies the digest. It
   returns `{resume_cursor}`; no earlier response exposes a tail handle.

At begin publication, in the same durable transaction as the snapshot row/
spool pointer, create one `resume_sessions` row in `bootstrap_pending` with a
random current-handle hash, captured scope/policy/auth/lineage, captured audit
boundary as `current`, and no pending delivery. It cannot stream or ACK while
pending. Completion revalidates authority and digest, atomically marks that
session active and the snapshot complete, then returns the already-created
current handle. Thus every committed source group after the captured boundary
remains in audit for the first tail scan; no write between capture, paging, and
completion can be skipped and no raw boundary is public. Completion-response
loss is idempotent: a retained completion record returns the identical current
handle; it never creates another session. Spool bytes delete on completion, but
the handle/digest completion record remains until the one-hour TTL.

Bindings are embedded Rust methods with the names above; HTTP
`POST /v1/replicas/bootstrap`, `POST /v1/replicas/bootstrap/page`, and
`POST /v1/replicas/bootstrap/complete`; gRPC `BeginReplicaBootstrap`,
`ReadReplicaSnapshotPage`, and `CompleteReplicaBootstrap`; GraphQL
`beginReplicaBootstrap`, `replicaSnapshotPage`, and
`completeReplicaBootstrap`; MCP `axon_replica_bootstrap_begin`, `_page`, and
`_complete`; and TypeScript `LocalReplica.bootstrap`, which owns the full
page/apply/complete sequence. Malformed input is 400/gRPC `INVALID_ARGUMENT`/
GraphQL or MCP invalid-argument; quota is 429/`RESOURCE_EXHAUSTED`; digest or
authority drift is 409/`ABORTED` with the named core code; unavailable snapshot
state is 410/gRPC `NOT_FOUND` `snapshot_unavailable`. GraphQL extensions, MCP
runtime data, and TypeScript typed errors preserve the same code/details.

Crash tests cover before/after spool publication, precreated-session insert,
every page, client local apply, completion activation, spool deletion, and
response loss. Retry must yield neither duplicate session nor gap; a source
write barrier around capture/completion proves snapshot items plus tail equal
one ordered authorized source history.

- Immutable spool ordered `(kind, collection, id)` with entity+link
  continuation; every page uses the same boundary.
- Server-resolved random snapshot handle, one-hour default TTL, quotas/cleanup
  at completion or expiry. Client bytes encode no scope, boundary, count, or
  audit ID.
- The server-side snapshot row binds handle hash, tenant, database, principal,
  exact collection/query scope, immutable source boundary, policy epoch/hash,
  tenant auth epoch, exact `SchemaLineageV1`, spool identity/digest, issued-at,
  expiry, and page-continuation state. Before every page—including page 1—the
  server reauthenticates and requires exact tenant/database/principal/scope,
  policy epoch/hash, auth epoch, and schema-lineage equality. Policy/auth/schema
  mismatch atomically revokes the handle and deletes the spool, then returns
  `policy_changed`, `auth_changed`, or `schema_changed`; bootstrap does not use
  the compatible-stream continuation rule because every page must share one
  materialized schema. Unknown, expired, or any tenant/database/principal/scope
  mismatch returns the same `snapshot_unavailable` shape/status/timing and does
  not delete another principal's spool. Tests change grants, credentials, masks/write policy,
  compatible and incompatible schema, and principal/scope between every pair of
  pages and prove no later page bytes are served.
- Quotas: at most 2 active snapshots per principal/database, 10 per tenant,
  2,000,000,000 spool bytes per snapshot, and 10,000,000,000 per tenant.
  Admission over quota returns `snapshot_quota_exceeded`; never evict an active
  snapshot. A spool crossing its byte cap aborts and deletes itself. Completion
  deletes spool bytes immediately but retains the idempotent completion record
  until TTL; expiry cleanup orders oldest-expired first and is
  idempotent across restart. Tests cover races, cleanup failure, and quotas.
- Concurrent source writes cannot change pages; tail starts after boundary.
- Audit is append-only in this plan, so normal operation does not expire the
  boundary. Before successful completion, every missing spool/boundary/imported-
  store failure uses `snapshot_unavailable` (with owner-visible reason
  `tail_boundary_unavailable` where authorized). Only after completion, use of
  the returned resume cursor can produce `cursor_unavailable`. These are
  separate golden surface tests. This defense is not a finish-line-B retention
  test or new erasure feature; recovery is purge/rebootstrap.

### Token/security/SDK

The public token is a random 256-bit server-resolved handle, not readable
base64 JSON. This replaces the existing self-describing `CursorToken` format;
Phase 1 amends ADR-025 and all consumers explicitly. The finish-line-A durable
adapter-owned `resume_sessions` table (manifested and non-`CollectionId`-
addressable) is reused unchanged for replica delivery and already stores every
finish-line-A field enumerated in Phase 3, including exact pending bytes/digest
and current/pending lineage. Finish line B adds no replica-only column to that
table. Handles are unguessable and reveal no audit boundary/op count.

Tail delivery is ACK-gated: issue at most one pending `next_cursor`; do not
send the next batch until the client atomically applies events and ACKs that
handle. ACK promotes pending→current and deletes the predecessor in one source
transaction. Lost response/ACK can replay from current; server restart reloads
both records. Thus each session has at most two handles, while TTL/session
cleanup bounds abandoned rows. Tests decoding client bytes recover no scope,
count, or `audit_id`, and restart/lost-ACK tests prove resumability.

Producer restart and FEAT-017-compatible schema changes preserve the handle;
the stream emits a compatible schema-control event and continues. Scope,
expiry, revocation, any policy/auth epoch change, or schema-incompatible change
invalidates the handle, purges local state, and requires bootstrap. Policy
narrowing, broadening, redaction, and incompatible link-schema changes all
invalidate.

Local reads use a security freshness lease issued after handle validation:
default and maximum five minutes. While disconnected, reads may continue only
until expiry—this is the explicit maximum revocation staleness. At expiry the
SDK atomically locks the replica, purges it with the file-level protocol below,
and returns `replica_scope_unverified`. Reconnect validates
policy epoch/hash, tenant auth epoch, and stored `SchemaLineageV1` before
unlock. Exact lineage may resume; a compatible delta remains locked until the
ordered tail applies+ACKs every intervening `SchemaControlBatch` and promotes
lineage; incompatible or missing dependencies purge and rebootstrap. Policy/
auth mismatch also purges. Tests revoke/change grants or compatible/
incompatible schema while
disconnected at lease−1, lease expiry, and reconnect; no read succeeds after
expiry without validation.

Local SQLite is encrypted at rest with a SQLCipher-compatible page store and a
random per-bootstrap key held only in process memory—never persisted in the DB,
token, logs, or ordinary local storage. Process restart therefore requires
online handle validation and rebootstrap; the unreadable old file is deleted.
Phase 1 freezes the library/platform support and key-zeroization contract.

Purge never relies on row deletion after key loss. First lock all public reads,
stop apply/search workers, write+fsync a non-secret `purge_pending` sidecar, and
close/checkpoint every SQLCipher connection while the key is still present.
Then unlink the database, WAL, SHM, rollback-journal, temp, and replica spool
files and fsync the containing directory; zeroize the in-memory key on both
success and any unlink/close error. On full success remove+fsync the sidecar. On
failure remain locked with the key gone; startup sees `purge_pending`, deletes
the now-unreadable file set before creating any new replica, and never attempts
to reopen it. Crash injection before/after marker, connection close, every
unlink, directory fsync, key zeroization, and marker removal proves that either
the live authorized replica remains before expiry or no readable old state can
be queried after expiry/restart.

Lease threat model: it protects against ordinary revocation/disconnect on an
honest client OS, not a malicious user with filesystem/process control over
already-authorized plaintext. The SDK records both trusted wall-clock expiry
and monotonic elapsed deadline; rollback, unavailable/uncertain time,
process restart, or suspend/resume immediately locks/purges instead of extending
the lease. The five-minute maximum claim holds only under this stated honest-OS
model; tests inject wall rollback, monotonic discontinuity, restart, and resume.

Replace delimiter composite keys with nested maps or length-delimited tuples.
Wire `StorageCursorStore` into the real producer. Complete bootstrap/tail,
dedup/reconnect, search/sort/filter, declared-link traversal, and entity/link
wire delete events/internal transient apply markers. They remove local
entity/link rows and are never query-visible tombstone records; reads return
not-found/absent after apply. Real server+SDK tests inspect local SQLite and prove denied rows,
hidden-endpoint links, and unredacted fields never enter, including replay and
rebootstrap.

Axon `TARGET_RELEASE` intentionally stops exposing/accepting raw `audit_id` cursors for
GraphQL subscription resume, MCP notification resume, SDK change readers, and
external CDC resume. Only opaque tokens work on those four surfaces. Release
notes specify the pre-1.0 break: discard old resume cursor and bootstrap. No
ambiguous dual mode. Numeric audit IDs remain legal administrator identifiers
for audit query pagination/inspection, transaction grouping, and rollback;
those are not replica/resume capabilities.
The Phase 1 ADR-025/CONTRACT-006 amendment explicitly replaces the former
dual-token deprecation suggestion with this pre-1.0 hard cut.

## 11. Evidence gates and handoff

### Finish line A — core/pilot gate

Archive full output from the pinned worktree:

- fetched `TARGET_RELEASE` reconciliation across workspace, PRD, release notes,
  manifests, and every version-scoped contract claim
- `cargo check --workspace`
- `cargo test --workspace`
- CI-exact clippy ratchets with `-D warnings`
- `cargo fmt --all -- --check`
- PostgreSQL-required repeated storage and handler/graph/server E2E
- PostgreSQL 16 allocator-contention evidence with observed `40001`, bounded
  transparent recovery, forced exhaustion, and contiguous committed audit groups
- tenant auth-audit allocator/co-commit/order/restart evidence, including proof
  it is absent from every database CDC/replica surface
- `AuthAuditRedactionV1` secret-substring golden vectors for every credential type
- gRPC/protobuf payload, transaction, policy, graph, and reserved-name E2E
- cross-backend idempotency request/outcome golden vectors, exact replay,
  durable lease-clock/skew/restart, fence/takeover, and crash-point evidence
- crash-resumed store-wide policy/auth backfill plus pre-migration-token
  rejection, legacy-idempotency tombstones, schema-bound `LinkKey` rewrite,
  all-tenant in-progress cleanup, exhaustive union tenant manifest/orphan
  rejection, every unscoped auth-record mapping class, and tenant-wide auth-
  epoch invalidation evidence
- schema+policy activation atomicity/idempotency/audit/error parity on all admin
  surfaces, including dry-run key rejection, crash at each half, and proof no
  two-step path exists
- migration retry/restore-required/aborted-restored/activation-boundary evidence
- PostgreSQL stale-snapshot maintenance CAS and non-advisory governed-writer test
- PostgreSQL migration capability preflight plus crash recovery at both database
  renames; managed/no-privilege apply refusal occurs before mutation
- authoritative external/in-store migration-gate mismatch/open-refusal matrix
- fresh SQLite/PostgreSQL owner-init crash matrix with exact bootstrap catalogs,
  epochs, hashes, audit, UUID-bound credential artifact reuse/consume/abort, and
  no runtime auto-init
- public-stream cap-boundary, hidden-only no-frame, fixed-cadence, lost-ACK,
  schema-control tagged-frame order/caps/digest/replay, public ACK result parity,
  and restart evidence on GraphQL, MCP, TypeScript, and external CDC
- Kafka-feature check/test/clippy plus required real broker/registry ordering,
  delete parity, schema, duplicate, restart, backpressure, file/SSE parity, and
  performance evidence from Phase 7
- pending-delivery authority revalidation before every initial/reconnect/timeout
  send and finish-line-A resume-session manifest/restore evidence
- reserved-namespace exact error parity and external Rust compile-fail evidence
- checked-in physical DML manifest, `cargo xtask audit-dml-boundary`, negative
  direct-SQL lint fixture, and every governed DML co-commit fault test
- generic/system/auth audit-query namespace, filtering, cursor-secrecy, and
  physical-row non-observability parity evidence
- exact wire-ingress error vectors for HTTP/GraphQL/MCP/gRPC/CLI/SDK at the
  compression, 12,000,000-byte, and 12,000,001-byte boundaries
- raw-body gRPC guard proof that compressed flags/bombs never reach Tonic decode
- canonical schema/template/policy/activation/generated-frame byte+item limit
  vectors, `TransactionUserBytesV1` per-op/common attribution including embedded
  Rust, and schema-catalog OCC/hash vectors
- AXON-BACKUP-ROW-1 cross-backend row/key/type/collation/no-PK golden vectors
- link read-set/set-version/unique-constraint concurrent write-skew matrix
- pinned-runner unavailable resume-handle timing/length distributions
- `ui/` (`axon-admin-ui`) frozen-install, build, typecheck, lint, unit,
  citation/story-coverage, SQLite E2E, and PostgreSQL E2E commands from Phase 9
- TypeScript SDK build/test/lint; embedded Rust API/AC traceability
- frozen graph benchmarks
- required consumer release matrix
- deployment/TLS/backup/restore/monitoring/doctor/security evidence
- `ddx doc validate`, stale docs, release-claim inventory

Only then update the frozen core PRD criterion statuses and evidence links and
emit `pilot-ready` or `hold`; criterion text cannot change after Phase 1
without explicit scope-change review.

### Finish line B — local-replica gate

This gate begins only after `pilot-ready`, reruns every finish-line-A item at
the final SHA, and additionally archives:

- immutable bootstrap snapshot consistency, page ordering, quotas, cleanup,
  expiry, unavailable-handle timing/length distributions, and between-page
  policy/auth/schema revocation plus spool deletion;
- begin/page/complete surface parity and idempotent snapshot-to-precreated-resume
  handoff crash/write-barrier proof with no raw boundary, duplicate, or gap;
- transaction-framed local apply, public digest verification, lost-ACK/restart
  de-duplication, schema-control application, and source-boundary handoff;
- local search/sort/filter/declared-link traversal and entity/link delete-marker
  behavior with no query-visible tombstones;
- policy/redaction/hidden-endpoint fixtures proving forbidden entity/link bytes
  never enter local SQLite during bootstrap, tail, replay, or rebootstrap;
- SQLCipher-compatible encryption, in-memory-key zeroization, process-restart
  file/WAL/SHM/temp purge crash matrix, five-minute freshness lease, disconnect,
  wall-clock/monotonic fault, suspend/resume, and honest-OS threat-model evidence;
- qualified durable SQLite/PostgreSQL 16 source matrices and the TypeScript
  `LocalReplica` build/test/lint/E2E suite.

Only this separate gate updates frozen FR-32/FEAT-032 statuses and emits
`current-scope-complete` or `hold`. No snapshot/spool/local-materialization/
lease evidence blocks or is required for the finish-line-A `pilot-ready`
verdict.

No bead closes without a referenced commit, valid execution bundle, or
explicit tracker-only/scope disposition. A closed bead whose live behavior
fails is not evidence.

## Dependency graph

```text
fetched baseline
  -> contract / PRD / tracker freeze
  -> typed internal boundary
  -> policy/auth/hash/gate/backup primitives
  -> exclusive legacy migration
  -> schema fail-closed/evolution
  -> declared links + mixed-transaction atomicity

contract freeze -> payload/error
contract freeze -> PG fixture -> backend isolation/audit/durability

schema + links + payload + durability
  -> common framed public streams + opaque resume sessions
  -> V1 graph implementation/evidence
  -> operations + three required consumers
  -> core evidence gate
  -> pilot-ready | hold

frozen FEAT-032 + durability + pilot-ready
  -> local replica materialization + immutable snapshot + lease/encryption
  -> finish-line-B evidence
  -> current-scope-complete | hold
```

PostgreSQL fixture and payload work may run in parallel after contract freeze.
Graph work may start where independent, but final parity waits for schema,
links, payload, and durability.

## Review acceptance

Execution starts only when independent review finds no BLOCKING ambiguity in:
baseline/tracker truth; namespace/raw-write enforcement; migration locking;
schema/link evolution and delete/final-state semantics; canonical measurement;
PostgreSQL/memory audit truth; graph gates; or replica transaction/snapshot/
security behavior.

## Reviewer output contract

Produce exactly:

### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING or WARNING or NOTE | area | specific issue | section/path/requirement | concrete correction |

### Verdict: APPROVE | REQUEST_CHANGES | BLOCK

### Summary

Two to four sentences. Do not praise the plan or omit disagreements.
