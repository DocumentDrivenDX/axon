# Adversarial Review Target: Axon Gap-Closure Plan v1

## Role and framing

You are a critic, not a validator. Your job is to find every way this plan
could fail, every constraint it leaves undefined, every assumption it bakes in
without stating, and every interface it leaves ambiguous. A BLOCKING finding
is anything that would cause implementation rework, a migration hazard, a
security or durability regression, or a spec gap that competent agents would
interpret differently. Do not balance criticism with praise.

Every finding must cite a specific plan section, governing-artifact path and
requirement, code path, test gap, or repository fact. Unsupported findings are
invalid.

## Repository facts

- Current dirty checkout: master at v0.3.2, HEAD 64d2bf60, 115 commits behind
  the local origin/master ref. User-owned changes exist in
  crates/axon-api/src/bead.rs and crates/axon-cli/src/main.rs, plus an untracked
  execution directory.
- Local origin/master ref: ede4ade3, workspace version 0.4.0. No fetch was
  performed for this planning review.
- Local origin/master tracker: 1128 closed and 19 open readiness beads. Final
  readiness beads axon-b4f5bb82 and axon-5744d96b are open.
- The v0.3.2 checkout's cargo check, required clippy, and format gate pass.
  cargo test --workspace fails because the PostgreSQL test fixture times out
  acquiring connections; a single-thread targeted PostgreSQL test also times
  out. UI unit/type checks, SDK checks, and traceability checks pass.
- origin/master includes later audit-atomicity work, so the plan is
  verification-first for that gap rather than assuming it still needs the
  v0.3.2 implementation.
- Current code still permits entity_schema=None and explicitly permits links
  in untyped collections or schemas with an empty link_types catalog.
- StorageCursorStore and a TypeScript LocalReplica exist, but FEAT-032 and
  architecture text still describe them as absent; end-to-end replica
  integration remains incomplete.

## Goal and finish lines

Finish line A: Axon 0.4.x is pilot-ready as a governed storage core: schema
binding and typed links fail closed; entity/link writes are atomic and audited;
memory, SQLite, and PostgreSQL satisfy the same contracts; the documented V1
read-only graph subset is covered; and deployment, recovery, performance, and
consumer evidence are durable.

Finish line B: Axon is complete against the additional current P2 commitment
for a governed local read replica (FR-32 / FEAT-032).

Explicit non-goals: Cypher writes, shortest-path and graph analytics, offline
writes and reconciliation (FR-33), multi-region/distributed placement,
cryptographic audit chaining, and audit-retention/erasure behavior beyond the
existing recorded dispositions.

## Plan

### 0. Establish a safe baseline

- Create a clean gap-closure branch and worktree from local origin/master.
- Leave the dirty v0.3.2 checkout untouched.
- Preserve DDx attempt history: no squash, rebase, amend, or history filtering.
- Re-read the 19 open readiness beads and verify which reviewed gaps have
  already landed upstream.

Exit criteria:
- Clean 0.4.x worktree based on origin/master.
- User changes preserved.
- Queue and dependency facts captured from the clean baseline.
- No duplicate implementation work filed for upstream fixes.

### 1. Reconcile governing artifacts and tracker

- Reconcile the PRD's obsolete local-sync wording with FR-32's current P2
  read-replica scope.
- Refresh FEAT-032, ADR-025, architecture, implementation plan, and test plan
  to describe the partial implementation honestly.
- Add blocking work items for schema fail-closed behavior, typed-link
  integrity, shared payload limits, and PostgreSQL fixture reliability.
- Make final readiness bead axon-b4f5bb82 depend on every new core blocker.

Exit criteria:
- ddx doc validate passes.
- No artifact claims absent code is missing or incomplete code is delivered.
- New work items carry spec-id, named tests, commands, non-scope, and durable
  closure evidence.

### 2. Make schema-first fail closed

- Reject collection schemas with entity_schema=None at every public boundary.
- Reject entity create/update/patch/delete and transaction operations against
  unregistered collections or collections without an active entity schema.
- Keep low-level storage capability for reserved system collections, but
  prevent it from becoming a public handler/API bypass.
- Add doctor diagnostics for existing schema-less or unregistered data.
- Provide an explicit operator migration path to valid schemas; do not ship a
  permanent permissive public-write mode.

Required tests:
- create_collection_rejects_missing_entity_schema
- put_schema_rejects_missing_entity_schema
- create_entity_rejects_unregistered_collection
- public-surface parity for the stable error
- valid schema CRUD and schema evolution regressions

### 3. Make links declared and atomic

- Reject link creation when the source or target is unregistered or lacks an
  active schema, when the source has no link_types declarations, or when the
  requested link type is undeclared.
- Add a strict storage mutation primitive that atomically covers endpoint
  existence, declared type and metadata, cardinality, duplicate detection,
  forward/reverse state, and durable audit.
- Make target deletion and force-cascade atomic with link integrity.
- Add diagnostics and migration guidance for existing undeclared links.

Required cross-backend concurrency/fault tests:
- create_link_rejects_untyped_collection
- create_link_rejects_empty_link_catalog
- concurrent_duplicate_link_exactly_one_succeeds
- concurrent_target_delete_never_leaves_dangling_link
- link_audit_failure_rolls_back_forward_and_reverse_rows

### 4. Enforce request and payload limits below every surface

- Implement the CONTRACT-001 entity-size default and hard maximum in the
  shared handler path.
- Cover entity bodies, link metadata, individual transaction operations, and
  whole batches.
- Return one structured error vocabulary across handler, HTTP, GraphQL, MCP,
  CLI, and SDK.
- Reject before storage and audit mutation.

Exit criteria:
- Boundary tests at limit minus one, limit, and limit plus one.
- Oversized batches apply no partial operations.
- Surface parity fixtures agree.

### 5. Close durability and PostgreSQL verification

- Verify origin/master's audit-atomicity changes with SQLite and PostgreSQL
  fault injection and restart tests; fix only evidence-backed residual gaps.
- Pin supported PostgreSQL versions, wait for readiness, bound provisioning
  concurrency, avoid poisoned global locks, clean up resources, and prohibit
  silent skips in release qualification.
- Add handler and graph parity on PostgreSQL, not only adapter conformance.
- Run the default parallel storage suite repeatedly.

Exit criteria:
- cargo test -p axon-storage passes three consecutive times against a real,
  supported PostgreSQL server.
- Release mode cannot pass when PostgreSQL tests are skipped.
- Durable audit failure rolls back the mutation.
- Memory, SQLite, and PostgreSQL pass the same schema/link/transaction/audit
  contract.

### 6. Complete the declared V1 graph contract

- Do not expand the language beyond the documented read-only Cypher subset.
- Close STP-074 through STP-077 named-query/subscription coverage.
- Add PostgreSQL parity for filters, sorting, aggregations, traversals,
  existence checks, and bounded paths.
- Verify row filtering and redaction before existence, traversal, and
  aggregation.
- Add graph and named-query p99 benchmarks with environment/backend metadata
  and ratchets.

Exit criteria:
- No UNTESTED or UNCITED_COVERAGE rows in the in-scope graph STPs.
- GraphQL, MCP, handler, and SDK agree on results and policy decisions.
- Every published latency claim has a reproducible artifact.

### 7. Finish operational and consumer readiness

- Execute the existing readiness work for installer/service behavior,
  actionable doctor output, health/tenant/auth/TLS proof, SQLite/PostgreSQL
  backup and restore, monitoring, security architecture, and evidence-linked
  deployment checklists.
- Complete Nexiq, DDx, and Cayce release-mode workloads with clean source SHAs,
  native test counts, zero release-blocking skips, and real Axon traffic or
  postcondition evidence.
- Do not substitute documentation deferrals for failing core invariants.

### 8. Complete FEAT-032 after the pilot gate

- Author FEAT-032 user stories and acceptance criteria.
- Test durable cursors across real SQLite reopen and PostgreSQL restart.
- Use one opaque, scope-bound token vocabulary across GraphQL, MCP, SDK, and
  CDC.
- Implement snapshot-then-tail SDK orchestration, deduplication, reconnect,
  search, sorting/filtering, link traversal, and link/entity tombstones.
- Define policy narrowing. Recommended default: invalidate replica scope,
  purge local state, and require governed re-bootstrap.
- Prove denied rows and redacted values never reach the wire or local store.

FEAT-032 does not block finish line A because it is P2. It blocks finish line B.

### 9. Final gates and verdict

Required gates include:
- cargo check
- cargo test
- cargo clippy -- -D warnings
- cargo fmt --check
- CI-exact clippy ratchets
- UI unit, type, citation, and SQLite/PostgreSQL E2E suites
- TypeScript SDK build, test, and lint
- Rust AC traceability
- benchmark suite and artifacts
- consumer self-test and release matrix
- deployment, TLS, backup/restore, and monitoring evidence

Only after evidence exists, update the PRD success criteria with artifact
paths. The final HELIX review must emit exactly one verdict: pilot-ready,
GA-ready, or hold. Beads close only with commits, execution evidence, or an
explicit recorded scope disposition.

## Dependency order

0 baseline -> 1 artifact/tracker alignment -> 2/3/4 fail-closed invariants ->
5 backend durability -> 6 graph contract -> 7 operations/consumers -> 9 final
pilot verdict.

Phase 8 depends on the durable cursor/audit substrate in phase 5 and is the
additional blocker for finish line B.

## Review questions

1. What BLOCKING ambiguities or missing decisions would make two competent
   implementers choose incompatible designs?
2. Does the sequencing preserve existing data and DDx audit history while
   avoiding duplicate upstream work?
3. Are schema, typed-link, transaction, audit, PostgreSQL, graph-policy, and
   local-replica invariants specified strongly enough to test?
4. Does any proposed migration or compatibility path create a public bypass?
5. Can the release gates produce false confidence through skipped backends,
   uncited tests, stale documents, or unarchived evidence?
6. Are any existing open readiness beads missing dependencies on newly found
   core gaps?
7. Is finish line A honestly pilot-ready, and is finish line B honestly
   product-complete against current committed scope?

## Output contract

Produce exactly:

### Findings

| Severity | Area | Finding | Evidence | Required change |
|---|---|---|---|---|
| BLOCKING or WARNING or NOTE | area | specific issue | section/path/requirement | concrete correction |

### Verdict: APPROVE | REQUEST_CHANGES | BLOCK

### Summary

Two to four sentences. Do not omit disagreements or downgrade a finding merely
because another reviewer might accept the risk.
