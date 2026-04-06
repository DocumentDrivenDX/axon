---
dun:
  id: FEAT-007
  depends_on:
    - helix.prd
---
# Feature Specification: FEAT-007 - Entity-Graph-Relational Data Model

**Feature ID**: FEAT-007
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

The entity-graph-relational data model is Axon's foundational abstraction. Entities are deeply nested, schema-validated structures representing real-world objects. Links are typed, directional relationships between entities. Together, entities and links form a graph that can be traversed, queried, and aggregated — while retaining the schema guarantees and audit trail of a structured database.

This model avoids the false choice between documents (rich but isolated), graphs (connected but schema-loose), and tables (structured but flat).

## Problem Statement

Agentic applications model the world as things and relationships: beads depend on other beads, customers author documents, accounts have ancestor relationships, invoices flow through approval chains. Current storage options force these natural structures into flat rows (relational), disconnected blobs (document stores), or schema-less property graphs (graph DBs). Each loses something important.

- Current situation: Developers model entities as rows or documents and relationships as foreign keys, join tables, or ad-hoc reference fields
- Pain points: Flat schemas can't express deep nesting. Foreign keys can't express typed, directional relationships with metadata. Graph databases lack schema enforcement and SQL-like aggregation
- Desired outcome: A data model where entities have depth, links have types, and both are schema-validated, audited, and queryable

## Requirements

### Entity Model

- **Entity structure**: Entities are deeply nested JSON-like structures. An entity can contain objects, arrays, and primitive values at arbitrary depth
- **Entity identity**: Every entity has a unique ID within its collection (UUIDv7 by default, or client-provided). IDs are immutable
- **Entity versioning**: Every entity carries a monotonically increasing version number, incremented on each update
- **System metadata**: `_id`, `_version`, `_created_at`, `_updated_at`, `_created_by`, `_updated_by` — always present, managed by Axon
- **Schema binding**: Every entity belongs to a collection and is validated against that collection's schema on every write
- **Recursive structures**: Entity schemas can define recursive types (e.g., a tree node containing child nodes of the same type). Low cardinality at each level is expected but not enforced

### Link Model

- **Link structure**: A link connects a source entity to a target entity with a typed relationship
- **Link components**:
  - `source`: Entity reference (collection + ID)
  - `target`: Entity reference (collection + ID)
  - `link_type`: Named relationship type (e.g., `depends-on`, `authored-by`, `is-ancestor-of`)
  - `metadata`: Optional key-value properties on the link itself (e.g., `weight`, `confidence`, `since`)
  - `_id`, `_version`, `_created_at`, `_created_by`: System metadata (links are versioned and audited)
- **Directionality**: Links are directional (source -> target). Bidirectional relationships are modeled as two links or a single link queried in both directions
- **Cross-collection links**: Links can connect entities in different collections (customer in `customers` -> document in `documents`)
- **Link-type schema**: Link-types are declared in the database schema. Each link-type can have its own metadata schema (e.g., `depends-on` links require a `priority` field)
- **Link uniqueness**: A given (source, target, link_type) triple is unique — you can't create two `depends-on` links between the same pair of entities. Different link-types between the same pair are allowed

### Link Operations

- **Create link**: Create a typed link between two existing entities. Both entities must exist. Link metadata is validated against link-type schema
- **Delete link**: Remove a link. Audit record captures the deleted link
- **Query links from entity**: "What entities does X link to via `depends-on`?" — forward traversal
- **Query links to entity**: "What entities link to X via `authored-by`?" — reverse traversal
- **Traverse with depth**: "Follow `depends-on` links from X up to depth 3" — multi-hop graph traversal
- **Filter during traversal**: "Follow `depends-on` links from X where target.status = 'done'" — combine graph traversal with entity filters

### Collection-Level Queries

Entities within a collection support familiar query operations:

- **Filter**: Field-level predicates with standard operators (eq, ne, gt, lt, gte, lte, in, contains, exists)
- **Sort**: Order by one or more fields
- **Aggregate** (P1): COUNT, SUM, AVG, MIN, MAX, GROUP BY across entities
- **Pagination**: Cursor-based for stable iteration

These queries operate at **moderate scale** — designed for thousands to low millions of entities per collection, not warehouse-scale analytics.

## User Stories

### Story US-017: Model Entities with Nested Structure [FEAT-007]

**As a** developer
**I want** to store entities with deeply nested fields
**So that** I can represent real-world objects without flattening them into rows

**Acceptance Criteria:**
- [ ] Entity with 5 levels of nesting is stored and retrieved correctly
- [ ] Nested fields are individually queryable (e.g., `address.city = "Seattle"`)
- [ ] Schema validates nested structure including required fields within nested objects
- [ ] Recursive schema (tree node with children of same type) is supported
- [ ] Required fields within nested objects are validated; missing required nested fields cause entity write to fail
- [ ] Nested object fields are queryable by dot-path: `address.city = "Seattle"` returns matching entities

### Story US-018: Create and Traverse Links [FEAT-007]

**As an** agent managing a dependency graph
**I want** to create typed links between entities and traverse them
**So that** I can model and query relationships like "bead A depends on bead B"

**Acceptance Criteria:**
- [ ] `axon link create --from beads/bead-A --to beads/bead-B --type depends-on` creates a link
- [ ] `axon link list --from beads/bead-A --type depends-on` lists all dependencies of bead-A
- [ ] `axon link traverse --from beads/bead-A --type depends-on --depth 3` shows the transitive dependency tree
- [ ] Link creation fails if source or target entity doesn't exist
- [ ] Duplicate (source, target, type) triple is rejected with conflict error
- [ ] Attempting to create a link with a non-existent target entity fails with a not-found error identifying the missing entity

### Story US-019: Query Across Entity-Link Graph [FEAT-007]

**As an** agent
**I want** to combine entity filters with link traversal
**So that** I can answer questions like "find all pending beads that depend on completed beads"

**Acceptance Criteria:**
- [ ] Query: "entities in `beads` where status = 'pending' AND linked-via `depends-on` to entities where status = 'done'"
- [ ] Results include both the matching entities and the traversal path
- [ ] 3-hop traversal over a 10K-entity collection completes in under 500ms p99
- [ ] Traversal with no matching results returns an empty result set (not an error)

## Edge Cases and Error Handling

- **Dangling links**: If a target entity is deleted, its inbound links become dangling. Options: (a) reject entity deletion if inbound links exist, (b) cascade-delete links, (c) allow dangling links with a `dangling` status. V1: reject deletion if inbound links exist; option to force-delete with link cascade
- **Circular links**: `depends-on` from A->B and B->A is allowed at the link level (circularity detection is application-level, not enforced by Axon, except for specific link-types that opt into acyclicity)
- **Cross-collection link integrity**: Link creation validates that both source and target entities exist at creation time. Subsequent deletion is handled by the dangling-link policy
- **Link-type not declared**: Creating a link with an undeclared link-type fails with a validation error
- **Deep traversal**: Traversals deeper than 10 hops emit a warning. Configurable max depth

## Success Metrics

- Entity CRUD with nested structures performs within latency targets
- Link creation/traversal performs within latency targets
- The data model feels natural for bead dependency graphs, document authorship, and account hierarchies

## Constraints and Assumptions

### Constraints
- Entities are always schema-bound (no schemaless entities)
- Links are directional; bidirectional requires explicit modeling
- Graph traversal is not optimized for massive graphs in V1 (designed for thousands of nodes, not millions)
- Aggregation queries (GROUP BY, SUM, etc.) are P1

### Assumptions
- Most entity graphs in agentic applications have < 100,000 nodes
- Link traversals rarely exceed depth 5 in practice
- The entity-graph model maps naturally to bead DAGs, org hierarchies, document relationships, and workflow chains

## Dependencies

- **FEAT-002** (Schema Engine): Entity and link-type schemas
- **FEAT-003** (Audit Log): Entity and link mutations are audited

## Out of Scope

- Full graph query language (Cypher/Gremlin/SPARQL equivalent) — P2
- Graph visualization — P2
- Link inference / reasoning (OWL-style) — P2
- Weighted shortest-path or other graph algorithms — P2

## Prior Art

| System | Relevant Pattern | What Axon Takes | What Axon Does Differently |
|--------|-----------------|-----------------|--------------------------|
| **Neo4j** | Property graph with typed relationships | Typed, directional links with metadata | Schema-enforced entities and links; ACID transactions; audit trail |
| **EdgeDB** | Object types with links (graph-relational) | Graph-relational hybrid model | Simpler schema language; audit-first; agent-native API |
| **TypeDB** | Typed entities and relations with reasoning | Typed link model | No reasoning engine; simpler; focuses on transactional correctness |
| **JSON-LD / RDF** | Typed links (predicates) between subjects and objects | Subject-predicate-object as conceptual model | Schema enforcement; ACID; no inference; practical API over academic standards |
| **Postgres + ltree/recursive CTEs** | Hierarchical queries in relational DB | Hierarchical traversal patterns | First-class link model instead of encoding relationships in columns |

## Traceability

### Related Artifacts
- **Parent PRD Section**: Section 2 (Data Model), Requirements Overview > P0 #1-2 (Entity/Link Model)
- **User Stories**: US-017, US-018, US-019
- **Test Suites**: `tests/FEAT-007/`
- **Implementation**: `src/model/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-002 (Schema Engine), FEAT-003 (Audit Log)
- **Depended By**: FEAT-001 (Collections), FEAT-004 (Entity Operations), FEAT-006 (Bead Adapter)
