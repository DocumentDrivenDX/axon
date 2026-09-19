---
ddx:
  id: axon-gap-closure-surface-closure-audit
---

# Axon Gap-Closure Surface Closure Audit

## Scope

This audit rechecks closed-bead claims on the fetched `origin/master`
tracker snapshot (`ede4ade306ccd7ac0070d0cc959551dc91659d02`) against live
code and tests in this workspace.

The named surfaces are the ones in the bead contract: graph/Cypher policy and
limits, public change streams/CDC/opaque resume state, Kafka, deployment,
backup/restore, doctor, monitoring, security, UI, SDK, Nexiq, DDx, Cayce, and
FR-32 local replica/bootstrap/tail/lease/encryption.

## Legend

- `pass`: live code/tests satisfy the closure claim.
- `fail`: live code/tests contradict the claim.
- `stale-evidence`: the closure record points at older or mismatched evidence.
- `superseded`: a later closed bead or live implementation now owns the claim.

## Surface Summary

- Graph/Cypher: one link-enforcement closure still fails open for schema-less
  link catalogs; the later graph/query beads are implemented and the earlier
  review beads are superseded.
- CDC/replica: durable cursor storage, opaque resume tokens, Kafka/file/SSE
  sinks, schema-registry facade, GraphQL change-feed subscriptions, and the
  TypeScript local replica all have live code and passing tests.
- Deploy/ops/security: the unified CLI, `doctor`, service install, TLS
  bootstrap, CORS, and `/health` paths exist. Backup/restore and monitoring are
  still checklist/runbook obligations rather than dedicated closed
  implementation beads.
- UI/SDK: the story-coverage scanner and the local-replica SDK tests both pass.
- Nexiq/DDx/Cayce: the consumer gate records missing workloads honestly; it
  does not fake green results.

## Graph / Cypher

| Bead(s) | Original claim | Live evidence | Result | Successor disposition |
|---|---|---|---|---|
| `axon-f48352d5` | Enforce declared link types at write time: target collection, metadata schema, and duplicate triple rejection. | `crates/axon-api/src/handler.rs:7893-8030` only enforces when `schema.link_types` is non-empty; tests `create_link_allows_untyped_collections` and `create_link_allows_schema_without_link_types` still allow those paths (`crates/axon-api/src/handler.rs:15941-16135`). | `fail` | Draft successor: `Fail-closed link catalogs and staged final-state validation`, scoped to `crates/axon-api/src/handler.rs` and the `create_link_*` tests above. Command-grade ACs: reject undeclared link types even when the catalog is empty, keep target-collection and metadata validation on the write path, reject duplicate triples, and prove it with `cargo test -p axon-api create_link_` plus `cargo clippy -- -D warnings`. Dependency recommendation: follow the link-cardinality/lifecycle owners already closed in `axon-7ac24886`. |
| `hx-75b5d567`, `axon-848ab0fe`, `axon-05c1019d`, `axon-06ed05c1` | Link cardinality review, graph-model review, DDx ready/blocked queue scope, and the FR-32 frame-or-redefer decision. | Later closures and code now own the work: `axon-7ac24886` enforces cardinality in `crates/axon-api/src/handler.rs:7916-7973`; `axon-95c347bc` and `axon-aa655901` close the traversal and link/named-query contract gaps; `axon-8b91e47d` closes the ready/blocked benchmark; `axon-03269bc7` owns the local replica. | `superseded` | Successor claims now live in `axon-7ac24886`, `axon-95c347bc`, `axon-aa655901`, `axon-8b91e47d`, and `axon-03269bc7`. |
| `axon-7ac24886`, `axon-95c347bc`, `axon-aa655901`, `axon-100687fd`, `axon-243091c3`, `axon-390c67d1`, `axon-51d884f0`, `axon-52265708`, `axon-53c8d772`, `axon-15de5e84`, `axon-2705d3ac`, `axon-8b91e47d` | Graph store, planner, named queries, GraphQL/MCP exposure, DDx graph-query integration, and the ready/blocked p99 gates. | `crates/axon-cypher/src/schema.rs:73-174` declares `ready_beads` and `blocked_beads`; `crates/axon-cypher/benches/ddx_benchmark.rs:245-279` pins the 1k/10k p99 gates; `cargo test -p axon-cypher ddx_beads_named_queries_activate_without_error` passed; the current live code path is the `crates/axon-cypher`, `crates/axon-graphql`, and `crates/axon-server` graph stack. | `pass` | No successor needed. |

## CDC / Replica

| Bead(s) | Original claim | Live evidence | Result | Successor disposition |
|---|---|---|---|---|
| `axon-34c4dd4b`, `axon-36a3ce2b`, `axon-7c28cec8`, `axon-03269bc7` | Durable CDC cursors, opaque scope-bound resume tokens, and the local read replica. | `crates/axon-audit/src/cursor.rs:1-35` defines the durable cursor trait boundary; `crates/axon-audit/src/cursor_token.rs:1-165` defines opaque, scope-bound resume tokens with round-trip and scope-mismatch tests; `crates/axon-storage/src/cursor_store.rs:1-159` persists cursors durably; `crates/axon-audit/src/log.rs:142-196` resumes from the stored cursor; `sdk/typescript/src/local-replica.ts:1-203` and `sdk/typescript/test/local-replica.test.ts:20-202` cover snapshot/bootstrap, delta tailing, tombstones, cursor tracking, and reconnect-by-resume. `cd sdk/typescript && bun test test/local-replica.test.ts` passed. | `pass` | No successor needed. |
| `axon-5b76063f`, `axon-588d0913`, `axon-88caddb4`, `axon-bbaeac20`, `axon-c507e968`, `axon-3fbdffab` | Kafka CDC, non-Kafka CDC sinks, delete tombstones, CONTRACT-006 envelope alignment, and the Confluent-compatible schema-registry facade. | `crates/axon-audit/src/cdc.rs:1-18,30-218,420-597,1242-1535` shows the Debezium-compatible envelope, Kafka/file/memory sinks, delete tombstones, and the `emit_tombstone` contract; `crates/axon-registry/src/lib.rs:1-18,510-718` exposes the Confluent-compatible registry endpoints and tests them. | `pass` | No successor needed. |
| `axon-11f27cab`, `axon-2a706412`, `axon-6d8e6890` | GraphQL change-feed subscriptions, initial snapshot delivery, filtered delivery, per-collection events, and reconnect/resume semantics. | `crates/axon-graphql/src/subscriptions.rs:1-220` defines the change-feed broker and resume-oriented event shape; `crates/axon-graphql/src/dynamic.rs:13798-14190` covers named-query subscription SDL, initial snapshot delivery, entity-add/status-change/link-add re-evaluation, policy filtering, and disconnect cleanup; `crates/axon-graphql/src/dynamic.rs:12155-12440` covers generic subscription SDL, filter semantics, per-collection fanout, collection-drop rebuilds, and the latency target. | `axon-6d8e6890` is `superseded`; `axon-11f27cab` and `axon-2a706412` pass | The live successor is the `crates/axon-graphql` subscription stack and the tests above. |

## Deploy / Ops / Security

| Bead(s) | Original claim | Live evidence | Result | Successor disposition |
|---|---|---|---|---|
| `axon-ad7a3163`, `axon-be5d1a70`, `axon-72093dd3`, `axon-5dadc6f6`, `axon-81157524`, `axon-a51dde79` | Deployment runbook and checklist, unified CLI, `doctor`, service install, `/health`, TLS bootstrap, and browser CORS support. | `docs/helix/05-deploy/deployment-checklist.md:34-70` requires backups before upgrade and verifies `axon doctor`, `/health`, and TLS; `docs/helix/05-deploy/runbook.md:155-183` uses `axon doctor`, backup, and the current monitoring probes; `crates/axon-cli/src/main.rs:105-170,223-240,878-904` wires `Serve`, `Mcp`, `Doctor`, `Server install/uninstall`, client mode, and CORS commands; `crates/axon-cli/src/doctor.rs:1-50` prints config, data dir, storage backend, and reachability; `crates/axon-cli/src/service.rs:73-119,410-526` installs systemd/launchd units with `--tls-self-signed` by default and keeps `--no-auth` opt-in; `crates/axon-server/src/serve.rs:126-187,420-507` exposes `--tls-self-signed-san`, `--tls-cert`, `--tls-key`, and HTTPS bootstrap; `crates/axon-server/src/gateway.rs:3910-3944,6279-6316` serves `/health` with version, uptime, backing_store, and default_namespace fields and tests the health payload; `crates/axon-server/src/path_router.rs:54-105` treats `/health`, `/metrics`, `/ui`, and `/control` as reserved non-data-plane paths. | `pass` | No successor needed. |

Backup/restore and monitoring are still only checklist/runbook obligations in
the closed set above. I did not find a dedicated closed backup/restore bead,
and the runbook explicitly says the monitoring setup doc is not yet authored.

## UI / SDK

| Bead(s) | Original claim | Live evidence | Result | Successor disposition |
|---|---|---|---|---|
| `axon-6c33fab3`, `axon-5aecd1a1`, `axon-0dd766be` | Real-server E2E triage, story-arc and reverse-route coverage, and canonical UI/SDK story citations. | `cd ui && bun run check:story-coverage` passed and reported COVERED story rows for US-113 through US-119; `ui/tests/e2e/*.spec.ts` includes the current browser workflows; `sdk/typescript/test/local-replica.test.ts:72-202` covers the SDK local replica slice; `cd sdk/typescript && bun test test/local-replica.test.ts` passed. | `pass` | No successor needed. |

## Nexiq / DDx / Cayce

| Bead(s) | Original claim | Live evidence | Result | Successor disposition |
|---|---|---|---|---|
| `axon-3d8dac83`, `axon-46c878f7`, `axon-6026b76b`, `axon-89fa770a`, `axon-8d2b9e99` | Seed Nexiq before workload execution, fail release mode on dirty downstream checkouts, require native test counts, require real Axon traffic proof, and record DDx/Cayce gaps honestly. | `tests/test_consumer_workload_runner.py:244-340` checks Nexiq missing checkout, Cayce missing source, DDx `contract_gap`, and fake-transport rejection; `python3 -m unittest tests.test_consumer_workload_runner` passed. The runner intentionally reports `missing_workload` and `contract_gap` as non-pass states. | `pass` | No successor needed. |
| `axon-72b6f0b4`, `axon-86f6dba4`, `axon-cfd4ae4f` | Reconcile release/readiness docs, record the release target and decision dispositions, and add the claim-inventory gate. | `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md:103-142` confirms no consumer is deferred, records the 0.4.x release target, and lists the evidence index; `python3 tests/test_release_readiness_claims.py --format text` passed and reported 67 release target claims, 0/8 PRD success criteria checked, and 11 stale DDx frontmatter/hash entries. | `pass` | No successor needed. |

Consumer/backend/environment gaps remain hold evidence, not pass evidence:
`missing_workload` and `contract_gap` stay recorded as non-green outcomes in
the consumer runner, and preserved candidates were not promoted.

## FR-32 Notes

- The FR-32 decision bead `axon-06ed05c1` is superseded by the implemented
  FEAT-032 path above.
- The closed local-replica beads cover snapshot/bootstrap, tailing, and resume
  behavior. They do not claim lease or encryption behavior; no closed bead in
  the fetched set surfaced those subtopics.

## Validation So Far

- `python3 tests/test_release_readiness_claims.py --format text` passed.
- `python3 -m unittest tests.test_consumer_workload_runner` passed.
- `cd ui && bun run check:story-coverage` passed.
- `cd sdk/typescript && bun test test/local-replica.test.ts` passed.
- `AXON_TEST_POSTGRES='postgres://postgres:postgres@192.168.215.10:5432/postgres' cargo test` passed.
- `AXON_TEST_POSTGRES='postgres://postgres:postgres@192.168.215.10:5432/postgres' cargo clippy -- -D warnings` passed.
- `cargo fmt --check` passed.
- `ddx doc validate` passed, with an unrelated warning about `metrics-dashboard` depending on a missing metric graph node.
- `git diff --check -- docs/helix/06-iterate` passed.
