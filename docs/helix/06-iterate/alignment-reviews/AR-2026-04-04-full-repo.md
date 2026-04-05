---
dun:
  id: helix.ar-2026-04-04
  depends_on:
    - helix.prd
    - helix.test-plan
---
# Alignment Review: Full Repo

**Date**: 2026-04-04
**Scope**: Full repository — all planning artifacts vs implementation
**Review Epic**: axon-15e3318f
**Test Status**: 83 tests passing (81 unit + 2 doc-tests), 0 failures

---

## 1. Scope and Governing Artifacts

| Phase | Artifacts |
|-------|----------|
| 00-discover | Product vision, FoundationDB DST research, use case research, schema format research |
| 01-frame | PRD, principles, technical requirements, competitive analysis, FEAT-001 through FEAT-010 |
| 02-design | ADR-001 (Rust), ADR-002 (schema format), SPIKE-001 (backing stores) |
| 03-test | Test plan (INV-001–008, SCN-001–010, PROP-001–005, BM-001–010) |
| Implementation | 8 crates: axon-core, axon-schema, axon-audit, axon-storage, axon-api, axon-server, axon-cli, axon-sim |

---

## 2. Intent Summary

**Vision**: Cloud-native, auditable, schema-first entity-graph-relational data store for agentic applications.

**Key commitments**: ACID transactions with serializable isolation, immutable audit on every mutation, JSON Schema entity validation with link-type definitions (ADR-002), stateless servers, multi-backend storage, FoundationDB-style deterministic simulation testing.

**Principle P1** (test suite first) is the most critical process commitment — the test plan should govern the implementation.

---

## 3. Planning Stack Findings

| Link | Status | Notes |
|------|--------|-------|
| Vision → PRD | ALIGNED | PRD elaborates vision faithfully |
| PRD → Feature Specs | DIVERGENT | FEAT-001, FEAT-004 use "document" terminology; PRD uses "entity" after Section 4 update |
| Feature Specs → ADRs | ALIGNED | ADR-002 matches FEAT-002 direction |
| ADR-002 → Implementation | INCOMPLETE | JSON Schema validation works; link-type enforcement not implemented |
| PRD → Technical Requirements | ALIGNED | Technical requirements elaborate PRD correctly |
| Principles → Test Plan | ALIGNED | Test plan directly implements P1 (test-first) |
| Test Plan → Implementation | DIVERGENT | Implementation was built before test plan; test plan coverage is ~15% |
| PRD → Missing Specs | UNDERSPECIFIED | P1 features (change feeds, schema evolution, auth) have no feature specs |

---

## 4. Gap Register

| Area | Classification | Planning Evidence | Implementation Evidence | Resolution Direction | Review Issue |
|------|----------------|-------------------|--------------------------|----------------------|-------------|
| Feature spec terminology | STALE_PLAN | FEAT-001–004 say "document" | Code uses "entity" throughout | plan-to-code | axon-96fed9cf |
| StorageAdapter transactions | DIVERGENT | Tech req: begin_tx/commit_tx/abort_tx | Trait has no tx methods; OCC via app-level CAS with TOCTOU window | code-to-plan | axon-f5b2f618 |
| Link-type enforcement | INCOMPLETE | ADR-002: link-type definitions enforced at write time | Parsed but never enforced; link metadata not validated | code-to-plan | axon-c7d68fca |
| Audit diff field | INCOMPLETE | FEAT-003: structured diff of changed fields | AuditEntry has before/after but no diff | code-to-plan | axon-f75fcfce |
| Audit revert | INCOMPLETE | FEAT-003 US-008: revert from audit entry | Not implemented | code-to-plan | axon-f75fcfce |
| Audit query: actor, operation, pagination | INCOMPLETE | FEAT-003 US-007 | Only entity and time-range queries implemented | code-to-plan | axon-f75fcfce |
| Collection lifecycle | INCOMPLETE | FEAT-001: create/drop with audit, metadata, schema binding | Collections are implicit HashMap keys, no lifecycle | code-to-plan | axon-96fed9cf |
| Query/filter system | INCOMPLETE | FEAT-004 US-011: filter, sort, paginate | Not implemented — P0 gap | code-to-plan | axon-c7d68fca |
| Partial update (patch) | INCOMPLETE | FEAT-004 US-012 | Not implemented | code-to-plan | axon-c7d68fca |
| Schema persistence | INCOMPLETE | FEAT-002: schema stored alongside collection | In-memory only, not persisted | code-to-plan | axon-8a5df6c2 |
| Duplicate link prevention | INCOMPLETE | FEAT-007: (source, target, type) unique | put() overwrites; no uniqueness check | code-to-plan | axon-c7d68fca |
| Conflict response with current state | INCOMPLETE | FEAT-004, FEAT-008 | Returns version numbers only, not entity data | code-to-plan | axon-65873862 |
| Write-skew prevention | INCOMPLETE | FEAT-008 US-022 | No test, no storage-level isolation | code-to-plan | axon-65873862 |
| Business scenario tests (SCN-001–010) | INCOMPLETE | Test plan L2: 10 scenarios | 0 implemented | code-to-plan | axon-7fd9e413 |
| Property-based tests (PROP-001–005) | INCOMPLETE | Test plan L3: 5 properties | 0 implemented | code-to-plan | axon-7fd9e413 |
| Performance benchmarks (BM-001–010) | INCOMPLETE | Test plan L5: 10 benchmarks | 0 implemented | code-to-plan | axon-7fd9e413 |
| Correctness invariant workloads | INCOMPLETE | Test plan L1: 8 invariants | Only INV-002 (cycle test) fully implemented | code-to-plan | axon-7fd9e413 |
| Backend conformance (parameterized) | INCOMPLETE | Test plan L4: identical suite across backends | Tests duplicated, not parameterized | code-to-plan | axon-f5b2f618 |
| Change feeds feature spec | UNDERSPECIFIED | PRD P1 #2: subscribe to collection changes | No FEAT spec | decision-needed | axon-96fed9cf |
| Schema evolution feature spec | UNDERSPECIFIED | PRD P1 #1: add/remove/modify fields | No FEAT spec | decision-needed | axon-96fed9cf |
| Auth/authz feature spec | UNDERSPECIFIED | PRD P1 #6: API keys, per-collection perms | No FEAT spec | decision-needed | axon-96fed9cf |
| Bead adapter | INCOMPLETE | FEAT-006: P1 | 0% implemented | code-to-plan | axon-c7d68fca |
| Workflow state machines | INCOMPLETE | FEAT-010: P2 | 0% implemented | deferred (P2) | axon-c7d68fca |

---

## 5. Acceptance Criteria Status

| Feature | Total Criteria | SATISFIED | UNTESTED | UNIMPLEMENTED | PARTIAL |
|---------|---------------|-----------|----------|---------------|---------|
| FEAT-001 Collections | 12 | 0 | 1 | 11 | 0 |
| FEAT-002 Schema Engine | 14 | 7 | 2 | 4 | 1 |
| FEAT-003 Audit Log | 15 | 4 | 1 | 9 | 1 |
| FEAT-004 Entity Ops | 16 | 4 | 0 | 10 | 2 |
| FEAT-005 API Surface | 9 | 2 | 0 | 6 | 1 |
| FEAT-006 Bead Adapter | 10 | 0 | 0 | 10 | 0 |
| FEAT-007 Entity-Graph | 13 | 4 | 2 | 6 | 1 |
| FEAT-008 Transactions | 14 | 5 | 2 | 4 | 3 |
| FEAT-009 Traversal | 13 | 2 | 1 | 10 | 0 |
| FEAT-010 State Machines | 10 | 0 | 0 | 10 | 0 |
| **TOTAL** | **126** | **28 (22%)** | **9 (7%)** | **80 (63%)** | **9 (7%)** |

---

## 6. Traceability Matrix

| Vision Item | PRD Section | Feature | ADR | Test Plan | Code Status | Classification |
|-------------|-------------|---------|-----|-----------|-------------|----------------|
| Audit-first | Sec 1 (Value Props) | FEAT-003 | — | INV-003, INV-004 | 50% | INCOMPLETE |
| Entity-graph model | Sec 4 (Data Model) | FEAT-007 | — | INV-006, SCN-004 | 60% | INCOMPLETE |
| ACID transactions | Sec 5 (Transaction Model) | FEAT-008 | — | INV-001, INV-002, INV-008 | 65% | INCOMPLETE |
| Schema-first | Sec 8 (Requirements) | FEAT-002 | ADR-002 | INV-005 | 60% | INCOMPLETE |
| Cloud-native | Sec 8 (Requirements) | FEAT-005 | ADR-001 | L6 | 50% | INCOMPLETE |
| Agent-native API | Sec 1 (Value Props) | FEAT-005 | — | L6 | 50% | INCOMPLETE |
| Test-first (P1) | Principles | — | — | L1-L6 | 15% | DIVERGENT |

---

## 7. Critical Finding

**Principle P1 violation**: The implementation was built before the test plan was written. The test plan (03-test/test-plan.md) specifies 8 invariants, 10 business scenarios, 5 property tests, and 10 benchmarks. Only INV-002 (cycle test) is fully implemented. The remaining 32 test specifications have no corresponding code. **The test plan must be implemented before further feature work proceeds.**

---

## 8. Open Decisions

1. Should FEAT-001 through FEAT-004 be rewritten to use "entity" terminology, or should we create new consolidated specs?
2. Should change feeds, schema evolution, and auth get feature specs now (they're P1) or defer until P0 is solid?
3. Should the StorageAdapter trait add explicit transaction methods, or is the app-level OCC pattern sufficient with storage-level locking via SQLite/Postgres?

---

*Review complete. Execution issues follow.*
