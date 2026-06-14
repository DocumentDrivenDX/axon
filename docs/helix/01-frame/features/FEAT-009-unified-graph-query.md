---
ddx:
  id: FEAT-009
  depends_on:
    - helix.prd
  review:
    self_hash: 08784dee672189395e039843c292e6513155f125f9c9ec50bb29f2cc593c7bca
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-009 — Unified Graph Query (Cypher)

**Feature ID**: FEAT-009
**Status**: approved
**Priority**: P0
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-3
**Cross-Subsystem Rationale**: None — single subsystem.
**Requirement Prefix**: QRY

## Overview

A single read-side query language unifies filter, sort, aggregate, traversal, neighbor discovery, and pattern matching: a read-only subset of openCypher (ADR-021) over the document-shaped data model (ADR-020). This feature implements PRD FR-3 — one policy-aware read model for all collection and graph queries.

This feature is **the owner of all read/query paths** in Axon (product-owner decision 2026-06-10: the Cypher unification stands). It absorbed the former FEAT-020 (link discovery and graph queries; retired — its stories US-070..US-073 live here), and FEAT-018's aggregation queries are projections of this feature's planner: aggregation surfaces compile to the same language, planner, and policy path rather than maintaining a separate read engine.

## Ideal Future State

A developer or agent expresses every read — "pending invoices over this vendor relationship", "open beads with no open dependencies", "is A transitively blocked by B" — in one language, through two surfaces: schema-declared named queries (compiled, policy- and index-validated at schema time, exposed as typed GraphQL fields and MCP tools) and ad-hoc queries (same enforcement path, stricter cost budgets). Policy decisions, redactions, and counts behave identically everywhere because there is exactly one planner and one enforcement path.

## Problem Statement

- **Current situation**: Entities in isolation are documents; entities connected by typed links are a graph. Without a unified query language, every read pattern requires its own surface — connection arguments for filter/sort, traversal directives, neighbor queries, link-discovery queries, ad-hoc patterns.
- **Pain points**: Multiplied spec, test, and policy surface; graph-shaped reasoning pushed into client code with N+1 round trips. The DDx consumer use case (axon-05c1019d) sharpened this: the ready-queue query "open beads with no open `depends_on` targets" requires a single round-trip predicate over outgoing-link target state that none of the prior per-pattern surfaces expressed.
- **Desired outcome**: One read language, one planner, one policy path subsuming filter, sort, aggregate, traversal, neighbor discovery, and pattern matching.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Query language | "How do I express a read?" | Read-only openCypher subset over entities and links |
| Named queries | "Declare a reusable, validated query" | Schema-declared, compile-time validated, typed GraphQL/MCP surfaces |
| Ad-hoc queries | "Answer a one-off question now" | Runtime query execution under policy and cost budgets |
| Live results | "Tell me when the answer changes" | Subscriptions on named queries |
| Planning and acceleration | "Make common reads fast" | Rule-based planning over secondary indexes and link storage |
| Policy integration | "Never leak what I can't see" | Row policy, redaction, leak-safe existence and aggregation |

## Requirements

### Functional Requirements by Area

#### Query Language

- **QRY-01**. The read model must be a read-only openCypher subset covering match, optional match, predicate filtering, projection with ordering and pagination, existence checks, bounded variable-length paths, and standard aggregations. The normative clause list, parameter and type rules, and exclusions (write clauses, shortest-path, procedure calls, unbounded path patterns) are defined in [CONTRACT-007 — Cypher Query Surface](../../02-design/contracts/CONTRACT-007-cypher-query-surface.md), per ADR-021.
- **QRY-02**. Queries must only reference labels, properties, and relationship types present in the active schema; unknown references are rejected at parse time with the stable error codes defined in CONTRACT-007 §Stable error codes.
- **QRY-03**. Aggregation reads (counts, numeric aggregates, grouping — the FEAT-018 surface) must compile to this feature's planner and policy path; no separate aggregation engine exists. Aggregation projection rules are defined in CONTRACT-007 §Aggregation projections.

#### Schema-Declared Named Queries

- **QRY-04**. A collection schema must be able to declare named, described, parameterized queries; the declaration grammar is defined in CONTRACT-007 §Named-query declaration (FEAT-002 / ESF extension, CONTRACT-010).
- **QRY-05**. Each named query must be type-checked at schema-write time against the active collection schemas.
- **QRY-06**. The schema compiler must validate index usage: queries requiring unindexed scans on collections above the configured threshold are rejected with a diagnostic suggesting an index declaration.
- **QRY-07**. The schema compiler must validate policy compatibility: queries that would require policy bypass to be useful are rejected with the documented error code (CONTRACT-007).
- **QRY-08**. Each activated named query must generate one typed GraphQL field and one MCP tool; field and tool shapes, including connection-style pagination, are defined in [CONTRACT-002 — GraphQL Surface](../../02-design/contracts/CONTRACT-002-graphql-surface.md) and CONTRACT-003 — MCP Surface.
- **QRY-09**. Schema dry-runs must return a compile report including named-query diagnostics without activating the schema.

#### Ad-Hoc Queries

- **QRY-10**. An ad-hoc query surface must accept a query string and parameters at request time, using the same parser, planner, and policy-enforcement path as named queries. The GraphQL field and result shape (rows, column type metadata, plan/policy metadata) are defined in CONTRACT-007 §Ad-hoc query field and CONTRACT-002.
- **QRY-11**. Ad-hoc queries must run under stricter cost budgets than named queries (unindexed scans, intermediate cardinality); named queries may opt into larger budgets at schema-declaration time. Budget and timeout rejections use the stable error codes in CONTRACT-007.

#### Live Results

- **QRY-12**. Named queries must be subscribable through the GraphQL subscription path (FEAT-015): subscribers receive an initial snapshot, then policy-filtered updates whenever an underlying entity or link change affects the result set. Ad-hoc queries are not subscribable in V1.

#### Planning and Acceleration

- **QRY-13**. Query planning must be rule-based (not cost-based; ADR-021 §Compilation): label + property predicates use declared secondary indexes (FEAT-013), relationship traversals and existence checks use link-storage indexes, index-covered orderings avoid explicit sorts, and everything else falls back to budget-limited scans.
- **QRY-14**. Query result metadata must report plan information and index usage so developers can diagnose slow queries.

#### Policy Integration

- **QRY-15**. Row policy must apply at each label match; field redaction applies at projection, and redacted fields must not be usable in predicates or aggregations (FEAT-029 / ADR-019 / CONTRACT-007 §Policy enforcement obligations).
- **QRY-16**. Existence checks must be policy-aware — hidden targets do not leak through existence results — and counts and aggregates must only reflect rows the subject is allowed to see.

### Non-Functional Requirements

| Operation | Target (p99) | Notes |
|---|---|---|
| Single-entity match by label + property | < 5 ms | Index lookup |
| 3-hop traversal over 10K entities | < 50 ms | Traversal budget |
| Ready/blocked pattern @ 1K beads | < 100 ms | DDx Use Case A latency budget |
| Ready/blocked pattern @ 10K beads | < 500 ms | DDx Use Case A latency budget |
| Ad-hoc query parse + plan | < 10 ms | Excludes execution |
| Schema-time named query compile | < 50 ms | Per query |

Bounded streaming memory; 30-second wall-clock timeout; 10-hop default depth cap (normative limits in CONTRACT-007 §Limits).

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-023 | Traverse a Dependency Graph | [US-023](../user-stories/US-023-traverse-a-dependency-graph.md) |
| US-024 | Explode a Bill of Materials | [US-024](../user-stories/US-024-explode-a-bill-of-materials.md) |
| US-025 | Check Reachability | [US-025](../user-stories/US-025-check-reachability.md) |
| US-070 | Find Link Targets | [US-070](../user-stories/US-070-find-link-targets.md) |
| US-071 | List Entity Neighbors | [US-071](../user-stories/US-071-list-entity-neighbors.md) |
| US-072 | Explore Graph via GraphQL | [US-072](../user-stories/US-072-explore-graph-via-graphql.md) |
| US-073 | Discover Links via MCP | [US-073](../user-stories/US-073-discover-links-via-mcp.md) |
| US-074 | Pattern Query for Ready/Blocked Queue | [US-074](../user-stories/US-074-pattern-query-for-ready-blocked-queue.md) |
| US-075 | Schema-Declared Named Query | [US-075](../user-stories/US-075-schema-declared-named-query.md) |
| US-076 | Ad-hoc Cypher Query | [US-076](../user-stories/US-076-ad-hoc-cypher-query.md) |
| US-077 | Subscribe to a Named Query | [US-077](../user-stories/US-077-subscribe-to-a-named-query.md) |

US-070..US-073 were inherited from the retired FEAT-020. US-074b (Query by
Gate Status) belongs to FEAT-019, not this feature.

## Edge Cases and Error Handling

- **Disconnected entity**: traversal from an entity with no outgoing links of the matched type returns an empty result, not an error.
- **Cross-collection traversal**: links spanning collections traverse naturally.
- **Deleted link target**: traversal skips dangling links.
- **Large fan-out**: pagination and bounded streaming prevent memory exhaustion; budget rejection applies before execution where predictable.
- **Diamond pattern**: an entity reachable via multiple paths is returned once, with all paths recoverable through collection projection.
- **Empty target collection**: candidate queries return empty rows, not an error.
- **Self-referential pattern**: self-loops match correctly; cycle-bearing traversals terminate safely under the depth cap.

## Success Metrics

- All read-pattern latency targets in the NFR table hold on reference hardware.
- One enforcement path: policy parity fixtures produce identical allow/deny/redaction/count results for the same query across GraphQL, MCP, and ad-hoc surfaces.
- DDx drops its two-phase ready-queue fallback once the named-query path lands (single round-trip).
- Zero visibility leaks through existence, counts, or aggregates under the leak-safety contract suite.

## Constraints and Assumptions

### Constraints

- The query language is read-only in V1; all writes go through schema-validated mutation flows (FEAT-004, FEAT-008, FEAT-030).
- Planning is rule-based in V1; cost-based optimization is deferred.
- Depth and wall-clock limits apply to every query (CONTRACT-007 §Limits).

### Assumptions

- Most production reads will be named queries; ad-hoc queries are primarily for development, operations, and debugging.
- Typical operational graphs stay within the moderate-scale envelope (thousands to low millions of records).
- The openCypher subset is expressive enough for all 10 researched use-case domains (ERP BOM, dependency DAGs, identity lineage, ready/blocked queues).

## Dependencies

- **Other features**: FEAT-002 (Schema Engine — label/property typing, named-query block), FEAT-007 (Entity-Graph Data Model — the entities and links being queried), FEAT-013 (Secondary Indexes — acceleration), FEAT-015 (GraphQL — field generation, subscriptions, ad-hoc resolver), FEAT-016 (MCP — per-named-query tools, ad-hoc tool), FEAT-029 (Access Control — row policy, redaction, leak-safe existence), FEAT-018 (Aggregation Queries — its surfaces are projections of this planner).
- **External services**: None. Normative surfaces: [CONTRACT-007 — Cypher Query Surface](../../02-design/contracts/CONTRACT-007-cypher-query-surface.md) (language subset, named-query grammar, error codes, limits, policy obligations, aggregation projections), [CONTRACT-002 — GraphQL Surface](../../02-design/contracts/CONTRACT-002-graphql-surface.md) (field/connection shapes, subscriptions), CONTRACT-003 — MCP Surface (tool shapes).
- **PRD requirements**: FR-3 (P0).

## Out of Scope

- Cypher write clauses — V2+; would conflict with the FEAT-030 mutation-intent flow.
- Shortest-path and weighted path computation — V2.
- Subqueries, procedure calls, and user-defined functions — V2.
- `UNION` / `UNION ALL` — revisit when a real use case appears.
- Spilling to disk for large orderings/collections — V2.
- Subscriptions on ad-hoc queries — V2.
- SPARQL or alternative query grammars — rejected per ADR-020.
- Graph visualization — UI concern (FEAT-011).
- Graph analytics (PageRank, centrality, community detection) — analytical workloads belong in CDC → downstream systems.

## Review Checklist

Use this checklist when reviewing a feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No exact API/CLI/event/schema/config surface is defined inline; normative surface links to Contract artifacts
