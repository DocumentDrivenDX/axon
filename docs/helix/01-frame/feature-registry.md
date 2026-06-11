---
ddx:
  id: helix.feature-registry
  depends_on:
    - helix.prd
---

# Feature Registry

**Project**: Axon
**Status**: Active
**Registry Owner**: erik
**Last Updated**: 2026-06-10

Compact source of truth for feature IDs, names, status, priority, owner,
dependencies, and trace links. Full behavior lives in the feature specs under
[`features/`](features/); requirements live in the [PRD](prd.md).

## Active Features

31 features total: 13 P0, 14 P1, 3 P2, 1 superseded (no priority).
Status mix: 23 draft, 1 in_review, 6 approved, 1 superseded.

| ID | Name | Description | Priority | Status | Owner | Source | Updated |
|----|------|-------------|----------|--------|-------|--------|---------|
| FEAT-001 | Collections | Named, schema-bound entity containers with audited lifecycle and discovery | P0 | draft | erik | [spec](features/FEAT-001-collections.md) | 2026-06-10 |
| FEAT-002 | Schema Engine | Active schema validation of every write with structured, actionable errors | P0 | draft | erik | [spec](features/FEAT-002-schema-engine.md) | 2026-06-10 |
| FEAT-003 | Audit Log | Immutable audit record per mutation with repair-grade provenance and history queries | P0 | draft | erik | [spec](features/FEAT-003-audit-log.md) | 2026-06-10 |
| FEAT-004 | Entity Operations | Schema-validated, version-tracked, audited entity CRUD with stale-write rejection | P0 | draft | erik | [spec](features/FEAT-004-entity-operations.md) | 2026-06-10 |
| FEAT-005 | API Surface | HTTP API and CLI surface through which agents, applications, and humans use Axon | P0 | draft | erik | [spec](features/FEAT-005-api-surface.md) | 2026-06-10 |
| FEAT-006 | Bead Storage Adapter | Opinionated bead (work item) schema, lifecycle, and ready-queue queries; primary dogfooding vehicle | P1 | draft | erik | [spec](features/FEAT-006-bead-storage-adapter.md) | 2026-06-10 |
| FEAT-007 | Entity-Graph Data Model | Deeply nested entities plus typed, directional, first-class links with their own metadata and audit | P0 | draft | erik | [spec](features/FEAT-007-entity-graph-model.md) | 2026-06-10 |
| FEAT-008 | ACID Transactions | Atomic multi-entity/link commits with snapshot isolation and conflict reporting | P0 | draft | erik | [spec](features/FEAT-008-acid-transactions.md) | 2026-06-10 |
| FEAT-009 | Unified Graph Query (Cypher) | Read-only openCypher subset unifying filter, sort, aggregate, traversal, and pattern matching | P0 | approved | erik | [spec](features/FEAT-009-unified-graph-query.md) | 2026-06-10 |
| FEAT-010 | Entity State Machines and Transition Guards | Declared lifecycle states with enforced, guarded transitions on entities | P1 | draft | erik | [spec](features/FEAT-010-entity-state-machines.md) | 2026-06-10 |
| FEAT-011 | Admin Web UI | Browser console for schema management, data inspection, audit, and repair | P1 | approved | erik | [spec](features/FEAT-011-admin-web-ui.md) | 2026-06-10 |
| FEAT-012 | Authentication, Identity, and Authorization | Users, credentials, roles, and Tailscale-based authentication | P1 | approved | erik | [spec](features/FEAT-012-authorization.md) | 2026-06-10 |
| FEAT-013 | Secondary Indexes and Query Acceleration | Declared secondary/compound/unique indexes with background builds | P1 | draft | erik | [spec](features/FEAT-013-secondary-indexes.md) | 2026-06-10 |
| FEAT-014 | Tenancy, Namespace Hierarchy, and Path-Based Addressing | Tenant/database/schema/collection hierarchy with per-tenant isolation | P1 | draft | erik | [spec](features/FEAT-014-multi-tenancy.md) | 2026-06-10 |
| FEAT-015 | GraphQL Query Layer | Read/write GraphQL API auto-generated from ESF schemas, with subscriptions | P0 | draft | erik | [spec](features/FEAT-015-graphql-query-layer.md) | 2026-06-10 |
| FEAT-016 | MCP Server | Model Context Protocol server exposing Axon discovery, CRUD, query, and governed writes to agents | P0 | draft | erik | [spec](features/FEAT-016-mcp-server.md) | 2026-06-10 |
| FEAT-017 | Schema Evolution and Migration | Compatibility classification, breaking-change detection, revalidation, and schema diff | P1 | draft | erik | [spec](features/FEAT-017-schema-evolution.md) | 2026-06-10 |
| FEAT-018 | Aggregation Queries | Summary statistics (count, sum, avg, group-by) over collections, policy-aware | P1 | draft | erik | [spec](features/FEAT-018-aggregation-queries.md) | 2026-06-10 |
| FEAT-019 | Validation Rules and Actionable Errors | Cross-field validation rules, gates, and structured error reporting beyond JSON Schema | P1 | draft | erik | [spec](features/FEAT-019-validation-rules.md) | 2026-06-10 |
| FEAT-020 | Link Discovery and Graph Queries | Superseded — scope folded into FEAT-009 | — | superseded | — | [spec](features/FEAT-020-link-discovery-and-graph-queries.md) | 2026-05-02 |
| FEAT-021 | Change Feeds (CDC) | Durable, replayable Debezium-compatible change feeds | P1 | draft | erik | [spec](features/FEAT-021-change-feeds-cdc.md) | 2026-06-10 |
| FEAT-022 | Agent Guardrails | Preventive scope and rate controls for agent interactions, per agent identity | P1 | draft | erik | [spec](features/FEAT-022-agent-guardrails.md) | 2026-06-10 |
| FEAT-023 | Rollback and Recovery | Audit-powered rollback with dry-run repair plans and conflict detection | P1 | draft | erik | [spec](features/FEAT-023-rollback-recovery.md) | 2026-06-10 |
| FEAT-024 | Application Substrate | Generated typed clients and schema-driven apps backed by Axon | P2 | draft | erik | [spec](features/FEAT-024-application-substrate.md) | 2026-06-10 |
| FEAT-025 | BYOC Deployment Control Plane | Lightweight management plane for multi-deployment Axon fleets | P1 | draft | erik | [spec](features/FEAT-025-control-plane.md) | 2026-06-10 |
| FEAT-026 | Markdown Template Rendering | Per-collection markdown templates rendering entities for operator inspection | P2 | approved | erik | [spec](features/FEAT-026-markdown-templates.md) | 2026-06-10 |
| FEAT-027 | Git Mirror | Read-only projection of collection state into a git repository (parked) | P2 | draft | erik | [spec](features/FEAT-027-git-mirror.md); parked in [parking lot](../parking-lot.md) | 2026-06-10 |
| FEAT-028 | Unified Binary & Service Management | One `axon` binary as CLI + server, with service install and client/embedded modes | P1 | draft | erik | [spec](features/FEAT-028-unified-binary.md) | 2026-06-10 |
| FEAT-029 | Data-Layer Access Control Policies | Schema-declared entity/field read-write policies enforced at the data layer across all surfaces | P0 | approved | erik | [spec](features/FEAT-029-access-control.md) | 2026-06-10 |
| FEAT-030 | Mutation Intents and Approval | Previewable, explainable, approval-routable mutations as the governed write path | P0 | in_review | erik | [spec](features/FEAT-030-mutation-intents-approval.md) | 2026-06-10 |
| FEAT-031 | Policy and Intents Admin UI | Web UI for policy authoring/testing and mutation-intent review and approval | P0 | approved | erik | [spec](features/FEAT-031-policy-intents-admin-ui.md) | 2026-06-10 |

## Status Definitions

Feature specs use this vocabulary (the `**Status**` header field in each spec):

- **draft**: Spec exists; requirements still being refined.
- **in_review**: Spec content complete; under review before approval.
- **approved**: Spec reviewed and accepted as the governing definition.
- **superseded**: Scope folded into another feature; spec retained as a pointer.

Downstream lifecycle (designed, in test, in build, built, deployed) is tracked
through design/test/build artifacts and the issue tracker (`ddx bead`), not in
this registry.

## Dependencies

FEAT-to-FEAT edges from each spec's `ddx.depends_on` frontmatter (ADR/contract
and `helix.prd` dependencies are recorded in the specs themselves).

| Feature | Depends On | Type | Notes |
|---------|------------|------|-------|
| FEAT-001 | FEAT-002, FEAT-003, FEAT-014 | Required | Schema binding, lifecycle audit, namespace placement |
| FEAT-004 | FEAT-001, FEAT-002, FEAT-003 | Required | CRUD over collections with validation and audit |
| FEAT-005 | FEAT-001, FEAT-002, FEAT-003, FEAT-004, FEAT-021, FEAT-023, FEAT-029, FEAT-030 | Required | Surfaces the underlying capabilities |
| FEAT-006 | FEAT-001, FEAT-004, FEAT-005, FEAT-010 | Required | Built entirely on generic primitives |
| FEAT-007 | FEAT-002, FEAT-003 | Required | Link schemas and link audit |
| FEAT-008 | FEAT-003, FEAT-004, FEAT-007 | Required | Transactions span entities and links |
| FEAT-009 | FEAT-002, FEAT-007, FEAT-013, FEAT-015, FEAT-016, FEAT-029 | Required | Query planner over model, indexes, surfaces, policy |
| FEAT-010 | FEAT-007, FEAT-008, FEAT-009, FEAT-019 | Required | Guards evaluate via validation and query primitives |
| FEAT-011 | FEAT-001, FEAT-002, FEAT-004, FEAT-005, FEAT-012, FEAT-015 | Required | Console over API/GraphQL with auth |
| FEAT-012 | FEAT-005, FEAT-014, FEAT-015, FEAT-016, FEAT-025 | Required | Identity spans all surfaces and tenancy |
| FEAT-013 | FEAT-001, FEAT-002, FEAT-004 | Required | Indexes declared in schemas over collections |
| FEAT-014 | FEAT-001, FEAT-012, FEAT-025 | Required | Tenancy scope for collections, users, deployments |
| FEAT-015 | FEAT-002, FEAT-004, FEAT-005, FEAT-009, FEAT-013, FEAT-029, FEAT-030 | Required | Generated from ESF; policy-aware reads, governed writes |
| FEAT-016 | FEAT-004, FEAT-005, FEAT-009, FEAT-013, FEAT-015, FEAT-029, FEAT-030 | Required | MCP leg of the same governed surface |
| FEAT-017 | FEAT-002, FEAT-013 | Required | Evolves schemas; index rebuilds on migration |
| FEAT-018 | FEAT-004, FEAT-009, FEAT-013, FEAT-015, FEAT-016 | Required | Aggregation facet of the unified read model |
| FEAT-019 | FEAT-002, FEAT-004, FEAT-013 | Required | Rules layered on schema validation |
| FEAT-020 | FEAT-009 | Superseded-by | Scope folded into FEAT-009 |
| FEAT-021 | FEAT-003, FEAT-015, FEAT-017 | Required | CDC sourced from audit; schema-registry aware |
| FEAT-022 | FEAT-012, FEAT-014, FEAT-029, FEAT-030 | Required | Guardrails keyed to agent identity and policy |
| FEAT-023 | FEAT-003, FEAT-005, FEAT-008, FEAT-015, FEAT-029, FEAT-030 | Required | Rollback is a governed write over audit history |
| FEAT-024 | FEAT-002, FEAT-005, FEAT-011, FEAT-015, FEAT-028 | Required | Generation builds on schemas and generated surfaces |
| FEAT-025 | FEAT-012, FEAT-014 | Required | Fleet management over tenancy and identity |
| FEAT-026 | FEAT-002, FEAT-003, FEAT-005 | Required | Templates bound to schemas, audited, served via API |
| FEAT-027 | FEAT-001, FEAT-003, FEAT-004, FEAT-021 | Required | Parked; mirror consumes the FR-18 change feed |
| FEAT-028 | FEAT-005, FEAT-014 | Required | Binary hosts the API; service data dirs are tenant-aware |
| FEAT-029 | FEAT-002, FEAT-012, FEAT-013, FEAT-014, FEAT-015, FEAT-016, FEAT-019, FEAT-030 | Required | Policies declared in schemas, enforced on all surfaces |
| FEAT-030 | FEAT-003, FEAT-005, FEAT-012, FEAT-015, FEAT-016, FEAT-017, FEAT-029 | Required | Intents audited, policy-checked, surfaced everywhere |
| FEAT-031 | FEAT-011, FEAT-015, FEAT-016, FEAT-029, FEAT-030 | Required | UI over the policy and intent capabilities |

## Trace Links

FEAT → covered PRD requirements, from each spec's `**Covered PRD Requirements**`
header field. Stories are ledgered in
[`user-stories/README.md`](user-stories/README.md); designs in
[`../02-design/`](../02-design/); tests in [`../03-test/`](../03-test/).

| Feature | Spec | Covered PRD Requirements |
|---------|------|--------------------------|
| FEAT-001 | [spec](features/FEAT-001-collections.md) | FR-1 (collection container, lifecycle, discovery aspects) |
| FEAT-002 | [spec](features/FEAT-002-schema-engine.md) | FR-1 (active-schema-validation aspect) |
| FEAT-003 | [spec](features/FEAT-003-audit-log.md) | FR-15, FR-16, FR-17 |
| FEAT-004 | [spec](features/FEAT-004-entity-operations.md) | FR-1 (entity CRUD); FR-6 (single-entity scope) |
| FEAT-005 | [spec](features/FEAT-005-api-surface.md) | FR-22, FR-28, FR-29; FR-24 (CLI flows) |
| FEAT-006 | [spec](features/FEAT-006-bead-storage-adapter.md) | Dogfooding extension — no direct FR; builds on FR-1, FR-2, FEAT-010 lifecycles |
| FEAT-007 | [spec](features/FEAT-007-entity-graph-model.md) | FR-2; FR-1 (entity model shape) |
| FEAT-008 | [spec](features/FEAT-008-acid-transactions.md) | FR-5; FR-6 (multi-entity scope) |
| FEAT-009 | [spec](features/FEAT-009-unified-graph-query.md) | FR-3 |
| FEAT-010 | [spec](features/FEAT-010-entity-state-machines.md) | FR-10 (lifecycle/transition declaration and enforcement aspect) |
| FEAT-011 | [spec](features/FEAT-011-admin-web-ui.md) | FR-24 (admin-UI flows; policy-testing/approval flows owned by FEAT-031) |
| FEAT-012 | [spec](features/FEAT-012-authorization.md) | FR-25 |
| FEAT-013 | [spec](features/FEAT-013-secondary-indexes.md) | FR-4 |
| FEAT-014 | [spec](features/FEAT-014-multi-tenancy.md) | FR-25 (tenant/database scope model); supports FR-26 |
| FEAT-015 | [spec](features/FEAT-015-graphql-query-layer.md) | FR-20; GraphQL legs of FR-12/FR-13, FR-28, FR-31 |
| FEAT-016 | [spec](features/FEAT-016-mcp-server.md) | FR-21; MCP legs of FR-12, FR-28, FR-31 |
| FEAT-017 | [spec](features/FEAT-017-schema-evolution.md) | PRD Should-Have P1-1; FR-1 (validation as schema changes) |
| FEAT-018 | [spec](features/FEAT-018-aggregation-queries.md) | FR-3 (aggregation facet) |
| FEAT-019 | [spec](features/FEAT-019-validation-rules.md) | FR-1 (cross-field validation and gate readiness) |
| FEAT-020 | [spec](features/FEAT-020-link-discovery-and-graph-queries.md) | — (superseded; scope now under FEAT-009 / FR-3) |
| FEAT-021 | [spec](features/FEAT-021-change-feeds-cdc.md) | FR-18, FR-31 |
| FEAT-022 | [spec](features/FEAT-022-agent-guardrails.md) | FR-9 |
| FEAT-023 | [spec](features/FEAT-023-rollback-recovery.md) | FR-19; FR-30 (repair-plan and rollback dry-run views) |
| FEAT-024 | [spec](features/FEAT-024-application-substrate.md) | PRD Nice-to-Have P2 #1 — no dedicated FR; builds on FR-20, FR-28 |
| FEAT-025 | [spec](features/FEAT-025-control-plane.md) | FR-27 |
| FEAT-026 | [spec](features/FEAT-026-markdown-templates.md) | FR-24 (operator-facing data inspection); supports PRD P2 #1 |
| FEAT-027 | [spec](features/FEAT-027-git-mirror.md) | Deferred — no FR allocated; consumes the FR-18 change feed |
| FEAT-028 | [spec](features/FEAT-028-unified-binary.md) | FR-23, FR-24 |
| FEAT-029 | [spec](features/FEAT-029-access-control.md) | FR-10, FR-11, FR-12, FR-13, FR-14 |
| FEAT-030 | [spec](features/FEAT-030-mutation-intents-approval.md) | FR-7, FR-8; FR-28 (governed path by default) |
| FEAT-031 | [spec](features/FEAT-031-policy-intents-admin-ui.md) | FR-24 (policy/approval UI flows); FR-30 (operator UI portion) |

## Feature Categories

### Core Data Model and Storage
- FEAT-001: Collections
- FEAT-002: Schema Engine
- FEAT-004: Entity Operations
- FEAT-007: Entity-Graph Data Model
- FEAT-008: ACID Transactions
- FEAT-013: Secondary Indexes and Query Acceleration
- FEAT-017: Schema Evolution and Migration
- FEAT-019: Validation Rules and Actionable Errors

### Audit, Recovery, and Change Data
- FEAT-003: Audit Log
- FEAT-021: Change Feeds (CDC)
- FEAT-023: Rollback and Recovery
- FEAT-027: Git Mirror (parked)

### Query and API Surfaces
- FEAT-005: API Surface
- FEAT-009: Unified Graph Query (Cypher)
- FEAT-015: GraphQL Query Layer
- FEAT-016: MCP Server
- FEAT-018: Aggregation Queries
- FEAT-026: Markdown Template Rendering

### Safety, Governance, and Access Control
- FEAT-010: Entity State Machines and Transition Guards
- FEAT-012: Authentication, Identity, and Authorization
- FEAT-022: Agent Guardrails
- FEAT-029: Data-Layer Access Control Policies
- FEAT-030: Mutation Intents and Approval
- FEAT-031: Policy and Intents Admin UI

### Tenancy, Operations, and Deployment
- FEAT-011: Admin Web UI
- FEAT-014: Tenancy, Namespace Hierarchy, and Path-Based Addressing
- FEAT-024: Application Substrate
- FEAT-025: BYOC Deployment Control Plane
- FEAT-028: Unified Binary & Service Management

### Dogfooding Modules
- FEAT-006: Bead Storage Adapter

(FEAT-020 is superseded and intentionally uncategorized; see below.)

## ID Rules

1. Sequential numbering: FEAT-XXX (zero-padded 3 digits). Next free ID: **FEAT-032**.
2. Never reuse IDs, even for cancelled or superseded features.
3. Do not encode category or priority into the ID.
4. Keep full behavior in feature specifications, not in this registry.
5. Parked ideas live in the [parking lot](../parking-lot.md); promotion back into
   this registry is an explicit recorded transition (new sequential ID,
   parking-lot back-link in Source, traceability seeded).

## Deprecated/Cancelled

| ID | Name | Status | Reason | Date |
|----|------|--------|--------|------|
| FEAT-020 | Link Discovery and Graph Queries | Superseded | Scope folded into FEAT-009 — Unified Graph Query per ADR-020 (data model) and ADR-021 (Cypher subset); stories US-070..US-073 moved to FEAT-009 | 2026-05-02 |
| FEAT-027 | Git Mirror | Parked (deferred) | Deferred to the [parking lot](../parking-lot.md) ("Git Mirror" entry): change-feed consumer adding git operational scope before the governed core and FR-18 feeds are proven; spec retained | 2026-06-10 |
