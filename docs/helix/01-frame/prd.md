---
ddx:
  id: helix.prd
  depends_on: []
---
# Product Requirements Document: Axon

**Version**: 0.2.0
**Date**: 2026-04-04
**Revised**: 2026-04-22
**Status**: Draft
**Author**: Erik LaBianca

## Executive Summary

Axon is an entity-first online transaction processing (OLTP) store designed for
real-time human and agentic workflows. It provides a unified, schema-driven
interface for storing, querying, validating, approving, and auditing structured
entities with graph relationships.

Axon combines entity richness (document stores), relationship modeling (graph databases), transactional correctness (relational databases), and audit-first design (unique value). Entity data is stored opaquely as JSON blobs; secondary indexes use an Entity-Attribute-Value (EAV) pattern for uniform, schema-agnostic query acceleration across all entity types. Users push JSON, get JSON back, and work with the schema — the EAV indexing strategy is an implementation detail.

Target users are developers building agents that safely mutate durable business
records and teams building business workflows that need approval, audit, and
policy. Axon also serves as an application substrate for domain-specific
systems (ERP, CDP, artifact management). V1 focuses on the core entity-graph
data model, ACID transactions, audit system, schema engine, GraphQL/MCP
surfaces, data-layer policies, and mutation preview/approval - enough to prove
the governed-agent-write value proposition.

---

## 1. Key Value Propositions

| # | Value Proposition | Customer Benefit |
|---|-------------------|------------------|
| 1 | **Entity-graph-relational model** — entities + typed links + SQL-like queries in one system | Model real-world relationships (dependencies, hierarchies, ownership) without joining across systems or sacrificing document richness |
| 2 | **Audit-first architecture** — every mutation produces an immutable, queryable audit record | Full provenance chain: who/what changed what, when, and why. Agents and humans can verify, revert, and reason about state history |
| 3 | **Schema-first collections** — define structure upfront, get validation, migration, and documentation for free | Agents work with well-typed data instead of guessing at shapes. Schema evolution is managed, not hoped for |
| 4 | **ACID transactions** — multi-entity atomic operations with optimistic concurrency | Debit A and credit B atomically. Read-your-writes guarantee. No silent overwrites from stale state |
| 5 | **Cloud-native abstraction** — Axon is Axon regardless of backing storage | Developers don't couple to storage engines. Axon can run embedded, on a server, or as a managed service — same API |
| 6 | **GraphQL-primary application surface** — generated read/write GraphQL with policy-aware traversal, pagination, and mutation intents | UI, SDK, and operator workflows use one expressive API for data, approval, and audit |
| 7 | **Agent-native MCP surface** — generated tools/resources with the same policy model as GraphQL | Agents discover valid writes, policy envelopes, conflicts, and approval requirements before corrupting state |
| 8 | **Governed mutation intents** — preview, explain, approve, and transactionally bind risky writes | Low-risk agent work can proceed autonomously while high-risk writes route to human approval |
| 9 | **Workflow primitives** — entity state machines and transition guards built into the collection layer | Business processes (invoice approval, document review, bead lifecycle) get first-class state guards without Axon becoming a durable workflow engine |

---

## 2. Strategic Fit

- **Ecosystem alignment**: Axon complements niflheim (high-performance analytics/entity resolution), tablespec (schema definitions), and DDx (document management). Axon is the transactional layer; niflheim is the analytical layer. Tablespec's UMF format informs Axon's schema system
- **Application substrate**: Axon is designed to serve as the backend for lightweight domain applications (ERP, CDP, artifact management). Auto-generated TypeScript clients, admin UIs, and trivial deployment to Cloud Run or Workers lower the barrier to shipping Axon-backed apps
- **Timing**: The agent era is creating massive demand for structured, auditable state management. Current solutions (Firebase, Supabase, PocketBase) were built for mobile/web apps, not agents. DoltDB and Turso add versioning but lack agent-native APIs and audit-first design. The window to define the category is open now

---

## 3. Problem Statement

### The Problem

Agent state management is an unsolved infrastructure problem. As AI agents become central to software development and business processes, they need a place to store, query, and mutate structured state with guarantees that matter:

- **No audit trail**: Agents modify state without provenance. When something goes wrong, there's no way to trace what happened, who/what caused it, or how to revert
- **No schema enforcement**: Agents write malformed data, schemas drift silently, and downstream consumers break on unexpected shapes
- **No transactional guarantees**: Concurrent agent operations produce corrupt or inconsistent state
- **No governed write path**: Agents can often write or fail, but cannot
  preview a diff, explain policy, route risky changes for approval, or bind an
  approval to the reviewed pre-image
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
| **Postgres + RLS + Hasura/PostGraphile + OpenFGA/Oso/Cerbos** | Credible DIY GraphQL and policy stack | Multiple policy models, custom approval plumbing, TOCTOU-prone preview flows, incomplete prompt/tool lineage, hard-to-test redaction and count-leak behavior |
| **Workflow engines (Temporal/Restate/Inngest/DBOS/LangGraph)** | Durable orchestration and agent execution | Not the governed system of record. They still need a schema-first, auditable, policy-aware data store for business entities |
| **JSON files on disk** | Zero setup, agent-accessible | No concurrency, no audit, no schema, no query, no sync. Breaks immediately at scale |

### Opportunity

The agent era is creating a new infrastructure category: **governed
agent-native state management**. No existing product owns this space. The
requirements - audit-first, schema-first, GraphQL-first, MCP-native,
policy-aware, previewable, and approval-routable - are not incremental features
on existing databases. They require purpose-built architecture.

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
| **Isolation** | Snapshot Isolation by default in V1 (see Isolation Levels below). Serializable isolation is P1. |
| **Durability** | Committed transactions survive process restarts. The audit log is the durability mechanism |

### Isolation Levels

The default isolation level in V1 is **snapshot isolation**, implemented via optimistic concurrency control (OCC) with write-set conflict detection. Serializable isolation (preventing write skew) is a P1 post-V1 goal; see the known gap below.

| Level | Guarantee | Prevents | Axon Support |
|-------|-----------|----------|-------------|
| **Serializable** | Transactions behave as if executed one at a time in some serial order. Strongest guarantee. No anomalies possible | Dirty reads, non-repeatable reads, phantom reads, write skew | **P1 — requires read-set tracking not yet implemented**. |
| **Snapshot Isolation (SI)** | Each transaction reads from a consistent snapshot taken at transaction start. Writers don't block readers. Vulnerable to write skew | Dirty reads, non-repeatable reads, phantom reads | **Default in V1 — write-set OCC provides snapshot isolation**. |
| **Read Committed** | Each statement sees only data committed before the statement began. Different statements in the same transaction may see different snapshots | Dirty reads | **Available** as explicit opt-in. Useful for reporting queries that tolerate minor inconsistency |
| **Read Uncommitted** | **Not supported**. Axon will never expose uncommitted data | — | Not supported |

> **V1 known gap: write skew is not prevented.** OCC with write-set conflict detection provides Snapshot Isolation, not Serializability. Write skew — where two concurrent transactions each read disjoint entities and write to each other's read set — is not detected. Read-set tracking is required for full serializability and is deferred to P1.

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
| Multi-entity transaction | Yes | Snapshot Isolation (V1). Atomic batch: update entities A, B, and link L in one transaction. All-or-nothing |
| Cross-collection transaction | Yes | Snapshot Isolation (V1). Debit in `accounts` collection and create record in `ledger` collection — same transaction |
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
| Multi-entity transaction | Snapshot Isolation (V1); Serializable is P1 | OCC with write-set conflict detection at commit |
| Concurrent writes to same entity | Exactly one wins, others get conflict error with current state | Version-based OCC |
| Long-running read query | Snapshot isolation (P1) or read-committed | Configurable per-query |

---

## 6. Goals and Objectives

### Business Goals

1. **Become the default data layer for internal agentic projects** — beads, DDx document state, daily loop, and future tools all store state in Axon
2. **Prove the governed agent-native state management category** — demonstrate that purpose-built infrastructure for agents delivers measurable benefits over general-purpose databases and assembled Postgres policy stacks
3. **Establish Axon as an open-source project with external adoption** — attract developers building agentic applications who need audit, schema, policy, approval, and GraphQL/MCP out of the box

### Success Metrics

| Metric | Target | Measurement Method | Timeline |
|--------|--------|-------------------|----------|
| Internal project integrations | 3+ projects (beads, DDx, daily loop) | Direct integration count | 6 months |
| Schema enforcement | 100% of production collections validated | Schema validation pass rate | V1 launch |
| Audit completeness | 100% of mutations audited | Audit log gap detection | V1 launch |
| API latency (p99) | <10ms for single-entity operations | Benchmark suite | V1 launch |
| Time to first trusted agent write | <1 day for a competent developer | Invoice/procurement tutorial from schema to audited GraphQL/MCP write | V1 launch |
| GraphQL policy correctness | 100% pass on relationship, pagination, redaction, and count-leak test suite | FEAT-015/029 contract tests | V1 launch |
| Approval safety | 100% stale intent rejection for changed pre-image, policy, schema, grant, or operation hash | FEAT-030 contract tests | V1 launch |
| External early adopters | 10+ projects | GitHub stars, integration PRs | 12 months |
| Agent framework integrations | 3+ (beads, LangChain state, CrewAI state) | Published integration packages | 18 months |

### Non-Goals

- **Replacing analytical databases** — Axon is transactional, not analytical. Niflheim handles analytics
- **Building a full ORM** — Axon provides a data API, not an object-relational mapper
- **Supporting arbitrary SQL** — Axon has a structured query interface, not a general SQL engine
- **Being a durable workflow engine** — Axon enforces entity transitions and records approvals, but does not orchestrate long-running execution
- **Being REST-first** — REST can exist as a compatibility surface, but GraphQL and MCP are the product-defining interfaces
- **Multi-region replication in V1** — node topology is designed (ADR-011) and database placement is modeled, but actual multi-node routing and database migration are P2
- **GIN / backend-specific query acceleration** — secondary indexes use the portable EAV pattern, not backend-specific features like JSONB containment operators

Deferred items tracked in `docs/helix/parking-lot.md`.

---

## 7. Users and Personas

### Primary Persona: Agent Developer ("Ava")

**Role**: Software engineer building agentic applications that mutate durable
business records
**Background**: Experienced developer working with AI agent frameworks. Ships
code daily with AI assistance. Comfortable with GraphQL, APIs, and CLIs, not
interested in assembling database triggers, policy engines, approval services,
and audit pipelines.

**Goals**:
- Give agents a governed place to read, preview, and write business state
- Query historical state to debug agent behavior and policy decisions
- Run agents locally during development, deploy to cloud in production — same data layer

**Pain Points**:
- Agents corrupt state because there's no schema validation
- Can't trace what an agent did because there's no audit trail
- Can't approve risky writes without custom, stale-prone application plumbing
- Different storage in dev (SQLite) vs prod (Postgres) causes bugs
- Building audit/schema/policy/approval from scratch for every project

**Needs**:
- GraphQL and MCP APIs that agents can call without hand-holding
- Schema that prevents garbage writes
- Policy envelopes that explain autonomous, approval-routed, and denied writes
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
8. **ACID transactions** — multi-entity/link atomic operations with snapshot isolation (V1). All-or-nothing commits. Serializable isolation is P1.
9. **Optimistic concurrency** — version-based conflict detection. Stale writes are rejected with current state
10. **GraphQL-first API surface** — generated read/write GraphQL suitable for
    UI, SDK, operator, audit, policy, and approval workflows
11. **MCP surface** — generated tools/resources for agents, mirroring GraphQL
    semantics
12. **Data-layer access policies** — schema-declared row, field, relationship,
    and transition policies enforced below GraphQL and MCP
13. **Mutation preview and approval** — preview diffs, explain policy, route
    high-risk writes for approval, and bind execution to the reviewed
    pre-image
14. **Embedded mode** — run Axon in-process for development and testing
15. **CLI** — command-line tool for collection management, schema operations,
    data inspection, policy testing, and audit queries

### Should Have (P1)

1. **Schema evolution** — breaking change detection, compatibility classification, entity revalidation, schema diff. Adding optional fields must be zero-downtime. Adding required fields must require a default value or migration plan. Tightening constraints must validate existing data and report violations without silently corrupting. Schema versions tracked per entity. Migration tooling to scan entities against new schema versions and report/fix violations. Migration declarations deferred to V2. See FEAT-017, ADR-007
2. **Change feeds** — Debezium-compatible CDC records on Kafka topics with Confluent-compatible Schema Registry. Multi-sink: Kafka (production), HTTP SSE (real-time clients), file (debugging/replay). Initial snapshot for bootstrapping consumers. At-least-once delivery with audit_id cursors. Real-time push also via GraphQL subscriptions (FEAT-015). See FEAT-021, ADR-014
3. **Aggregation queries** — COUNT, SUM, AVG, MIN, MAX, GROUP BY across entities in a collection. Accelerated by secondary indexes. Exposed via structured API, GraphQL, and MCP. See FEAT-018
4. **Graph traversal queries** — follow typed links with depth limits, filters, and path queries
5. **Server mode** — run Axon as a standalone service with network API
6. **Authentication, identity, and authorization** — First-class `User` type with stable UUIDs; external identities federate via a `user_identities` table (Tailscale today, OIDC/email tomorrow). Users are M:N with tenants. Credentials are tenant-scoped JWTs carrying a `grants` claim that is checked against the URL path's `(tenant, database, op)` tuple on every request. Grant ops are `{read, write, admin}` and must be ≤ the issuer's role in the tenant. RBAC + ABAC with schema-declared entity-level, row-level, and field-level data policies layer on top of the grants check. See FEAT-012, FEAT-029, and ADR-018 (governing)
7. **Bead storage adapter** — purpose-built entity/link schemas and API for beads lifecycle
8. **Admin web UI** — browser-based console for collection management, entity browsing, schema editing, and audit log inspection. Served by the axon-server binary. See FEAT-011
9. **Secondary indexes** — EAV-pattern typed indexes (string, integer, float, datetime, boolean) declared in schema. Single-field, compound, and unique indexes. Query planner uses indexes for equality, range, and sort acceleration. Background index build for existing collections. See FEAT-013
10. **Tenancy, namespace hierarchy, and path-based addressing** — four-level conceptual hierarchy `tenant → database → schema → collection`, with tenant as a first-class global account boundary that owns users, credentials, and multiple databases. A single `axon-server` deployment hosts many tenants. Pure path-based wire protocol: `/tenants/{tenant}/databases/{database}/{resource...}` for every data-plane route. No `X-Axon-Database` header, no un-prefixed routes. Users are M:N with tenants via `tenant_users(tenant_id, user_id, role)`. Access control grants are carried in JWT credentials and scoped to `(tenant, database, op)` tuples. Default tenant and database auto-bootstrap on first authenticated request of a zero-tenant deployment. See FEAT-014 and ADR-018 (governing)
11. **Physical storage architecture** — numeric collection IDs (O(1) renames), native UUID entity IDs, dedicated links table with DB-enforced referential integrity, portable design across SQL and KV backends. See ADR-010
12. **GraphQL API hardening** — subscriptions, advanced relationship queries,
    admin/control-plane GraphQL, and performance hardening beyond the baseline
    policy-safe V1 path. See FEAT-015, ADR-012
13. **MCP server hardening** — stdio/HTTP transports, subscriptions, prompts,
    tool grouping, and agent ergonomics beyond the baseline GraphQL bridge and
    policy-envelope tools. See FEAT-016, ADR-013
14. **Validation rules** — cross-field conditional validation (ESF Layer 5) with severity levels (error/warning/info). Actionable error messages with fix suggestions and "did you mean?" near-match detection. Enhanced JSON Schema errors. See FEAT-019
15. **Link discovery and graph queries** — fast, indexed queries for finding link targets, listing entity neighbors, and exploring the entity graph. Powers autocomplete, relationship building, and graph exploration. Exposed via structured API, GraphQL relationship fields, and MCP tools. See FEAT-020
16. **Agent guardrails** — preventive controls beyond audit trails. Scope constraints (agent can only modify entities within assigned scope), rate limiting (prevent bulk mutation without approval), delegated authority, credential rotation, and semantic validation hooks only after mutation intents are proven. See FEAT-022, FEAT-030
17. **Rollback and recovery** — point-in-time rollback (undo all changes after a timestamp), entity-level rollback (revert specific entity to previous state), transaction-level rollback (undo a specific transaction), dry-run rollback (show what would change without committing). Powered by the audit log. See FEAT-023
18. **BYOC deployment control plane** — lightweight management plane (PostgreSQL-backed) for fleets of Axon deployments across customer clouds. Observes and provisions `axon-server` deployments — each of which may host many tenants internally (see FEAT-014). Single pane of glass for monitoring, operations, and deployment lifecycle across the fleet. Never reads per-deployment data. Strict separation from the embedded per-deployment control plane that manages tenants/users/credentials inside a single instance (see FEAT-012 + FEAT-014). See FEAT-025 and ADR-017

### Nice to Have (P2)

1. **Local-first sync** — CRDTs or OT for offline-capable clients with conflict resolution
2. **Workflow primitives beyond transition guards** — state machine definitions,
  richer lifecycle hooks, and integration points with external durable workflow
  engines. Axon does not become the workflow orchestrator
3. **Schema registry** — shared schema definitions across Axon instances
4. **Node topology and database migration** — geographic node registry, database-to-node placement, request routing/proxy, database migration between nodes. See FEAT-014 (P2 section)
5. **Niflheim bridge** — CDC export from Axon collections to niflheim for analytics
6. **Tablespec/UMF integration** — import schemas from UMF format
7. **Plugin system** — custom validators, transformers, and hooks
8. **Application substrate** — Axon as a trivially deployable backend for lightweight applications. Cross-cutting template producing Axon-backed apps deployable to Cloud Run, Cloudflare Workers, or similar. Auto-generated TypeScript client from schema. Auto-generated admin UI from schema. See FEAT-024
### Not Scheduled

The following capabilities have been discussed and are architecturally
compatible with Axon but are not prioritized for any release:

- **Cypher subset (read-only)** — graph pattern matching query language. Valuable for complex multi-hop, cycle detection, and cross-link-type queries. Would compile to the same query planner as GraphQL
- **SQL DML** — batch updates, bulk operations, data migration via SQL write syntax. Read-side SQL is better served by CDC → DuckDB
- **Semantic search (vector indexes)** — vector similarity as an ESF index type. Eliminates need for a separate vector store. Would surface as a `near` filter in GraphQL. Architecturally feasible because entity storage and indexing are decoupled
- **Document search (Tantivy)** — full-text search with inverted indexes, BM25 ranking, faceting. Significant integration effort. Same decoupled index architecture applies
- **PostgreSQL-compatible SQL** — relational-style structured queries and batch updates. SQL is well-suited for this; no reason to reinvent it. Deferred until a use case demands it
- **Git backend** — entity model backed by Git for version history, branch-based experimentation, and merge/conflict resolution. Speculative but architecturally interesting for artifact graph use cases

---

## 9. User Journey

### Primary Flow: Trusted Agent Invoice Update

1. **Model**: Developer defines `invoices`, `vendors`, and `users` collections
   with ESF schemas, link types, indexes, transition guards, and
   `access_control` policy.
2. **Policy**: Developer declares an autonomous envelope for invoice updates
   under $10,000 and an approval envelope above that threshold.
3. **Test**: Developer dry-runs the policy against fixture subjects and sample
   mutations. Axon reports GraphQL nullability changes, missing indexes, and
   approval routes before activation.
4. **Discover**: Agent inspects MCP tools or GraphQL introspection and sees
   allowed fields, redactions, autonomous limits, and approval requirements.
5. **Preview**: Agent proposes an invoice update. Axon returns a diff, policy
   explanation, pre-image versions, and either `allow`, `needs_approval`, or
   `deny`.
6. **Approve**: For a high-risk update, a finance approver reviews the GraphQL
   intent, records a reason, and approves.
7. **Commit**: Axon executes only if the entity versions, policy version,
   schema version, grant version, and operation hash still match the preview.
8. **Audit**: Operator queries one audit trail showing agent identity, delegated
   authority, tool call, policy decision, approval, and redacted pre/post images.

### Primary Flow: Agent Storing a Bead

1. **Setup**: Developer defines a `beads` collection with a schema describing bead structure (id, type, status, content, dependencies, metadata)
2. **Create**: Agent creates a new bead entity via API. Axon validates against schema, assigns version, writes audit record
3. **Update**: Agent updates bead status from `pending` to `in_progress`. Axon validates transition, bumps version, writes audit record with before/after diff
4. **Query**: Agent queries for all beads with `status=pending` and `type=code-review`. Axon returns matching entities
5. **Audit**: Developer inspects audit log to understand agent behavior: "show me all mutations to bead X in the last hour"
6. **Revert**: Developer reverts bead X to a previous state using audit log reference

### Alternative Flows

- **Schema violation**: Agent attempts to write a bead with missing required field. Axon rejects with structured error describing the violation. Agent can self-correct
- **Concurrent write**: Two agents update the same bead simultaneously. Second write fails with version conflict. Agent retries with fresh state
- **Bulk operation**: Agent completes a batch of beads atomically — all succeed or none do
- **Approval stale**: A human approves a mutation after the entity changed.
  Axon rejects the intent as stale and requires a new preview

---

## 10. Constraints and Assumptions

### Constraints

- **Technical**: Must support embedded mode (in-process, no external dependencies) for development. Server mode for production. Same API for both
- **Performance**: Single-entity operations under 10ms p99. Audit log writes must not significantly degrade mutation throughput
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
| EAV performance at scale | Medium | High | EAV requires joins for every property access. Mitigated by dedicated index tables, but complex queries (graph traversal, vector search) need careful benchmarking |
| Transactional guarantees across heterogeneous indexes | Low | High | Adding vector/BM25 indexes as separate services creates distributed transaction problems. Keep indexes co-located as long as possible |
| Agent semantic misuse | Medium | Medium | Agents can submit structurally valid but semantically wrong data. Schema validation catches structure, not intent. Agent guardrails (FEAT-022) help but this remains an open problem |
| Backend abstraction leakage | Medium | High | If PostgreSQL-specific behaviors bleed through the API, future backend migration will be painful. Storage adapter trait must be rigorously tested across backends |
| Policy authoring too complex | Medium | High | Use ADR-019's closed declarative grammar, compile reports, fixture tests, and historical dry-runs. Avoid arbitrary code and hidden resolver logic |
| GraphQL policy leaks | Medium | High | Make GraphQL relationship traversal, pagination, redaction, and count safety a P0 contract suite before broad feature expansion |
| Agent identity and delegation unclear | Medium | High | Model stable `user_id`, `agent_id`, `delegated_by`, credential ID, and grant version in every policy decision and audit record |
| Immutable audit conflicts with erasure | Medium | High | Support caller-side redaction, tenant/field encryption, crypto-shredding, and erasure tombstones while preserving non-sensitive lineage |

---

## 12. Timeline and Milestones

### Phase 1: Foundation (8 weeks)

- Entity-graph-relational data model and storage abstraction
- Entity and link CRUD operations
- Schema engine with validation for entities and link-types
- Audit log architecture (append-only, immutable)
- ACID transactions with OCC and snapshot isolation (V1; serializable is P1)
- Embedded mode working end-to-end
- CLI for basic operations

### Phase 2: API and Integration (6 weeks)

- Server mode with GraphQL primary API, MCP server, and compatibility gateways
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

- **Week 4**: Embedded Axon storing and querying entities with schema validation
- **Week 8**: Audit log queryable; CLI operational
- **Week 14**: Server mode with API; bead adapter working
- **Week 18**: Production-ready V1; internal projects migrating

---

## 13. Success Criteria

### Definition of Done

- [ ] All P0 requirements implemented and tested
- [ ] Audit log captures 100% of mutations with actor, timestamp, operation, diff
- [ ] Schema validation rejects all invalid writes with structured errors
- [ ] GraphQL and MCP enforce identical data-layer policy decisions for the same subject and operation
- [ ] Mutation preview/approval rejects stale pre-image, schema, policy, grant, and operation-hash changes
- [ ] Embedded and server modes pass identical test suites
- [ ] API latency targets met (p99 <10ms single-entity operations)
- [ ] CLI supports collection management, schema operations, data inspection, audit queries

### Launch Criteria

- [ ] At least one internal project (beads) successfully using Axon as primary data store
- [ ] Invoice/procurement reference workflow demonstrates time-to-first trusted agent write through GraphQL and MCP
- [ ] Documentation covers getting started, schema definition, API reference, audit queries
- [ ] Benchmark suite demonstrates performance characteristics
- [ ] No known data corruption or audit gap bugs

---

## 14. Organizational and Licensing

- **GitHub organization**: Separate from DocumentDrivenDX. Axon is a standalone product that does not require DDx to be useful. Bundling under the DDx namespace would limit perceived scope and complicate licensing.
- **License**: Source-available with time-delayed transition to Apache. Protects against commoditization (the Elasticsearch/Amazon scenario) while allowing adoption. Hosting or operating Axon as a service requires a commercial license.
- **BYOC model**: Primary commercial offering. Customer runs Axon in their cloud. Central control plane provides management and monitoring. Customer retains data sovereignty. Follows the Redpanda model.

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
- **API**: GraphQL is the primary application surface; MCP is the
  agent-native surface; gRPC/native handler APIs remain internal and SDK
  integration surfaces; REST/JSON is a compatibility fallback
- **Embedded mode**: SQLite in-process. Well-understood, battle-tested

### C. Prior Art from Internal Projects

- **niflheim**: Storage engine patterns, WAL, Delta tables, partition-centric design
- **tablespec**: UMF schema format, multi-format schema generation, type mapping system
- **DDx bead tracker**: Bead data model, DAG dependencies, ready queue, import/export
- **beads (steveyegge)**: Bead lifecycle, wisp/molecule/formula patterns, gate workflows

---

*This PRD is a living document and will be updated as we learn more.*
