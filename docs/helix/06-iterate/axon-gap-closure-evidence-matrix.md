---
ddx:
  id: axon-gap-closure-evidence-matrix
  depends_on:
    - axon-gap-closure-tracker-audit
    - axon-gap-closure-baseline
    - axon-gap-closure-open-tracker-audit
    - axon-gap-closure-core-closure-audit
    - axon-gap-closure-surface-closure-audit
---

# Axon Gap-Closure Requirements-to-Live-Evidence Matrix

## Purpose

This matrix maps the Phase 0 domain buckets to live code/tests and a queued
or passing disposition. It is built from the tracker audit plus the
open-tracker, governed-core, and surface closure audits.

Tracker input:

- Fetched snapshot: `1,147 / 19 / 1,128`
- Branch-local overlay: `1,157 / 22 / 1,135`

## Legend

- `passing`: live code/tests satisfy the claim.
- `queued`: live evidence exists, but the corrective bead or consumer
  disposition remains open.

## Matrix

| Domain | Fetched closure family | Live evidence | Disposition | Queue anchor |
|---|---|---|---|---|
| Namespace / raw write | `axon-2d159505`, `axon-130f129f`, `hx-19f9b034`, `hx-be04425b`, `hx-ce0d891b`, `hx-d63cdf34`, `hx-f2e7a098`, `hx-5e1611b4`, `hx-3c07a1fb`, `hx-3bcda877`, `hx-e34e61a2` | `crates/axon-core/src/id.rs`; `crates/axon-server/src/tenant_router.rs`; `crates/axon-server/src/gateway.rs`; `crates/axon-storage/src/postgres.rs`; `crates/axon-storage/src/sqlite.rs` | passing | Hidden names stay sealed behind typed internal APIs. |
| Migration / schema | `axon-0478a1de`, `axon-40a8aa60`, `hx-5cf3905f`, `hx-c3792652`, `hx-4ad8a437`, `hx-f041d39e`, `hx-693047cd` | `crates/axon-storage/src/auth_schema.rs`; `crates/axon-server/src/control_plane_routes.rs`; `crates/axon-server/src/control_plane.rs`; `crates/axon-storage/src/memory.rs`; `crates/axon-server/tests/postgres_tenant_isolation.rs`; `crates/axon-server/tests/api_contract.rs` | passing | Schema binding and migration init are live in the core audit. |
| Link / graph | `axon-f48352d5`, `hx-75b5d567`, `axon-848ab0fe`, `axon-05c1019d`, `axon-06ed05c1`, plus the graph/query families `axon-7ac24886`, `axon-95c347bc`, `axon-aa655901` | `crates/axon-api/src/handler.rs`; `crates/axon-cypher/src/schema.rs`; `crates/axon-graphql/src/dynamic.rs`; `crates/axon-server/tests/graphql_contract.rs`; successor bead `axon-gap-closure-bdec95f2` is queued | queued | `axon-gap-closure-bdec95f2` depends on `axon-7ac24886`. |
| Transaction / audit / idempotency / payload / PostgreSQL | `axon-d13bf628`, `axon-b8bdd7a3`, `axon-e3887131`, `axon-fdcc5440`, `axon-a0a91c61`, `axon-97a8ca10`, `axon-b189dfa9`, `axon-06459077`, `axon-49bcf929`, `axon-4cf3fa53`, `axon-66310e71`, `axon-ae54c5a5`, `axon-b9a7237f`, `axon-18fb7517`, `hx-2a25bfc6`, `hx-f032260d`, `hx-18afd3f5`, `hx-34723382`, `hx-b4f18fbf`, `axon-a9837532`, `axon-dc5d2b32`, `axon-84088cbe` | `crates/axon-api/src/transaction.rs`; `crates/axon-server/src/gateway.rs`; `crates/axon-api/src/intent.rs`; `crates/axon-api/src/proptest_api.rs`; `crates/axon-server/tests/graphql_policy_contract.rs`; `AXON_TEST_POSTGRES=postgres://postgres:postgres@192.168.215.10:5432/postgres cargo test`; `cargo clippy -- -D warnings` | passing | PostgreSQL qualification and canonical payload checks are green. |
| Stream | `axon-34c4dd4b`, `axon-36a3ce2b`, `axon-7c28cec8`, `axon-03269bc7`, `axon-5b76063f`, `axon-588d0913`, `axon-88caddb4`, `axon-bbaeac20`, `axon-c507e968`, `axon-3fbdffab`, `axon-11f27cab`, `axon-2a706412`, `axon-6d8e6890` | `crates/axon-audit/src/cursor.rs`; `crates/axon-audit/src/cursor_token.rs`; `crates/axon-storage/src/cursor_store.rs`; `crates/axon-audit/src/log.rs`; `crates/axon-audit/src/cdc.rs`; `crates/axon-registry/src/lib.rs`; `crates/axon-graphql/src/subscriptions.rs`; `sdk/typescript/src/local-replica.ts` | passing | CDC, opaque cursor storage, and the local replica are live. |
| Consumer | `axon-3d8dac83`, `axon-46c878f7`, `axon-6026b76b`, `axon-89fa770a`, `axon-8d2b9e99`, `axon-72b6f0b4`, `axon-86f6dba4`, `axon-cfd4ae4f` | `tests/test_consumer_workload_runner.py`; `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md` | queued | `missing_workload` and `contract_gap` remain recorded as honest non-green outcomes. |
| FR-32 | `axon-06ed05c1`, `axon-03269bc7`, `axon-11f27cab`, `axon-2a706412`, `axon-6d8e6890` | `sdk/typescript/src/local-replica.ts`; `sdk/typescript/test/local-replica.test.ts`; `crates/axon-graphql/src/dynamic.rs`; `crates/axon-graphql/src/subscriptions.rs` | queued | The governed local read replica stack is live, but FR-32 remains queued because lease/encryption are not yet claimed here. |

## Queue Notes

- `axon-gap-closure-bdec95f2` is the only new corrective bead in this
  matrix.
- The other domains are live evidence rows, not new filing targets.
- The matrix is intentionally stricter than the older open-tracker snapshot:
  it tracks live disposition, not just historical closure labels.
