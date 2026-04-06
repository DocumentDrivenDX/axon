# Alignment Review: AR-2026-04-06-post-implementation

**Date**: 2026-04-06
**Scope**: Full repository — all 21 FEATs against implementation
**Build State**: 672 tests passing, clippy clean, 0 open tracker issues

---

## Executive Summary

Post-implementation alignment review across 10 crates (~30K LOC, 672 tests) against 21 feature specifications (78 user stories). The core data engine (FEAT-001 through FEAT-004, FEAT-007, FEAT-008, FEAT-017) is **strong** — schema validation, entity CRUD, OCC transactions, audit log, schema evolution, and graph traversal all work with solid test coverage. Service layers (GraphQL, MCP, CDC) and cross-cutting features (auth, indexes, state machines, UI) have **scaffolding but incomplete wiring**.

| Rating | Count | FEATs |
|--------|-------|-------|
| **Strong** (>80%) | 6 | FEAT-001, 002, 003, 006, 008, 017 |
| **Partial** (40-80%) | 9 | FEAT-004, 005, 007, 009, 014, 018, 019, 020, 021 |
| **Scaffold** (10-40%) | 4 | FEAT-012, 013, 015, 016 |
| **Missing** (<10%) | 2 | FEAT-010, 011 |

---

## Feature-by-Feature Assessment

### Tier 1: Strong Implementation (>80% criteria pass)

#### FEAT-001: Collections — 95%
- **PASS**: create/list/describe/drop lifecycle, name validation, schema-at-create, audit entries
- **GAP**: `DropCollectionRequest` lacks `confirm` field; CLI lacks `--confirm` flag
- **GAP**: Drop audit entry doesn't record entity count in metadata

#### FEAT-002: Schema Engine — 100%
- **PASS**: YAML/JSON schema, all JSON Schema types, required/optional/defaults, nested objects, validation with actionable errors, "did you mean?" via Levenshtein, multiple violations reported

#### FEAT-003: Audit Log — 90%
- **PASS**: Query with filters (collection, entity, actor, time range), cursor pagination, revert with schema validation, force-revert
- **GAP**: Entity write requests (`CreateEntityRequest`, `UpdateEntityRequest`, `DeleteEntityRequest`) lack `audit_metadata` field — callers can't attach context to mutations

#### FEAT-006: Bead Storage Adapter — 90%
- **PASS**: Schema init, create/list/transition, ready-queue, dependency tracking, cycle detection
- **GAP**: No CLI command for dependency removal; transition error format is debug-level not user-friendly

#### FEAT-008: ACID Transactions — 85%
- **PASS**: Atomic multi-entity commit, version conflict with current entity state, transaction ID in audit, 30s timeout, 100-op limit
- **GAP**: OCC provides snapshot isolation, not full serializability — write skew not prevented (US-022)
- **GAP**: No configurable isolation level per transaction

#### FEAT-017: Schema Evolution — 95%
- **PASS**: Compatible/breaking classification, field-level diff, force-apply, revalidation, dry-run, version history, cross-version diff
- **GAP**: Revalidation is synchronous only (no background indicator for large collections)

### Tier 2: Partial Implementation (40-80% criteria pass)

#### FEAT-004: Entity Operations — 70%
- **PASS**: Full CRUD with OCC, filtered queries (eq/ne/gt/gte/lt/lte/in/contains), AND/OR combinators, sort, cursor pagination, count_only
- **MISSING**: Partial update / PATCH (US-012) — `UpdateEntityRequest` requires full replacement, no merge-patch semantics

#### FEAT-005: API Surface — 75%
- **PASS**: gRPC service, HTTP gateway, CLI with all subcommands, structured error codes, JSON/table output
- **GAP**: No Go SDK; TypeScript SDK directory exists but not compiled/tested
- **GAP**: No documented parity testing between embedded and server modes

#### FEAT-007: Entity-Graph-Relational — 75%
- **PASS**: Nested JSON with dot-path queries, typed links with metadata, BFS traversal with depth/direction/type filter, referential integrity on delete
- **GAP**: Combined entity+link filter queries require two operations (no single "entities linked to X where Y" query)
- **GAP**: Traversal loads all links into memory — O(total_links) per hop

#### FEAT-009: Graph Traversal — 75%
- **PASS**: BFS traversal, path recording, hop filtering, reachability with short-circuit, cycle-safe via visited set
- **MISSING**: Cycle detection with structured response (cycles silently skipped, not reported)
- **MISSING**: Multi-path collection for shared components (only one path per entity returned)

#### FEAT-014: Multi-Tenancy — 60%
- **PASS**: Database create/drop, namespace create/list/drop, `default.default` zero-config, force-drop non-empty
- **MISSING**: Audit entries don't capture database/schema context
- **MISSING**: Access control scoped to databases (US-038)
- **MISSING**: Node registry and placement (US-039, P2)

#### FEAT-018: Aggregation — 57%
- **PASS**: COUNT, SUM, AVG, MIN, MAX with GROUP BY, filter+aggregate, null handling
- **MISSING**: GraphQL aggregate query types (US-064)
- **MISSING**: MCP aggregate tools (US-065)
- **MISSING**: Index acceleration for aggregation

#### FEAT-019: Validation Rules & Gates — 60%
- **PASS**: Cross-field rules (when/require pattern), 12+ operators, save gate blocks persistence, custom gates reported, advisory rules, gate inclusion, rule definition validation
- **MISSING**: Gate status not materialized/persisted — not queryable via filters
- **MISSING**: Lifecycle `requires_gate` enforcement
- **MISSING**: Gate-based query filter (`_gate.complete: true`)

#### FEAT-020: Link Discovery — 70%
- **PASS**: find_link_candidates with already-linked indicator, cardinality info, filter support; list_neighbors grouped by type/direction with entity data
- **MISSING**: GraphQL relationship fields with DataLoader (US-072)
- **MISSING**: MCP link discovery tools (US-073)

#### FEAT-021: Change Feeds (CDC) — 30%
- **PASS**: Debezium envelope format, CdcOp types, JSONL file sink, Kafka config structure, Schema Registry endpoint stubs
- **MISSING**: `KafkaCdcSink` is a stub (buffers in memory, no rdkafka)
- **MISSING**: Initial snapshot emission (`op: "r"`)
- **MISSING**: Cursor persistence, replay from audit_id
- **MISSING**: Link events on `__links__` topics
- **MISSING**: SSE sink

### Tier 3: Scaffold Only (10-40% criteria pass)

#### FEAT-012: Authorization — 30%
- **EXISTS**: Role enum, Operation enum, CallerIdentity with check(), MaskPolicy, WritePolicy, tag-to-role mapping — well-designed and unit-tested
- **MISSING**: Zero middleware integration — no handler actually checks permissions. Auth code is dormant.
- **MISSING**: No Tailscale whois API calls
- **MISSING**: No 401/403 response generation
- **NOTE**: `--no-auth=true` is the default

#### FEAT-013: Secondary Indexes — 33%
- **EXISTS**: IndexDef/CompoundIndexDef in schema, EAV index tables maintained on write, index_lookup/index_range/unique_check methods
- **MISSING**: Query planner does NOT consult indexes — all queries do full collection scan
- **MISSING**: Unique constraint not enforced from handler (only at storage level, never called)
- **MISSING**: Background index build (US-034)

#### FEAT-015: GraphQL — 35%
- **EXISTS**: Dynamic schema generation from collections, query field stubs, ChangeFeedBroker for subscriptions
- **MISSING**: All mutation resolvers are stubs (return NULL)
- **MISSING**: No relationship fields from link_types
- **MISSING**: No Relay pagination (Connection/Edge/PageInfo)
- **MISSING**: No DataLoader for N+1 prevention
- **MISSING**: No WebSocket transport integration

#### FEAT-016: MCP Server — 40%
- **EXISTS**: JSON-RPC 2.0 protocol, tool registry, CRUD tools, `axon.query` GraphQL bridge, stdio transport
- **MISSING**: Resource subscriptions (`axon://collection/id` pattern)
- **MISSING**: Dynamic tool generation from live schema
- **MISSING**: Lifecycle transition tools
- **MISSING**: HTTP+SSE transport
- **MISSING**: Link discovery / neighbor tools

### Tier 4: Not Implemented

#### FEAT-010: Workflow State Machines — 0%
- **EXISTS**: Bead-specific hardcoded state machine in `BeadStatus`
- **MISSING**: No general-purpose configurable state machine engine
- **MISSING**: No guard conditions, no transition metadata, no introspection API

#### FEAT-011: Admin Web UI — 10%
- **EXISTS**: SvelteKit project structure with placeholder route pages
- **MISSING**: Zero API integration, no data fetching, no components, no server-side serving

---

## Cross-Cutting Gaps

### 1. Index-Query Disconnect (Critical)
Secondary indexes are maintained on every write but **never consulted during reads**. This affects FEAT-013, 018, 019 (gate queries), 020 (candidate search). Every query is a full collection scan regardless of index declarations.

### 2. GraphQL/MCP Mutation Wiring (Critical for P1)
Both FEAT-015 and FEAT-016 are P1 features with scaffolding but non-functional mutations. GraphQL resolvers are stubs; MCP tools only cover basic CRUD.

### 3. Auth Middleware Gap
FEAT-012 has well-designed auth types but zero enforcement. No request touches the permission system.

### 4. Gate Materialization
Validation gates evaluate correctly at write time but results are not persisted, preventing gate-based queries and lifecycle enforcement.

---

## Compliance Matrix

| FEAT | US Stories | Pass | Partial | Missing | % |
|------|-----------|------|---------|---------|---|
| 001 Collections | 3 | 12 | 1 | 2 | 80% |
| 002 Schema Engine | 3 | 12 | 0 | 0 | 100% |
| 003 Audit Log | 3 | 12 | 1 | 1 | 86% |
| 004 Entity Ops | 3 | 10 | 1 | 4 | 67% |
| 005 API Surface | 2 | 7 | 2 | 0 | 78% |
| 006 Bead Adapter | 2 | 11 | 2 | 0 | 85% |
| 007 EGR Model | 3 | 14 | 3 | 1 | 78% |
| 008 ACID Txns | 3 | 10 | 2 | 2 | 71% |
| 009 Graph Traversal | 3 | 8 | 1 | 2 | 73% |
| 010 State Machines | 3 | 0 | 0 | 14 | 0% |
| 011 Admin UI | 3 | 0 | 3 | 14 | 0% |
| 012 Authorization | 3 | 3 | 5 | 6 | 21% |
| 013 Sec. Indexes | 4 | 2 | 0 | 4 | 33% |
| 014 Multi-Tenancy | 5 | 8 | 3 | 6 | 47% |
| 015 GraphQL | 4 | 3 | 6 | 10 | 16% |
| 016 MCP Server | 5 | 5 | 5 | 10 | 25% |
| 017 Schema Evo | 4 | 12 | 0 | 0 | 100% |
| 018 Aggregation | 4 | 8 | 0 | 3 | 73% |
| 019 Validation | 4 | 10 | 1 | 5 | 63% |
| 020 Link Discovery | 4 | 7 | 1 | 3 | 64% |
| 021 CDC | 5 | 4 | 2 | 10 | 25% |

**Overall**: 157 pass / 39 partial / 97 missing out of ~293 criteria = **54% full pass, 67% partial-or-better**

---

## Recommended Priority Order

### P0 — Core engine completeness
1. **PATCH operation** (FEAT-004 US-012) — merge-patch semantics for entity updates
2. **Index query integration** (FEAT-013) — wire index_lookup into query_entities, aggregate, find_link_candidates
3. **Unique constraint enforcement** (FEAT-013 US-032) — call unique_check from handler on create/update
4. **Gate materialization** (FEAT-019) — persist gate results, enable gate-based queries
5. **Audit metadata on writes** (FEAT-003 US-009) — add `audit_metadata` to entity request types
6. **Drop confirmation** (FEAT-001 US-003) — add `confirm` to DropCollectionRequest and CLI

### P1 — Service layer wiring
7. **GraphQL mutation resolvers** (FEAT-015) — wire stubs to handler methods
8. **GraphQL relationship fields** (FEAT-015/020) — generate from link_types, add DataLoader
9. **MCP resource subscriptions** (FEAT-016) — implement `axon://` resource pattern
10. **MCP dynamic tool registration** (FEAT-016) — generate tools from live schema
11. **Auth middleware** (FEAT-012) — integrate CallerIdentity checks into HTTP/gRPC handlers
12. **CDC Kafka producer** (FEAT-021) — replace stub with rdkafka integration

### P2 — Advanced features
13. **State machine engine** (FEAT-010) — configurable workflows, guards, transition API
14. **Admin UI** (FEAT-011) — API integration, components, server-side serving
15. **Serializable isolation** (FEAT-008 US-022) — read-set tracking for write skew prevention
16. **Background index build** (FEAT-013 US-034)
17. **CDC snapshot/replay** (FEAT-021 US-075)

---

## Test Health

| Crate | Tests | Status |
|-------|-------|--------|
| axon-api | 175 | All pass |
| axon-storage | 128+60 | All pass |
| axon-schema | 73 | All pass |
| axon-core | 52 | All pass |
| axon-audit | 48 | All pass |
| axon-mcp | 36 | All pass |
| axon-server | 29+12 | All pass |
| axon-graphql | 26+12 | All pass |
| axon-sim | 29 | All pass |
| axon-cli | 8 | All pass |
| **Total** | **672** | **All pass, clippy clean** |

2 warnings in test builds: unused helper functions `task_with_status` and `task_with_priority` in axon-storage.
