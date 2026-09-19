# Adversarial Review Target: Axon Gap-Closure Plan v2

## Role and framing

You are a critic, not a validator. Find any remaining ambiguity that would
produce incompatible implementations, unsafe migration, false readiness,
security bypass, lost or unaudited writes, backend divergence, or an
unmeasurable finish line. Do not praise the plan. Every finding must cite a
plan section, governing artifact, code path, test gap, or repository fact.

## What “complete” means

This plan does not claim that Axon is a general-purpose or distributed graph
database. It closes the current governed 0.4.x product scope in two explicit
stages:

- **Finish line A — pilot-ready governed core.** Every public collection and
  write is schema-bound; every supported link mutation is declared,
  referentially safe, atomic, and append-only audited; mixed entity/link
  transactions are all-or-nothing; SQLite and PostgreSQL are restart-durable;
  memory has equivalent logical semantics but is explicitly non-durable; the
  documented read-only Cypher subset and policy rules are implemented and
  tested; operational and consumer evidence is archived.
- **Finish line B — current committed scope complete.** Finish line A plus the
  P2 governed local read replica in FR-32/FEAT-032.

“Audited” at these finish lines means durable append-only logical audit for
governed mutations. Cryptographic tamper evidence remains out of scope.

Non-goals remain: Cypher writes; shortest-path, centrality, and other graph
analytics; offline writes/reconciliation (FR-33); distributed transactions or
multi-region placement; cryptographic audit chaining; and new audit retention
or erasure behavior.

## Governing decisions this plan preserves

- ADR-004 and FEAT-008 mandate OCC. SQLite commits use `BEGIN IMMEDIATE`,
  PostgreSQL commits use `SERIALIZABLE`, and V1 must not introduce
  `SELECT FOR UPDATE` or application-level pessimistic locking.
- ADR-007 defines the active schema as the latest committed schema version.
  FEAT-017 owns compatibility classification, dry-run, forced breaking
  changes, lazy reads, and revalidation.
- FEAT-007 defines source-owned, unidirectional link declarations. The target
  schema need not declare a reverse link. V1 supports link create and delete;
  it does not expose in-place link or link-metadata update.
- FEAT-004 defines entity deletion as not-found on the live read path, with
  history in audit rather than tombstone reads.
- CONTRACT-006 selects monotonic database audit sequence (`audit_id`) as the
  CDC/replica offset. PostgreSQL LSNs and transaction IDs are not replica
  cursors.
- CONTRACT-007 fixes the V1 graph limits: default path depth 10,
  unindexed-scan threshold 1,000 entities, worst-case cardinality 1M
  intermediate rows, and default timeout 30 seconds.

## Phase 0 — Establish authoritative baseline and tracker truth

1. Fetch `origin` with pruning. Record remote URL, fetch timestamp, and the
   exact `origin/master` SHA. If fetch is unavailable, stop; a stale local ref
   is not an implementation baseline.
2. Create an isolated clean worktree and branch from that recorded SHA. Run
   all DDx and build commands from it, never from the dirty v0.3.2 checkout.
3. Preserve the user's modified `bead.rs`, `main.rs`, and untracked execution
   bundle. Record a binary-safe diff, untracked-file inventory, and checksums.
   Compare those changes to the pinned 0.4.x files, but do not port or discard
   them without a separate user decision; mark them parked.
4. Build a requirements-to-evidence matrix from the pinned baseline. Audit
   open *and closed* core/readiness beads, not only the 19 open beads. Re-run
   each claimed behavioral acceptance test.
5. Specifically re-audit `axon-f48352d5`, `axon-36a3ce2b`,
   `axon-03269bc7`, and `axon-06459077`. If a closed bead's promised behavior
   is absent, reopen it with the failed command and evidence, or create a
   superseding corrective bead that blocks readiness. Do not file duplicate
   parallel work while leaving a false closure intact.

Exit evidence:

- Pinned fetched SHA and clean worktree path.
- Parked-user-change manifest.
- Open/closed bead audit with command output and live code/test references.
- No implementation work begins until false tracker closures are repaired.

## Phase 1 — Freeze contracts, success criteria, and dependency graph

Update governing artifacts before implementation:

1. Correct PRD Goal 7's offline/bidirectional-sync promise to match FR-32
   read replica and FR-33 deferral; name FR-31 resumable streams as a
   dependency.
2. Refresh FEAT-032, ADR-025, architecture, implementation plan, and test plan
   to describe the existing `StorageCursorStore`, snapshot response, and
   TypeScript `LocalReplica` as partial and currently unwired/incomplete.
3. Add PRD success criteria for:
   schema fail-closed behavior; schema evolution/revalidation; typed-link and
   mixed-transaction integrity; payload limits; PostgreSQL qualification;
   graph-policy/limit coverage; and FR-32 replica completion.
4. Add or amend normative contracts for:
   the sealed system-collection registry and audit exemptions; schema/link
   stable errors; canonical payload measurement and aggregate transaction
   limit; full surface error mappings; and the PostgreSQL support statement.
5. Make PostgreSQL 16 the sole release-qualified PostgreSQL major for Axon
   0.4.x, matching the existing CI service and compose image. Change broad
   “15+” claims to “16”; other majors are unsupported until separately added
   to the qualification matrix.
6. Author and freeze FEAT-032 user stories and acceptance criteria before any
   replica implementation bead is claimed.

Tracker wiring:

- Every core corrective/verification bead blocks `axon-b4f5bb82`.
- Preserve the existing `axon-b4f5bb82 -> axon-5744d96b` readiness chain, so
  all core blockers transitively block the final pilot verdict.
- Create a separate finish-line-B evidence gate and review. It depends on the
  pilot verdict plus every FR-32 implementation/evidence bead; P2 replica work
  must not delay or silently contaminate the finish-line-A verdict.
- Work items must name governing requirement IDs, files, behavioral tests,
  exact verification commands, non-scope, and durable closure evidence.

Exit evidence:

- `ddx doc validate` and stale-document checks pass.
- Requirements, contracts, PRD criteria, and tracker dependencies agree.
- No implementation bead defines its own missing product semantics.

## Phase 2 — Seal internal storage and provide a one-time legacy upgrade

### Internal/public boundary

Create one sealed system-collection registry containing the current internal
names only:

- `__axon_links__`
- `__axon_links_rev__`
- `_cdc_cursors`

Adding a name requires updating the registry and its contract tests. Remove
the public handler's unconditional `__` naming exemption. Every public
handler, HTTP, GraphQL, MCP, CLI, and SDK path rejects reserved names; only
crate-private typed internal APIs can construct or access them. Raw
`StorageAdapter::put` remains an adapter capability, not an exported governed
write surface.

Audit classification is explicit:

- Governed logical mutations requiring atomic durable audit: public/admin
  collection, schema, template, entity, link, force-cascade, and transaction
  mutations.
- Internal derived/checkpoint state exempt from separate actor audit:
  forward/reverse physical index maintenance, secondary indexes,
  `_cdc_cursors`, idempotency cache state, and in-memory audit projections.
  These writes must be reachable only through sealed internal APIs. A logical
  link mutation produces one link audit record, not separate forward/reverse
  physical-row audit records.

### Legacy upgrade tool

Implement an offline-only `axon migrate schema-bindings` command. It requires
the server to be stopped, an operator-provided mapping from every legacy
public collection to a complete schema/link catalog, and a verified backup.

- `--dry-run` scans all public collections, entities, and links through a
  read-tolerant internal reader and emits deterministic validation errors.
- `--apply` proceeds only when every entity validates and every existing link
  maps to a declared type/target/cardinality. It installs registrations and
  schemas with an idempotent migration journal. It never silently invents a
  schema, deletes data, or quarantines records.
- A failed apply leaves the pre-upgrade catalog active; rerunning the same
  mapping is idempotent. Recovery restores the required backup and recorded
  catalog snapshot.
- Until migrated, legacy public collections remain readable for recovery but
  all public writes fail with `schema_required`. There is no network-exposed
  force flag or permanent permissive mode.

Required tests include reserved-name rejection on every surface, proof that
internal cursor/link paths still work, dry-run/apply/idempotency/rollback, and
proof that the migration reader is unreachable from public routes.

## Phase 3 — Make schemas fail closed without breaking evolution semantics

1. `create_collection` and `put_schema` reject `entity_schema: null` or an
   invalid JSON Schema with 422 `schema_validation` and structured reason
   `entity_schema_required`.
2. Entity create/update/patch/delete and every transaction operation require
   a registered public collection and active schema. An unknown collection is
   404 `not_found`; a legacy registered collection without active schema is
   409 `schema_required`.
3. Active means the latest committed version per ADR-007. Each write records
   the schema version used for validation. If activation changes before
   commit, the transaction aborts with retryable 409 `schema_mismatch`; no
   operation or audit row commits.
4. Implement/verify all FEAT-017 behavior: compatibility classification and
   dry-run before activation; concurrent schema updates produce one winner;
   forced breaking changes are explicit and audited; background revalidation
   reports invalid old entities; lazy reads continue with stored schema
   version and warnings; every subsequent write validates the active schema.
   V1 does not invent automatic transformation rules that FEAT-017 defers.
5. Run a final integrity scan proving that every non-forced pilot dataset is
   registered and schema-bound and that every forced-breaking exception has
   an explicit evidence-linked disposition.

Contract tests cover create, replace, patch, delete, lifecycle transitions,
rollback, intents, direct transactions, generated GraphQL, MCP, HTTP, CLI,
and both SDKs. Include schema-change-during-transaction and legacy-read-only
fixtures on memory, SQLite, and PostgreSQL.

## Phase 4 — Make declared links and mixed transactions atomic

### Link declaration semantics

- Source and target collections must both be registered with active entity
  schemas.
- The source's active schema must declare the exact link type. An absent or
  empty `link_types` catalog fails closed.
- The declaration's `target_collection` must equal the requested target. A
  reverse declaration in the target is not required.
- Metadata validates against the source declaration's active
  `metadata_schema`.
- Cardinality has one definition everywhere: `one-to-one` limits both source
  outbound and target inbound to one; `one-to-many` limits target inbound;
  `many-to-one` limits source outbound; `many-to-many` adds no one-side cap.
- `required: true` means each surviving source entity must have at least one
  outgoing link of that type in the transaction's final state. Creating such
  an entity requires a mixed transaction that also creates the required link;
  deleting the last required link is rejected unless the source entity is
  deleted in the same transaction.

### Mutation and deletion semantics

Supported V1 logical link mutations are `create_link`, `delete_link`, and
link deletions caused by entity force-cascade. In-place link/metadata update is
not a V1 surface and must be stated as such in contracts.

- A source delete atomically removes and audits its outbound links.
- A target delete with inbound links is rejected by default. `force=true`
  atomically deletes inbound and outbound links, writes one audit entry per
  logical link plus the entity deletion under one transaction ID, and leaves
  no live-read tombstone.
- Direct link create/delete and mixed entity/link transactions use the same
  storage transaction primitive. Endpoint existence, active schema versions,
  type/metadata, required-link final state, cardinality, duplicate triple,
  forward/reverse state, entity changes, and durable audit commit together.

### Backend mechanism and tests

Preserve OCC rather than adding pessimistic locks:

- Memory: one adapter transaction critical section with whole-store rollback;
  equivalent logical atomicity, no restart-durability claim.
- SQLite: `BEGIN IMMEDIATE` plus uniqueness constraints and one SQL commit.
- PostgreSQL: `SERIALIZABLE` plus uniqueness constraints; translate unique or
  serialization conflicts to the stable retry/conflict contract.

Use deterministic `axon-sim` schedules for backend-neutral state-machine
invariants and real threaded/process concurrency tests for SQLite/PostgreSQL.
Inject failures after forward state, after reverse/index state, after entity
state, before audit append, after audit append/before commit, and at commit.
Every pre-commit failure must leave neither logical mutation nor audit after
restart; success must leave both.

Required tests include untyped/empty-catalog rejection, wrong target,
metadata failure, all cardinalities, required links, duplicate races,
delete/create races, force cascade, mixed rollback, audit failure, and restart
recovery on all applicable backends.

## Phase 5 — Enforce canonical limits and structured error parity

Amend CONTRACT-001 before code with these exact logical measurements:

- Entity data: compact UTF-8 JSON bytes after patch/default application and
  before adding the system envelope; default 1,000,000 bytes per collection,
  configurable only downward/upward to the 10,000,000-byte hard maximum.
- Link metadata: compact UTF-8 JSON bytes, maximum 64,000 bytes.
- Transaction count: maximum 100 operations.
- Aggregate transaction user payload: sum of each operation's compact entity
  data/link metadata bytes, maximum 10,000,000 bytes. This is separate from a
  transport body limit, which must allow the fixed JSON envelope overhead but
  may never relax the logical limits.

Measure at the shared governed handler below every public surface and before
storage or audit. For patch, measure the merged final entity. Reject a batch
before applying operation 1 if any operation or aggregate limit fails.

Add `AxonError::PayloadLimitExceeded { kind, limit_bytes, actual_bytes,
operation_index }` in `axon-core`. Preserve the contract's distinct classes:
payload/count failures map to 400 `invalid_argument`; schema shape failures
remain 422 `schema_validation`. Define exact mappings for HTTP body/extensions,
GraphQL extensions, MCP `ToolError`, CLI exit/status JSON, Rust SDK, and
TypeScript SDK. “One vocabulary” means shared reason/detail fields, not one
wire code.

Tests cover minus one/exact/plus one, Unicode/multibyte JSON, whitespace/key
order equivalence after parsing, patch expansion, 100/101 operations,
aggregate overflow, no partial write/audit, and cross-surface golden fixtures.

## Phase 6 — Make PostgreSQL and durability qualification trustworthy

### Fixture and release mode

Consolidate PostgreSQL setup into one test-support module:

- CI/release uses the existing PostgreSQL 16 service and
  `AXON_TEST_POSTGRES`; connection/readiness failure is fatal after a bounded
  30-second readiness loop.
- Local developer mode may provision testcontainers when the variable is
  absent. A missing container runtime may skip only outside release mode and
  must print an explicit skipped count/reason.
- `AXON_REQUIRE_POSTGRES=1` converts every unavailable/skip path into failure.
- Use unique databases or schemas per test, bounded pools, and deterministic
  teardown instead of a poisonable process-global mutex. Run the suite under
  default parallelism.

### Capability matrix and fault proof

Publish and test this matrix:

| Capability | Memory | SQLite | PostgreSQL 16 |
|---|---|---|---|
| Governed validation/link/transaction semantics | yes | yes | yes |
| Atomic logical audit view | yes, process lifetime | yes | yes |
| Reopen/restart durability | no | yes | yes |
| Backup/restore qualification | no | file copy/restore | dump/restore and documented PITR boundary |

Verify all governed mutation classes—not only entities—co-locate data and
audit on SQLite/PostgreSQL. Fault points are: before mutation, after logical
data/index writes, before durable audit, after durable audit/before commit,
commit failure, and process restart. The only allowed states are neither data
nor audit, or both data and audit. Internal checkpoint/index exemptions follow
Phase 2 and are tested as unreachable from public surfaces.

Release commands must include
`AXON_REQUIRE_POSTGRES=1 AXON_TEST_POSTGRES=... cargo test -p axon-storage`
three consecutive times plus handler, graph, and server PostgreSQL E2E suites.
No pass may contain a PostgreSQL skip.

## Phase 7 — Complete—not merely cite—the V1 graph contract

Split work into two classes:

1. **Implementation gaps:** implement STP-075 index-threshold rejection,
   policy-bypass rejection, and dry-run compile; STP-076 cardinality-budget
   rejection; and any other live behavior still marked UNTESTED.
2. **Evidence gaps:** add missing `@covers`/citations such as STP-077 only
   after the named tests pass.

Do not expand CONTRACT-007. Enforce its exact limits and stable errors. Policy
semantics are likewise exact: row policy at every label match; redacted fields
are null at projection and unusable in predicates/aggregations; hidden targets
do not affect `EXISTS`; counts/aggregates include only visible rows; link
properties receive the same redaction. Query schema/policy snapshots are fixed
at query start.

Run the same fixtures on handler, GraphQL, MCP, Rust SDK, TypeScript SDK,
SQLite, and PostgreSQL 16 for filters, order/pagination, aggregation,
existence, named queries, subscriptions, and bounded paths. Add reproducible
p99 benchmarks with hardware, dataset, backend, build SHA, configuration, raw
samples, and ratchet thresholds.

Exit evidence:

- All STP-074..077 behavior implemented; no in-scope UNTESTED or
  UNCITED_COVERAGE row.
- Policy leak fixtures and limit rejections agree on every surface/backend.
- Published latency claims link to archived benchmark artifacts.

## Phase 8 — Close operational and consumer readiness

Execute the existing readiness queue for installer/service behavior,
actionable doctor output, health/tenant/auth/TLS proof, SQLite/PostgreSQL
backup and restore, monitoring, threat/security architecture, and
evidence-linked deployment checklists.

Run Nexiq, DDx, and Cayce release workloads from recorded clean source SHAs.
Archive native command, test count, skip count/reason, Axon request evidence,
and postconditions. A documented deferral may remove an optional consumer from
the pilot claim, but it cannot waive a core schema/link/durability invariant.

## Phase 9 — Complete the governed local read replica (finish line B)

Implement only after FEAT-032 acceptance criteria are frozen and Phase 6's
durable audit/cursor substrate is green.

### Consistency contract

- Snapshot captures a database `audit_id` high-water mark at start; every
  page carries the same boundary and stable entity-ID continuation.
- Tail applies events with `audit_id > boundary`. Delivery is at-least-once;
  the dedupe key is `(tenant, database, audit_id)`. Cursor persistence advances
  only after the local transaction commits.
- Cross-entity ordering follows `audit_id` within a database; per-entity
  ordering follows the same sequence. Entity and link delete events/tombstones
  remove local rows/edges before advancing the cursor.
- Reconnect requests replay after the last durable cursor; duplicate replay is
  harmless. SQLite reopen and PostgreSQL 16 restart tests prove the behavior.

### Scope and security contract

One signed opaque token vocabulary serves GraphQL subscriptions, MCP
notifications, SDK change readers, and CDC. Bind the token to tenant,
database, authenticated principal/grant, collection/query scope, schema
manifest hash, policy hash/version, audit boundary, issued-at, and expiry.
Scope mismatch, expiry, revocation, or hash change returns a stable
`cursor_scope_invalid` error and never broadens access.

For V1, *any* schema-manifest or policy hash change—narrowing, broadening,
redaction change, or link-type change—invalidates the replica token, purges
local state, and requires governed re-bootstrap. This conservative rule avoids
incremental visibility mistakes. Denied rows and unredacted values must never
cross the wire or enter the local database.

Wire `StorageCursorStore` into the real producer/change-reader path, then
complete snapshot-then-tail orchestration, dedup/reconnect, local
search/sort/filter, declared-link traversal, and entity/link tombstones. No
offline writes or reconciliation are added.

Finish-line-B evidence includes real server + TypeScript SDK E2E tests,
restart/replay fault tests, token-scope negative tests, policy/schema-change
purge tests, and the new FR-32 PRD success criterion.

## Phase 10 — Evidence gates and verdicts

Core gate runs from the pinned clean worktree and archives full output:

- `cargo check --workspace`
- `cargo test --workspace`
- CI-exact clippy command with all ratchets and `-D warnings`
- `cargo fmt --all -- --check`
- PostgreSQL-required repeated storage plus handler/graph/server E2E commands
- UI unit/type/citation/E2E suites
- TypeScript SDK build/test/lint and Rust SDK/AC traceability
- graph and named-query benchmarks
- consumer release matrix
- deployment, TLS, backup/restore, monitoring, doctor, and security evidence
- `ddx doc validate`, stale-document checks, and release-claim inventory

Only after these pass may `axon-b4f5bb82` update the core PRD criteria, and
only then may `axon-5744d96b` emit `pilot-ready`, `GA-ready`, or `hold`.

The separate finish-line-B gate runs the core evidence plus Phase 9's replica
criteria and emits a second scope-completion verdict. No bead closes without a
referenced commit, valid execution bundle, or explicit tracker-only/scope
disposition; a closed bead whose live behavior fails is not accepted as
evidence.

## Dependency order

```text
authoritative baseline
  -> contract / PRD / tracker freeze
  -> sealed internal boundary + legacy migration
  -> schema fail-closed
  -> declared links + mixed transaction atomicity

contract freeze -> payload/error enforcement
contract freeze -> PostgreSQL fixture reliability -> durability proof

schema + links + payload + durability
  -> V1 graph implementation/evidence
  -> operations/consumers
  -> core evidence gate
  -> pilot verdict (finish line A)

frozen FEAT-032 + durability proof + pilot verdict
  -> replica implementation/evidence
  -> finish-line-B verdict
```

PostgreSQL fixture work and payload enforcement may proceed in parallel after
Phase 1. Graph work may start in parallel only where it does not depend on
schema/link contract changes; its final parity gate waits for both.

## Review questions

1. Does any undefined public/internal or migration boundary remain?
2. Can any supported mutation commit data without its required logical audit,
   or audit without data, under failure or concurrency?
3. Do schema evolution, required links, mixed transactions, deletion, and
   limit/error semantics admit incompatible implementations?
4. Can a skipped/flaky PostgreSQL path, false closed bead, stale ref, or
   citation-only test produce a false readiness verdict?
5. Are graph policy/limit semantics and replica consistency/scope semantics
   testable without inventing missing requirements during implementation?
6. Is finish line A honest about memory durability and product scope, and is
   finish line B independently measurable?

## Output contract

Produce exactly:

### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING or WARNING or NOTE | area | specific issue | section/path/requirement | concrete correction |

### Verdict: APPROVE | REQUEST_CHANGES | BLOCK

### Summary

Two to four sentences. Unsupported findings are invalid. Do not omit
disagreements or downgrade a finding merely because another reviewer might
accept the risk.
