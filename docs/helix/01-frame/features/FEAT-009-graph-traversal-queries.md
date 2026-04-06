---
dun:
  id: FEAT-009
  depends_on:
    - helix.prd
    - FEAT-007
---
# Feature Specification: FEAT-009 - Graph Traversal Queries

**Feature ID**: FEAT-009
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

Graph traversal queries enable following typed links between entities with depth control, direction, filters at each hop, and path reporting. Use case research across 10 domains confirms that graph traversal is the single most consistent query pattern — BOM explosion (ERP), dependency DAGs (agentic apps, issue tracking), approval chains (workflow automation), identity resolution lineage (CDP, MDM), document version history (document management), and payment application chains (AP/AR) all use the same primitive.

## Problem Statement

Entities in isolation are documents. Entities connected by typed links are a knowledge graph. Without traversal queries, agents must fetch entities one at a time and follow links in application code — O(N) round trips for a depth-N traversal, no server-side filtering, no cycle detection, no path reporting.

## Requirements

### Functional Requirements

- **Forward traversal**: From entity X, follow outgoing links of type T to depth D. `TRAVERSE FROM entities/x VIA depends-on DEPTH 3`
- **Reverse traversal**: From entity X, follow incoming links of type T. "What entities link to X via `authored-by`?"
- **Multi-type traversal**: Follow multiple link types in a single query. `TRAVERSE FROM x VIA [depends-on, blocks] DEPTH 5`
- **Filter at each hop**: Apply entity-level predicates at each traversal step. "Follow `depends-on` links, but only to entities where `status != 'done'`"
- **Path reporting**: Return the traversal path (sequence of entity IDs and link types), not just the leaf entities
- **Cycle detection**: Detect and report cycles rather than looping infinitely. Configurable behavior: stop (default), error, or report cycle
- **Depth limits**: Configurable per-query. Default max 10, hard max configurable per database
- **Result shape**: Return entities, paths, or both. Support projection (return only specified fields from traversed entities)
- **Pagination**: Cursor-based pagination over traversal results

### Query Patterns (from use case research)

| Pattern | Domain | Example |
|---------|--------|---------|
| Dependency DAG | Agentic apps, Issue tracking | "Find all transitive dependencies of bead-A" |
| BOM explosion | ERP | "Expand product-X into all sub-assemblies and raw materials" |
| Approval chain | Workflow automation, Time tracking | "Walk the approval chain for invoice-123" |
| Identity lineage | CDP, MDM | "Show the merge history tree for golden record G" |
| Version history | Document management | "List all versions of document-D in order" |
| Reachability | Issue tracking, Agentic apps | "Is sprint-X reachable from epic-Y via `contains` links?" |
| Ancestor/descendant | CRM, ERP | "Find all parent companies of subsidiary-Z" |

### Non-Functional Requirements

- **Performance**: 3-hop traversal over 10K entities < 50ms p99
- **Consistency**: Traversal reads a consistent snapshot (no torn reads across hops)
- **Memory**: Bounded memory usage during traversal (streaming results, not materializing full graph)

## User Stories

### Story US-023: Traverse a Dependency Graph [FEAT-009]

**As an** agent managing a work queue
**I want** to find all transitive dependencies of a bead
**So that** I can determine if the bead is ready to execute

**Acceptance Criteria:**
- [ ] `TRAVERSE FROM beads/bead-A VIA depends-on DEPTH 10` returns all transitive dependencies
- [ ] Result includes the path from bead-A to each dependency
- [ ] Circular dependencies are detected and reported (not infinite loop)
- [ ] Filtering at each hop works: `WHERE status != 'done'` returns only incomplete dependencies
- [ ] Cycle detection: traversal that encounters a cycle returns a structured response identifying the cycle path
- [ ] Cycle detection terminates safely (no infinite loop or timeout)

### Story US-024: Explode a Bill of Materials [FEAT-009]

**As an** ERP application
**I want** to recursively expand a product into its component parts
**So that** I can calculate total cost and check inventory for all sub-assemblies

**Acceptance Criteria:**
- [ ] `TRAVERSE FROM products/widget-X VIA contains DEPTH 8` returns the full BOM tree
- [ ] Link metadata (`quantity` field on `contains` links) is included in results
- [ ] Leaf nodes (entities with no outgoing `contains` links) are identified
- [ ] Shared components (same entity reached via multiple paths) are reported once with all paths
- [ ] Deleted link targets are skipped during traversal (no error, traversal continues)
- [ ] Shared components reached via multiple paths appear once in results with all paths listed

### Story US-025: Check Reachability [FEAT-009]

**As a** project management tool
**I want** to check if issue A is transitively blocked by issue B
**So that** I can warn users about hidden dependencies

**Acceptance Criteria:**
- [ ] Reachability query returns true/false without materializing the full path
- [ ] Short-circuits on first path found (doesn't explore entire graph)
- [ ] Works across link types: `VIA [blocks, depends-on]`
- [ ] Non-reachable query returns `{reachable: false, depth: null}` without exploring the full graph
- [ ] Reachability check short-circuits on first path found

## Edge Cases

- **Disconnected entities**: Traversal from an entity with no outgoing links of the specified type returns empty result (not error)
- **Cross-collection traversal**: Links span collections. Traversal follows links regardless of collection boundaries
- **Deleted target**: If a link target was force-deleted, traversal skips the dangling link and continues
- **Large fan-out**: Entity with 10,000 outgoing links. Pagination and streaming prevent memory exhaustion
- **Diamond pattern**: Entity reachable via multiple paths. Returned once with all paths (configurable: first-path-only or all-paths)

## Dependencies

- **FEAT-007** (Entity-Graph Model): Links must exist for traversal to work
- **FEAT-008** (ACID Transactions): Traversal reads from a consistent snapshot

## Out of Scope

- Shortest-path algorithms (P2)
- Weighted path computation (P2)
- Full graph query language (Cypher/SPARQL equivalent) — P2
- Graph visualization

## Traceability

### Related Artifacts
- **Parent PRD Section**: Section 4 (Data Model — query model), Section 8 (P1 #4 graph traversal)
- **Use Case Research**: All 10 domains use traversal; ERP BOM explosion, agentic dependency DAGs, and CDP identity lineage are the most demanding
- **User Stories**: US-023, US-024, US-025
- **Test Suites**: `tests/FEAT-009/`

### Feature Dependencies
- **Depends On**: FEAT-007 (Entity-Graph Model), FEAT-008 (ACID Transactions)
- **Depended By**: FEAT-006 (Bead Adapter — ready queue uses dependency traversal)
