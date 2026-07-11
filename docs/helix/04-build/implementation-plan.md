---
ddx:
  id: helix.implementation-plan
  depends_on:
    - helix.prd
    - TP-001
  review:
    self_hash: 0e148fd35ce924a18f94e43f85ad3ede981ae523021ed942b820d9365aa8ed97
    deps:
      TP-001: 11ebe8dbc3f32b2b1d254c076f87f297f8283d53821440a8fce077a3963815c3
      helix.prd: b11053b18982ec8f95158d284546dc20773f504bca99ec6c1970d71628f703ad
    reviewed_at: "2026-07-07T08:49:13Z"
---
# Build Plan: Axon

**Version**: 0.4.x (pilot)
**Date**: 2026-04-10
**Revised**: 2026-07-07
**Status**: Living document

This is the build sequencing and execution-readiness artifact. It is the one
artifact in the stack where implementation status belongs; feature-spec
`status` fields describe spec lifecycle, not implementation state.

---

## Scope

**Governing Artifacts**:
- PRD v0.4.x (pilot) release target — `docs/helix/01-frame/prd.md`. The
  earlier operator-requested 0.7.1 target (recorded 2026-06-14) was revoked
  and 0.4.x confirmed on 2026-07-06; see
  [`../06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md`](../06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md).
- Feature specifications FEAT-001..031 (all rewritten to helix 0.6.1 on
  2026-06-10; briefly release-aligned to a 0.7.1 planning target on
  2026-06-14, since revoked in favor of 0.4.x)
  — `docs/helix/01-frame/features/`
- Interface contracts CONTRACT-001..010 —
  `docs/helix/02-design/contracts/`
- Architecture record: ADR-001..024 — `docs/helix/02-design/adr/`
  (no monolithic architecture.md exists; the ADR corpus is the
  architecture authority)
- Test plan — `docs/helix/03-test/test-plan.md` and
  `docs/helix/03-test/feature-story-e2e-traceability.md`

**In scope**: the build slices in this plan. Most are now delivered
(contract conformance, FEAT-021 Kafka transport, BYOC control plane,
opt-in Serializable for key-addressed read sets in B-104, and the local-first
CQRS reframe design in B-105); the remaining open work is predicate/phantom
serializability (future, out of this plan) and test-coverage closeout (B-107).
**Out of scope**:
reopening product or design decisions; live issue state (the tracker owns
that — all 1074 tracker beads are now closed and the queues are empty).

## Current Implementation Baseline (verified 2026-06-10)

Corrects the 0.2.0 plan, which understated delivered work.

**Workspace**: 15 crates (the 0.2.0 plan listed 13 and omitted `axon-cypher`
and `axon-cypher-ast`):

```
axon-api  axon-audit  axon-cli  axon-config  axon-control-plane
axon-core  axon-cypher  axon-cypher-ast  axon-graphql  axon-mcp
axon-render  axon-schema  axon-server  axon-sim  axon-storage
```

Plus three non-Rust surfaces: Admin UI (`ui/`, SvelteKit + Playwright e2e),
TypeScript SDK (`sdk/typescript/`), website (`website/`).

**Corrections to previously misreported status**:

| Prior claim (plan 0.2.0) | Verified reality |
|---|---|
| SCN-001..010 "defined but not coded" | Implemented — `crates/axon-api/tests/business_scenarios.rs`; SCN-011 cross-tenant isolation in `crates/axon-server/tests/scn_011_cross_tenant_isolation_test.rs` |
| BM-001..010 "no benchmark code exists" | Implemented — `crates/axon-api/benches/benchmarks.rs` covers BM-001 through BM-010 |
| FEAT-010 entity state machines "Not started" | Build landed — `crates/axon-server/tests/lifecycle_test.rs` exercises lifecycle/transition behavior |
| FEAT-029 access control "Not started" | Build landed — `crates/axon-api/src/policy.rs`, `crates/axon-server/tests/graphql_policy_contract.rs`, `feat_029_contract_parent.rs` |
| FEAT-030 mutation intents "Not started" | Build landed — `crates/axon-api/src/intent.rs`, `crates/axon-server/tests/{graphql,mcp}_intents_contract.rs` |
| FEAT-031 policy/intents admin UI "Not started" | Build landed — `ui/tests/e2e/` specs incl. `policy-enforcement`, `graphql-policy-console`, `intent-audit-lineage`, plus US-tagged coverage checks (commit `1bebf4e7`) |
| "ACID transactions ... serializable isolation" | **Snapshot Isolation** is the default (FEAT-008 TXN-05). Opt-in **Serializable** validates the key-addressed read set (B-104), preventing write skew over entities read by id; predicate/phantom serializability (SSI/predicate locking) remains future. No surface claims unqualified "serializable" |
| FEAT-009 "graph traversal" | Spec renamed: `FEAT-009-unified-graph-query.md` (Cypher surface, CONTRACT-007); `axon-cypher`/`axon-cypher-ast` implement it with SQLite executor parity and ready/blocked benchmark gates (commit `2ea08772`) |
| FEAT-010 filename | Renamed: `FEAT-010-entity-state-machines.md` |

**Storage backends**: memory, SQLite, PostgreSQL all pass the
`storage_conformance_tests!` macro suite. FoundationDB not started.

**Known contract-vs-implementation gaps** (from CONTRACT-001..010 authoring,
2026-06-10) are tracked in `docs/helix/06-iterate/improvement-backlog.md` and
as tracker beads; they form Slice B-101 below.

## Shared Constraints

- **Isolation honesty**: Snapshot Isolation is the default (FEAT-008 TXN-05).
  Opt-in Serializable (B-104) is scoped to **key-addressed read sets** — no
  artifact, doc, or API may claim unqualified "serializable"; predicate/phantom
  serializability is not provided.
- **Authority order**: Vision > PRD > Technical Requirements > Features >
  Tests > Code. Contract changes go through contract amendment, not code drift.
- **Test-first**: failing tests exist before implementation; no `unwrap()` in
  library code; `cargo clippy -- -D warnings` clean.
- **ADR-018 wire protocol**: pure path-based, tenant-prefixed routing; no new
  un-prefixed routes may be added while B-101 retires the legacy ones.
- **GraphQL primary, MCP agent-native, REST fallback** (2026-04-22 product
  decision): new surface work lands GraphQL/MCP first.
- **Durable closure evidence**: build issues close only with a commit
  reference, execution bundle, or explicit notes
  (`docs/helix/06-iterate/review-malfunction-audit-2026-04-20.md`).

## Implementation Slices

Originally ordered by dependency: conformance debt first (it blocks
contract-frozen surfaces), then transport/guardrail completion, then isolation
upgrade, then the two design-first P1 expansions, then coverage closeout. As
of 2026-06-27 the conformance, transport, guardrail, BYOC, B-104 (opt-in
Serializable for key-addressed read sets), and B-105 (local-first CQRS reframe
design) slices are delivered; the open work is predicate/phantom serializability
(future, out of plan) and B-107 coverage closeout.

Status legend: **DELIVERED** = shipped and verified in-tree; **OUTSTANDING** =
real open gap, not yet built.

| Slice | Status | Story / Area | Governing Artifacts | Depends On | Validation Gate | Notes |
|-------|--------|--------------|---------------------|------------|-----------------|-------|
| B-101 | **DELIVERED** | Contract conformance: dropped `dbName`/`dbPath` (GraphQL+SDK), retired un-prefixed legacy routes (`/auth/me`, `/databases/*`), added SDK governed-workflow methods, tenant-aware MCP endpoints/URIs, Idempotency-Key header deprecation, CONTRACT-008 auth-default amendment | CONTRACT-001, -002, -003, -008, -009; ADR-018; FEAT-028 BIN-10 | None | Verified: `grep -r 'dbName\|dbPath' sdk/typescript/src crates/` returns nothing; `cargo test -p axon-server` contract tests pass; SDK tests pass | Delivered. Beads closed: axon-b8078b63, axon-b684338f, axon-784bc974, axon-95b137d0, axon-c62971d9, axon-87fee98b |
| B-102 | **DELIVERED** | FEAT-021 Kafka CDC transport (Kafka producer, delivery semantics, config; envelope + JSONL/in-memory sinks pre-existed) | FEAT-021, CONTRACT-006, ADR-014 | B-101 (config surface frozen by CONTRACT-008 amendment) | Verified: `KafkaCdcSink` shipped in `crates/axon-audit/src/cdc.rs`; `cargo test -p axon-audit` passes | Delivered — the last FEAT-021 transport is implemented |
| B-103 | **DELIVERED** | FEAT-022 remaining guardrail scope (semantic validation hooks; rate limiting + actor scope already shipped) | FEAT-022, ADR-016, ADR-024 | B-101 | Guardrail hook tests in `crates/axon-server` exercise hook rejection paths; `cargo test -p axon-server` passes | Completes the P0 safety-guardrail vision scope |
| B-104 | **DELIVERED** | Opt-in **Serializable for key-addressed read sets**: `IsolationLevel` + `Transaction::record_read` read-set tracking, commit-time read validation (first-committer-wins, surfaced as `ConflictingVersion`), per-transaction `isolation_level()` inspectability (TXN-05) | FEAT-008 (TXN-05, Constraints), ADR-004 | B-101 | Verified: `cargo test -p axon-sim write_skew` (SI allows skew, Serializable prevents it) + `cargo test -p axon-api --lib proptest_api` (`serializable_prevents_write_skew_that_snapshot_allows`) + `transaction::tests::write_skew_*` | Delivered honest-scope per adversarial review (`AR-2026-06-27` §2 H2). **Predicate/phantom serializability (SSI/predicate locking) is NOT included** and remains future work — no surface claims unqualified "serializable" |
| B-105 | **DELIVERED** | Local-first **CQRS reframe** (replaces the original standalone sync design): FR-32 rewritten as a local read-replica projection; FEAT-032 authored; ADR-014 amended + ADR-025 (client-projection cursor API); architecture read-side made explicit. The in-tree primitives (`StorageCursorStore`, `CursorToken`, `LocalReplica`, snapshot bootstrap) already exist; the remaining work is end-to-end wiring onto the opaque cursor path. FR-33 writeback parked | PRD FR-32/FR-33, FEAT-032, ADR-014/ADR-025 | None (design) | Design merged in `docs/helix/`; FEAT-032 + ADR-025 present; parking-lot/Non-Goals updated | Delivered per `AR-2026-06-27` §2 H1. Read-replica build work now converges the wired surfaces on the existing primitives; FR-33 remains parked |
| B-106 | **DELIVERED** | BYOC (FR-27 P1) control plane: tenant/user/credential/database/member management shipped | PRD FR-27, ADR-017, FEAT-025 | B-101 | Verified: control plane shipped in `crates/axon-control-plane`; FEAT-025 acceptance criteria pass in `crates/axon-control-plane`/`axon-server` tests | Delivered. Any residual BYOC packaging beyond the shipped control plane carries forward into ordinary release work, not a distinct open slice |
| B-107 | partial | Story-test-plan / coverage closeout: PROP-002..005 property tests, L6 contract-suite completion, story-test-plans for remaining non-guardrail features per test-plan §AC allocation | test-plan.md, feature-story-e2e-traceability.md, STP set | B-101..B-104 (tests target final surfaces) | `cargo test` workspace green; traceability doc shows no unallocated ACs | Last: validates the completed surfaces; final closeout follows B-104 |

## Issue Decomposition

Story-level work is tracked as beads in `.ddx/beads.jsonl` via `ddx bead`.

**Per-issue requirements**:
- Labels: `helix`, `activity:build`, plus area labels (`area:api`,
  `area:sdk`, `area:mcp`, ...)
- References: governing FEAT/CONTRACT/ADR paths in the description
- Acceptance criteria naming observable repo states (grep/test commands)
- Blockers as `--depends-on` links
- Closure requires durable evidence (commit ref, execution bundle, or notes)

| Story / Area | Goal | Dependencies | Status |
|--------------|------|--------------|--------|
| B-101 conformance (6 beads closed: axon-b8078b63, axon-b684338f, axon-784bc974, axon-95b137d0, axon-c62971d9, axon-87fee98b) | Code matches CONTRACT-001..010 as written | None | DELIVERED |
| B-102 Kafka transport | FEAT-021 complete | axon-87fee98b (config contract) | DELIVERED |
| B-103 guardrail hooks | FEAT-022 complete | None hard; sequence after B-101 | DELIVERED |
| B-104 opt-in Serializable (key-addressed read sets) | FEAT-008 TXN-05 write-skew constraint discharged for key-addressed reads | Sequenced after B-101 | DELIVERED (predicate serializability future) |
| B-105 local-first CQRS reframe (FEAT-032 + ADR-025) | FR-32 reframed as read-replica projection; FR-33 writeback parked | None (design work) | DELIVERED |
| B-106 BYOC remainder | FR-27 P1 scope complete | B-101 | DELIVERED |
| B-107 coverage closeout | No unallocated ACs; PROP-002..005 coded | B-101..B-104 | partial |

## Validation Plan

- [ ] Failing tests exist before implementation starts (test-first)
- [ ] Each slice's validation gate (table above) passes before its beads close
- [ ] `cargo check && cargo test && cargo clippy -- -D warnings && cargo fmt --check` green at every slice exit
- [ ] Behavior changes update canonical documents (contracts amended before code diverges)
- [ ] Bead closures carry durable evidence per the review-malfunction audit rule
- [ ] Code review complete before activity exit

## Risks and Rollbacks

| Risk | Impact | Response | Rollback |
|------|--------|----------|----------|
| Retiring legacy routes (`/auth/me`, `/databases/*`) breaks an unnoticed consumer | M | Land as 410 + deprecation header first, removal in a follow-up release | Re-register legacy handlers (single router commit revert) |
| CONTRACT-008 auth-default flip locks operators out of fresh installs | H | Ship explicit opt-out flag and doctor diagnostic in the same change | Revert default in config layer; contract amendment is additive |
| Kafka transport pulls heavyweight deps into axon-audit | M | Feature-gate behind a cargo feature; keep JSONL sink the default | Disable the cargo feature |
| Read-set tracking for serializable regresses commit throughput | L | Delivered with read-set capture gated to Serializable transactions only (Snapshot path unchanged, allocation-free) and bounded by `MAX_READS`; SI remains the default. BM suite (`crates/axon-api/benches/benchmarks.rs`) guards >10% regression | Keep SI the default; serializable behind a transaction option |
| Local-first read-replica scope balloons during build | M | FEAT-032 scopes the remaining wiring surface (durable/opaque cursors, client query engine) explicitly; FR-33 writeback parked until concrete adopter demand | N/A (design only so far) |
| SDK governed methods drift from server intents API | M | SDK tests run against the same contract fixtures as `graphql_intents_contract.rs` | Mark methods experimental in SDK semver |

## Exit Criteria

- [ ] Build issue set is defined with sequence and dependencies (B-101 beads filed; B-102+ beads created when their slice opens)
- [ ] Shared constraints are documented
- [ ] Verification expectations are explicit per slice
- [ ] Runtime issues can be created from this plan without inventing scope

## Review Checklist

- [x] Governing artifacts are listed and exist on disk
- [x] Shared constraints trace back to requirements, design, or architecture
- [x] Build sequence has a justified ordering — conformance debt first, design-first items gated on ADRs
- [x] Dependencies between build steps are explicit
- [x] Each slice references its governing artifacts
- [x] Issue decomposition follows tracker conventions (labels, refs, deps, closure evidence)
- [x] Quality gates are specific and enforceable
- [x] Risks have concrete responses and rollbacks
- [x] Plan is consistent with the governing test plan and FEAT-008 isolation truth

---

*This document tracks build sequencing against the planning stack. Updated as slices complete or priorities change.*
