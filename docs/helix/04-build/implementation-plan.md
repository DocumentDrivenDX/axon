---
ddx:
  id: helix.implementation-plan
  depends_on:
    - helix.prd
    - helix.test-plan
---
# Implementation Plan: Axon

**Version**: 0.2.0
**Date**: 2026-04-10
**Revised**: 2026-04-22
**Status**: Living document

---

## 1. Crate Responsibility Map

| Crate | Purpose | Lines | Build Status |
|-------|---------|------:|:------------:|
| `axon-core` | Core types (Entity, Link, CollectionId, EntityId, Namespace), error hierarchy (`AxonError`), auth types (Identity, Role, Grants, JwtClaims), topology model | 3,544 | OK |
| `axon-schema` | Schema definitions (ESF format), JSON Schema validation, schema evolution/diffing, validation rules (Layer 5 cross-field), gate evaluation | 3,594 | OK |
| `axon-audit` | Immutable audit log (append-only), CDC envelope format (Debezium-compatible), replay/cursor pagination | 2,630 | OK |
| `axon-storage` | `StorageAdapter` trait, in-memory backend, SQLite backend, PostgreSQL backend, conformance macro, EAV secondary indexes | 13,718 | OK |
| `axon-api` | `AxonHandler` orchestrator: all entity/link/collection/schema/audit/rollback operations, transaction commit, bead adapter, proptest harness, audit attribution | 16,557 | OK |
| `axon-render` | Mustache-based markdown template rendering, field reference extraction and validation against schemas | 890 | OK |
| `axon-graphql` | Dynamic `async-graphql` schema from collections: queries, mutations, aggregations, graph traversal, subscriptions via change feed broker | 2,566 | OK |
| `axon-mcp` | MCP JSON-RPC 2.0 server: typed CRUD tools auto-generated from schemas, resource discovery, prompt registry, query/aggregate/neighbor tools | 3,834 | OK |
| `axon-server` | Axum HTTP gateway, Tailscale whois auth (Unix socket), JWT auth pipeline (9-step), RBAC enforcement, rate limiting, actor scope constraints, control plane routes (tenant/user/credential/database/member management), path-based routing (ADR-018), GraphQL + MCP endpoints, self-signed TLS bootstrap | 16,822 | OK |
| `axon-sim` | FoundationDB-style DST framework: SimRng, Buggify fault injector, cycle test, concurrent writer, audit completeness/immutability, schema enforcement, transaction atomicity, link integrity workloads | 2,505 | OK |
| `axon-cli` | CLI binary (`axon`): unified serve/mcp/doctor/install subcommands, embedded SQLite mode, client mode against running server, collection/entity/link/schema/audit/template/namespace/database commands | 3,854 | OK |
| `axon-config` | XDG-compliant path resolution, TOML configuration loading (`AxonConfig`), platform-appropriate defaults | 510 | OK |
| `axon-control-plane` | Standalone control plane crate: tenant data model, `ControlPlaneStore` trait, `ControlPlaneService` business logic, HTTP handlers. Data-sovereignty-aware (never reads customer entity data) | 1,591 | OK |

**Total**: ~72,615 lines of Rust across 13 crates.

Additional surfaces:
- **Admin UI** (`ui/`): SvelteKit + Bun, 12 Svelte pages with tenant-scoped routing, Playwright E2E tests
- **TypeScript SDK** (`sdk/typescript/`): GraphQL-first client target with
  compatibility HTTP/gRPC clients still present during transition
- **Website** (`website/`): Marketing/documentation site

---

## 2. Dependency Graph (Build Order)

```
axon-core
  +-- axon-schema          (depends on axon-core)
  +-- axon-audit           (depends on axon-core)
  +-- axon-storage         (depends on axon-core, axon-audit, axon-schema)
  +-- axon-render          (depends on axon-core)
  +-- axon-config          (standalone)
  +-- axon-control-plane   (standalone)
  +-- axon-api             (depends on axon-core, axon-schema, axon-audit, axon-storage, axon-render)
  +-- axon-graphql         (depends on axon-core, axon-schema, axon-api, axon-storage)
  +-- axon-mcp             (depends on axon-core, axon-schema, axon-api, axon-storage)
  +-- axon-sim             (depends on axon-core, axon-schema, axon-audit, axon-api, axon-storage)
  +-- axon-server          (depends on axon-core, axon-schema, axon-audit, axon-api, axon-storage, axon-graphql, axon-mcp)
  +-- axon-cli             (depends on axon-core, axon-api, axon-audit, axon-server)
```

Build order (topological): `axon-core` -> `axon-schema`, `axon-audit`, `axon-render`, `axon-config`, `axon-control-plane` (parallel) -> `axon-storage` -> `axon-api` -> `axon-graphql`, `axon-mcp`, `axon-sim` (parallel) -> `axon-server` -> `axon-cli`.

---

## 3. Feature Implementation Status

### P0 (Must Have) Features

| Feature | Description | Crate(s) | Status | Notes |
|---------|------------|-----------|:------:|-------|
| FEAT-001 | Collections | axon-core, axon-api, axon-storage | Done | Create, drop, list, describe. Qualified names with namespace support |
| FEAT-002 | Schema engine | axon-schema, axon-api | Done | ESF format, JSON Schema validation, Layer 5 rules, gate evaluation, compilation |
| FEAT-003 | Audit log | axon-audit, axon-api | Done | Append-only, query by entity/actor/operation/time, cursor pagination, replay |
| FEAT-004 | Entity operations | axon-api, axon-storage | Done | Create, read, update (full replace), patch (RFC 7396 merge-patch), delete, list, query with filter/sort/pagination |
| FEAT-005 | API surface | axon-server, axon-api | Done | HTTP gateway via Axum with pure path-based routing (ADR-018), transaction endpoint, audit endpoint |
| FEAT-015 | GraphQL API | axon-graphql, axon-server | Done; policy hardening pending | Dynamic schema from collections, queries with filter/sort/pagination, mutations (CRUD + OCC), subscriptions via WebSocket. GraphQL is now the primary public application surface |
| FEAT-016 | MCP server | axon-mcp, axon-server | Done; policy hardening pending | JSON-RPC 2.0 protocol, auto-generated CRUD tools, resources, prompts, query/aggregate/neighbor tools, stdio + HTTP transports. MCP mirrors GraphQL for agents |
| FEAT-007 | Entity-graph model | axon-core, axon-api, axon-storage | Done | Typed directional links, link metadata, link-type constraints via schema |
| FEAT-008 | ACID transactions | axon-api, axon-storage | Done | Multi-entity atomic commit, OCC via version-based compare-and-swap, serializable isolation |
| FEAT-009 | Graph traversal | axon-api | Done | Traverse with depth limit, direction control, reachable check, neighbor listing, link candidate finder. HTTP contract test for `direction=reverse` complete |
| FEAT-029 | Data-layer access control policies | axon-schema, axon-api, axon-graphql, axon-mcp | Not started | P0 product hardening. GraphQL/MCP row policies, field redaction, relationship traversal safety, policy authoring compiler. Execution parent: `axon-d556e197` |
| FEAT-030 | Mutation intents and approval | axon-api, axon-graphql, axon-mcp, axon-audit | Not started | P0 product hardening. Preview, policy explanation, approval routing, TOCTOU-safe intent tokens. Execution parent: `axon-c7111156` |
| FEAT-031 | Policy and intents admin UI | ui/ | Not started | P0 product hardening. Axon web UI coverage for policy explanation, dry-run, redaction, denied writes, mutation preview, approval inbox, stale-intent handling, MCP envelope visibility, and audit lineage. Execution parent: `axon-c5a64173` |
| P0-10 | Embedded mode | axon-cli, axon-storage | Done | SQLite in-process via `axon-cli`, same AxonHandler as server mode |
| P0-11 | CLI | axon-cli | Done | Collection mgmt, entity CRUD, link ops, schema ops, audit queries, template management, namespace/database commands |

### P1 (Should Have) Features

| Feature | Description | Crate(s) | Status | Notes |
|---------|------------|-----------|:------:|-------|
| FEAT-011 | Admin web UI | ui/ | Done | 12 SvelteKit pages: tenants, databases, collections, entities, schemas, audit, rollback, users, credentials, members, GraphQL playground. Tenant-scoped routing per ADR-018. 6 Playwright E2E specs |
| FEAT-012 | Auth/authorization | axon-core, axon-server | Done (V1+V5) | Tailscale whois via Unix socket (ADR-005); 9-step JWT auth pipeline (ADR-018); RBAC role checks on all write paths; 42+ integration tests; M:N tenant membership; credential issue/revoke; `--no-auth` and `--guest-role` modes |
| FEAT-013 | Secondary indexes | axon-storage, axon-schema | Done | EAV-pattern indexes (string, int, float, datetime, bool), compound indexes, unique indexes, background build |
| FEAT-014 | Multi-tenancy | axon-core, axon-api, axon-server | Done | Four-level hierarchy (tenant → database → schema → collection) per ADR-018. Pure path-based routing. DatabaseRouter + DatabaseAdapterFactory. Per-tenant SQLite isolation. No legacy routes |
| FEAT-017 | Schema evolution | axon-schema, axon-api | Done | Compatibility classification (compatible/breaking/metadata-only), field-level diff, revalidation, schema_version stamping on all entities, 409 on breaking changes without `force`, `axon schema revalidate` CLI. Migration declarations deferred |
| FEAT-018 | Aggregation queries | axon-api | Done | COUNT, SUM, AVG, MIN, MAX with GROUP BY, exposed via handler, GraphQL, and MCP |
| FEAT-019 | Validation rules | axon-schema | Done | Cross-field when/require rules (ESF Layer 5), severity levels, gate evaluation, advisory rules, structured error messages with fix suggestions |
| FEAT-020 | Link discovery | axon-api | Done | find_link_candidates, list_neighbors, reachable check. Exposed via MCP tools |
| FEAT-021 | Change feeds (CDC) | axon-audit | Partial | Debezium-compatible envelope format, JSONL file sink, in-memory sink. GraphQL subscriptions implemented. Kafka transport not implemented |
| FEAT-022 | Agent guardrails | axon-server | Partial | Rate limiting (per-actor sliding window, 429 + Retry-After). Actor scope constraints. Semantic validation hooks not yet implemented |
| FEAT-023 | Rollback/recovery | axon-api | Done | Entity-level rollback, collection-level rollback, transaction-level rollback, revert to audit entry. Dry-run supported. UI rollback pages |
| FEAT-025 | Control plane | axon-server, axon-control-plane | Done | SQLite-backed tenant/user/credential/database/member management. 20+ HTTP routes. Retention policies. Data-sovereignty-aware. Monitoring not implemented |
| FEAT-026 | Markdown templates | axon-render, axon-api | Done | Mustache rendering, field reference validation, LRU template cache, per-collection views |
| FEAT-028 | Unified binary | axon-cli, axon-config, axon-server | Done | `axon serve/mcp/doctor/install` subcommands. XDG-compliant paths. TOML config. Client mode against running server. Install script. Self-signed TLS bootstrap. PostgreSQL per-tenant provisioning |
| FEAT-010 | Entity state machines and transition guards | axon-schema, axon-api, axon-graphql, axon-mcp | Not started | P1. Guarded entity transitions only; durable workflow orchestration is explicitly out of scope |

### P2 (Nice to Have) Features

| Feature | Description | Status | Notes |
|---------|------------|:------:|-------|
| Local-first sync | CRDTs, offline clients | Not started | -- |
| Schema registry | Shared schemas across instances | Not started | -- |
| FEAT-024 | Application substrate | Deferred | graphql-codegen deemed sufficient for now |
| FEAT-027 | Git mirror | Draft spec only | No implementation |
| Niflheim bridge | CDC export to niflheim | Not started | CDC envelope format exists (FEAT-021) |
| Tablespec/UMF integration | Import schemas from UMF | Not started | -- |
| Plugin system | Custom validators/hooks | Not started | -- |

---

## 3a. GraphQL-Primary Policy Workstream

The 2026-04-22 product review reset the near-term execution order around the
non-negotiable API target: GraphQL is primary, MCP is agent-native, and REST is
fallback only.

### P0 Proof Slice

The next product proof must demonstrate a realistic invoice/procurement schema:

1. ESF schema with invoices, vendors, users, link types, indexes, and entity
   transition guards.
2. Schema-adjacent `access_control` policy compiled under ADR-019.
3. GraphQL query with relationship traversal, row filtering before pagination,
   safe `totalCount`, and nullable redacted fields.
4. MCP tool metadata exposing the same policy envelopes as GraphQL.
5. GraphQL/MCP mutation preview that returns diff, policy decision, pre-image
   versions, and intent token.
6. Approval-routed mutation that fails if entity version, schema version,
   policy version, grant version, or operation hash changes before commit.
7. Audit trail linking agent identity, delegated authority, tool call,
   policy decision, approval, and redacted pre/post images.

### Priority Order

1. FEAT-029 GraphQL/MCP policy compiler and enforcement.
2. FEAT-030 mutation preview, approval, and intent execution.
3. Agent identity/delegation audit completeness: `user_id`, `agent_id`,
   `delegated_by`, credential ID, grant version, policy version.
4. Developer policy test harness: fixture subjects, simulated agents, compile
   reports, and optional historical audit dry-run.
5. FEAT-031 Axon web UI policy and intent workflows: policy explain/dry-run,
   policy-safe entity browsing, mutation preview, approval inbox, stale-intent
   handling, MCP envelope visibility, and audit lineage.
6. Operator controls beyond FEAT-031: break-glass audit, credential rotation,
   cost/quota visibility.
7. Minimal compliance/erasure story: redacted audit reads, tenant/field
   encryption hooks, crypto-shred/tombstone semantics.

### Explicit Cuts

- Broad REST parity for policy, preview, and approval.
- Durable long-running workflow orchestration.
- Generated app substrate as a core product path.
- Git mirror as primary adoption path.
- Broad framework integrations before the GraphQL/MCP policy slice is proven.
- Arbitrary graph-wide point-in-time rollback.

## 4. Storage Backend Status

| Backend | Trait impl | Conformance tests | CRUD | Transactions | Indexes | Schema persistence | Namespace catalog | Audit co-location |
|---------|:----------:|:-----------------:|:----:|:------------:|:-------:|:-----------------:|:-----------------:|:-----------------:|
| Memory | Done | Via macro | Done | Done | Done | Done | Done | N/A (in-memory audit) |
| SQLite | Done | Via macro | Done | Done | Done | Done | Done | Done |
| PostgreSQL | Done | Via macro | Done | Done | Done | Done | Done | Done |
| FoundationDB | Not started | -- | -- | -- | -- | -- | -- | -- |

The `storage_conformance_tests!` macro generates the identical test suite for each backend. Memory, SQLite, and PostgreSQL all pass. An open bead (`axon-6d816f52`) tracks migration to sqlx with async `Pool<DB>` to eliminate the `block_in_place` hack in the PostgreSQL adapter.

---

## 5. Test Coverage Status

### Test Counts

**Total: 1,461 tests passing** (0 failures) across all crates and doc-tests.

### Test Plan Layer Coverage

| Layer | Description | Status | Notes |
|-------|------------|:------:|-------|
| L1: Correctness invariants | DST workloads via axon-sim | Implemented | INV-001 through INV-008 all have corresponding workloads |
| L2: Business scenarios | End-to-end workflow tests | Not started | SCN-001 through SCN-010 defined in test plan but not coded |
| L3: Property-based tests | Proptest for schemas, storage, API | Partial | PROP-001 (schema round-trip) and storage proptests exist. PROP-002 through PROP-005 not yet coded |
| L4: Backend conformance | Parameterized tests via macro | Done | Memory, SQLite, and PostgreSQL all pass |
| L5: Performance benchmarks | criterion benchmarks | Not started | BM-001 through BM-010 defined but no benchmark code exists |
| L6: API contract tests | HTTP conformance | Partial | Direction=reverse contract test exists. Full suite not yet coded |

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

## 6. Architecture Decisions Enacted

| ADR | Decision | Implemented in |
|-----|----------|----------------|
| ADR-001 | Implementation language: Rust | All crates |
| ADR-002 | Schema format: ESF (JSON Schema-based) | axon-schema |
| ADR-003 | Backing store: SQLite default, PG opt-in | axon-storage |
| ADR-004 | Transaction model: OCC with version-based CAS | axon-api, axon-storage |
| ADR-005 | Auth: Tailscale whois via Unix socket | axon-server (auth.rs) |
| ADR-006 | Admin UI: SvelteKit + Bun | ui/ (12 pages) |
| ADR-007 | Schema versioning | axon-schema (evolution module) |
| ADR-008 | Schema lifecycles | axon-schema (gates, rules) |
| ADR-009 | Patch semantics + ID generation | axon-api (RFC 7396 merge-patch, UUIDv7) |
| ADR-010 | Physical storage: numeric collection IDs, EAV indexes | axon-storage |
| ADR-011 | Multi-tenancy namespace hierarchy | axon-core, axon-api, axon-storage |
| ADR-012 | GraphQL query layer | axon-graphql |
| ADR-013 | MCP server | axon-mcp |
| ADR-014 | Change feeds: Debezium CDC | axon-audit (CDC module) |
| ADR-015 | Rollback and recovery | axon-api (rollback module) |
| ADR-016 | Agent guardrails | axon-server (rate_limit, actor_scope) |
| ADR-017 | BYOC control plane | axon-server, axon-control-plane |
| ADR-018 | Tenant as global account boundary, M:N users, JWT credentials, path-based wire protocol | axon-core, axon-server (auth_pipeline, path_router, control_plane_routes, gateway) |

---

## 7. What Is Complete vs. What Remains

### Complete (functional and tested)

- Entity-graph-relational data model with typed links
- Schema engine with JSON Schema validation, ESF Layer 5 rules, gates
- Immutable audit log with full provenance chain and audit attribution
- ACID transactions with OCC and serializable isolation
- Entity CRUD (create, read, update, patch, delete) with query/filter/sort/pagination
- Link CRUD, graph traversal, reachability, neighbor listing, link candidate discovery
- Aggregation queries (COUNT, SUM, AVG, MIN, MAX, GROUP BY)
- Multi-tenancy with four-level hierarchy (tenant → database → schema → collection)
- Pure path-based wire protocol (ADR-018) — no legacy routes
- Secondary indexes (EAV pattern, single/compound/unique)
- GraphQL API (dynamic schema, queries, mutations, subscriptions)
- MCP server (CRUD tools, resources, prompts, query/aggregate/neighbor tools)
- Markdown template rendering with LRU cache
- Rollback and recovery (entity, collection, transaction level) with dry-run
- Schema evolution detection, diffing, schema_version stamping, revalidate CLI
- CDC envelope format (Debezium-compatible)
- Bead storage adapter (lifecycle states, dependency DAG, ready queue)
- Unified CLI with serve/mcp/doctor/install subcommands and client mode
- XDG-compliant configuration (axon-config)
- HTTP gateway with path-based routing and self-signed TLS bootstrap
- Authentication: Tailscale whois + JWT auth pipeline (9-step) + RBAC enforcement
- Control plane: tenant/user/credential/database/member management (20+ routes)
- Admin UI: 12 SvelteKit pages with tenant-scoped routing
- TypeScript SDK: GraphQL-first client target with compatibility HTTP/gRPC
  clients still present during transition
- Rate limiting (per-actor sliding window) and actor scope constraints
- DST framework with all 8 correctness invariant workloads
- Storage conformance macro (memory, SQLite, PostgreSQL all passing)

### Remaining Work

| Item | Priority | Blocking? | Est. Effort |
|------|:--------:|:---------:|:-----------:|
| FEAT-029 GraphQL/MCP data-layer policy proof | P0 | Yes | Large |
| FEAT-030 mutation intents and approval | P0 | Yes | Large |
| Policy authoring compiler, fixture tests, and dry-run reports | P0 | Yes | Large |
| Agent identity/delegation audit fields | P0 | Yes | Medium |
| Minimal compliance/erasure story for immutable audit | P1 | No | Medium |
| sqlx migration for storage adapters (`axon-6d816f52`) | P1 | No | Medium |
| L2 business scenario tests (SCN-001 through SCN-010) | P1 | No | Large |
| L5 performance benchmarks (criterion) | P1 | No | Medium |
| L6 API contract test suite | P1 | No | Medium |
| L3 property tests PROP-002 through PROP-005 | P1 | No | Medium |
| Kafka CDC transport (FEAT-021) | P1 | No | Large |
| Agent guardrails: semantic validation hooks (FEAT-022) | P1 | No | Medium |
| Control plane monitoring (FEAT-025) | P1 | No | Medium |
| Schema migration declarations (FEAT-017) | P1 | No | Medium |
| Entity state machines and transition guards (FEAT-010) | P1 | No | Large |
| FoundationDB backend | P2 | No | Large |
| Local-first sync | P2 | No | Large |
| Git mirror (FEAT-027) | P2 | No | Medium |

---

## 8. Phase Alignment (PRD Timeline)

| Phase | PRD Scope | Status |
|-------|-----------|:------:|
| Phase 1: Foundation (8 weeks) | Entity-graph model, CRUD, schema engine, audit log, ACID/OCC, embedded mode, CLI | Done |
| Phase 2: API and Integration (6 weeks) | Server mode, query/filter/sort, graph traversal, bead adapter | Done |
| Phase 3: Production Readiness (4 weeks) | Auth, change feeds, batch ops, schema evolution, benchmarks, docs | Substantially complete |

The project has completed Phases 1–2 and is substantially through Phase 3. Auth (FEAT-012 V1+V5) is live with JWT credentials and RBAC enforcement. Schema evolution public surfaces are complete. Admin UI and TypeScript SDK are shipped. The ADR-018 migration (the largest architectural change since project inception) is fully enacted. Key Phase 3 gaps remaining: Kafka CDC, performance benchmarks, and L2 business scenario tests.

---

*This document tracks implementation against the planning stack. Updated as features are completed or priorities change.*
