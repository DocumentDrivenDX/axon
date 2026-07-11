---
ddx:
  id: axon-gap-closure-evidence-matrix
  depends_on:
    - axon-gap-closure-tracker-audit
    - axon-gap-closure-baseline
    - axon-gap-closure-open-tracker-audit
    - axon-gap-closure-core-closure-audit
    - axon-gap-closure-surface-closure-audit
  review:
    self_hash: 18b81a2464003166cd67554058eab556186f3d266af74e862338bbb56f95f87d
    deps:
      axon-gap-closure-baseline: c10f7a4176b5b6c5ac8db2cde2a367be9705a216a33427c06b001588265484ac
      axon-gap-closure-core-closure-audit: 89a95564d1d89c4832401b8662acd6da870ca0a94795ef4a4100d32503d6c50c
      axon-gap-closure-open-tracker-audit: e78c348d3e57e59f72616ec3cd35899006145bda140b0ed8e69e668965964f4d
      axon-gap-closure-surface-closure-audit: 2b4a988418d40452f8a4ef8e305050cdcbd02252c9eb235767bd34fba1094bd0
      axon-gap-closure-tracker-audit: bfaaedaab7dc679d9d8dc79d29979a92b35d8fd71f565a9a3be8f97640c2ac0b
    reviewed_at: "2026-07-11T03:45:20Z"
---

# Axon Gap-Closure Requirements-to-Live-Evidence Matrix

## Purpose

This matrix maps the Phase 0 domain buckets to live code/tests and a queued
or passing disposition. It is built from the tracker audit plus the
open-tracker, governed-core, and surface closure audits.

Tracker input:

- Fetched snapshot: `1,147 / 19 / 1,128`
- Branch-local Phase 1 handoff checkpoint: `1,186 / 43 / 1,142`

## Legend

- `passing`: live code/tests satisfy the claim.
- `queued`: live evidence exists, but the corrective bead or consumer
  disposition remains open.

## Matrix

| Domain | Fetched closure family | Live evidence | Disposition | Queue anchor |
|---|---|---|---|---|
| Namespace / raw write | `axon-2d159505`, `axon-130f129f`, `hx-19f9b034`, `hx-be04425b`, `hx-ce0d891b`, `hx-d63cdf34`, `hx-f2e7a098`, `hx-5e1611b4`, `hx-3c07a1fb`, `hx-3bcda877`, `hx-e34e61a2` | Existing lexical guards and adapter internals are live, but the reviewed finish line additionally requires typed manifests, parsed DML enforcement, compile-time raw-SPI sealing, and exact public/admin audit parity. | queued | Phase 2 epic `axon-gap-closure-5dbe3159`; first bead `axon-gap-closure-4b8d95f9`; boundary beads `d4c16007`, `73625cab`, and `09206fb8`. |
| Migration / schema | `axon-0478a1de`, `axon-40a8aa60`, `hx-5cf3905f`, `hx-c3792652`, `hx-4ad8a437`, `hx-f041d39e`, `hx-693047cd` | Existing schema binding and migrations are live; Phase 1 froze fail-closed schema, policy hash, schema-catalog hash, evolution, and PostgreSQL 16 authority. Exclusive legacy activation and full runtime wiring remain. | queued | Phase 2A `axon-gap-closure-5f04f327`, Phase 3 `axon-gap-closure-53319ef4`, and Phase 4 `axon-gap-closure-fa0607f1`. |
| Link / graph | `axon-f48352d5`, `hx-75b5d567`, `axon-848ab0fe`, `axon-05c1019d`, `axon-06ed05c1`, plus the graph/query families `axon-7ac24886`, `axon-95c347bc`, `axon-aa655901` | Existing handler/Cypher/GraphQL tests are live; the failed link-catalog closure still has one successor, and the reviewed V1 graph hard-limit/performance evidence is not yet terminal. | queued | Link successor `axon-gap-closure-bdec95f2`; Phase 5 `axon-gap-closure-fddc28c3`; Phase 8 `axon-gap-closure-02e48894`. |
| Transaction / audit / idempotency / payload / PostgreSQL 16 | `axon-d13bf628`, `axon-b8bdd7a3`, `axon-e3887131`, `axon-fdcc5440`, `axon-a0a91c61`, `axon-97a8ca10`, `axon-b189dfa9`, `axon-06459077`, `axon-49bcf929`, `axon-4cf3fa53`, `axon-66310e71`, `axon-ae54c5a5`, `axon-b9a7237f`, `axon-18fb7517`, `hx-2a25bfc6`, `hx-f032260d`, `hx-18afd3f5`, `hx-34723382`, `hx-b4f18fbf`, `axon-a9837532`, `axon-dc5d2b32`, `axon-84088cbe` | Existing tests are green on PostgreSQL 16, and Phase 1 froze canonical bytes/outcomes and backend-qualified semantics. The reviewed finish line still requires fenced idempotency, auth audit/epochs, full mixed-transaction co-commit, cross-surface parity, and restart/fault qualification. | queued | Phase 2 auth/idempotency beads `df2330e0` and `7ca0310b`; Phases 5-7 `fddc28c3`, `23496391`, and `3f85d0fb`. |
| Stream | `axon-34c4dd4b`, `axon-36a3ce2b`, `axon-7c28cec8`, `axon-03269bc7`, `axon-5b76063f`, `axon-588d0913`, `axon-88caddb4`, `axon-bbaeac20`, `axon-c507e968`, `axon-3fbdffab`, `axon-11f27cab`, `axon-2a706412`, `axon-6d8e6890` | CDC, durable cursor storage, opaque token primitives, snapshot routes, and LocalReplica exist. Transaction framing, public ACK, leases, encryption, and complete invalidation/wiring are not finish-line-B evidence yet. | queued | Phase 10 `axon-gap-closure-97ec443b` and finish-line-B gate `axon-gap-closure-05672022`. |
| Consumer | `axon-3d8dac83`, `axon-46c878f7`, `axon-6026b76b`, `axon-89fa770a`, `axon-8d2b9e99`, `axon-72b6f0b4`, `axon-86f6dba4`, `axon-cfd4ae4f` | `tests/test_consumer_workload_runner.py`; `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md` | queued | `missing_workload` and `contract_gap` remain recorded as honest non-green outcomes. |
| FR-32 | `axon-06ed05c1`, `axon-03269bc7`, `axon-11f27cab`, `axon-2a706412`, `axon-6d8e6890` | Phase 1 now records implemented primitives versus partial/unwired end-to-end behavior in FEAT-032, ADR-025, architecture, test plan, and implementation plan. | queued | Phase 10 `axon-gap-closure-97ec443b`; finish line B waits for finish line A plus Phase 10 at `axon-gap-closure-05672022`. |

## Queue Notes

- `axon-gap-closure-bdec95f2` is the only new corrective bead that repairs a
  failed fetched closure; it remains a separate link fix.
- Phase 1 closed the contract-freeze children and seeded the Phase 2-11
  implementation/evidence spine. Roadmap beads are not assertions that their
  implementation already passes.
- Finish line A is the pilot governed-core gate `axon-gap-closure-ce1e94a9`
  and is independent of FR-32. Finish line B is
  `axon-gap-closure-05672022` and joins the terminal pilot verdict with Phase
  10.
- The matrix is stricter than the older Phase 0 snapshot: it separates live
  primitives from reviewed finish-line evidence that remains queued.
