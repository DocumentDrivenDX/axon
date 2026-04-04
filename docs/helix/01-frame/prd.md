---
dun:
  id: helix.prd
  depends_on: []
---
# Product Requirements Document: Axon

**Version**: 0.1.0
**Date**: 2026-04-04
**Status**: Draft
**Author**: Erik LaBianca

## Executive Summary

Axon is a cloud-native, auditable, schema-first transactional data store built on an entity-graph-relational data model, designed as the central nervous system for agentic applications and business workflows. Entities (deeply nested, schema-validated structures) are the atoms; typed links (directional relationships between entities) are the bonds; ACID transactions, immutable audit trails, and enforced schemas are the guarantees.

The problem: agents and workflows need structured, trustworthy, interconnected state — but current options force a choice between document stores (rich but isolated), graph databases (connected but schema-loose), relational databases (structured but flat), and BaaS platforms (convenient but unaudited). None combine entity richness, relationship modeling, transactional correctness, and audit-first design. Axon eliminates this tradeoff.

Target users are developers building agentic applications (beads, research agents, coding agents) and teams building business workflows (approval chains, document management, time tracking). V1 focuses on the core entity-graph data model, ACID transactions, audit system, schema engine, and API surface — enough to replace ad-hoc storage in internal projects and prove the agent-native value proposition.

---

## 1. Key Value Propositions

| # | Value Proposition | Customer Benefit |
|---|-------------------|------------------|
| 1 | **Entity-graph-relational model** — entities + typed links + SQL-like queries in one system | Model real-world relationships (dependencies, hierarchies, ownership) without joining across systems or sacrificing document richness |
| 2 | **Audit-first architecture** — every mutation produces an immutable, queryable audit record | Full provenance chain: who/what changed what, when, and why. Agents and humans can verify, revert, and reason about state history |
| 3 | **Schema-first collections** — define structure upfront, get validation, migration, and documentation for free | Agents work with well-typed data instead of guessing at shapes. Schema evolution is managed, not hoped for |
| 4 | **ACID transactions** — multi-entity atomic operations with optimistic concurrency | Debit A and credit B atomically. Read-your-writes guarantee. No silent overwrites from stale state |
| 5 | **Cloud-native abstraction** — Axon is Axon regardless of backing storage | Developers don't couple to storage engines. Axon can run embedded, on a server, or as a managed service — same API |
| 6 | **Agent-native API surface** — designed for how agents consume and produce structured data | Agents get transactional batches, optimistic concurrency, change feeds, and structured queries — not raw SQL or file I/O |
| 7 | **Local-first sync** — collections can sync between local and cloud instances with conflict resolution | Offline-capable UIs and agents. Edit locally, sync when connected. CRDTs or OT where appropriate |
| 8 | **Workflow primitives** — state machines, transition guards, and lifecycle hooks built into the collection layer | Business processes (invoice approval, document review, bead lifecycle) get first-class state management without external orchestrators |

---

## 2. Strategic Fit

- **Ecosystem alignment**: Axon complements niflheim (high-performance analytics/entity resolution), tablespec (schema definitions), and DDx (document management). Axon is the transactional layer; niflheim is the analytical layer. Tablespec's UMF format informs Axon's schema system
- **Timing**: The agent era is creating massive demand for structured, auditable state management. Current solutions (Firebase, Supabase, PocketBase) were built for mobile/web apps, not agents. DoltDB and Turso add versioning but lack agent-native APIs and audit-first design. The window to define the category is open now

---

## 3. Problem Statement

### The Problem

Agent state management is an unsolved infrastructure problem. As AI agents become central to software development and business processes, they need a place to store, query, and mutate structured state with guarantees that matter:

- **No audit trail**: Agents modify state without provenance. When something goes wrong, there's no way to trace what happened, who/what caused it, or how to revert
- **No schema enforcement**: Agents write malformed data, schemas drift silently, and downstream consumers break on unexpected shapes
- **No transactional guarantees**: Concurrent agent operations produce corrupt or inconsistent state
- **No agent-native API**: Agents are forced to use APIs designed for human-driven UIs — raw SQL, REST CRUD, file I/O — none of which match how agents naturally produce and consume state
- **Storage coupling**: Applications are locked to a specific database engine, making it impossible to run the same logic embedded, on-premise, or in the cloud

### Current State

| Approach | What Works | What's Missing |
|----------|-----------|----------------|
| **Firebase/Supabase** | Real-time sync, auth, quick setup | No audit trail, no schema enforcement, no agent-native API, vendor lock-in |
| **PocketBase** | Embeddable, simple API | No audit trail, limited schema system, no sync protocol, single-node |
| **DoltDB** | Git-like versioning, SQL | No real-time sync, no agent-native API, versioning != audit, SQL-only interface |
| **Turso** | Edge-distributed SQLite, libSQL | No audit trail, no schema enforcement beyond SQL, no agent affordances |
| **Raw Postgres/SQLite** | Mature, flexible, well-understood | Everything must be built: audit, schemas, sync, agent API. Massive undifferentiated effort |
| **JSON files on disk** | Zero setup, agent-accessible | No concurrency, no audit, no schema, no query, no sync. Breaks immediately at scale |

### Opportunity

The agent era is creating a new infrastructure category: **agent-native state management**. No existing product owns this space. The requirements — audit-first, schema-first, agent-friendly API, cloud-native abstraction, local-first sync — are not incremental features on existing databases. They require purpose-built architecture.

Timing factors:
- Agent frameworks (LangChain, CrewAI, beads) are proliferating but each reinvents state storage
- "Vibe coding" is making structured data management more important, not less — agents need guardrails
- Local-first architecture patterns (CRDTs, sync engines) have matured enough to build on
- The internal ecosystem (niflheim, DDx, beads, tablespec) provides both prior art and immediate customers

---

## 4. Data Model: Entity-Graph-Relational

### Core Concepts

Axon's data model is **entity-graph-relational** — a hybrid that draws from document stores, graph databases, and relational systems:

| Concept | Definition | Example |
|---------|-----------|---------|
| **Entity** | A deeply nested, schema-validated structure representing a real-world object. Low cardinality at each nesting level. Self-contained but connectable | A customer, a bead, an invoice, a document, an account |
| **Link** | A typed, directional relationship between two entities. First-class object with its own metadata, audit trail, and optional schema | `customer-123 --[authored]--> document-456`, `bead-A --[depends-on]--> bead-B`, `account-X --[is-ancestor-of]--> account-Y` |
| **Link-type** | A named relationship category that defines the semantics of a link. Link-types are declared in the schema | `depends-on`, `authored-by`, `is-ancestor-of`, `approves`, `contains` |
| **Collection** | A grouping of entities sharing a schema. Enables SQL-like queries and aggregations across like-kind entities | All invoices, all beads, all customer records |

### Why Entity-Graph-Relational?

The common data models each excel at one thing but fall short on others:

| Model | Strength | Weakness for Axon Use Cases |
|-------|----------|---------------------------|
| **Document (MongoDB, Firebase)** | Rich nested structures, flexible | No relationships, no joins, no referential integrity |
| **Graph (Neo4j, SPARQL)** | Relationships are first-class, traversal queries | Weak schema enforcement, poor aggregation, operational complexity |
| **Relational (Postgres, SQLite)** | Strong schemas, ACID, aggregation | Flat rows, poor nesting, graph queries require recursive CTEs |
| **Entity-Graph-Relational (Axon)** | Nested entities + typed links + SQL-like queries + ACID | New model — must prove value, limited ecosystem initially |

Axon's bet: most agentic and workflow applications model the world as *things* (entities) and *relationships between things* (links). The data model should reflect this directly, not force it through a relational or document lens.

### Query Model

Entities within a collection are queryable with familiar operations:

- **Filter**: `status = "pending" AND priority > 3`
- **Sort**: `ORDER BY created_at DESC`
- **Aggregate**: `COUNT(*) WHERE status = "done" GROUP BY assignee`
- **Traverse links**: `FOLLOW depends-on FROM bead-A DEPTH 3` — graph traversal over typed links
- **Join via links**: Find all documents authored by customers in segment X — link traversal + entity filter

The query model targets **moderate scale** — thousands to low millions of entities per collection. Axon is not a data warehouse; analytical workloads at scale belong in niflheim.

---

## 5. Transaction Model

### ACID Guarantees

Axon provides full ACID transactions across entities and links within a single Axon instance:

| Property | Guarantee |
|----------|-----------|
| **Atomicity** | A transaction that updates multiple entities and/or links either fully commits or fully rolls back. Debit account A and credit account B — both or neither |
| **Consistency** | Every committed transaction leaves the database in a schema-valid state. Schema violations abort the transaction |
| **Isolation** | Serializable isolation by default (see Isolation Levels below) |
| **Durability** | Committed transactions survive process restarts. The audit log is the durability mechanism |

### Isolation Levels

Axon's isolation model is defined in terms of the standard database isolation hierarchy. The default and recommended level is **serializable**; weaker levels are available as explicit opt-ins for performance-sensitive paths.

| Level | Guarantee | Prevents | Axon Support |
|-------|-----------|----------|-------------|
| **Serializable** | Transactions behave as if executed one at a time in some serial order. Strongest guarantee. No anomalies possible | Dirty reads, non-repeatable reads, phantom reads, write skew | **Default**. V1 implementation via optimistic concurrency with conflict detection at commit time |
| **Snapshot Isolation (SI)** | Each transaction reads from a consistent snapshot taken at transaction start. Writers don't block readers. Vulnerable to write skew | Dirty reads, non-repeatable reads, phantom reads | **P1**. Useful for long-running read transactions that shouldn't block writers |
| **Read Committed** | Each statement sees only data committed before the statement began. Different statements in the same transaction may see different snapshots | Dirty reads | **Available** as explicit opt-in. Useful for reporting queries that tolerate minor inconsistency |
| **Read Uncommitted** | **Not supported**. Axon will never expose uncommitted data | — | Not supported |

**Linearizability**: For single-entity operations, Axon provides **linearizable** semantics — once a write is acknowledged, all subsequent reads (from any client on the same instance) will see that write. This is stronger than serializable for single-object operations and critical for the optimistic concurrency model: when you read an entity at version 5 and update it, you are guaranteed that version 5 was the most recent committed version at the time of your read.

**Read-your-writes**: Within a session (or connection), a client always sees its own writes immediately, regardless of isolation level.

### Optimistic Concurrency Control (OCC)

Axon uses optimistic concurrency as its primary concurrency mechanism:

- Every entity carries a **version number** (monotonically increasing, strictly ordered)
- Write operations include the expected version: "update this entity, but only if its version is still 5"
- If another transaction has committed a change to the entity since the reader's version, the write fails with a **version conflict** error
- The conflict response includes the **current committed state** of the entity, enabling the caller to merge and retry
- This guarantees **no lost updates**: a write is never applied based on stale state. If you update a customer balance to reflect a $100 debit, only a client who has observed your $100 debit can subsequently overwrite that balance

**Why OCC over pessimistic locking**: Agentic workloads are characterized by high read-to-write ratios and low contention. Agents typically work on different entities concurrently. Pessimistic locking (row locks, table locks) would serialize agents unnecessarily and create deadlock hazards. OCC maximizes concurrency while guaranteeing correctness.

### Transaction Scope

| Scope | V1 Support | Notes |
|-------|-----------|-------|
| Single-entity read/write | Yes | Linearizable. Optimistic concurrency via version |
| Multi-entity transaction | Yes | Serializable. Atomic batch: update entities A, B, and link L in one transaction. All-or-nothing |
| Cross-collection transaction | Yes | Serializable. Debit in `accounts` collection and create record in `ledger` collection — same transaction |
| Cross-instance transaction | No (P2) | Distributed transactions across Axon instances are deferred. Will require consensus protocol (Raft, Paxos) or saga pattern |

### Transaction API (Conceptual)

```
BEGIN TRANSACTION
  UPDATE accounts/acct-A SET balance = balance - 100 WHERE _version = 5
  UPDATE accounts/acct-B SET balance = balance + 100 WHERE _version = 12
  CREATE ledger-entries { type: "transfer", from: "acct-A", to: "acct-B", amount: 100 }
COMMIT
```

Each operation within the transaction is validated (schema, version) and the entire batch commits atomically. If any operation fails (schema violation, version conflict, constraint violation), none are applied. The audit log records the full transaction as a single auditable unit with a shared transaction ID.

### Consistency Model Summary

| Scenario | Guarantee | Mechanism |
|----------|-----------|-----------|
| Single entity read after write (same client) | Linearizable + read-your-writes | Version tracking, session affinity |
| Single entity read after write (different client, same instance) | Linearizable | Committed state is immediately visible |
| Multi-entity transaction | Serializable | OCC with conflict detection at commit |
| Concurrent writes to same entity | Exactly one wins, others get conflict error with current state | Version-based OCC |
| Long-running read query | Snapshot isolation (P1) or read-committed | Configurable per-query |

---

## 6. Goals and Objectives

### Business Goals

1. **Become the default data layer for internal agentic projects** — beads, DDx document state, daily loop, and future tools all store state in Axon
2. **Prove the agent-native state management category** — demonstrate that purpose-built infrastructure for agents delivers measurable benefits over general-purpose databases
3. **Establish Axon as an open-source project with external adoption** — attract developers building agentic applications who need audit, schema, and sync out of the box

### Success Metrics

| Metric | Target | Measurement Method | Timeline |
|--------|--------|-------------------|----------|
| Internal project integrations | 3+ projects (beads, DDx, daily loop) | Direct integration count | 6 months |
| Schema enforcement | 100% of production collections validated | Schema validation pass rate | V1 launch |
| Audit completeness | 100% of mutations audited | Audit log gap detection | V1 launch |
| API latency (p99) | <10ms for single-entity operations | Benchmark suite | V1 launch |
| External early adopters | 10+ projects | GitHub stars, integration PRs | 12 months |
| Agent framework integrations | 3+ (beads, LangChain state, CrewAI state) | Published integration packages | 18 months |

### Non-Goals

- **Replacing analytical databases** — Axon is transactional, not analytical. Niflheim handles analytics
- **Building a full ORM** — Axon provides a data API, not an object-relational mapper
- **Supporting arbitrary SQL** — Axon has a structured query interface, not a general SQL engine
- **Multi-region replication in V1** — local-first sync is in scope; geo-distributed clusters are not
- **GUI/dashboard in V1** — API-first; admin UIs come later

Deferred items tracked in `docs/helix/parking-lot.md`.

---

## 7. Users and Personas

### Primary Persona: Agent Developer ("Ava")

**Role**: Software engineer building agentic applications
**Background**: Experienced developer working with AI agent frameworks. Ships code daily with AI assistance. Comfortable with APIs and CLIs, not interested in database administration.

**Goals**:
- Store agent work items (beads) with full lifecycle tracking
- Query historical state to debug agent behavior
- Run agents locally during development, deploy to cloud in production — same data layer

**Pain Points**:
- Agents corrupt state because there's no schema validation
- Can't trace what an agent did because there's no audit trail
- Different storage in dev (SQLite) vs prod (Postgres) causes bugs
- Building audit/schema/sync from scratch for every project

**Needs**:
- Transactional API that agents can call without hand-holding
- Schema that prevents garbage writes
- Audit log that answers "what happened and why?"
- Same API locally and in the cloud

### Secondary Persona: Workflow Builder ("Wei")

**Role**: Developer or technical lead building internal business tools
**Background**: Building approval workflows, document management, time tracking. Wants structured state with lifecycle management, not another spreadsheet or Notion database.

**Goals**:
- Model business processes as state machines with enforced transitions
- Maintain full audit trail for compliance
- Build UIs that work offline and sync when connected

**Pain Points**:
- Business state lives in spreadsheets with no audit trail
- Custom CRUD apps require building audit, auth, and workflow from scratch
- Existing BaaS platforms don't support state machine semantics

**Needs**:
- Collections with lifecycle states and transition guards
- Complete audit trail for compliance review
- Local-first sync for offline-capable UIs

---

## 8. Requirements Overview

### Must Have (P0) — V1

1. **Entity model** — entities are deeply nested, schema-validated structures. First-class objects with identity, version, and metadata
2. **Link model** — typed, directional relationships between entities. Links are first-class audited objects with link-types defined in schema
3. **Collections** — named, schema-bound containers grouping entities of a like kind
4. **Schema engine** — define entity and link schemas; validate all writes; reject invalid data
5. **Audit log** — immutable, append-only log of every mutation with actor, timestamp, operation, before/after state
6. **Entity operations** — create, read, update, delete, list, query with filtering, sorting, pagination
7. **Link operations** — create, traverse, query, delete links between entities
8. **ACID transactions** — multi-entity/link atomic operations with serializable isolation. All-or-nothing commits
9. **Optimistic concurrency** — version-based conflict detection. Stale writes are rejected with current state
10. **API surface** — well-defined API (gRPC or HTTP) suitable for agent and human consumption
11. **Embedded mode** — run Axon in-process for development and testing
12. **CLI** — command-line tool for collection management, schema operations, data inspection, and audit queries

### Should Have (P1)

1. **Schema evolution** — add/remove/modify fields with migration support
2. **Change feeds** — subscribe to collection changes in real-time
3. **Aggregation queries** — COUNT, SUM, AVG, GROUP BY across entities in a collection
4. **Graph traversal queries** — follow typed links with depth limits, filters, and path queries
5. **Server mode** — run Axon as a standalone service with network API
6. **Authentication/authorization** — API keys, per-collection permissions
7. **Bead storage adapter** — purpose-built entity/link schemas and API for beads lifecycle

### Nice to Have (P2)

1. **Local-first sync** — CRDTs or OT for offline-capable clients with conflict resolution
2. **Workflow primitives** — state machine definitions, transition guards, lifecycle hooks
3. **Schema registry** — shared schema definitions across Axon instances
4. **Multi-tenancy** — database-level isolation for SaaS deployment
5. **Niflheim bridge** — CDC export from Axon collections to niflheim for analytics
6. **Tablespec/UMF integration** — import schemas from UMF format
7. **Plugin system** — custom validators, transformers, and hooks

---

## 9. User Journey

### Primary Flow: Agent Storing a Bead

1. **Setup**: Developer defines a `beads` collection with a schema describing bead structure (id, type, status, content, dependencies, metadata)
2. **Create**: Agent creates a new bead document via API. Axon validates against schema, assigns version, writes audit record
3. **Update**: Agent updates bead status from `pending` to `in_progress`. Axon validates transition, bumps version, writes audit record with before/after diff
4. **Query**: Agent queries for all beads with `status=pending` and `type=code-review`. Axon returns matching documents
5. **Audit**: Developer inspects audit log to understand agent behavior: "show me all mutations to bead X in the last hour"
6. **Revert**: Developer reverts bead X to a previous state using audit log reference

### Alternative Flows

- **Schema violation**: Agent attempts to write a bead with missing required field. Axon rejects with structured error describing the violation. Agent can self-correct
- **Concurrent write**: Two agents update the same bead simultaneously. Second write fails with version conflict. Agent retries with fresh state
- **Bulk operation**: Agent completes a batch of beads atomically — all succeed or none do

---

## 10. Constraints and Assumptions

### Constraints

- **Technical**: Must support embedded mode (in-process, no external dependencies) for development. Server mode for production. Same API for both
- **Performance**: Single-document operations under 10ms p99. Audit log writes must not significantly degrade mutation throughput
- **Storage**: Backing storage is an implementation detail. V1 may use SQLite (embedded) and Postgres (server), but the API must not leak storage semantics
- **Compatibility**: Must integrate with Go and Rust ecosystems (internal projects use both). TypeScript client for UI consumption

### Assumptions

- Agentic applications will increasingly need structured, auditable state management — this is not a niche requirement
- Developers will accept defining schemas upfront if the payoff (validation, migration, documentation) is obvious
- Local-first sync can be deferred to P2 without losing early adopters, but it's essential for Year 1 vision
- The beads ecosystem provides enough immediate demand to validate V1

### Dependencies

- **tablespec** — UMF schema format for potential schema interchange (P2)
- **niflheim** — potential analytical backend for CDC export (P2)
- **DDx** — bead tracker provides the first production use case
- **beads** (steveyegge/beads) — bead data model informs collection schema design

---

## 11. Risks and Mitigation

| Risk | Probability | Impact | Mitigation Strategy |
|------|------------|--------|-------------------|
| Schema system too rigid — developers find it burdensome | Medium | High | Allow "flexible" schema zones (like JSON columns). Study what makes Firebase/Supabase feel easy and preserve that ergonomics |
| Audit log storage grows unbounded | Medium | Medium | Configurable retention policies. Tiered storage (hot/warm/cold). Audit compaction for old records |
| Performance overhead from audit-on-every-write | Low | High | Audit writes are append-only and can be async/batched. Benchmark early, optimize the critical path |
| Competing with established BaaS platforms | High | Medium | Don't compete on their strengths (UI, hosted). Compete on agent-native affordances they can't easily add |
| Scope creep from workflow/sync/multi-tenancy | High | High | Strict V1 scope. P0 features only. Parking lot for everything else. Prove core value before expanding |
| Language choice limits adoption | Medium | Medium | Provide client SDKs in Go, Rust, TypeScript, Python. Server language is internal — API is what matters |
| Entity-graph-relational model is unfamiliar | Medium | High | Provide clear documentation, examples, and migration guides from document/relational models. The model must feel natural, not academic |
| Transaction overhead on every write | Medium | Medium | OCC has minimal overhead for low-contention workloads (typical for agents). Benchmark early. Single-entity fast path bypasses transaction machinery |
| Name collision with Axon Framework (Java CQRS) | Low | Medium | Different domain (Java enterprise vs agent-native). SEO and naming will need attention |

---

## 12. Timeline and Milestones

### Phase 1: Foundation (8 weeks)

- Entity-graph-relational data model and storage abstraction
- Entity and link CRUD operations
- Schema engine with validation for entities and link-types
- Audit log architecture (append-only, immutable)
- ACID transactions with OCC and serializable isolation
- Embedded mode working end-to-end
- CLI for basic operations

### Phase 2: API and Integration (6 weeks)

- Server mode with network API (gRPC + HTTP gateway)
- Query/filter/sort/paginate across collections
- Graph traversal queries over typed links
- Bead storage adapter (entity + link schemas)
- Go and TypeScript client SDKs

### Phase 3: Production Readiness (4 weeks)

- Authentication and authorization
- Change feeds
- Batch operations
- Schema evolution/migration
- Performance benchmarking and optimization
- Documentation

### Key Milestones

- **Week 4**: Embedded Axon storing and querying documents with schema validation
- **Week 8**: Audit log queryable; CLI operational
- **Week 14**: Server mode with API; bead adapter working
- **Week 18**: Production-ready V1; internal projects migrating

---

## 13. Success Criteria

### Definition of Done

- [ ] All P0 requirements implemented and tested
- [ ] Audit log captures 100% of mutations with actor, timestamp, operation, diff
- [ ] Schema validation rejects all invalid writes with structured errors
- [ ] Embedded and server modes pass identical test suites
- [ ] API latency targets met (p99 <10ms single-doc operations)
- [ ] CLI supports collection management, schema operations, data inspection, audit queries

### Launch Criteria

- [ ] At least one internal project (beads) successfully using Axon as primary data store
- [ ] Documentation covers getting started, schema definition, API reference, audit queries
- [ ] Benchmark suite demonstrates performance characteristics
- [ ] No known data corruption or audit gap bugs

---

## Appendices

### A. Competitive Analysis

| Capability | Axon | Firebase | Supabase | PocketBase | DoltDB | Turso |
|------------|:----:|:--------:|:--------:|:----------:|:------:|:-----:|
| **Audit-first architecture** | Core | No | Partial (pg audit) | No | Git log (different) | No |
| **Schema enforcement** | Core | No (schemaless) | SQL schemas | Basic | SQL schemas | SQL schemas |
| **Agent-native API** | Core | No | No | No | No | No |
| **Embedded mode** | Yes | No | No | Yes | No | Yes (libSQL) |
| **Server mode** | Yes | Managed only | Self-host or managed | Yes | Yes | Yes |
| **Local-first sync** | P2 | Offline persistence | No | No | No | Embedded replicas |
| **Change feeds** | P1 | Yes | Yes (realtime) | No | No | No |
| **Version control** | Via audit log | No | No | No | Git-style | No |
| **Open source** | Yes | No | Yes | Yes | Yes | Yes |

### B. Technical Feasibility

Core architecture is well-understood:
- **Storage abstraction**: Proven pattern (niflheim uses similar layering). SQLite for embedded, Postgres for server
- **Schema engine**: JSON Schema or similar. Tablespec's UMF provides prior art for schema-to-multi-format generation
- **Audit log**: Append-only event log. Can leverage niflheim's WAL patterns for performance
- **API**: gRPC with HTTP gateway is standard. Protobuf schema = self-documenting API
- **Embedded mode**: SQLite in-process. Well-understood, battle-tested

### C. Prior Art from Internal Projects

- **niflheim**: Storage engine patterns, WAL, Delta tables, partition-centric design
- **tablespec**: UMF schema format, multi-format schema generation, type mapping system
- **DDx bead tracker**: Bead data model, DAG dependencies, ready queue, import/export
- **beads (steveyegge)**: Bead lifecycle, wisp/molecule/formula patterns, gate workflows

---

*This PRD is a living document and will be updated as we learn more.*
