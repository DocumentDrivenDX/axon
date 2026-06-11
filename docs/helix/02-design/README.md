# Phase 2: Design

**Project**: Axon
**Last Updated**: 2026-06-10

## Overview

Design artifacts capturing architecture decisions, normative interface
contracts, and technical spikes.

## Contents

### Architecture Decision Records

- [ADR-001: Rust as Implementation Language](adr/ADR-001-implementation-language.md)
- [ADR-002: Schema Format — JSON Schema + Link-Type Definitions](adr/ADR-002-schema-format.md)
- [ADR-003: Backing Store Architecture — SQLite + PostgreSQL with Application-Layer Audit](adr/ADR-003-backing-store-architecture.md)
- [ADR-004: Transaction Model — Optimistic Concurrency Control](adr/ADR-004-transaction-model.md)
- [ADR-005: Authentication via Tailscale LocalAPI](adr/ADR-005-authentication-tailscale-localapi.md)
- [ADR-006: Admin UI — SvelteKit + Bun + Vite](adr/ADR-006-admin-ui-sveltekit-bun.md)
- [ADR-007: Schema Versioning and Link Type Validation](adr/ADR-007-schema-versioning.md)
- [ADR-008: Lifecycle State Machines as Schema Declarations](adr/ADR-008-schema-lifecycles.md)
- [ADR-009: JSON Merge Patch and Optional ID Generation](adr/ADR-009-patch-and-id-generation.md)
- [ADR-010: Physical Storage Schema and Secondary Indexes](adr/ADR-010-physical-storage-and-secondary-indexes.md)
- [ADR-011: Multi-Tenancy, Namespace Hierarchy, and Node Topology](adr/ADR-011-multi-tenancy-and-namespace-hierarchy.md) (amended by ADR-018)
- [ADR-012: GraphQL Query Layer (Auto-Generated from ESF)](adr/ADR-012-graphql-query-layer.md)
- [ADR-013: MCP Server (Model Context Protocol)](adr/ADR-013-mcp-server.md)
- [ADR-014: Change Feeds — Debezium-Compatible CDC with Kafka and Schema Registry](adr/ADR-014-change-feeds-debezium-cdc.md)
- [ADR-015: Rollback and Recovery — Compensating Transaction Semantics](adr/ADR-015-rollback-recovery.md)
- [ADR-016: Agent Guardrails — Rate Limiting and Scope Constraints](adr/ADR-016-agent-guardrails.md) (rate-limiter algorithm superseded by ADR-024)
- [ADR-017: Control Plane Topology and BYOC Deployment Model](adr/ADR-017-control-plane.md) (tenant model and route prefix superseded by ADR-018)
- [ADR-018: Tenant as Global Account Boundary, M:N Users, JWT Credentials, and Path-Based Wire Protocol](adr/ADR-018-tenant-user-credential-model.md)
- [ADR-019: Policy Authoring and Mutation Intents](adr/ADR-019-policy-authoring-and-intents.md)
- [ADR-020: Data Model — Document-Shaped Entities, Not Native RDF](adr/ADR-020-data-model-document-vs-rdf.md)
- [ADR-021: Graph Query Language — openCypher Subset](adr/ADR-021-graph-query-language.md)
- [ADR-022: Create Semantics — Storage Upsert, Strict Create at Typed Surfaces](adr/ADR-022-create-semantics.md)
- [ADR-023: Preview-Record Audit Threading](adr/ADR-023-preview-audit-threading.md)
- [ADR-024: Rate Limiting Semantics — Per-Actor Sliding Window](adr/ADR-024-rate-limiting-semantics.md)

### Contracts

Normative shared interface surface lives exclusively in the contract suite:

- [CONTRACT-001: HTTP API Surface](contracts/CONTRACT-001-http-api-surface.md)
- [CONTRACT-002: GraphQL Surface](contracts/CONTRACT-002-graphql-surface.md)
- [CONTRACT-003: MCP Surface](contracts/CONTRACT-003-mcp-surface.md)
- [CONTRACT-004: Policy Grammar](contracts/CONTRACT-004-policy-grammar.md)
- [CONTRACT-005: Audit Record](contracts/CONTRACT-005-audit-record.md)
- [CONTRACT-006: CDC Envelope](contracts/CONTRACT-006-cdc-envelope.md)
- [CONTRACT-007: Cypher Query Surface](contracts/CONTRACT-007-cypher-query-surface.md)
- [CONTRACT-008: CLI and Config](contracts/CONTRACT-008-cli-and-config.md)
- [CONTRACT-009: SDK Surface](contracts/CONTRACT-009-sdk-surface.md)
- [CONTRACT-010: ESF Schema Format](contracts/CONTRACT-010-esf-schema-format.md)

[`api-contracts.md`](api-contracts.md) is **deprecated as an authoritative
source**; it remains only as an index mapping its former sections to their
normative homes in the contract suite above.

### Technical Spikes

- [SPIKE-001: Backing Store Evaluation](spikes/SPIKE-001-backing-store-evaluation.md) — Completed (closed as overtaken; ADR-003 accepted on the spike's literature analysis, benchmarks never executed)

## Conventions

- ADRs are numbered sequentially: `ADR-XXX-short-name.md` — one decision per
  file; accepted decisions are never edited in place (annotate, amend, or
  supersede with a new ADR)
- Contracts are numbered sequentially: `CONTRACT-XXX-short-name.md` — the sole
  home of exact shared interface surface; ADRs carry decision-time records
  only
- Spikes are numbered sequentially: `SPIKE-XXX-short-name.md`
- ADRs trace back to PRD requirements and feature specs
