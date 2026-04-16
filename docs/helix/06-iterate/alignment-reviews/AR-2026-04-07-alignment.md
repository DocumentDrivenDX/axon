# Alignment Review: Full Repository — 2026-04-07

**Review Epic**: `axon-673c1164`
**Scope**: Full repository
**Reviewer**: Claude (alignment review)
**Date**: 2026-04-07

---

## 1. Review Metadata

| Field | Value |
|-------|-------|
| Review type | Full repository alignment |
| Governing artifacts | Product Vision, PRD v0.2.0, Technical Requirements v0.2.0, 25 Feature Specs, 14 ADRs, Test Plan |
| Build status | `cargo check` passes, `cargo test` passes (695 tests, 0 failures) |
| Crates reviewed | axon-core, axon-schema, axon-audit, axon-storage, axon-api, axon-cli, axon-server, axon-mcp, axon-graphql, axon-sim |

---

## 2. Scope and Governing Artifacts

| Layer | Artifact | Status |
|-------|----------|--------|
| Vision | `docs/helix/00-discover/product-vision.md` | Draft (revised 2026-04-06) |
| PRD | `docs/helix/01-frame/prd.md` | v0.2.0 Draft |
| Technical Requirements | `docs/helix/01-frame/technical-requirements.md` | v0.2.0 Draft |
| Feature Specs | `docs/helix/01-frame/features/FEAT-001..025` | 25 specs |
| ADRs | `docs/helix/02-design/adr/ADR-001..014` | 14 decided |
| Test Plan | `docs/helix/03-test/test-plan.md` | Draft |
| Spikes | `docs/helix/02-design/spikes/SPIKE-001` | Complete |

---

## 3. Intent Summary

**Vision**: Entity-first OLTP store for agentic and human workflows. Schema-driven, auditable, agent-accessible.

**V1 scope (P0)**: Entity-graph data model, ACID transactions with OCC, immutable audit log, schema engine with validation, API surface (HTTP + gRPC), embedded mode, CLI. 12 must-have capabilities.

**P1 scope**: Schema evolution, CDC, aggregation, graph traversal, server mode, auth, admin UI, secondary indexes, multi-tenancy, GraphQL, MCP, validation rules, link discovery, agent guardrails, rollback/recovery, control plane.

---

## 4. Planning Stack Findings

### 4a. Vision -> PRD

**ALIGNED**. PRD v0.2.0 reflects the revised vision (OLTP positioning, EAV index strategy, broader market, commercial model). No contradictions.

### 4b. PRD -> Feature Specs

**ALIGNED with gaps**:
- FEAT-001 through FEAT-021: All have feature specs matching PRD requirements.
- FEAT-022 through FEAT-025: New specs from 2026-04-06 evolution. Match PRD P1/P2 entries.
- **Gap**: PRD P1 #2 (Change feeds) references FEAT-021 which is comprehensive, but CDC Kafka integration is not tested end-to-end (only in-memory and file sinks tested).

### 4c. Feature Specs -> ADRs

**ALIGNED**. Each feature that requires an architectural decision has a corresponding ADR. Traceability matrix in technical requirements is complete.

### 4d. ADRs -> Implementation

**Findings**:
- ADR-005 (Tailscale auth): **INCOMPLETE** — stub only, returns placeholder identity.
- ADR-006 (SvelteKit admin UI): **INCOMPLETE** — scaffolded but not functional.
- ADR-010 (Physical storage / EAV indexes): **ALIGNED** in Memory and SQLite. **INCOMPLETE** in PostgreSQL.
- ADR-012 (GraphQL): **ALIGNED** — async-graphql crate with dynamic schema generation.
- ADR-013 (MCP): **INCOMPLETE** — CRUD tools work; aggregate/query/neighbors tools are stubs.
- ADR-014 (CDC/Debezium): **INCOMPLETE** — audit-to-CDC conversion works; Kafka sink exists but untested against real broker.

---

## 5. Implementation Map

### Crate Completeness

| Crate | Impl % | Tests | Notes |
|-------|--------|-------|-------|
| axon-core | 100% | 40+ | All types, RBAC, topology design types |
| axon-schema | 100% | 80+ | ESF engine, gates, rules, evolution, diff |
| axon-audit | 100% | 60+ | Append-only, CDC conversion, paginated queries |
| axon-storage | 80% | 150+ | Memory 100%, SQLite 95%, PostgreSQL 60% |
| axon-api | 95% | 188+ | All core ops; namespace/db stubs only |
| axon-cli | 85% | 29+ | CRUD, audit, schema; output formatting partial |
| axon-server | 80% | 12+ | HTTP 100%; gRPC 60%; auth stub |
| axon-mcp | 60% | 73+ | CRUD tools work; 4 advanced tool stubs |
| axon-graphql | 80% | 60+ | Schema gen, queries, mutations, subscriptions |
| axon-sim | 90% | 128+ | 9 invariants; deterministic replay |

### Test Summary

- **695 tests passing, 0 failures**
- 10 business scenarios (SCN-001..010)
- 6 backend parity tests (Memory + SQLite)
- 3 property-based tests (PROP-002, PROP-004, PROP-005)
- 10 performance benchmarks (BM-001..010)
- 9 simulation invariants
- 12 gRPC contract tests
- TypeScript SDK with generated client

---

## 6. Acceptance Criteria Status

### P0 Features (Must-Have V1)

| Feature | Story/Criterion | Status | Evidence |
|---------|----------------|--------|----------|
| FEAT-001 Collections | Create, list, drop, describe | SATISFIED | handler tests, SCN-*, backend_parity |
| FEAT-002 Schema Engine | ESF validation, gates, rules, evolution | SATISFIED | schema tests, SCN-005, SCN-010 |
| FEAT-003 Audit Log | Append-only, immutable, queryable | SATISFIED | audit tests, PROP-002, INV-002/003 |
| FEAT-004 Entity CRUD | Create, read, update, patch, delete | SATISFIED | handler tests, backend_parity, SCN-* |
| FEAT-005 API Surface | HTTP REST API | SATISFIED | api_contract tests, 25+ endpoints |
| FEAT-007 Entity-Graph Model | Entities + typed links | SATISFIED | link tests, SCN-004, SCN-009, PROP-005 |
| FEAT-008 ACID Transactions | Multi-entity atomic ops, OCC | SATISFIED | PROP-004, INV-001, transaction_atomicity sim |
| FEAT-009 Graph Traversal | BFS traversal with depth limits | SATISFIED | traverse tests, SCN-004, SCN-006 |

### P0 Features — Partial

| Feature | Criterion | Status | Evidence |
|---------|-----------|--------|----------|
| FEAT-005 API Surface | gRPC API | TESTED_NOT_PASSING → partial | 12 gRPC tests pass; list/query/schema ops not wired |
| FEAT-005 API Surface | Embedded mode | SATISFIED | CLI uses embedded SQLite |
| CLI (PRD P0 #12) | All subcommands | SATISFIED | collection, entity, link, audit, schema commands |

### P1 Features

| Feature | Status | Evidence |
|---------|--------|----------|
| FEAT-010 Workflow State Machines | UNIMPLEMENTED | Gates provide partial workflow; no formal state machine |
| FEAT-011 Admin Web UI | INCOMPLETE | SvelteKit scaffolded (ADR-006); not functional |
| FEAT-012 Authorization | INCOMPLETE | RBAC types + grant registry in core; Tailscale auth stubbed |
| FEAT-013 Secondary Indexes | SATISFIED | EAV indexes in Memory + SQLite; BM-007 benchmark |
| FEAT-014 Multi-Tenancy | INCOMPLETE | Namespace hierarchy types exist; persistence stubbed |
| FEAT-015 GraphQL | SATISFIED | async-graphql with dynamic schema, subscriptions |
| FEAT-016 MCP Server | INCOMPLETE | CRUD tools work; aggregate/query/neighbors stubs |
| FEAT-017 Schema Evolution | SATISFIED | Compatibility detection, diff, revalidation |
| FEAT-018 Aggregation | SATISFIED | SUM/AVG/MIN/MAX/COUNT/GROUP BY; BM-007 |
| FEAT-019 Validation Rules | SATISFIED | Cross-field rules, gate evaluation, severity levels |
| FEAT-020 Link Discovery | SATISFIED | find_link_candidates, list_neighbors |
| FEAT-021 CDC | INCOMPLETE | Audit-to-CDC conversion works; Kafka sink untested |
| FEAT-022 Agent Guardrails | UNIMPLEMENTED | New (2026-04-06) |
| FEAT-023 Rollback/Recovery | UNTESTED | revert_entity exists; point-in-time/transaction rollback not implemented |
| FEAT-024 Application Substrate | UNIMPLEMENTED | New (2026-04-06) |
| FEAT-025 Control Plane | UNIMPLEMENTED | New (2026-04-06) |

---

## 7. Gap Register

| # | Area | Classification | Planning Evidence | Implementation Evidence | Resolution Direction | Review Issue |
|---|------|----------------|-------------------|------------------------|---------------------|-------------|
| G-01 | PostgreSQL adapter | INCOMPLETE | TR Section 3: PostgreSQL required for server mode | `postgres.rs`: CRUD works; indexes, compounds, links, gates return no-op | code-to-plan | `axon-c6cd37ae` |
| G-02 | gRPC service | INCOMPLETE | FEAT-005: gRPC primary API | `service.rs`: 9 RPCs defined, ~5 fully wired; list/query/schema stubbed | code-to-plan | `axon-bba3ced7` |
| G-03 | MCP advanced tools | INCOMPLETE | FEAT-016: tools auto-generated from ESF | `handlers.rs`: 4 stubs (aggregate, query, link-candidates, neighbors) | code-to-plan | `axon-3d52d938` |
| G-04 | Tailscale auth | INCOMPLETE | ADR-005, FEAT-012 | `auth.rs`: stub returning placeholder identity | code-to-plan | `axon-b5f998a5` |
| G-05 | Admin UI | INCOMPLETE | FEAT-011, ADR-006 | SvelteKit scaffolded; not functional | code-to-plan | `axon-09ada1e6` |
| G-06 | Multi-tenancy persistence | INCOMPLETE | FEAT-014, ADR-011 | Namespace types exist; create/drop are in-memory stubs | code-to-plan | `axon-92f06b94` |
| G-07 | CDC Kafka integration | INCOMPLETE | FEAT-021, ADR-014 | KafkaCdcSink exists; no integration test against broker | code-to-plan | `axon-b5f998a5` |
| G-08 | Workflow state machines | UNDERSPECIFIED | FEAT-010 (P2) | Gates provide partial workflow semantics; no formal FSM | decision-needed | `axon-b5f998a5` |
| G-09 | Agent guardrails | UNIMPLEMENTED | FEAT-022 (P1, new) | No implementation | code-to-plan | `axon-5e455d65` |
| G-10 | Rollback/recovery | INCOMPLETE | FEAT-023 (P1, new) | revert_entity exists; point-in-time and transaction-level rollback missing | code-to-plan | `axon-5e455d65` |
| G-11 | Control plane | UNIMPLEMENTED | FEAT-025 (P1, new) | No implementation | code-to-plan | `axon-92f06b94` |
| G-12 | Application substrate | UNIMPLEMENTED | FEAT-024 (P2, new) | TypeScript SDK exists; no codegen from schema | code-to-plan | `axon-09ada1e6` |
| G-13 | CLI output formatting | INCOMPLETE | PRD P0 #12: JSON/YAML output | Flags parsed; partial format support | quality-improvement | `axon-15356a0f` |
| G-14 | Server storage backend | DIVERGENT | TR Section 3: server mode uses PostgreSQL | Server uses MemoryStorageAdapter, not PostgreSQL | code-to-plan | `axon-bba3ced7` |
| G-15 | GraphQL tests | INCOMPLETE | FEAT-015: full GraphQL API | Dynamic schema generation works; no dedicated test suite | quality-improvement | `axon-3d52d938` |
| G-16 | Schema version per entity | INCOMPLETE | FEAT-017 (refined): entities track schema version | Entity struct has no `schema_version` field | code-to-plan | `axon-e4c08270` |

---

## 8. Traceability Matrix

| Vision Item | Requirement | Feature | ADR | Test Reference | Code Status | Classification |
|-------------|-------------|---------|-----|----------------|-------------|---------------|
| Entity-first OLTP | Entity model (P0) | FEAT-004, FEAT-007 | — | SCN-*, PROP-*, backend_parity | Complete | ALIGNED |
| Schema-driven | Schema engine (P0) | FEAT-002 | ADR-002, ADR-007, ADR-008 | schema tests, SCN-005 | Complete | ALIGNED |
| Auditable | Audit log (P0) | FEAT-003 | — | PROP-002, INV-002/003, audit tests | Complete | ALIGNED |
| Agent-accessible | MCP (P1) | FEAT-016 | ADR-013 | mcp tests | CRUD only; advanced stubs | INCOMPLETE |
| Agent-accessible | GraphQL (P1) | FEAT-015 | ADR-012 | graphql tests | Dynamic schema works | ALIGNED |
| ACID transactions | Transactions (P0) | FEAT-008 | ADR-004 | PROP-004, INV-001, sim | Complete | ALIGNED |
| Cloud-native | Server mode (P1) | FEAT-005 | ADR-003 | api_contract | HTTP works; gRPC partial | INCOMPLETE |
| Cloud-native | PostgreSQL (P0) | TR-3 | ADR-003 | — | 60% complete | INCOMPLETE |
| Agent guardrails | P1 #16 | FEAT-022 | — | — | Not started | UNIMPLEMENTED |
| Rollback/recovery | P1 #17 | FEAT-023 | — | — | Partial (entity revert) | INCOMPLETE |
| Control plane | P1 #18 | FEAT-025 | — | — | Not started | UNIMPLEMENTED |
| BYOC commercial | Vision | FEAT-025 | — | — | Not started | UNIMPLEMENTED |

---

## 9. Review Issue Summary

| Review Issue | Functional Area | Key Findings | Direction |
|-------------|-----------------|-------------|-----------|
| `axon-5e455d65` | core-data-model | ALIGNED — all core types, RBAC, entity model complete | — |
| `axon-e4c08270` | schema-engine | ALIGNED with gap: per-entity schema version tracking missing | code-to-plan |
| `axon-a2ab6904` | audit-log | ALIGNED — append-only, immutable, CDC conversion | — |
| `axon-26619c23` | transactions | ALIGNED — OCC, serializable, multi-entity atomic | — |
| `axon-c6cd37ae` | storage-adapters | INCOMPLETE — PostgreSQL 60%, Memory/SQLite complete | code-to-plan |
| `axon-bba3ced7` | api-surface | INCOMPLETE — HTTP 100%, gRPC 60%, server uses Memory not PG | code-to-plan |
| `axon-15356a0f` | cli | ALIGNED with minor gap: output formatting incomplete | quality-improvement |
| `axon-b5f998a5` | server-mode | INCOMPLETE — auth stub, CDC Kafka untested | code-to-plan |
| `axon-3d52d938` | graphql | ALIGNED with gap: no dedicated test suite | quality-improvement |
| `axon-92f06b94` | mcp | INCOMPLETE — 4 advanced tool stubs | code-to-plan |
| `axon-b2abc920` | simulation-testing | ALIGNED — 9 invariants, deterministic replay | — |
| `axon-09ada1e6` | admin-ui | INCOMPLETE — scaffolded, not functional | code-to-plan |

---

## 10. Execution Issues Generated

| Issue ID | Type | Labels | Goal | Dependencies |
|---------|------|--------|------|-------------|
| `axon-6268a787` | task | storage-adapters | Complete PostgreSQL adapter (indexes, compounds, links, gates) | — |
| `axon-234393ca` | task | api-surface | Complete gRPC service (list, query, schema, collection ops) | — |
| `axon-e55ef6e7` | task | mcp | Wire MCP advanced tools (aggregate, query, link-candidates, neighbors) | `axon-234393ca` |
| `axon-5aeeb89d` | task | multi-tenancy | Persist namespace/database operations | — |
| `axon-7e66f08f` | task | cdc | CDC integration test with Redpanda broker | `axon-6268a787` |
| `axon-57a0b092` | task | cli | Complete JSON/YAML output formatting | — |
| `axon-ac3f5f6f` | task | graphql | Add dedicated GraphQL test suite | — |

---

## 11. Issue Coverage Verification

| Gap | Covering Issue | Status |
|-----|---------------|--------|
| G-01 PostgreSQL adapter | `axon-6268a787` | covered |
| G-02 gRPC service completion | `axon-234393ca` | covered |
| G-03 MCP advanced tools | `axon-e55ef6e7` | covered |
| G-04 Tailscale auth | deferred per ADR-005 | deferred |
| G-05 Admin UI | deferred (scaffolded) | deferred |
| G-06 Multi-tenancy persistence | `axon-5aeeb89d` | covered |
| G-07 CDC Kafka integration | `axon-7e66f08f` | covered |
| G-08 Workflow state machines | P2, deferred | deferred |
| G-09 Agent guardrails | `axon-91bfabea` | covered |
| G-10 Rollback/recovery | `axon-3f15171a` | covered |
| G-11 Control plane | `axon-bd579636` | covered |
| G-12 Application substrate | `axon-61a657c7` | covered |
| G-13 CLI output formatting | `axon-57a0b092` | covered |
| G-14 Server storage backend | covered by G-01 (`axon-6268a787`) | covered |
| G-15 GraphQL tests | `axon-ac3f5f6f` | covered |
| G-16 Schema version per entity | `axon-8c5c2442` | covered |

---

## 12. Execution Order

### Critical Path

```
PostgreSQL adapter (G-01) ──> Server uses PG (G-14)
                          ──> CDC Kafka test (G-07)

gRPC completion (G-02)   ──> MCP tools wiring (G-03)

Schema version/entity (G-16) [independent]
Rollback/recovery (G-10)     [independent]
```

### Recommended Execution Sets

**Set 1 (parallel, highest impact)**:
- PostgreSQL adapter completion (unblocks server mode for production)
- gRPC service completion (unblocks full MCP)
- Schema version per entity (incremental, low risk)

**Set 2 (parallel, after Set 1)**:
- MCP advanced tools (depends on gRPC)
- Server mode: switch to PostgreSQL backend
- CDC Kafka integration test

**Set 3 (parallel, new features)**:
- Rollback/recovery (builds on audit log)
- Agent guardrails (builds on RBAC)
- CLI output formatting (polish)
- GraphQL test suite (quality)

**Set 4 (deferred)**:
- Control plane
- Application substrate
- Admin UI
- Tailscale auth
- Multi-tenancy persistence

---

## 13. Open Decisions

1. **G-08 Workflow state machines**: Gates provide partial workflow semantics. Is a formal FSM implementation needed for V1, or are gates sufficient? Currently P2.
2. **Server storage backend**: Server currently uses MemoryStorageAdapter. Should it default to SQLite (like CLI) or require PostgreSQL configuration?
3. **CDC testing**: Should CDC Kafka integration be tested against a real broker (Redpanda in Docker) or is the in-memory sink sufficient for V1?
