# Alignment Review: Full-Scope

**Date**: 2026-04-05
**Reviewer**: Claude (HELIX automated review)
**Scope**: All HELIX artifacts and implementation
**Review Epic**: axon-bfdb0301

---

## 1. Review Metadata

| Field | Value |
|-------|-------|
| Review ID | AR-2026-04-05 |
| Scope | Full project |
| Tracker Epic | axon-bfdb0301 |
| Commits reviewed through | c4cf932 |
| Artifacts cataloged | 18 planning docs, 8 crates |

---

## 2. Scope and Governing Artifacts

| Layer | Artifact | Path | Status |
|-------|----------|------|--------|
| Vision | Product Vision | `docs/helix/00-discover/product-vision.md` | Draft |
| Requirements | PRD | `docs/helix/01-frame/prd.md` | Draft v0.1.0 |
| Requirements | Technical Requirements | `docs/helix/01-frame/technical-requirements.md` | Draft |
| Feature | FEAT-001 Collections | `docs/helix/01-frame/features/FEAT-001-collections.md` | Draft P0 |
| Feature | FEAT-002 Schema Engine | `docs/helix/01-frame/features/FEAT-002-schema-engine.md` | Draft P0 |
| Feature | FEAT-003 Audit Log | `docs/helix/01-frame/features/FEAT-003-audit-log.md` | Draft P0 |
| Feature | FEAT-004 Entity Operations | `docs/helix/01-frame/features/FEAT-004-entity-operations.md` | Draft P0 |
| Feature | FEAT-005 API Surface | `docs/helix/01-frame/features/FEAT-005-api-surface.md` | Draft P0 |
| Feature | FEAT-006 Bead Storage Adapter | `docs/helix/01-frame/features/FEAT-006-bead-storage-adapter.md` | Draft P1 |
| Feature | FEAT-007 Entity-Graph Model | `docs/helix/01-frame/features/FEAT-007-entity-graph-model.md` | Draft P0 |
| Feature | FEAT-008 ACID Transactions | `docs/helix/01-frame/features/FEAT-008-acid-transactions.md` | Draft P0 |
| Feature | FEAT-009 Graph Traversal Queries | `docs/helix/01-frame/features/FEAT-009-graph-traversal-queries.md` | Draft P1 |
| Feature | FEAT-010 Workflow State Machines | `docs/helix/01-frame/features/FEAT-010-workflow-state-machines.md` | Draft P2 (deferred) |
| Architecture | ADR-001 Implementation Language | `docs/helix/02-design/adr/ADR-001-implementation-language.md` | Accepted |
| Architecture | ADR-002 Schema Format | `docs/helix/02-design/adr/ADR-002-schema-format.md` | Accepted |
| Architecture | ADR-003 Backing Store Architecture | `docs/helix/02-design/adr/ADR-003-backing-store-architecture.md` | Accepted |
| Test | Test Plan (L1-L6) | `docs/helix/03-test/test-plan.md` | Draft v0.1.0 |

---

## 3. Intent Summary

**Vision**: Axon is a cloud-native, auditable, schema-first transactional data store for agentic applications. It combines entity storage, typed links, schema validation, immutable audit, and OCC transactions into one system.

**V1 scope**: P0 features (FEAT-001 through FEAT-005, FEAT-007, FEAT-008) plus P1 features (FEAT-006, FEAT-009). FEAT-010 (workflow state machines) is deferred to post-V1.

**Architecture**: Rust workspace with trait-based storage abstraction. SQLite + PostgreSQL backends. gRPC primary API with HTTP/JSON gateway. FoundationDB-inspired deterministic simulation testing.

---

## 4. Planning Stack Findings

### Vision -> Requirements: ALIGNED
The PRD faithfully translates the vision's "central nervous system for agentic apps" into concrete capabilities. All six vision pillars (entities, links, schema, audit, transactions, API) appear as PRD requirements.

### Requirements -> Feature Specs: ALIGNED
Every PRD requirement maps to at least one FEAT spec. Feature specs include user stories with acceptance criteria. Priority ordering (P0-P2) is consistent across documents.

### Feature Specs -> Architecture: ALIGNED with minor gaps
ADR-001 (Rust), ADR-002 (JSON Schema + link types), and ADR-003 (storage architecture) support the feature specs. However:
- **No ADR for transaction model**: FEAT-008 specifies OCC transactions but no ADR documents the design tradeoffs vs pessimistic locking or MVCC.
- **No ADR for audit architecture**: FEAT-003 specifies audit requirements but the application-layer-vs-storage-layer decision is documented only in ADR-003, not as a standalone decision.

### Architecture -> Tests: ALIGNED
Test plan L1-L6 covers all V1 features. Test plan references specific features and ADRs. L1 invariants map to correctness properties from the PRD.

### Planning Stack Conflicts: NONE DETECTED
No same-layer conflicts or contradictions found.

---

## 5. Implementation Map

| Crate | Purpose | Maps to |
|-------|---------|---------|
| `axon-core` | Types, traits, error hierarchy | Foundation for all features |
| `axon-schema` | ESF parsing, JSON Schema validation, link type defs | FEAT-002, ADR-002 |
| `axon-audit` | Immutable audit log, entry types, query API | FEAT-003 |
| `axon-storage` | StorageAdapter trait, Memory + SQLite backends | FEAT-005 (storage), ADR-003 |
| `axon-api` | Handler, request/response types, transactions | FEAT-001, FEAT-004, FEAT-007, FEAT-008, FEAT-009 |
| `axon-server` | gRPC service, HTTP gateway | FEAT-005 (API) |
| `axon-sim` | Deterministic simulation testing | Test Plan L1 |
| `axon-cli` | CLI entry point | FEAT-005 (CLI) |

### Unplanned code paths
- **Field-level diff computation** in audit entries: goes beyond spec (which mentions only snapshots). This is a quality improvement, not a divergence.
- **Reverse-index collections** (`__axon_links_rev__`): implementation detail not in spec but architecturally sound for inbound-link queries.

---

## 6. Acceptance Criteria Status

### FEAT-001: Collections

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| US-001 | Create collection with unique name and schema | `handler::tests::create_collection_*` | SATISFIED | 8+ tests |
| US-001 | Reject duplicate collection names | `create_collection_duplicate_name_fails` | SATISFIED | Test exists |
| US-002 | List all collections | `handler::tests::list_collections_*` | SATISFIED | Tests exist |
| US-002 | Describe collection with schema and entity count | `handler::tests::describe_collection_*` | SATISFIED | Tests exist |
| US-003 | Drop collection | `handler::tests::drop_collection_*` | SATISFIED | Tests exist |
| NF | Collection metadata includes creation time | none | UNIMPLEMENTED | CollectionMetadata has no timestamp fields |
| NF | Collection metadata survives restart | `memory::tests::*`, `sqlite::tests::*` | SATISFIED | Persisted via StorageAdapter |

### FEAT-002: Schema Engine

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| - | JSON Schema 2020-12 validation on create | `schema_validation_rejects_invalid_write` | SATISFIED | Enforced in handler |
| - | JSON Schema 2020-12 validation on update | `schema_validation_rejects_invalid_write` | SATISFIED | Enforced in handler |
| - | Structured validation errors with field paths | `invalid_entity_returns_structured_errors_with_field_path` | SATISFIED | SchemaValidationError has field_path |
| - | ESF document parsing (YAML/JSON) | `parse_esf_from_adr_002` | SATISFIED | Tests exist |
| - | Link-type definitions parsed from ESF | `esf_into_collection_schema` | SATISFIED | LinkTypeDef parsed |

### FEAT-003: Audit Log

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| - | Every mutation recorded | `create_update_delete_produce_audit_entries` | SATISFIED | All MutationType variants covered |
| - | Before/after snapshots captured | `audit_reconstruction_with_deletes` | SATISFIED | data_before/data_after stored |
| - | Query by entity | `query_audit` handler tests | SATISFIED | query_by_entity implemented |
| - | Query by actor | audit log query API | SATISFIED | query_by_actor implemented |
| - | Query by time range | audit log query API | SATISFIED | query_by_time_range implemented |
| - | Transaction correlation | `audit_entries_share_transaction_id` | SATISFIED | transaction_id in AuditEntry |

### FEAT-004: Entity Operations

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| - | Create entity with ID | `entity_creation_and_retrieval` | SATISFIED | handler.rs:117 |
| - | Get entity by ID | same | SATISFIED | handler.rs:143 |
| - | Update with OCC (expected_version) | `update_increments_version` | SATISFIED | compare_and_swap |
| - | Delete with referential integrity | `delete_link_produces_audit_entry` area | SATISFIED | Rejects if inbound links exist |
| - | ConflictingVersion includes current_entity | `conflict_response_includes_current_entity` | SATISFIED | handler + gRPC + HTTP tests |

### FEAT-005: API Surface

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| US-013 | gRPC client SDK (Go, TypeScript) | none | UNIMPLEMENTED | Only Rust gRPC service exists |
| US-013 | Structured error types | `gateway::tests::http_errors_are_structured*` | SATISFIED | ApiError with code+detail |
| US-014 | CLI commands for all operations | none | UNTESTED | axon-cli crate exists but minimal |
| - | HTTP/JSON gateway | `gateway::tests::*` | SATISFIED | 16+ routes |
| - | gRPC service | `service::tests::*` | SATISFIED | Entity CRUD over gRPC |
| NF | Protobuf definitions with comments | `crates/axon-server/proto/` | SATISFIED | Proto files exist |

### FEAT-006: Bead Storage Adapter

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| US-015 | Pre-defined bead schema | none | UNIMPLEMENTED | No bead-specific schema or collection |
| US-015 | Bead lifecycle state machine | none | UNIMPLEMENTED | No bead-specific logic |
| US-016 | Dependency tracking | none | UNIMPLEMENTED | No bead-specific dependency logic |
| US-016 | Ready-queue computation | none | UNIMPLEMENTED | No bead-specific query |

### FEAT-007: Entity-Graph Model

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| - | Create typed links | `link_creation_between_entities` | SATISFIED | handler.rs:690 |
| - | Link-type enforcement | `create_link_rejects_undeclared_link_type` | SATISFIED | Validates against schema |
| - | Target collection validation | `create_link_rejects_wrong_target_collection` | SATISFIED | handler.rs:722 |
| - | Link metadata validation | `create_link_validates_metadata_against_schema` | SATISFIED | validate_link_metadata |
| - | Duplicate triple rejection | `create_link_rejects_duplicate_triple` | SATISFIED | handler.rs:740 |
| - | Cardinality enforcement | none | UNIMPLEMENTED | LinkTypeDef has cardinality but not enforced |
| - | BFS traversal with depth limit | `traversal_follows_links_to_depth` | SATISFIED | handler.rs:854 |
| - | Cycle detection | `traversal_does_not_revisit_cycles` | SATISFIED | visited set |

### FEAT-008: ACID Transactions

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| - | Multi-entity atomic commit | `atomic_debit_credit_succeeds` | SATISFIED | Transaction::commit |
| - | All-or-nothing rollback on conflict | `version_conflict_aborts_entire_transaction` | SATISFIED | Tests verify no partial writes |
| - | Audit entries share transaction_id | `audit_entries_share_transaction_id` | SATISFIED | tx_id in entries |
| - | Transaction API exposed via HTTP/gRPC | none | UNIMPLEMENTED | Handler-only; no network endpoint |

### FEAT-009: Graph Traversal Queries

| Story | Criterion | Test Reference | Status | Evidence |
|-------|-----------|----------------|--------|----------|
| US-023 | Forward traversal with depth | `traversal_follows_links_to_depth` | SATISFIED | BFS to configurable depth |
| US-023 | Circular dependency handling | `traversal_does_not_revisit_cycles` | SATISFIED | Visited set |
| US-024 | BOM explosion | `scn_004_bom_explosion_recursive_traversal` | SATISFIED | Business scenario test |
| US-023 | Path reporting in results | none | UNIMPLEMENTED | Traverse returns entities only, not paths |
| US-024 | Link metadata in traversal results | none | UNIMPLEMENTED | TraverseResponse has only entities |
| US-025 | Reachability query | none | UNIMPLEMENTED | No short-circuit reachability check |
| - | Reverse traversal | none | UNIMPLEMENTED | No reverse traversal API |
| - | Multi-type traversal | none | UNIMPLEMENTED | Single link_type filter only |
| - | Hop-level filtering | none | UNIMPLEMENTED | No per-hop entity predicates |

### FEAT-010: Workflow State Machines (P2, Deferred)

All criteria DEFERRED per priority classification.

---

## 7. Gap Register

| # | Area | Classification | Planning Evidence | Implementation Evidence | Resolution Direction | Review Issue | Notes |
|---|------|----------------|-------------------|--------------------------|----------------------|-------------|-------|
| G-01 | Collections | INCOMPLETE | FEAT-001: "creation time, schema version, entity count, modified time" | CollectionMetadata lacks timestamps | code-to-plan | axon-23e66f77 | Add created_at, updated_at to CollectionMetadata |
| G-02 | Links | INCOMPLETE | FEAT-007: "cardinality constraints hold" | LinkTypeDef.cardinality parsed but not enforced | code-to-plan | axon-848ab0fe | Enforce one-to-one, many-to-one limits in create_link |
| G-03 | Transactions | INCOMPLETE | FEAT-008: multi-entity transactions | Transaction exists in handler only | code-to-plan | axon-03f6c861 | Expose transaction API via HTTP/gRPC |
| G-04 | Traversal | INCOMPLETE | FEAT-009: path reporting, link metadata, reverse traversal, reachability | traverse() returns entities only, no paths/metadata | code-to-plan | axon-848ab0fe | Extend TraverseResponse; add reverse/reachability |
| G-05 | API | INCOMPLETE | FEAT-005: gRPC SDKs for Go and TypeScript | Only Rust gRPC service exists | code-to-plan | axon-eef97bd6 | Generate client SDKs (post-V1 for non-Rust) |
| G-06 | API | INCOMPLETE | FEAT-005: CLI for all operations | axon-cli crate is minimal | code-to-plan | axon-eef97bd6 | Build out CLI commands |
| G-07 | Bead | UNIMPLEMENTED | FEAT-006: purpose-built bead collection | No bead-specific code | code-to-plan | axon-eef97bd6 | Implement bead schema, lifecycle, ready-queue |
| G-08 | Architecture | UNDERSPECIFIED | No ADR for transaction model | OCC implemented without architectural rationale doc | plan-to-code | axon-03f6c861 | Write ADR-004 documenting OCC design decision |
| G-09 | Storage | INCOMPLETE | ADR-003: PostgreSQL backend | Only Memory + SQLite implemented | code-to-plan | axon-6ac13413 | PostgreSQL adapter (V1 target per ADR-003) |
| G-10 | Audit | INCOMPLETE | ADR-003: audit writes share storage transaction | Audit uses separate MemoryAuditLog, not co-located | code-to-plan | axon-973d5c7f | Co-locate audit in StorageAdapter transaction |
| G-11 | Traversal | INCOMPLETE | FEAT-009: "filter at each hop" | No per-hop predicates in traverse | code-to-plan | axon-848ab0fe | Add hop-level filter to TraverseRequest |
| G-12 | Schema | ALIGNED | FEAT-002 + ADR-002 | JSON Schema 2020-12 + link-type defs fully implemented | N/A | axon-d7a39fd0 | Quality: consider caching compiled validators |
| G-13 | Audit | ALIGNED | FEAT-003 | Full audit coverage with before/after/diff | N/A | axon-973d5c7f | Quality: field-level diff exceeds spec (good) |
| G-14 | Testing | ALIGNED | Test Plan L1-L5 | L1 (sim), L2 (scenarios), L3 (proptest), L4 (conformance), L5 (criterion) all implemented | N/A | axon-16ae360c | L6 (API contract tests) not yet implemented |
| G-15 | Testing | INCOMPLETE | Test Plan L6: API contract tests | No protobuf-generated client tests | code-to-plan | axon-16ae360c | Implement L6 contract tests |

---

## 8. Traceability Matrix

| Vision Item | Requirement | Feature | ADR | Test Reference | Code Status | Classification |
|-------------|-------------|---------|-----|----------------|-------------|----------------|
| Entity storage | PRD-entities | FEAT-004 | ADR-001 | L1 INV-001..008, L2 SCN-* | Complete | ALIGNED |
| Schema validation | PRD-schema | FEAT-002 | ADR-002 | L1 INV-005, L3 PROP-001 | Complete | ALIGNED |
| Typed links | PRD-links | FEAT-007 | ADR-002 | L1 INV-006, L2 SCN-004 | Partial (cardinality) | INCOMPLETE |
| Audit trail | PRD-audit | FEAT-003 | ADR-003 | L1 INV-002, L3 PROP-002 | Complete (in-memory only) | INCOMPLETE |
| Transactions | PRD-transactions | FEAT-008 | (no ADR) | L1 INV-003, L3 PROP-004 | Handler only | INCOMPLETE |
| Collections | PRD-collections | FEAT-001 | - | L2 SCN-* | Missing timestamps | INCOMPLETE |
| API surface | PRD-api | FEAT-005 | - | L6 (missing) | HTTP+gRPC; no SDKs/CLI | INCOMPLETE |
| Graph traversal | PRD-queries | FEAT-009 | - | L2 SCN-004, SCN-006 | Basic forward only | INCOMPLETE |
| Storage backends | PRD-storage | - | ADR-003 | L4 conformance | Memory+SQLite; no PostgreSQL | INCOMPLETE |
| Beads | PRD-agentic | FEAT-006 | - | - | Not started | UNIMPLEMENTED |
| State machines | PRD-workflow | FEAT-010 | - | - | Deferred (P2) | DEFERRED |

---

## 9. Review Issue Summary

| Review Issue | Functional Area | Key Findings | Recommended Direction |
|-------------|-----------------|--------------|----------------------|
| axon-23e66f77 | Data model & entity lifecycle | Missing collection timestamps (G-01) | code-to-plan |
| axon-d7a39fd0 | Schema & validation | ALIGNED. Quality: cache compiled validators | quality-improvement |
| axon-6ac13413 | Storage layer | Missing PostgreSQL backend (G-09) | code-to-plan |
| axon-848ab0fe | Link & graph model | Cardinality unenforced (G-02), traversal gaps (G-04, G-11) | code-to-plan |
| axon-03f6c861 | Transactions & OCC | No network API (G-03), no ADR (G-08) | code-to-plan + plan-to-code |
| axon-973d5c7f | Audit & compliance | In-memory only (G-10), field-diff exceeds spec | code-to-plan |
| axon-eef97bd6 | API surfaces | No SDKs (G-05), CLI minimal (G-06), beads not started (G-07) | code-to-plan |
| axon-16ae360c | Test coverage | L1-L5 complete, L6 missing (G-15) | code-to-plan |

---

## 10. Execution Issues Generated

See Section 12 below. Issues will be created after this report is reviewed.

---

## 11. Issue Coverage Verification

| Gap / Criterion | Covering Issue | Status |
|-----------------|---------------|--------|
| G-01: Collection timestamps | To be created | pending |
| G-02: Cardinality enforcement | To be created | pending |
| G-03: Transaction network API | To be created | pending |
| G-04: Traversal path/metadata | To be created | pending |
| G-05: gRPC client SDKs | deferred (post-V1) | deferred |
| G-06: CLI commands | To be created | pending |
| G-07: Bead storage adapter | To be created | pending |
| G-08: ADR-004 transaction model | To be created | pending |
| G-09: PostgreSQL backend | To be created | pending |
| G-10: Co-located audit storage | To be created | pending |
| G-11: Hop-level traversal filter | To be created | pending |
| G-15: L6 API contract tests | To be created | pending |

---

## 12. Execution Order

### Critical Path (V1 blockers)

**Phase 1 — Upstream artifacts first**
1. ADR-004: Transaction model design decision (G-08)

**Phase 2 — Core gaps (parallelizable)**
2. Collection timestamps (G-01) — small, independent
3. Cardinality enforcement (G-02) — extends existing link validation
4. Co-located audit in storage transaction (G-10) — architectural
5. PostgreSQL StorageAdapter (G-09) — independent backend work

**Phase 3 — API completeness (depends on Phase 2)**
6. Transaction HTTP/gRPC endpoint (G-03) — depends on G-08
7. Extended traversal: paths, metadata, reverse, reachability, hop filter (G-04, G-11)
8. CLI commands for all operations (G-06)
9. L6 API contract tests (G-15) — depends on stable API

**Phase 4 — Feature completions**
10. Bead storage adapter (G-07) — depends on traversal (G-04)

### First recommended execution set (unblocked, high impact)
- G-01 (collection timestamps)
- G-02 (cardinality enforcement)
- G-08 (ADR-004)
- G-10 (co-located audit storage)

---

## 13. Open Decisions

1. **Transaction API shape**: Should the HTTP/gRPC API expose multi-entity transactions as a batch endpoint or as a session-based protocol? Needs ADR-004.
2. **Audit storage co-location**: Should audit entries be stored in the same StorageAdapter table as entities, or remain a separate subsystem? ADR-003 implies co-location but implementation diverges.
3. **Client SDK priority**: Are Go/TypeScript SDKs V1 requirements or can they be deferred? FEAT-005 lists them as P0 but generating SDK stubs from protobuf is low effort.
4. **Bead adapter scope**: Is FEAT-006 required for V1 launch or can it be post-V1? Currently P1 but has zero implementation.
