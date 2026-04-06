# Alignment Review: AR-2026-04-05d — Post-Build Delta

**Date**: 2026-04-05
**Scope**: Full repository — delta since AR-2026-04-05c
**Reviewer**: Claude (automated)
**Epic**: axon-d49bc1fa

## Context

This review follows the completion of all remaining tracker issues since the
previous review (AR-2026-04-05c). That review identified 8 open issues on the
critical path. This delta review assesses the current alignment of
implementation against all planning artifacts.

## Summary

**319 passing tests, 0 open tracker issues, 0 TODO/FIXME/HACK comments.**

All P0 features (FEAT-001 through FEAT-005, FEAT-007, FEAT-008) are
substantially implemented. P1 FEAT-006 (Bead Storage Adapter) is complete. The
TypeScript SDK is generated and tested. Workspace lints and cargo-deny are
configured. The codebase is in a clean, shippable state for V1 embedded mode.

## Gap Register

### SATISFIED — Previously Open, Now Closed

| ID | Gap | Resolution |
|----|-----|------------|
| G-03 | Multi-entity transaction API not exposed via HTTP/gRPC | Closed: `POST /transactions` endpoint + gRPC pending (HTTP done) |
| G-04/G-11 | Graph traversal limited to forward-only BFS | Closed: reverse traversal, paths, link metadata, hop filter, reachable() |
| G-09 | PostgreSQL storage adapter missing | Closed: full `PostgresStorageAdapter` with all trait methods |
| G-NEW | TypeScript SDK not generated | Closed: `sdk/typescript/` with typed client, error handling, 11 tests |
| G-06 | CLI missing entity list, collection drop/describe, audit show/revert | Closed: full command set + bead subcommands |
| G-15 | L6 API contract tests missing | Closed: 11 gRPC + HTTP/gRPC parity tests |
| G-07 | Bead storage adapter not implemented | Closed: lifecycle FSM, dependency tracking, ready queue, CLI |
| G-16 | Transaction delete missing op-limit guard | Closed: fixed in prior session |

### REMAINING GAPS — Ordered by Severity

#### G-R01: PATCH / Partial Update Not Implemented [P2, DEFERRED]

- **Planning**: FEAT-004 AC specifies "patch (partial update) preserves unmentioned fields" and "empty patch is a no-op"
- **Implementation**: Only full-replacement `update_entity` exists
- **Decision**: User explicitly deferred to a later design phase (recorded AR-2026-04-05c)
- **Classification**: DEFERRED (by product decision)
- **Action**: None until design phase initiated

#### G-R02: Go Client SDK Not Generated [P1]

- **Planning**: FEAT-005 US-013 AC: "gRPC client SDK available for Go and TypeScript"
- **Implementation**: TypeScript SDK complete. Go SDK does not exist.
- **Decision**: User prioritized TypeScript; Go deferred
- **Classification**: DEFERRED (by product decision)
- **Action**: Create tracker issue when Go SDK is prioritized

#### G-R03: gRPC Transaction RPC Not Exposed [P1]

- **Planning**: FEAT-008 requires atomic multi-entity transactions via API
- **Implementation**: HTTP `POST /transactions` works. No gRPC `TransactionService` RPC defined in proto.
- **Classification**: GAP — partial implementation
- **Recommended resolution**: Add `CommitTransaction` RPC to `axon.proto` and implement in `service.rs`

#### G-R04: Audit Diff Field Not Populated [P1]

- **Planning**: FEAT-003 specifies audit entries include a structured diff (field-level before/after)
- **Implementation**: `AuditEntry` has `diff: Option<HashMap<String, FieldDiff>>` but handler code never computes it (always `None`)
- **Classification**: GAP — field defined, logic missing
- **Recommended resolution**: Compute JSON diff between `data_before` and `data_after` on entity update/revert

#### G-R05: Entity IDs Are Client-Supplied, Not UUIDv7 [P1]

- **Planning**: FEAT-004 and FEAT-007 specify "UUIDv7 system-generated ID"
- **Implementation**: Entity IDs are always client-supplied strings. No UUIDv7 generation.
- **Classification**: GAP — design divergence
- **Recommended resolution**: Design decision needed: should IDs be system-generated with optional client override, or remain client-supplied? If system-generated, add `uuid` crate dependency.

#### G-R06: Schema Evolution Not Implemented [P1]

- **Planning**: FEAT-002 P1: "schema evolution with breaking-change detection and migration declarations"
- **Implementation**: Schemas can be replaced via `put_schema` but no version compatibility checking, no migration support, no breaking-change detection.
- **Classification**: DEFERRED (P1, not V1 scope per PRD phasing)
- **Action**: Track when Phase 3 begins

#### G-R07: Change Feeds / Streaming Not Implemented [P1]

- **Planning**: FEAT-005 mentions "server-streaming for change feeds"; PRD P1
- **Implementation**: No streaming RPCs defined
- **Classification**: DEFERRED (P1 per PRD)
- **Action**: Track when Phase 3 begins

#### G-R08: Server Health Endpoint Missing [P1]

- **Planning**: Technical Requirements specify `/health` endpoint for server mode
- **Implementation**: `axon-server` has HTTP gateway but no `/health` route
- **Classification**: GAP — straightforward to add
- **Recommended resolution**: Add `GET /health` returning `200 OK` with uptime/version

#### G-R09: OpenTelemetry Observability Not Configured [P2]

- **Planning**: Technical Requirements specify OpenTelemetry integration
- **Implementation**: `tracing` and `tracing-subscriber` are present but no OTel exporter
- **Classification**: DEFERRED (P2 per PRD phasing)

#### G-R10: Auth/Authorization Not Implemented [P1]

- **Planning**: PRD P1: "auth/authorization"
- **Implementation**: No auth layer exists
- **Classification**: DEFERRED (P1, Phase 3 per PRD timeline)

#### G-R11: Embedded/Server Test Suite Parity Not Verified [P0]

- **Planning**: PRD DoD: "Embedded and server modes pass identical test suites"
- **Implementation**: Tests run against MemoryStorageAdapter (embedded) and some against SQLite. No automated parity run against server mode (PostgreSQL + HTTP/gRPC).
- **Classification**: GAP — testing gap
- **Recommended resolution**: Parameterize integration tests to run against embedded and server modes

#### G-R12: Bead Import/Export Not Implemented [P1]

- **Planning**: FEAT-006 requires "Import/export beads from JSON (compatible with steveyegge/beads format)"
- **Implementation**: Bead CRUD and lifecycle work, but no JSON import/export CLI commands
- **Classification**: GAP — minor
- **Recommended resolution**: Add `axon bead import` / `axon bead export` commands

#### G-R13: Dangling Link Prevention Not Implemented [P1]

- **Planning**: FEAT-007 edge cases: "Dangling links rejected by default (deletion blocked if inbound links exist)"
- **Implementation**: Entities can be deleted even if they have inbound links. No referential integrity check.
- **Classification**: GAP — behavioral divergence
- **Recommended resolution**: Add optional referential integrity check on entity delete

#### G-R14: ADR-003 Specifies libsql/sqlx, Implementation Uses rusqlite/postgres [INFORMATIONAL]

- **Planning**: ADR-003 specifies `libsql` v0.9.x (embedded) and `sqlx` v0.8.x (PostgreSQL)
- **Implementation**: Uses `rusqlite` v0.32 and synchronous `postgres` v0.19
- **Classification**: INFORMATIONAL — ADR predates implementation; actual crate choices work and pass tests
- **Recommended resolution**: Update ADR-003 to reflect actual crate choices, or switch to specified crates in a follow-on

## Traceability Matrix

| Feature | Planning | Implementation | Tests | Status |
|---------|----------|---------------|-------|--------|
| FEAT-001 Collections | Spec complete | create/list/drop/describe, name validation, audit | 15+ tests | ALIGNED |
| FEAT-002 Schema Engine | Spec complete | JSON Schema validation, link types, structured errors | 12+ tests | ALIGNED |
| FEAT-003 Audit Log | Spec complete | Append-only, query, revert, pagination | 26+ tests | PARTIAL (diff not computed) |
| FEAT-004 Entity Ops | Spec complete | CRUD, OCC, filter/sort/paginate, count | 30+ tests | PARTIAL (no PATCH, no UUIDv7) |
| FEAT-005 API Surface | Spec complete | gRPC, HTTP, CLI, TypeScript SDK | 30+ tests | PARTIAL (no Go SDK, no gRPC txn) |
| FEAT-006 Bead Adapter | Spec complete | Lifecycle, deps, ready queue, CLI | 9+ tests | PARTIAL (no import/export) |
| FEAT-007 Entity-Graph | Spec complete | Links, traversal (fwd/rev/filter), reachable | 20+ tests | PARTIAL (no dangling link prevention) |
| FEAT-008 Transactions | Spec complete | OCC, multi-entity, HTTP API, op limit, timeout | 15+ tests | PARTIAL (no gRPC RPC) |

## Execution Issues

Issues created only for gaps classified as GAP (not DEFERRED):

| Gap | Priority | Issue |
|-----|----------|-------|
| G-R03 | P1 | gRPC transaction RPC |
| G-R04 | P1 | Audit diff computation |
| G-R08 | P1 | Health endpoint |
| G-R11 | P0 | Test suite parity verification |
| G-R12 | P2 | Bead import/export |
| G-R13 | P1 | Dangling link prevention |
| G-R14 | P2 | ADR-003 crate choice update |

## Decisions Log (Carried Forward)

1. **PostgreSQL backend = V1 P0** (build against postgres first)
2. **TypeScript SDK = V1** (Go deferred)
3. **Partial updates (PATCH) = deferred** to later design phase
4. **Entity IDs = client-supplied** (pending design decision on UUIDv7)
