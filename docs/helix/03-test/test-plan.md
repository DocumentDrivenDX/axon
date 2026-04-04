---
dun:
  id: helix.test-plan
  depends_on:
    - helix.prd
    - helix.principles
    - helix.technical-requirements
---
# Axon Test Plan

**Version**: 0.1.0
**Date**: 2026-04-04
**Status**: Draft

---

## 1. Philosophy

> "Test suite first, implementation second." — Axon Principle P1

Following FoundationDB's approach: the test suite defines the system. Correctness properties are encoded as executable invariants. The implementation exists to pass them. If a property isn't tested, it isn't guaranteed.

This test plan specifies **what must be true** about Axon. Each section defines invariants, workloads, and acceptance criteria that must pass before the corresponding feature is considered implemented. The test plan is the governing artifact — code that doesn't satisfy these specifications is incomplete regardless of how many unit tests it has.

---

## 2. Test Architecture

### Layers

| Layer | Purpose | Framework | Runs |
|-------|---------|-----------|------|
| **L1: Correctness invariants** | Prove ACID, audit completeness, schema enforcement, link integrity under fault injection | `axon-sim` DST framework | CI: every commit. Nightly: extended seeds |
| **L2: Business scenario tests** | Validate real-world workflows from use case research (AP/AR, CRM, CDP, etc.) end-to-end | Integration tests against embedded Axon | CI: every commit |
| **L3: Property-based tests** | Generate random entities, links, schemas, transactions and verify invariants hold | `proptest` or `bolero` | CI: every commit (bounded iterations). Nightly: extended |
| **L4: Backend conformance** | Verify every StorageAdapter implementation passes the identical test suite | Parameterized tests across SQLite, Postgres, FoundationDB | CI: SQLite. Pre-merge: all backends |
| **L5: Performance benchmarks** | Verify latency and throughput targets from technical requirements | `criterion` benchmarks | Nightly. Ratcheted — can only improve |
| **L6: API contract tests** | Verify gRPC and HTTP gateway conform to protobuf contract | Generated client tests | CI: every commit |

### Deterministic Simulation Framework (`axon-sim`)

Built on the FoundationDB model (see [research](../00-discover/foundationdb-dst-research.md)):

| Component | Description |
|-----------|-------------|
| `SimRng` | Xorshift64 PRNG. Same seed → identical execution. Every random decision flows through this |
| `Buggify` | Fault injector activated probabilistically during simulation. Injects: transaction aborts, storage errors, write delays |
| Workloads | Composable test workloads with 4 phases: SETUP → EXECUTION → CHECK → METRICS |
| Runtime abstraction | `StorageAdapter` trait enables identical test code against memory, SQLite, Postgres backends |
| Seed exploration | CI runs N seeds per workload. Nightly runs M >> N seeds. Any failure seed is recorded and replayed in CI forever |

### Ratchets (HELIX Quality Gates)

| Ratchet | Direction | Metric | Enforced |
|---------|-----------|--------|----------|
| Correctness seeds passing | Increasing | Count of seeds with 0 invariant violations | CI gate |
| Invariant count | Increasing | Number of distinct invariants under test | Per-release review |
| Simulation hours | Increasing | Cumulative simulated hours of fault injection | Tracked in CI metrics |
| Test coverage (line) | Increasing | % lines exercised by L1-L4 tests | CI gate (ratchet file) |
| Performance p99 | Improving | Latency must not regress beyond threshold | Nightly gate |
| Audit gap count | Decreasing → 0 | Mutations without corresponding audit entries | CI gate |

---

## 3. L1: Correctness Invariants

These are the properties that must hold under all circumstances, including concurrent access and fault injection. Each invariant has a formal statement, a verification method, and a simulation workload.

### INV-001: No Lost Updates

**Statement**: If two transactions concurrently update the same entity, exactly one succeeds and the other receives a version conflict error with the current state. No write is silently overwritten.

**Verification**: Two concurrent simulated agents repeatedly read-modify-write the same entity. After N rounds, the entity's value must reflect exactly the writes that were acknowledged as successful.

**Workload**: `ConcurrentWriterWorkload`
- 2-10 simulated agents
- Single shared entity
- Each agent: read → compute → write with expected version
- CHECK: final value == sum of acknowledged writes
- BUGGIFY: inject version conflicts, storage errors, delays

### INV-002: Serializable Isolation (Cycle Test)

**Statement**: Concurrent transactions produce results equivalent to some serial execution order. No write skew, no phantom reads, no dirty reads.

**Verification**: FoundationDB's cycle test. N entities form a ring via typed links. Transactions atomically update pairs of adjacent nodes. The CHECK phase walks the ring — exactly N hops means isolation held.

**Workload**: `CycleWorkload`
- Ring of 5-100 entities connected by `next` links
- 100-10,000 swap transactions under concurrent execution
- CHECK: ring walk returns to start in exactly N hops
- BUGGIFY: inject transaction aborts, delays, storage errors
- Failure detection: fewer or more hops = isolation violation

### INV-003: Audit Completeness

**Statement**: Every committed mutation (entity create, update, delete; link create, delete) has a corresponding audit entry. There are no gaps — if the mutation is visible, the audit entry exists.

**Verification**: After any workload, count committed mutations vs audit entries. They must match exactly. Every entity version in storage must have a corresponding audit entry with matching before/after state.

**Workload**: `AuditCompletenessWorkload`
- Mixed CRUD operations across multiple collections
- CHECK: `count(mutations) == count(audit_entries)`
- CHECK: for each entity, walk audit entries and reconstruct current state — must match stored state
- BUGGIFY: inject failures between mutation and audit write (must either both commit or neither)

### INV-004: Audit Immutability

**Statement**: No audit entry is ever modified or deleted through any API path. The audit log is strictly append-only.

**Verification**: Record audit entry IDs and hashes before and after a workload. All pre-existing entries must be unchanged.

**Workload**: `AuditImmutabilityWorkload`
- Create entities, record audit entries and content hashes
- Execute more operations
- CHECK: all previously recorded audit entries still exist with identical content
- Attempt to update/delete audit entries via API — must fail

### INV-005: Schema Enforcement

**Statement**: No entity in storage violates its collection's schema. Every write is validated; invalid writes are rejected with structured errors.

**Verification**: After any workload, read every entity from storage and validate against its collection schema. All must pass.

**Workload**: `SchemaEnforcementWorkload`
- Define collections with schemas (required fields, types, enums, nesting)
- Attempt valid and invalid writes in random order
- CHECK: all invalid writes rejected; all entities in storage pass schema validation
- CHECK: error responses include field path, expected type, actual value

### INV-006: Link Integrity

**Statement**: No link references a non-existent entity (unless force-deleted with cascade). Link-type constraints (cardinality, required target collection) are enforced.

**Verification**: After any workload, for every link in storage, both source and target entities must exist. Link-type constraints must hold.

**Workload**: `LinkIntegrityWorkload`
- Create entities and links across collections
- Delete entities (should fail if inbound links exist, or cascade)
- CHECK: no dangling links; all link-type constraints satisfied

### INV-007: Version Monotonicity

**Statement**: Entity versions strictly increase. No version is reused or skipped. Version 1 is always the creation version.

**Verification**: For each entity, audit entries must show version 1, 2, 3, ... with no gaps or repeats.

**Workload**: Verified as a CHECK in every workload that modifies entities.

### INV-008: Transaction Atomicity

**Statement**: Multi-operation transactions either fully commit (all operations visible, all audit entries present with shared transaction ID) or fully abort (no operations visible, no audit entries).

**Verification**: Execute transactions that include intentional failures mid-transaction. Verify partial state never leaks.

**Workload**: `TransactionAtomicityWorkload`
- Transactions with 2-10 operations
- BUGGIFY: inject failures after some operations but before commit
- CHECK: for each transaction ID in audit log, all operations are present; for failed transactions, no operations are present
- AP/AR scenario: debit account A, credit account B, create ledger entry — must be all-or-nothing

---

## 4. L2: Business Scenario Tests

Real-world workflows from the use case research, encoded as deterministic integration tests. Each scenario tests a complete business process end-to-end against embedded Axon.

### SCN-001: AP/AR — Payment Application with Partial Payment

**Source**: Use case research, AP/AR domain, workflow #2

**Setup**:
- Create `invoices` and `payments` collections with schemas from use case research
- Create 3 invoices: INV-030 ($5,000), INV-035 ($5,000), INV-040 ($3,000)
- Create customer with balance $13,000

**Execution**:
1. Create payment PMT-107 for $7,500
2. In one transaction: apply $5,000 to INV-030 (→ paid), $2,500 to INV-035 (→ partially_paid), create ledger entries, update customer balance

**Check**:
- [ ] INV-030 status is `paid`, INV-035 status is `partially_paid`
- [ ] `paid-by` links carry correct `amount_applied` metadata
- [ ] Ledger entries balance: debit cash $7,500 == credit AR $7,500
- [ ] Customer balance reduced by $7,500
- [ ] Audit log shows all changes under one transaction ID
- [ ] Reverting the transaction restores all entities to pre-payment state

**Failure scenario**: Version conflict on customer balance mid-transaction → entire payment rolls back, no partial application.

### SCN-002: CRM — Contact Merge (Duplicate Resolution)

**Source**: Use case research, CRM domain, workflow #2

**Setup**:
- Create `contacts` and `deals` collections
- Contact-A and Contact-B represent the same person
- Contact-A has links: `works-at` → Company-X, `owns-deal` → Deal-1
- Contact-B has links: `works-at` → Company-X, `owns-deal` → Deal-2

**Execution**:
1. In one transaction: re-link Deal-1 from Contact-A to Contact-B, merge Contact-A fields into Contact-B, delete Contact-A

**Check**:
- [ ] Contact-A no longer exists
- [ ] Contact-B has both deals linked
- [ ] No orphaned links (no link references Contact-A)
- [ ] Audit log shows the merge as one transaction with before/after for all entities
- [ ] Contact-B's version incremented; merged fields present

### SCN-003: CDP — Identity Resolution and Profile Merge

**Source**: Use case research, CDP domain, workflow #1

**Setup**:
- Create `profiles` and `source-records` collections
- Two source records from different channels with matching email
- No unified profile exists yet

**Execution**:
1. Identity resolution creates a new unified profile
2. Links both source records to the profile via `resolved-from` with confidence scores
3. Profile gets canonical fields derived from highest-confidence sources

**Check**:
- [ ] Unified profile exists with canonical fields
- [ ] Both source records linked via `resolved-from` with metadata (confidence, match_rule)
- [ ] Audit trail shows profile creation and link creation
- [ ] Schema validation passes on the unified profile

### SCN-004: ERP — BOM Explosion via Recursive Traversal

**Source**: Use case research, ERP domain, workflow #2

**Setup**:
- Create `products` collection
- Widget-A contains Sub-Assembly-B (qty: 2) and Component-C (qty: 4)
- Sub-Assembly-B contains Component-C (qty: 1) and Component-D (qty: 3)

**Execution**:
1. Traverse from Widget-A via `contains` links to depth 3

**Check**:
- [ ] Traversal returns Sub-Assembly-B, Component-C (twice — direct and via B), Component-D
- [ ] Link metadata (`quantity`) is included at each hop
- [ ] Total Component-C needed: 4 (direct) + 2×1 (via B) = 6
- [ ] Leaf nodes identified (entities with no outgoing `contains` links)

### SCN-005: Workflow — Invoice Approval Chain

**Source**: Use case research, Workflow Automation + AP/AR domains

**Setup**:
- Create `invoices` collection with state machine: draft → submitted → approved → paid
- Invoice requires `approved-by` link before transition to `approved`

**Execution**:
1. Create invoice in `draft` state
2. Attempt to move directly to `approved` — must fail (invalid transition)
3. Move to `submitted`
4. Attempt to move to `approved` without `approved-by` link — must fail (guard condition)
5. Create `approved-by` link to approver contact
6. Move to `approved` — succeeds

**Check**:
- [ ] Invalid transition rejected with error listing valid transitions
- [ ] Guard condition failure rejected with specific reason
- [ ] Valid transition succeeds after guard is satisfied
- [ ] Audit trail captures each transition attempt (successful and failed)

### SCN-006: Issue Tracking — Dependency DAG and Ready Queue

**Source**: Use case research, Issue Tracking + Agentic Applications domains

**Setup**:
- Create `issues` collection
- Issue-A depends on Issue-B and Issue-C
- Issue-B depends on Issue-D
- Issue-C has no dependencies

**Execution**:
1. Query "ready" issues (status=open, all dependencies in status=done)
2. Close Issue-D, re-query
3. Close Issue-B, re-query
4. Close Issue-C, re-query

**Check**:
- [ ] Initially: only Issue-C and Issue-D are ready (no deps or deps done)
- [ ] After closing D: Issue-B becomes ready
- [ ] After closing B: no new ready issues (Issue-A still blocked by C)
- [ ] After closing C: Issue-A becomes ready
- [ ] Dependency traversal correctly identifies transitive blockers

### SCN-007: Agentic — Bead Lifecycle with Concurrent Agents

**Source**: Use case research, Agentic Applications domain

**Setup**:
- Create `beads` collection with lifecycle: draft → pending → ready → in_progress → review → done
- 5 beads with dependency DAG
- 3 simulated agents claiming and completing beads

**Execution**:
1. Agents concurrently query ready queue, claim beads (update status to in_progress)
2. OCC ensures no two agents claim the same bead
3. Agents complete beads, unblocking dependents

**Check**:
- [ ] No bead processed by more than one agent (OCC prevents double-claim)
- [ ] Dependency DAG respected — bead not started until deps done
- [ ] Audit log shows which agent processed each bead
- [ ] All beads reach `done` state

### SCN-008: MDM — Golden Record Merge with Survivorship

**Source**: Use case research, MDM domain

**Setup**:
- Create `golden-records` and `source-records` collections
- Two source records with overlapping but conflicting data
- Match rule triggers merge

**Execution**:
1. In one transaction: create golden record, link both sources via `sourced-from`, apply survivorship rules (highest-confidence field wins)

**Check**:
- [ ] Golden record has correct field values per survivorship rules
- [ ] Both source records linked with `sourced-from` metadata (confidence, match_rule, source_system)
- [ ] Audit trail records the merge with before/after state
- [ ] Schema validation passes on golden record

### SCN-009: Document Management — Version Chain

**Source**: Use case research, Document Management domain

**Setup**:
- Create `documents` collection
- Document with 3 versions, linked via `supersedes` links

**Execution**:
1. Create v1, then v2 (linked `supersedes` v1), then v3 (linked `supersedes` v2)
2. Traverse `supersedes` chain to find version history

**Check**:
- [ ] Traversal from v3 via `supersedes` returns [v2, v1] in order
- [ ] Each version has correct metadata and audit trail
- [ ] Latest version queryable without traversal (field-based query)

### SCN-010: Time Tracking — Approval and Billing

**Source**: Use case research, Time Tracking domain

**Setup**:
- Create `time-entries` and `projects` collections
- Time entries linked to projects via `logged-for`

**Execution**:
1. Create time entries for a project
2. Submit for approval (status transition)
3. Approve (requires `approved-by` link)
4. Bill (aggregation query: sum hours by project where status=approved)

**Check**:
- [ ] State machine enforces: draft → submitted → approved → billed
- [ ] Aggregation returns correct total hours
- [ ] Audit trail shows approval chain

---

## 5. L3: Property-Based Tests

### PROP-001: Schema Round-Trip

**Property**: For any valid ESF schema definition, generating a random valid entity and validating it against the schema always succeeds. Generating a random invalid entity (type mismatch, missing required field) always fails with a structured error.

### PROP-002: Audit Reconstruction

**Property**: For any sequence of CRUD operations on an entity, replaying the audit log from the beginning produces the current state of the entity.

### PROP-003: OCC Linearizability

**Property**: For any sequence of concurrent read-then-write operations on a single entity, the final state is consistent with some serial ordering of the successful writes. No acknowledged write is lost.

### PROP-004: Transaction Serializability

**Property**: For any set of concurrent multi-entity transactions, the final database state is consistent with some serial ordering of the committed transactions.

### PROP-005: Link Graph Consistency

**Property**: After any sequence of entity/link operations, the link graph is consistent: no dangling references, all link-type constraints hold, traversal from any entity follows only valid links.

---

## 6. L4: Backend Conformance

Every StorageAdapter implementation must pass the **identical** test suite. Tests are written against the trait, parameterized by backend.

| Test Suite | SQLite | PostgreSQL | FoundationDB | Memory |
|-----------|:------:|:----------:|:------------:|:------:|
| INV-001 through INV-008 | Required | Required | Required | Required |
| SCN-001 through SCN-010 | Required | Required | Required | Required |
| PROP-001 through PROP-005 | Required | Required | Required | Required |
| BM-001 through BM-010 | Required | Required | Required | N/A (memory not benchmarked) |

If a backend cannot pass any invariant, it is not shipped.

---

## 7. L5: Performance Benchmarks

From technical requirements. All benchmarks use `criterion` and are ratcheted.

| Benchmark | Target (p99) | Workload |
|-----------|-------------|----------|
| BM-001: Single entity read | < 5 ms | 10,000 random point lookups |
| BM-002: Single entity write | < 10 ms | 10,000 creates with schema validation + audit |
| BM-003: Multi-entity transaction (5 ops) | < 20 ms | 1,000 transactions (debit/credit pattern from SCN-001) |
| BM-004: Collection scan (1,000 entities) | < 100 ms | Filter + sort + paginate |
| BM-005: Audit log append overhead | < 2 ms | Measured as delta: write-with-audit minus write-without |
| BM-006: Link traversal (3 hops) | < 50 ms | BOM explosion pattern from SCN-004 |
| BM-007: Aggregation (10K entities) | < 500 ms | COUNT/SUM/GROUP BY over invoice collection |
| BM-008: Concurrent writers (100) | Linear throughput scaling | 100 agents writing to different entities |
| BM-009: Schema validation | < 1 ms | Validate typical entity (20 fields, 2 levels nesting) |
| BM-010: Audit query (single entity) | < 100 ms | Retrieve all audit entries for one entity (100 mutations) |

---

## 8. L6: API Contract Tests

- gRPC service methods match protobuf definitions exactly
- HTTP gateway produces identical results to gRPC for all operations
- Error responses conform to structured error format (code, detail, field path)
- Embedded API (Rust trait) produces identical results to network API

---

## 9. Test Execution Schedule

| Trigger | Test Layers | Seed Count | Timeout |
|---------|------------|-----------|---------|
| Every commit (CI) | L1 (100 seeds), L2, L3 (1K iterations), L4 (SQLite + memory), L6 | 100 | 10 min |
| Pre-merge | L1 (1K seeds), L2, L3 (10K iterations), L4 (all backends), L5, L6 | 1,000 | 30 min |
| Nightly | L1 (100K seeds), L2, L3 (100K iterations), L4 (all backends), L5 | 100,000 | 4 hours |
| Weekly | L1 (1M seeds), L3 (1M iterations) | 1,000,000 | 24 hours |

### Seed Management

- Every CI run records the seeds used and their pass/fail status
- Failed seeds are added to a **regression seed file** that runs on every CI build forever
- The regression seed file is a ratchet — seeds are added, never removed

---

## 10. Traceability

| Invariant / Scenario | Feature | Principle | Use Case Domain |
|----------------------|---------|-----------|-----------------|
| INV-001 No Lost Updates | FEAT-008 | P4 (Transactions) | All |
| INV-002 Cycle Test | FEAT-008 | P4 (Transactions) | All |
| INV-003 Audit Completeness | FEAT-003 | P2 (Audit) | AP/AR, CDP, MDM |
| INV-004 Audit Immutability | FEAT-003 | P2 (Audit) | AP/AR, Compliance |
| INV-005 Schema Enforcement | FEAT-002 | P5 (Schema) | All |
| INV-006 Link Integrity | FEAT-007 | P3 (Entities/Links) | CRM, ERP, CDP |
| INV-007 Version Monotonicity | FEAT-008 | P4 (Transactions) | All |
| INV-008 Transaction Atomicity | FEAT-008 | P4 (Transactions) | AP/AR, CRM, MDM |
| SCN-001 Payment Application | FEAT-008, FEAT-003 | P2, P4 | AP/AR |
| SCN-002 Contact Merge | FEAT-007, FEAT-008 | P3, P4 | CRM |
| SCN-003 Identity Resolution | FEAT-007, FEAT-003 | P3, P2 | CDP |
| SCN-004 BOM Explosion | FEAT-009 | P3 | ERP |
| SCN-005 Approval Chain | FEAT-010, FEAT-003 | P2 | Workflow, AP/AR |
| SCN-006 Dependency DAG | FEAT-009, FEAT-006 | P3 | Issue Tracking, Agentic |
| SCN-007 Bead Lifecycle | FEAT-006, FEAT-008 | P1, P4 | Agentic |
| SCN-008 Golden Record Merge | FEAT-007, FEAT-008 | P3, P4 | MDM |
| SCN-009 Version Chain | FEAT-007, FEAT-009 | P3 | Document Mgmt |
| SCN-010 Time Approval | FEAT-010, FEAT-003 | P2 | Time Tracking |

---

*This test plan is the governing artifact. Implementation is complete when all invariants, scenarios, properties, and benchmarks pass across all backends.*
