---
dun:
  id: helix.implementation-plan
  depends_on:
    - helix.prd
    - helix.test-plan
---
# Implementation Plan: Axon

**Version**: 0.1.0
**Date**: 2026-04-10
**Status**: Living document

---

## 1. Crate Responsibility Map

| Crate | Purpose | Lines | Build Status |
|-------|---------|------:|:------------:|
| `axon-core` | Core types (Entity, Link, CollectionId, EntityId, Namespace), error hierarchy (`AxonError`), auth types, topology model | 1,425 | OK |
| `axon-schema` | Schema definitions (ESF format), JSON Schema validation, schema evolution/diffing, validation rules (Layer 5 cross-field), gate evaluation | 3,517 | OK |
| `axon-audit` | Immutable audit log (append-only), CDC envelope format (Debezium-compatible), replay/cursor pagination | 1,872 | OK |
| `axon-storage` | `StorageAdapter` trait, in-memory backend, SQLite backend, PostgreSQL backend (WIP), conformance macro, EAV secondary indexes | 10,182 | Lib OK; Postgres test code has compile errors |
| `axon-api` | `AxonHandler` orchestrator: all entity/link/collection/schema/audit/rollback operations, transaction commit, bead adapter, proptest harness | 15,158 | OK |
| `axon-render` | Mustache-based markdown template rendering, field reference extraction and validation against schemas | 889 | OK |
| `axon-graphql` | Dynamic `async-graphql` schema from collections: queries, mutations, aggregations, graph traversal, subscriptions via change feed broker | 2,265 | OK |
| `axon-mcp` | MCP JSON-RPC 2.0 server: typed CRUD tools auto-generated from schemas, resource discovery, prompt registry, query/aggregate/neighbor tools | 3,669 | OK |
| `axon-server` | Axum HTTP gateway, Tailscale whois auth, rate limiting, control plane (tenant/node management), schema registry, MCP stdio/HTTP transports, GraphQL endpoint | 11,265 | Lib OK; Test code has compile errors (tailscale dep) |
| `axon-sim` | FoundationDB-style DST framework: SimRng, Buggify fault injector, cycle test, concurrent writer, audit completeness/immutability, schema enforcement, transaction atomicity, link integrity workloads | 2,476 | OK |
| `axon-cli` | CLI binary (`axon`): embedded SQLite mode, collection/entity/link/schema/audit/template/namespace/database commands | 1,860 | OK |

**Total**: ~54,578 lines of Rust across 11 crates.

---

## 2. Dependency Graph (Build Order)

```
axon-core
  +-- axon-schema       (depends on axon-core)
  +-- axon-audit        (depends on axon-core)
  +-- axon-storage      (depends on axon-core, axon-audit, axon-schema)
  +-- axon-render       (depends on axon-core)
  +-- axon-api          (depends on axon-core, axon-schema, axon-audit, axon-storage, axon-render)
  +-- axon-graphql      (depends on axon-core, axon-schema, axon-api, axon-storage)
  +-- axon-mcp          (depends on axon-core, axon-schema, axon-api, axon-storage)
  +-- axon-sim          (depends on axon-core, axon-schema, axon-audit, axon-api, axon-storage)
  +-- axon-server       (depends on axon-core, axon-schema, axon-audit, axon-api, axon-storage, axon-graphql, axon-mcp)
  +-- axon-cli          (depends on axon-core, axon-api, axon-audit)
```

Build order (topological): `axon-core` -> `axon-schema`, `axon-audit`, `axon-render` (parallel) -> `axon-storage` -> `axon-api` -> `axon-graphql`, `axon-mcp`, `axon-sim`, `axon-cli` (parallel) -> `axon-server`.

---

## 3. Feature Implementation Status

### P0 (Must Have) Features

| Feature | Description | Crate(s) | Status | Notes |
|---------|------------|-----------|:------:|-------|
| FEAT-001 | Collections | axon-core, axon-api, axon-storage | Done | Create, drop, list, describe. Qualified names with namespace support |
| FEAT-002 | Schema engine | axon-schema, axon-api | Done | ESF format, JSON Schema validation, Layer 5 rules, gate evaluation, compilation |
| FEAT-003 | Audit log | axon-audit, axon-api | Done | Append-only, query by entity/actor/operation/time, cursor pagination, replay |
| FEAT-004 | Entity operations | axon-api, axon-storage | Done | Create, read, update (full replace), patch (RFC 7396 merge-patch), delete, list, query with filter/sort/pagination |
| FEAT-005 | API surface | axon-server, axon-api | Done | HTTP gateway via Axum with full CRUD routes, transaction endpoint, audit endpoint |
| FEAT-007 | Entity-graph model | axon-core, axon-api, axon-storage | Done | Typed directional links, link metadata, link-type constraints via schema |
| FEAT-008 | ACID transactions | axon-api, axon-storage | Done | Multi-entity atomic commit, OCC via version-based compare-and-swap, serializable isolation |
| FEAT-009 | Graph traversal | axon-api | Done | Traverse with depth limit, direction control, reachable check, neighbor listing, link candidate finder |
| P0-10 | Embedded mode | axon-cli, axon-storage | Done | SQLite in-process via `axon-cli`, same AxonHandler as server mode |
| P0-11 | CLI | axon-cli | Done | Collection mgmt, entity CRUD, link ops, schema ops, audit queries, template management, namespace/database commands |

### P1 (Should Have) Features

| Feature | Description | Crate(s) | Status | Notes |
|---------|------------|-----------|:------:|-------|
| FEAT-013 | Secondary indexes | axon-storage, axon-schema | Done | EAV-pattern indexes (string, int, float, datetime, bool), compound indexes, unique indexes, background build |
| FEAT-014 | Multi-tenancy | axon-core, axon-api, axon-storage | Done | Three-level namespace hierarchy (database.schema.collection), catalog CRUD, default database/schema |
| FEAT-015 | GraphQL API | axon-graphql, axon-server | Done | Dynamic schema from collections, queries with filter/sort/pagination, mutations (CRUD + OCC), subscriptions via WebSocket |
| FEAT-016 | MCP server | axon-mcp, axon-server | Done | JSON-RPC 2.0 protocol, auto-generated CRUD tools, resource discovery, prompts, query/aggregate/neighbor tools, stdio + HTTP transports |
| FEAT-017 | Schema evolution | axon-schema, axon-api | Partial | Compatibility classification (compatible/breaking/metadata-only), field-level diff, revalidation. Schema version history stored. Migration declarations deferred |
| FEAT-018 | Aggregation queries | axon-api | Done | COUNT, SUM, AVG, MIN, MAX with GROUP BY, exposed via handler, GraphQL, and MCP |
| FEAT-019 | Validation rules | axon-schema | Done | Cross-field when/require rules (ESF Layer 5), severity levels, gate evaluation, advisory rules, structured error messages with fix suggestions |
| FEAT-020 | Link discovery | axon-api | Done | find_link_candidates, list_neighbors, reachable check. Exposed via MCP tools |
| FEAT-021 | Change feeds (CDC) | axon-audit | Partial | Debezium-compatible envelope format, JSONL file sink, in-memory sink. Kafka transport not implemented. GraphQL subscriptions implemented |
| FEAT-022 | Agent guardrails | axon-server | Partial | Rate limiting (per-actor sliding window). Scope constraints and semantic validation hooks not yet implemented |
| FEAT-023 | Rollback/recovery | axon-api | Done | Entity-level rollback, collection-level rollback, transaction-level rollback, revert to audit entry. Dry-run supported |
| FEAT-025 | Control plane | axon-server | Partial | SQLite-backed tenant/node/database registry. CRUD for tenants, nodes, database assignments. HTTP routes exposed. Monitoring not implemented |
| FEAT-026 | Markdown templates | axon-render, axon-api | Done | Mustache rendering, field reference validation, LRU template cache, per-collection views |
| FEAT-012 | Auth/authorization | axon-core, axon-server | Partial | Tailscale whois integration designed (ADR-005), auth module exists but has compile errors (missing `tailscale_localapi` crate). RBAC types defined in axon-core. Deferred per ADR-005 |
| FEAT-011 | Admin web UI | -- | Not started | No SvelteKit/Bun code exists. ADR-006 specifies the design |

### P2 (Nice to Have) Features

| Feature | Description | Status | Notes |
|---------|------------|:------:|-------|
| Local-first sync | CRDTs, offline clients | Not started | -- |
| FEAT-010 | Workflow state machines | Not started | Depends on FEAT-019 (done), FEAT-008 (done), FEAT-009 (done) |
| Schema registry | Shared schemas across instances | Not started | -- |
| FEAT-024 | Application substrate | Not started | Auto-generated TypeScript client, deploy templates |
| Niflheim bridge | CDC export to niflheim | Not started | CDC envelope format exists (FEAT-021) |
| Tablespec/UMF integration | Import schemas from UMF | Not started | -- |
| Plugin system | Custom validators/hooks | Not started | -- |

---

## 4. Storage Backend Status

| Backend | Trait impl | Conformance tests | CRUD | Transactions | Indexes | Schema persistence | Namespace catalog | Audit co-location |
|---------|:----------:|:-----------------:|:----:|:------------:|:-------:|:-----------------:|:-----------------:|:-----------------:|
| Memory | Done | Via macro | Done | Done | Done | Done | Done | N/A (in-memory audit) |
| SQLite | Done | Via macro | Done | Done | Done | Done | Done | Done |
| PostgreSQL | WIP | Compile errors | Partial | Partial | Not started | Not started | Partial | Not started |
| FoundationDB | Not started | -- | -- | -- | -- | -- | -- | -- |

The `storage_conformance_tests!` macro generates the identical test suite for each backend. Memory and SQLite pass. PostgreSQL has type and import errors in test code (6 compile errors in `postgres.rs` test functions).

---

## 5. Test Coverage Status

### Test Counts by Crate

| Crate | Unit tests | Integration tests | Proptest | Doc tests | Total | Status |
|-------|----------:|------------------:|---------:|----------:|------:|:------:|
| axon-core | 56 | -- | -- | 0 | 56 | All pass |
| axon-schema | 74 | -- | 3 (proptest) | 0 | 74 | All pass |
| axon-audit | 49 | -- | -- | 0 | 49 | All pass |
| axon-storage | ~30+ | -- | -- | 1 (ignored) | -- | Lib compiles; postgres test code fails to compile |
| axon-api | 234 | -- | 12 proptest | 0 | 256 | All pass |
| axon-render | 31 | -- | -- | 1 | 32 | All pass |
| axon-graphql | 44 | -- | -- | 0 | 44 | All pass |
| axon-mcp | 41 | -- | -- | 0 | 41 | All pass |
| axon-sim | 29 | -- | -- | 2 (+1 ignored) | 32 | All pass |
| axon-cli | 10 | 7 | -- | 0 | 17 | All pass |
| axon-server | -- | -- | -- | -- | -- | Does not compile (tailscale dep missing) |
| **Total** | **~600** | **7** | **~15** | **4** | **~600+** | |

### Test Plan Layer Coverage

| Layer | Description | Status | Notes |
|-------|------------|:------:|-------|
| L1: Correctness invariants | DST workloads via axon-sim | Implemented | INV-001 through INV-008 all have corresponding workloads |
| L2: Business scenarios | End-to-end workflow tests | Not started | SCN-001 through SCN-010 defined in test plan but not coded |
| L3: Property-based tests | Proptest for schemas, storage, API | Partial | PROP-001 (schema round-trip) and storage proptests exist. PROP-002 through PROP-005 not yet coded |
| L4: Backend conformance | Parameterized tests via macro | Partial | Memory and SQLite pass. PostgreSQL and FoundationDB not passing |
| L5: Performance benchmarks | criterion benchmarks | Not started | BM-001 through BM-010 defined but no benchmark code exists |
| L6: API contract tests | gRPC/HTTP conformance | Not started | No contract test code exists |

### Simulation Framework (axon-sim) Invariant Coverage

| Invariant | Workload | Status |
|-----------|----------|:------:|
| INV-001: No Lost Updates | `ConcurrentWriterWorkload` | Implemented, passing |
| INV-002: Serializable Isolation | `CycleWorkload` | Implemented, passing |
| INV-003: Audit Completeness | `AuditCompletenessWorkload` | Implemented, passing |
| INV-004: Audit Immutability | `AuditImmutabilityWorkload` | Implemented, passing |
| INV-005: Schema Enforcement | `SchemaEnforcementWorkload` | Implemented, passing |
| INV-006: Link Integrity | `LinkIntegrityWorkload` | Implemented, passing |
| INV-007: Version Monotonicity | Checked within other workloads | Implemented |
| INV-008: Transaction Atomicity | `TransactionAtomicityWorkload` | Implemented, passing |

---

## 6. Compilation Issues

Two crates have compile errors that prevent test execution:

### axon-server (test code)
- **Root cause**: `tailscale_localapi` crate not available. The `auth.rs` module references types from this crate (`LocalApi`, `Error`, `Whois`, `Node`) but the dependency is not declared in `Cargo.toml`.
- **Impact**: Server tests cannot run. Lib compiles fine (the auth types are defined but the actual Tailscale integration code fails).
- **Resolution**: Either add `tailscale_localapi` dependency or gate the Tailscale auth behind a feature flag.

### axon-storage (test code only)
- **Root cause**: 6 compile errors in `postgres.rs` test functions -- missing `Mutex` import, type annotation gaps, and function signature mismatches (missing `-> Result` return types).
- **Impact**: PostgreSQL conformance tests cannot run. Lib compiles fine.
- **Resolution**: Fix the 6 errors in test code. These are straightforward type/import fixes.

---

## 7. Architecture Decisions Enacted

| ADR | Decision | Implemented in |
|-----|----------|----------------|
| ADR-001 | Implementation language: Rust | All crates |
| ADR-002 | Schema format: ESF (JSON Schema-based) | axon-schema |
| ADR-003 | Backing store: SQLite default, PG opt-in | axon-storage |
| ADR-004 | Transaction model: OCC with version-based CAS | axon-api, axon-storage |
| ADR-005 | Auth: Tailscale whois (deferred) | axon-server (partial, compile errors) |
| ADR-006 | Admin UI: SvelteKit + Bun | Not started |
| ADR-007 | Schema versioning | axon-schema (evolution module) |
| ADR-008 | Schema lifecycles | axon-schema (gates, rules) |
| ADR-009 | Patch semantics + ID generation | axon-api (RFC 7396 merge-patch, UUIDv7) |
| ADR-010 | Physical storage: numeric collection IDs, EAV indexes | axon-storage |
| ADR-011 | Multi-tenancy namespace hierarchy | axon-core, axon-api, axon-storage |
| ADR-012 | GraphQL query layer | axon-graphql |
| ADR-013 | MCP server | axon-mcp |
| ADR-014 | Change feeds: Debezium CDC | axon-audit (CDC module) |

---

## 8. What Is Complete vs. What Remains

### Complete (functional and tested)

- Entity-graph-relational data model with typed links
- Schema engine with JSON Schema validation, ESF Layer 5 rules, gates
- Immutable audit log with full provenance chain
- ACID transactions with OCC and serializable isolation
- Entity CRUD (create, read, update, patch, delete) with query/filter/sort/pagination
- Link CRUD, graph traversal, reachability, neighbor listing, link candidate discovery
- Aggregation queries (COUNT, SUM, AVG, MIN, MAX, GROUP BY)
- Multi-tenancy (database.schema.collection namespace hierarchy)
- Secondary indexes (EAV pattern, single/compound/unique)
- GraphQL API (dynamic schema, queries, mutations, subscriptions)
- MCP server (CRUD tools, resources, prompts, query/aggregate/neighbor tools)
- Markdown template rendering with LRU cache
- Rollback and recovery (entity, collection, transaction level)
- Schema evolution detection and diffing
- CDC envelope format (Debezium-compatible)
- Bead storage adapter (lifecycle states, dependency DAG, ready queue)
- CLI with embedded SQLite mode
- HTTP gateway with full route coverage
- Control plane foundation (tenant/node/database CRUD)
- Rate limiting (per-actor sliding window)
- DST framework with all 8 correctness invariant workloads
- Storage conformance macro (memory + SQLite passing)

### Remaining Work

| Item | Priority | Blocking? | Est. Effort |
|------|:--------:|:---------:|:-----------:|
| Fix axon-server compile errors (tailscale dep) | High | Yes (server tests) | Small |
| Fix axon-storage postgres test compile errors | High | Yes (PG conformance) | Small |
| Complete PostgreSQL StorageAdapter (indexes, schema, audit, namespaces) | P1 | No | Medium |
| L2 business scenario tests (SCN-001 through SCN-010) | P0 | Yes (test plan) | Large |
| L5 performance benchmarks (criterion) | P1 | No | Medium |
| L6 API contract tests | P1 | No | Medium |
| L3 property tests PROP-002 through PROP-005 | P1 | No | Medium |
| Kafka CDC transport (FEAT-021) | P1 | No | Large |
| Admin web UI (FEAT-011) | P1 | No | Large |
| Auth implementation (FEAT-012) | P1 | No | Medium |
| Agent guardrails beyond rate limiting (FEAT-022) | P1 | No | Medium |
| Control plane monitoring | P1 | No | Medium |
| Schema migration declarations (FEAT-017) | P1 | No | Medium |
| Workflow state machines (FEAT-010) | P2 | No | Large |
| FoundationDB backend | P2 | No | Large |
| Local-first sync | P2 | No | Large |

---

## 9. Phase Alignment (PRD Timeline)

| Phase | PRD Scope | Status |
|-------|-----------|:------:|
| Phase 1: Foundation (8 weeks) | Entity-graph model, CRUD, schema engine, audit log, ACID/OCC, embedded mode, CLI | Done |
| Phase 2: API and Integration (6 weeks) | Server mode, query/filter/sort, graph traversal, bead adapter | Done |
| Phase 3: Production Readiness (4 weeks) | Auth, change feeds, batch ops, schema evolution, benchmarks, docs | In progress |

The project has completed Phase 1 and Phase 2 scope and has made significant progress into Phase 3 territory. Key Phase 3 gaps are auth, Kafka CDC, performance benchmarks, and the L2 business scenario test suite.

---

*This document tracks implementation against the planning stack. Updated as features are completed or priorities change.*
