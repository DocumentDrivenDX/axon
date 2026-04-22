# Product Vision: Axon

**Date**: 2026-04-04
**Revised**: 2026-04-22
**Author**: Erik LaBianca
**Status**: Draft

## Mission Statement

Axon is an **entity-first online transaction processing (OLTP) store** designed
for governed agent writes to real business state. It provides a unified,
schema-driven interface for storing, querying, validating, approving, and
auditing structured entities with graph relationships - serving as the durable,
reviewable state layer for agentic applications and business workflows.

## Core Thesis

In an agentic world, the transactional data layer must be:

- **Entity-aware**: Model the world as humans and agents think about it — people, invoices, projects, tasks — not as rows in tables.
- **Schema-driven and declarative**: All validation rules, relationship constraints, workflow states, and indexing directives live in the schema, not scattered across API layers, UI code, and database triggers.
- **Policy-governed**: Agents can discover what they may do, preview risky
  mutations, route high-risk writes for approval, and commit only under the
  active schema and policy version.
- **Auditable by default**: Every change is tracked. Rollbacks are possible. There is no ambiguity about who changed what and when.
- **Agent-accessible**: Agents interact with Axon via MCP, backed by the same
  GraphQL policy and mutation semantics used by human and UI clients.

## Vision

Every agentic application that changes durable business records has an Axon -
the place where structured state is created, queried, approved, audited, and
trusted by both agents and humans. Axon is the standard infrastructure layer for
governed agent state management: what Firebase was for mobile apps, but
audit-first, schema-first, policy-aware, and agent-native.

### Agent-First, Human-Friendly

Axon is designed for a world where agents are primary consumers of business data. But it must also work well for humans:

- The GraphQL interface is the primary application API for reads, writes,
  approvals, policy explanation, and audit.
- The MCP interface is natural for agents and mirrors the GraphQL semantics.
- JSON/REST endpoints are fallback and operational compatibility surfaces, not
  the product center of gravity.
- The schema is readable by all three audiences.

### What Axon Is Not

- Axon is **not an analytics engine**. It is OLTP, not OLAP. For analytics, consume Axon's CDC stream in an analytics system (e.g., niflheim).
- Axon is **not a general-purpose database**. It is an entity store with opinions about how data should be structured, validated, and audited.
- Axon is **not a distributed database** (currently). An Axon deployment is a single `axon-server` instance (or a small cluster fronting one backing store). Distributed coordination (Raft, Paxos) is explicitly out of scope. A single deployment hosts **many tenants**, each of which owns multiple databases — see ADR-018 for the tenant/database model.
- Axon is **not a durable workflow engine**. Temporal, Restate, Inngest,
  DBOS, and LangGraph can orchestrate long-running execution. Axon governs the
  durable business records those workflows read and mutate.
- Axon is **not a REST-first BaaS**. REST compatibility can exist, but the
  primary application surface is GraphQL and the agent-native surface is MCP.

### Stability Status

Axon is **pre-release**. The wire protocol, data model, and SDK surface are not yet frozen. Breaking changes may land in any minor version until v1.0 without a deprecation period. Production use is at the operator's own risk. Consumers building long-lived integrations should pin a specific version and track the CHANGELOG for API-impacting changes.

## Target Market

| Attribute | Primary: Governed Agent Application Developers | Secondary: Business Workflow Builders |
|-----------|-------------------------------------|---------------------------------------|
| Who | Developers building agents that read and mutate durable business records: procurement, invoicing, compliance, customer operations, and internal automation | Teams building internal tools: approval workflows, invoice processing, document management, time tracking |
| Size | Rapidly growing — millions of developers adopting AI agent frameworks | Millions of teams using low-code/internal-tool platforms |
| Pain | Agent state and business records are scattered across files, databases, and APIs with no unified audit trail, no policy envelope, no preview/approval path, and no transactional guarantees | Business state lives in spreadsheets, email threads, and siloed SaaS tools. No unified audit trail, no programmable state transitions, no agent-friendly API |
| Current Solution | Postgres plus RLS/triggers/Hasura/OpenFGA/Cerbos, Firebase/Supabase with custom audit code, or ad-hoc wrappers around existing systems | Spreadsheets, Airtable, Notion databases, custom CRUD apps, workflow tools that don't expose governed data programmatically |

### Beyond Agent Platforms

While Axon was conceived for the agentic application space, its design is general-purpose for any domain with entities, relationships, workflows, and validation rules:

- **ERP systems** (e.g., Nexic, Apogee) built on Axon as application substrate.
- **CDP use cases** — customer data platforms with entity resolution and graph relationships.
- **Artifact management** — the Helix artifact graph (vision → PRD → specs → ADRs) could be stored in Axon.
- **Any domain** where structured, auditable, schema-validated entities with relationships are the core abstraction.

## Commercial Model

- **BYOC (Bring Your Own Cloud)**: Primary commercial offering. Customer runs Axon in their infrastructure. Central control plane provides management and monitoring. Customer retains data sovereignty.
- **License**: Source-available with time-delayed transition to Apache. Hosting or operating Axon as a service requires a commercial license.
- **GitHub Organization**: Separate from DocumentDrivenDX — Axon is a standalone product.

---

*Vision approved by: [Pending]*
