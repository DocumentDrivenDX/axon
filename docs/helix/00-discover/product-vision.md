# Product Vision: Axon

**Date**: 2026-04-04
**Author**: Erik LaBianca
**Status**: Draft

## Mission Statement

Axon provides a cloud-native, auditable, schema-first transactional data store built on an entity-graph-relational data model — serving as the central nervous system for agentic applications and business workflows. Entities are the atoms; typed links are the bonds; transactions, audit trails, and schemas are the guarantees. Agents and humans share a trustworthy substrate for structured, interconnected state.

## Vision

Every agentic application has an Axon — the place where structured state is created, audited, queried, and trusted by both agents and humans. Axon is the standard infrastructure layer for agent state management: what Firebase was for mobile apps, but audit-first, schema-first, and agent-native.

## Target Market

| Attribute | Primary: Agent-Platform Developers | Secondary: Business Workflow Builders |
|-----------|-------------------------------------|---------------------------------------|
| Who | Developers building agentic applications (coding agents, research agents, automation agents) | Teams building internal tools: approval workflows, invoice processing, document management, time tracking |
| Size | Rapidly growing — millions of developers adopting AI agent frameworks | Millions of teams using low-code/internal-tool platforms |
| Pain | Agent state is scattered across files, databases, and APIs with no audit trail, no schema enforcement, and no transactional guarantees. Agents lose context, corrupt state, and produce unverifiable results | Business state lives in spreadsheets, email threads, and siloed SaaS tools. No unified audit trail, no programmable state transitions, no agent-friendly API |
| Current Solution | Ad-hoc SQLite files, JSON on disk, Firebase/Supabase with no agent-specific affordances, custom wrappers around Postgres | Spreadsheets, Airtable, Notion databases, custom CRUD apps, workflow tools that don't expose data programmatically |

## Data Model: Entity-Graph-Relational

Axon's core data model is **entity-graph-relational** — a hybrid that combines the strengths of document stores, graph databases, and relational systems:

- **Entities** are the primary data objects. They are deeply nested, schema-validated structures with low cardinality at each level. An entity might be a customer, a bead, an invoice, or a document — rich enough to be self-contained, structured enough to be queryable
- **Links** are typed, directional relationships between entities. A link has a source entity, a target entity, and a link-type (e.g., "is-ancestor-of", "depends-on", "authored-by", "approves"). Links are first-class objects with their own metadata, audit trail, and optional schemas
- **Collections** group entities of a like kind, enabling SQL-like queries and aggregations (filter, sort, group, count, sum) across entities sharing a schema — at moderate scale, not warehouse scale
- **Transactions** provide ACID guarantees across entities and links — debit account A and credit account B atomically, with serializable isolation and optimistic concurrency

This model avoids the false choice between documents (rich but isolated), graphs (connected but schema-loose), and tables (structured but flat). Entities give you depth, links give you connections, schemas give you type safety, and transactions give you correctness.

## Key Value Propositions

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

## Success Definition

| Metric | Target | Timeline | Measurement |
|--------|--------|----------|-------------|
| Internal projects using Axon as primary data store | 3+ (beads, DDx document state, daily loop) | Year 1 | Project integration count |
| Collections with enforced schemas | 100% of production collections | Year 1 | Schema validation pass rate |
| Audit log coverage | 100% of mutations audited | Year 1 | Audit completeness metric |
| External early adopters | 10+ projects | Year 1 | GitHub stars, integration PRs |
| Agent framework integrations | 3+ (beads, LangChain state, CrewAI state) | Year 2 | Published integration packages |

## Strategic Fit

- **Ecosystem alignment**: Axon complements niflheim (high-performance analytics/entity resolution), tablespec (schema definitions), and DDx (document management). Axon is the transactional layer; niflheim is the analytical layer. Tablespec's UMF format informs Axon's schema system
- **Resource availability**: Builds on proven patterns from niflheim's storage engine and DDx's bead tracker. Rust or Go implementation leveraging existing team expertise
- **Timing**: The agent era is creating massive demand for structured, auditable state management. Current solutions (Firebase, Supabase, PocketBase) were built for mobile/web apps, not agents. DoltDB and Turso add versioning but lack agent-native APIs and audit-first design. The window to define the category is open now

## Principles

1. **Audit is not optional** — every write is an audit event. The audit log is not a feature; it's the architecture
2. **Entities and links are the model** — the world is things and relationships. Axon models both as first-class, typed, audited objects
3. **Transactions mean transactions** — ACID semantics for multi-entity operations. If it can be partially applied, it's not a transaction
4. **Schema earns its keep** — schemas must provide enough value (validation, migration, documentation, query optimization) that defining them is obviously worthwhile
5. **Cloud-native means location-transparent** — same API whether embedded, self-hosted, or managed. Storage is an implementation detail
6. **Agents are first-class citizens** — API design optimizes for programmatic consumption, not human UI patterns
7. **Local-first is a requirement, not a feature** — offline operation and sync are core, not bolt-on
8. **Simplicity over flexibility** — a well-lit path for common patterns beats maximum configurability. Convention over configuration where possible

---

*Vision approved by: [Pending]*
