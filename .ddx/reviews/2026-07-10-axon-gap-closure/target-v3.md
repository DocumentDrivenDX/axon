# Adversarial Review Target: Axon Gap-Closure Plan v3

## Composition and precedence

Read `target-v2.md` in this directory completely. The v2 plan remains in force
except where this v3 overlay replaces or tightens it. If the two conflict, v3
governs. Review the composed plan, not this overlay in isolation.

This overlay resolves every accepted round-2 finding in
`aggregate-r2.md`. Unsupported assumptions from reviewers are not adopted.

## 1. Baseline, governing artifacts, and verdict amendments

### Derive tracker facts after fetch

The bead IDs/counts recorded in v2 are seed observations from the local
`origin/master` ref, not immutable facts. After the mandatory fetch, derive the
open count, core/readiness beads, final evidence gate, and verdict bead from the
pinned tracker. Candidate IDs (`axon-b4f5bb82`, `axon-5744d96b`,
`axon-f48352d5`, `axon-36a3ce2b`, `axon-03269bc7`, `axon-06459077`) are
re-audited only if present. Hard-fail planning if no final gate/verdict chain
exists; create or repair that chain before implementation.

### Expand the Phase 1 contract freeze

Phase 1 also updates:

- CONTRACT-010 and FEAT-001/002 so `entity_schema` is required for every
  governed public or governed system collection; physical/virtual internal
  namespaces use typed non-public contracts instead.
- ADR-003 and ADR-004 to distinguish desired isolation/audit behavior from
  current implementation. PostgreSQL `SERIALIZABLE` is **work to implement**:
  current code issues plain `BEGIN`. ADR-004's stale post-commit-audit text is
  reconciled with live and planned mutation classes.
- The stale `__axon_policies__` documentation in `axon-core::auth`: the pinned
  tree documents this pseudo-collection but never constructs or stores it.
  Phase 1 must either define and implement it as a governed system collection
  or correct the claim; later phases may not assume it exists.
- CONTRACT-005/006 with audit transaction framing and the opaque-cursor break
  described below.

The graph benchmark dataset, hardware class, backend configuration, warmup,
sample count, percentile method, and pass/ratchet thresholds are frozen in the
test plan before graph implementation beads close.

Nexiq, DDx, and Cayce are all required finish-line-A consumers. A failed one
produces `hold` unless a higher-authority scope change is approved before the
evidence run. This plan can emit only `pilot-ready` or `hold`; `GA-ready` is
reserved for a separate GA plan and criteria.

## 2. Replace Phase 2's three-name registry with a typed namespace taxonomy

### Complete known inventory

The initial pinned-tree inventory is:

| Name | Class | Storage/audit rule |
|---|---|---|
| `__axon_links__` | physical hidden collection | forward link state; one logical link audit covers it |
| `__axon_links_rev__` | physical hidden collection | reverse index; never separately audited |
| `_cdc_cursors` | physical hidden checkpoint | internal derived state; not actor-audited |
| `__axon_beads__` | governed system collection | full schema, policy, mutation, and atomic-audit rules apply |
| `__mutation_intents` | virtual audit subject | audit entry namespace, not generic entity storage |
| `__axon_policies__` | documented-only on pinned tree | resolve in Phase 1; do not silently treat as live |

Secondary-index tables, durable audit tables, schema catalogs, idempotency
state, and in-memory projections must be listed in an internal-storage
manifest and proven not to be addressable through `CollectionId`. Any
collection-backed internal discovered after fetch is added to the taxonomy
before implementation.

Centralize construction in typed enums/constructors (`SystemCollection` and
`AuditSubject`). A structural test scans source/migrations and fails on an
underscore-prefixed collection/table/audit constant absent from the manifest.
Adding a namespace requires its class, public-access rule, schema rule, audit
rule, backup rule, and tests.

### Positive enforcement, not exemption removal

Removing `validate_collection_name`'s `__` exemption is insufficient. Add one
positive collection-access guard used by every generic handler operation:
create/list/read/update/patch/delete, schema/template/lifecycle, link,
rollback/intent, query/traversal, and each staged transaction operation.

- Generic public paths reject every hidden, virtual, or governed-system name.
- Module-specific bead operations receive a sealed capability for
  `__axon_beads__`; the capability permits the name, not schema/audit bypass.
- Update the built-in bead schema to declare its self-targeting
  `depends-on` many-to-many link and schema-owned lifecycle. Its entity/link
  mutations still use the governed handler and durable audit.
- HTTP, GraphQL, MCP, CLI, both SDKs, and embedded handler tests attempt direct
  entity and transaction writes to every reserved name and must fail.

Inventory every raw `StorageAdapter::put/delete/put_link/delete_link` call.
Move the raw adapter mutation SPI out of Axon's supported public data-store API
and document it as trusted backend implementation surface; downstream embedded
users receive the governed handler/transaction API. Add a CI allowlist that
fails when a new raw mutation call site lacks a declared internal or governed
transaction purpose. This is an intentional pre-1.0 API tightening.

## 3. Replace Phase 2's legacy-read and migration safety rules

Add an explicit `LegacyUnbound` catalog state discovered during upgrade.
Normal embedded, HTTP, GraphQL, MCP, CLI data commands, and SDK reads/writes
against it return 409 `schema_required`. There are no public recovery reads.
Only local `axon migrate schema-bindings --dry-run` and `axon migrate export`
can use the tolerant reader, with database-owner credentials and no network
listener.

“Stop the server” remains operator guidance but is not the lock:

- Memory uses an exclusive in-process migration guard (test/ephemeral only).
- SQLite uses an exclusive database transaction/lock.
- PostgreSQL 16 uses a database-scoped advisory exclusive lock plus a
  maintenance/catalog epoch checked by ordinary governed write transactions.
- Dry-run records catalog epoch and a content signature. Apply acquires the
  exclusive lock, rejects concurrent writers, verifies the epoch/signature,
  and re-runs validation under lock before atomically changing one database's
  catalog. A per-database journal makes retry idempotent.

Tests start a second writer between dry-run/apply and during apply; it must
either make apply reject before mutation or receive the stable maintenance
error. No partial catalog is observable. Backup verification and rollback are
backend-specific evidence, not prose assertions.

## 4. Schema, error, and schema-evolution amendments

Do not overload the existing manifest-hash `schema_mismatch` code. A schema
version changing after transaction validation returns retryable 409
`schema_activation_changed` with `validated_version` and `active_version`.
Manifest mismatch retains its existing code and refresh semantics.

Extend FEAT-017 classification to link declarations:

- Adding `required: true`, optional→required, tightening cardinality, changing
  target collection, tightening metadata schema, or removing a type with
  existing links is breaking.
- Adding an optional link type or loosening a constraint is compatible.
- Dry-run reports affected source entities and links. Force remains explicit,
  audited, and followed by revalidation.
- Existing invalid entities/links remain lazy-readable with warnings. Only a
  repair transaction whose staged final state reduces/removes the violation
  may mutate them; an unrelated write fails. Repair may add a required link or
  delete excess/invalid links, and must finish fully valid.

## 5. Replace Phase 4's final-state and delete algorithms

### Staged final-state validation

Decompose the previously difficult required-link work into:

1. A backend-neutral overlay evaluator loads the transaction snapshot's
   affected entities and inbound/outbound links.
2. It applies all staged entity and link operations in memory without writing.
3. It validates endpoint existence, schema versions, link type/target,
   metadata, duplicate triples, cardinality, and `required` constraints on the
   complete proposed state.
4. Only a valid plan is passed to one adapter transaction; commit rechecks OCC
   and schema/catalog epochs before data and audit co-commit.

Direct single mutations use the same one-operation overlay. Unit tests cover
the evaluator; backend conformance tests cover commit/conflict behavior.

### One entity-delete algorithm

For the entity being deleted, collect the de-duplicated union of inbound and
outbound logical links before changing state.

- If any inbound link exists and `force=false`, reject the entire deletion;
  neither outbound links nor entity nor audit changes.
- If no inbound link exists and `force=false`, atomically delete all outbound
  links, their reverse-index rows, then the entity.
- With `force=true`, atomically delete the full inbound+outbound union,
  including forward and reverse physical state, then the entity.
- Validate required-link final state for every surviving affected source.
- Logical audit entries are ordered by canonical link key, followed by the
  entity deletion, and carry one transaction ID plus transaction framing.

Force cascade and reverse-index cleanup are greenfield implementation, not an
existing behavior to verify. Test entities that are source-only, target-only,
both, self-linked, and part of required-link constraints.

## 6. Replace Phase 5's JSON measurement definition

Create one `axon-core` measurement API implementing RFC 8785 JSON
Canonicalization Scheme (JCS) and returning canonical UTF-8 byte length.
Every surface passes parsed logical values to this function; no surface owns
an independent byte counter. Golden vectors are shared with the TypeScript SDK
and cover object order, Unicode/escaping, integer/float formatting, negative
zero, arrays, whitespace, and patch/default expansion.

Logical limits remain:

- entity data: 1,000,000-byte default, per-collection configurable up to
  10,000,000 bytes;
- link metadata: 64,000 bytes;
- transaction: 100 operations and 10,000,000 aggregate user-controlled bytes;
- user-supplied audit event: 64,000 bytes each and included in the aggregate.

The HTTP/GraphQL transport accepts at most 12,000,000 decompressed bytes,
leaving envelope overhead but never relaxing logical limits. Other surfaces
enforce the same logical measurement. All existing duplicate 100-op checks
delegate to the shared validator. Cross-surface tests assert both byte counts
and error details.

## 7. PostgreSQL isolation, memory audit, and durability amendments

Implement PostgreSQL transactions with an explicitly asserted
`SERIALIZABLE` isolation level, then test `SHOW transaction_isolation` inside
the real transaction. Translate SQLSTATE serialization failures and unique
conflicts to the stable retry/conflict contract. This is code work before the
Phase 4/6 proof, not a preserved current invariant.

Implement memory logical audit co-commit: `MemoryStorageAdapter` stages audit
entries inside the same transaction snapshot/critical section and rolls them
back with data. The handler's queryable in-memory audit object becomes a
derived view rebuilt/synchronized from committed adapter audit. Fault tests
must prove neither-or-both during the process lifetime. Memory remains
non-durable across process loss.

PostgreSQL restart durability, SQLite/PostgreSQL backup/restore tooling, and
the documented PostgreSQL PITR boundary are net-new implementation/evidence
work. Do not pre-mark capability-matrix cells yes until their executable tests
and runbooks pass.

## 8. Graph and consumer evidence amendments

The frozen benchmark contract from Phase 1 is the only allowed pass/fail
baseline; implementation results cannot redefine it post hoc. All three named
consumers are required and cannot be deferred by a documentation-only closure.

## 9. Replace Phase 9's replica consistency details

### Transaction-framed change batches

Amend CONTRACT-005/006 so audit rows from one committed source transaction
receive consecutive `audit_id`s and internal `transaction_index`,
`transaction_size`, and `transaction_last_audit_id` metadata. A standalone
mutation is a group of one. The server reads only complete groups.

External replica/subscription delivery is a `ChangeBatch` representing one
source transaction after policy filtering:

- It contains only visible/redacted events and one opaque `next_cursor` for
  the end of the complete source group; it does not expose hidden operation
  count or raw audit IDs.
- The local replica applies the visible events and stores `next_cursor` in one
  local SQLite transaction. A crash commits neither events nor cursor; replay
  reapplies the whole batch idempotently.
- A fully hidden group emits no data. The next visible batch's opaque cursor
  may advance across it without revealing its content.
- Force cascades and mixed transactions therefore remain atomic in the local
  authorized view. Tests crash after each event in a multi-event batch.

### Immutable snapshot materialization

At bootstrap, memory clones under its read lock; SQLite and PostgreSQL 16 read
entities, links, and max committed audit boundary in one consistent database
snapshot. The server materializes the caller's already policy-filtered and
redacted authorized view into an immutable snapshot spool before returning
page 1.

- Spool order is `(record_kind, collection, id)` and includes both entity and
  link continuations.
- The opaque, scope-bound snapshot token has a default one-hour TTL and every
  page reads the same spool/boundary.
- Concurrent source creates/updates/deletes cannot change pages. After the
  final page, tail begins at the recorded boundary.
- If required audit history is gone before tail begins, return
  `cursor_expired`, purge local state, and re-bootstrap; do not guess or skip.
  Finish-line-B environments must configure retention longer than snapshot
  TTL. This adds no new general retention/erasure product behavior.
- Snapshot spools are deleted at completion/expiry and covered by quota and
  cleanup tests.

### SDK correctness and public cursor break

Replace the TypeScript replica's delimiter-built composite key with a nested
map or length-delimited tuple, so collection/entity strings cannot collide.
Real server+SDK E2E tests inspect the local SQLite store and prove denied rows
and unredacted fields are absent, including after replay and re-bootstrap.

Axon 0.4.x makes an explicit pre-1.0 breaking change: GraphQL/MCP/SDK/CDC no
longer expose or accept raw `audit_id` cursors. Only signed opaque tokens are
valid. Document the break and recovery (`discard cursor; bootstrap for a new
token`) in release notes; do not run ambiguous dual token semantics.

## 10. Final-round acceptance

The composed plan is execution-ready only if reviewers find no BLOCKING item
in these areas:

- namespace inventory and positive public/raw-write enforcement;
- legacy migration exclusivity and recovery boundary;
- schema/link evolution and final-state/delete semantics;
- canonical payload measurement;
- PostgreSQL/memory transaction and audit truth;
- transaction-framed CDC, immutable snapshot paging, SDK local-state secrecy;
- fetched tracker derivation and evidence gates.

Warnings may remain only when the risk, owner, trigger, and non-blocking
disposition are explicit.

## Output contract

Produce exactly:

### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING or WARNING or NOTE | area | specific issue | section/path/requirement | concrete correction |

### Verdict: APPROVE | REQUEST_CHANGES | BLOCK

### Summary

Two to four sentences. Unsupported findings are invalid. Review the composed
v2+v3 plan and cite which document/section supports each finding.
