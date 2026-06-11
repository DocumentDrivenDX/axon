---
ddx:
  id: helix.implementation-plan
  depends_on:
    - helix.prd
    - TP-001
---
# Build Plan: Axon

**Version**: 0.3.0
**Date**: 2026-04-10
**Revised**: 2026-06-10
**Status**: Living document

This is the build sequencing and execution-readiness artifact. It is the one
artifact in the stack where implementation status belongs; feature-spec
`status` fields describe spec lifecycle, not implementation state.

---

## Scope

**Governing Artifacts**:
- PRD v0.4.0 — `docs/helix/01-frame/prd.md`
- Feature specifications FEAT-001..031 (all rewritten to helix 0.6.1 on
  2026-06-10) — `docs/helix/01-frame/features/`
- Interface contracts CONTRACT-001..010 —
  `docs/helix/02-design/contracts/`
- Architecture record: ADR-001..024 — `docs/helix/02-design/adr/`
  (no monolithic architecture.md exists; the ADR corpus is the
  architecture authority)
- Test plan — `docs/helix/03-test/test-plan.md` and
  `docs/helix/03-test/feature-story-e2e-traceability.md`

**In scope**: the forward build slices in this plan (contract conformance,
FEAT-021 Kafka transport, FEAT-022 remaining guardrails, serializable
isolation, local-first sync design, BYOC remaining scope, test-coverage
completion). **Out of scope**: reopening product or design decisions; live
issue state (the tracker owns that).

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
| "ACID transactions ... serializable isolation" | **Snapshot Isolation** is the V1 default (FEAT-008 TXN-05). Write-set OCC does not detect write skew; read-set tracking for Serializable is committed as **P1**. Read-committed is an opt-in |
| FEAT-009 "graph traversal" | Spec renamed: `FEAT-009-unified-graph-query.md` (Cypher surface, CONTRACT-007); `axon-cypher`/`axon-cypher-ast` implement it with SQLite executor parity and ready/blocked benchmark gates (commit `2ea08772`) |
| FEAT-010 filename | Renamed: `FEAT-010-entity-state-machines.md` |

**Storage backends**: memory, SQLite, PostgreSQL all pass the
`storage_conformance_tests!` macro suite. FoundationDB not started.

**Known contract-vs-implementation gaps** (from CONTRACT-001..010 authoring,
2026-06-10) are tracked in `docs/helix/06-iterate/improvement-backlog.md` and
as tracker beads; they form Slice B-101 below.

## Shared Constraints

- **Isolation honesty**: V1 is Snapshot Isolation (FEAT-008 TXN-05). No
  artifact, doc, or API description may claim serializable until B-104 lands.
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

Ordered by dependency: conformance debt first (it blocks contract-frozen
surfaces), then transport/guardrail completion, then isolation upgrade, then
the two design-first P1 expansions, then coverage closeout.

| Slice | Story / Area | Governing Artifacts | Depends On | Validation Gate | Notes |
|-------|--------------|---------------------|------------|-----------------|-------|
| B-101 | Contract conformance: drop `dbName`/`dbPath` (GraphQL+SDK), retire un-prefixed legacy routes (`/auth/me`, `/databases/*`), SDK governed-workflow methods, tenant-aware MCP endpoints/URIs, Idempotency-Key header deprecation, CONTRACT-008 auth-default amendment | CONTRACT-001, -002, -003, -008, -009; ADR-018; FEAT-028 BIN-10 | None | `grep -r 'dbName\|dbPath' sdk/typescript/src crates/axon-server crates/axon-control-plane` returns nothing; `cargo test -p axon-server` contract tests pass; SDK tests pass | First: every later slice extends these surfaces; conformance debt compounds. Beads: axon-b8078b63, axon-b684338f, axon-784bc974, axon-95b137d0, axon-c62971d9, axon-87fee98b |
| B-102 | FEAT-021 Kafka CDC transport (envelope + JSONL/in-memory sinks exist; Kafka producer, delivery semantics, config) | FEAT-021, CONTRACT-006, ADR-014 | B-101 (config surface frozen by CONTRACT-008 amendment) | Integration test publishes CDC envelopes to a Kafka testcontainer and replays them; `cargo test -p axon-audit` passes | Completes the last unimplemented FEAT-021 transport |
| B-103 | FEAT-022 remaining guardrail scope (semantic validation hooks; rate limiting + actor scope already shipped) | FEAT-022, ADR-016, ADR-024 | B-101 | Guardrail hook tests in `crates/axon-server` exercise hook rejection paths; `cargo test -p axon-server` passes | Completes the P0 safety-guardrail vision scope |
| B-104 | Serializable isolation (P1): read-set tracking to detect write skew; effective-isolation inspectability | FEAT-008 (TXN-05, Constraints), ADR-004 | B-101 | axon-sim `CycleWorkload` extended with a write-skew workload that fails under SI and passes under serializable; `cargo test -p axon-sim` | Upgrades the corrected SI baseline; do not relabel docs until this lands |
| B-105 | Local-first sync (FR-32, newly promoted P1) — **design first**: ADR + solution design before any build issue | PRD v0.4.0 FR-32 | None for design; build blocked on accepted ADR | ADR merged in `docs/helix/02-design/adr/`; design review recorded | No build scope may be invented before the ADR exists |
| B-106 | BYOC (FR-27 P1) remaining scope beyond shipped control plane (tenant/user/credential/database/member mgmt is live) | PRD FR-27, ADR-017, FEAT-025 | B-101 | Control-plane monitoring + remaining FEAT-025 acceptance criteria pass in `crates/axon-control-plane`/`axon-server` tests | Scope = FEAT-025 "monitoring not implemented" remainder plus BYOC packaging |
| B-107 | Story-test-plan / coverage closeout: PROP-002..005 property tests, L6 contract-suite completion, story-test-plans for remaining non-guardrail features per test-plan §AC allocation | test-plan.md, feature-story-e2e-traceability.md, STP set | B-101..B-104 (tests target final surfaces) | `cargo test` workspace green; traceability doc shows no unallocated ACs | Last: validates the completed surfaces, not the interim ones |

## Issue Decomposition

Story-level work is tracked as beads in `.ddx/beads.jsonl` via `ddx bead`.

**Per-issue requirements**:
- Labels: `helix`, `activity:build`, plus area labels (`area:api`,
  `area:sdk`, `area:mcp`, ...)
- References: governing FEAT/CONTRACT/ADR paths in the description
- Acceptance criteria naming observable repo states (grep/test commands)
- Blockers as `--depends-on` links
- Closure requires durable evidence (commit ref, execution bundle, or notes)

| Story / Area | Goal | Dependencies |
|--------------|------|--------------|
| B-101 conformance (6 beads filed: axon-b8078b63, axon-b684338f, axon-784bc974, axon-95b137d0, axon-c62971d9, axon-87fee98b) | Code matches CONTRACT-001..010 as written | None |
| B-102 Kafka transport | FEAT-021 complete | axon-87fee98b (config contract) |
| B-103 guardrail hooks | FEAT-022 complete | None hard; sequence after B-101 |
| B-104 serializable isolation | FEAT-008 P1 constraint discharged | None hard; sequence after B-101 |
| B-105 local-first sync ADR | Accepted design for FR-32 | None (design work) |
| B-106 BYOC remainder | FR-27 P1 scope complete | B-101 |
| B-107 coverage closeout | No unallocated ACs; PROP-002..005 coded | B-101..B-104 |

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
| Read-set tracking for serializable regresses commit throughput | M | Benchmark gate: BM suite (`crates/axon-api/benches/benchmarks.rs`) must not regress >10%; serializable stays opt-in initially | Keep SI the default; serializable behind a transaction option |
| Local-first sync scope balloons before design exists | M | B-105 is design-only; no build beads until the ADR is accepted | N/A (no code) |
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
