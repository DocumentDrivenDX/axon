# Product Vision: Axon

**Date**: 2026-04-04
**Revised**: 2026-04-06
**Author**: Erik LaBianca
**Status**: Draft

## Mission Statement

Axon is an **entity-first online transaction processing (OLTP) store** designed for real-time human and agentic workflows. It provides a unified, schema-driven interface for storing, querying, validating, and auditing structured entities with graph relationships — serving as the central nervous system for agentic applications and business workflows.

## Core Thesis

In an agentic world, the transactional data layer must be:

- **Entity-aware**: Model the world as humans and agents think about it — people, invoices, projects, tasks — not as rows in tables.
- **Schema-driven and declarative**: All validation rules, relationship constraints, workflow states, and indexing directives live in the schema, not scattered across API layers, UI code, and database triggers.
- **Auditable by default**: Every change is tracked. Rollbacks are possible. There is no ambiguity about who changed what and when.
- **Agent-accessible**: Agents interact with Axon via MCP, GraphQL, or JSON APIs and can understand what's valid before they try to write.

## Vision

Every agentic application has an Axon — the place where structured state is created, audited, queried, and trusted by both agents and humans. Axon is the standard infrastructure layer for agent state management: what Firebase was for mobile apps, but audit-first, schema-first, and agent-native.

### Agent-First, Human-Friendly

Axon is designed for a world where agents are primary consumers of business data. But it must also work well for humans:

- The GraphQL interface is natural for frontend developers.
- The JSON API is natural for backend developers.
- The MCP interface is natural for agents.
- The schema is readable by all three audiences.

### What Axon Is Not

- Axon is **not an analytics engine**. It is OLTP, not OLAP. For analytics, consume Axon's CDC stream in an analytics system (e.g., niflheim).
- Axon is **not a general-purpose database**. It is an entity store with opinions about how data should be structured, validated, and audited.
- Axon is **not a distributed database** (currently). Single-tenant instances with a lightweight control plane. Distributed coordination (Raft, Paxos) is explicitly out of scope.

## Target Market

| Attribute | Primary: Agent-Platform Developers | Secondary: Business Workflow Builders |
|-----------|-------------------------------------|---------------------------------------|
| Who | Developers building agentic applications (coding agents, research agents, automation agents) | Teams building internal tools: approval workflows, invoice processing, document management, time tracking |
| Size | Rapidly growing — millions of developers adopting AI agent frameworks | Millions of teams using low-code/internal-tool platforms |
| Pain | Agent state is scattered across files, databases, and APIs with no audit trail, no schema enforcement, and no transactional guarantees. Agents lose context, corrupt state, and produce unverifiable results | Business state lives in spreadsheets, email threads, and siloed SaaS tools. No unified audit trail, no programmable state transitions, no agent-friendly API |
| Current Solution | Ad-hoc SQLite files, JSON on disk, Firebase/Supabase with no agent-specific affordances, custom wrappers around Postgres | Spreadsheets, Airtable, Notion databases, custom CRUD apps, workflow tools that don't expose data programmatically |

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
