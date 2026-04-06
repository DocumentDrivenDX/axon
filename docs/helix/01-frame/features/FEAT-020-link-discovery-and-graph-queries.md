---
dun:
  id: FEAT-020
  depends_on:
    - helix.prd
    - FEAT-007
    - FEAT-009
    - FEAT-013
    - FEAT-015
    - FEAT-016
    - ADR-010
---
# Feature Specification: FEAT-020 - Link Discovery and Graph Queries

**Feature ID**: FEAT-020
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Link endpoint discovery answers "what can I link to?" and "what is
linked to this?" — fast, indexed queries against the link and entity
tables that power autocomplete, relationship building, and graph
exploration.

This is the entry point to graph querying. It starts with single-hop
link discovery (finding valid targets for a link type, listing an
entity's neighbors) and extends to multi-hop graph queries exposed
through GraphQL's relationship fields. The dedicated links table with
indexes (ADR-010) makes these operations fast.

## Problem Statement

Building links is one of the most common agent and UI operations.
To create a `depends-on` link from bead-42, the agent needs to know:
which entities can be targeted (the target collection), which are
already linked (avoid duplicates), and which match a search query
(narrow candidates). Without a dedicated discovery query, agents must
fetch all entities from the target collection and filter client-side.

Beyond link building, agents need to answer graph questions: "what
depends on this entity?", "is entity A transitively connected to entity
B?", "what are all the entities in this subgraph?" These are graph
queries that leverage the links table's indexes.

## Requirements

### Functional Requirements

#### Link Endpoint Discovery

- **Candidate query**: Given a source entity and link type, return
  candidate target entities from the target collection
- **Search filter**: Narrow candidates by field values (status, title
  substring, etc.)
- **Already-linked indicator**: Each candidate indicates whether it is
  already linked from the source entity
- **Schema-aware**: The query knows the link type's target collection
  and cardinality from the schema. For `one-to-one` and `many-to-one`
  link types, if the source already has a link of this type, indicate
  that creating another would violate cardinality
- **Fast**: Discovery queries must be index-backed. The common case
  (autocomplete while typing a link target) needs < 50ms latency

**Structured API:**
```json
{
  "source_collection": "beads",
  "source_id": "bead-42",
  "link_type": "depends-on",
  "search": "auth",
  "filter": { "field": "status", "op": "ne", "value": "cancelled" },
  "limit": 20
}
```

**Response:**
```json
{
  "target_collection": "beads",
  "link_type": "depends-on",
  "cardinality": "many-to-many",
  "existing_link_count": 3,
  "candidates": [
    {
      "id": "bead-17",
      "data": { "title": "Auth middleware", "status": "in_progress" },
      "already_linked": false
    },
    {
      "id": "bead-23",
      "data": { "title": "Auth token refresh", "status": "ready" },
      "already_linked": true
    }
  ]
}
```

#### Neighbor Queries

- **Outbound neighbors**: "What entities does entity X link to?" —
  query the links table by `(source_collection, source_id)`, optionally
  filtered by link type
- **Inbound neighbors**: "What entities link to entity X?" — query the
  links target index by `(target_collection, target_id)`, optionally
  filtered by link type
- **Both directions**: Single query that returns both inbound and
  outbound neighbors, grouped by link type and direction
- **Neighbor entity data**: Each neighbor result includes the linked
  entity's data (or a projection of it), not just its ID

#### Graph Queries via GraphQL

GraphQL relationship fields (ADR-012) expose graph queries naturally:

```graphql
# Single-hop: direct dependencies
query {
  bead(id: "bead-42") {
    dependsOn { edges { node { id title status } } }
    dependsOnInbound { edges { node { id title status } } }
  }
}

# Multi-hop: transitive dependencies (depth-limited)
query {
  bead(id: "bead-42") {
    dependsOn {
      edges {
        node {
          id title status
          dependsOn {
            edges {
              node { id title status }
            }
          }
        }
      }
    }
  }
}

# Link endpoint discovery as a GraphQL query
query {
  linkCandidates(
    sourceCollection: "beads"
    sourceId: "bead-42"
    linkType: "depends-on"
    search: "auth"
    filter: { status: { ne: "cancelled" } }
    limit: 20
  ) {
    targetCollection
    cardinality
    existingLinkCount
    candidates {
      id
      title
      status
      alreadyLinked
    }
  }
}
```

Multi-hop queries through GraphQL are depth-limited by the GraphQL
depth limiter (default 10). The query planner resolves each hop as a
links table query followed by a batch entity fetch (DataLoader).

#### Graph Queries via MCP

Collection-specific tools for link discovery and neighbor queries:

```
beads.link_candidates
  description: "Find entities that can be linked from a bead"
  parameters:
    source_id: string (required)
    link_type: string (required, enum from schema)
    search: string (optional — text search on target entities)
    filter: object (optional)
    limit: integer (optional, default 20)
  returns: candidate entities with already-linked indicator

beads.neighbors
  description: "List all entities linked to/from a bead"
  parameters:
    id: string (required)
    link_type: string (optional — filter by type)
    direction: string (optional — "outbound", "inbound", "both")
    limit: integer (optional, default 50)
  returns: grouped neighbor entities by link type and direction
```

#### Index Requirements

These queries depend on the links table indexes from ADR-010:

| Query | Index used |
|---|---|
| Outbound neighbors | PK: `(source_collection_id, source_id, link_type, ...)` |
| Inbound neighbors | `idx_links_target`: `(target_collection_id, target_id, link_type)` |
| Candidate search | EAV secondary indexes on target collection's fields |
| Already-linked check | Point lookup on links PK |

All discovery queries are index-backed. No full table scans.

### Non-Functional Requirements

- **Discovery latency**: < 50ms p99 for 20 candidates with search filter
  on an indexed field (target collection size up to 100K entities)
- **Neighbor latency**: < 20ms p99 for listing neighbors of an entity
  with < 100 links
- **GraphQL multi-hop**: Each hop adds ~10ms. 3-hop query < 50ms with
  warm DataLoader cache

## User Stories

### Story US-070: Find Link Targets [FEAT-020]

**As an** agent creating a dependency link
**I want** to discover which entities I can link to
**So that** I can pick the right target without fetching the entire collection

**Acceptance Criteria:**
- [ ] `link_candidates` query returns entities from the link type's target collection
- [ ] `search: "auth"` filters candidates by title or other searchable fields
- [ ] `filter: {status: {ne: "cancelled"}}` excludes cancelled entities
- [ ] Each candidate shows `already_linked: true/false`
- [ ] Response includes cardinality from the schema (many-to-many, one-to-one, etc.)
- [ ] For one-to-one link types where the source already has a link, response includes a warning
- [ ] Query returns results in < 50ms for a 10K-entity target collection with indexed filter

### Story US-071: List Entity Neighbors [FEAT-020]

**As an** agent understanding an entity's relationships
**I want** to see all entities linked to and from this entity
**So that** I can understand the entity's context in the graph

**Acceptance Criteria:**
- [ ] `neighbors` query returns both outbound and inbound links
- [ ] Results are grouped by link type and direction
- [ ] Each neighbor includes the linked entity's data (not just the ID)
- [ ] `direction: "inbound"` returns only inbound links
- [ ] `link_type: "depends-on"` filters to a specific link type
- [ ] Results are sorted by link type, then by target entity ID
- [ ] Query returns results in < 20ms for an entity with < 100 links

### Story US-072: Explore Graph via GraphQL [FEAT-020]

**As a** UI developer building a relationship explorer
**I want** to traverse the entity graph via GraphQL relationship fields
**So that** I can build interactive graph views with drill-down

**Acceptance Criteria:**
- [ ] `bead.dependsOn` resolves to linked entities with their full data
- [ ] `bead.dependsOnInbound` resolves to entities linking to this bead
- [ ] Nested relationship fields work (multi-hop): `bead.dependsOn.dependsOn`
- [ ] DataLoader prevents N+1 queries — a list of 50 beads with `dependsOn` resolves in 2 queries (entities + links), not 51
- [ ] Depth limit prevents infinite nesting (default 10 levels)
- [ ] Relationship fields accept `filter` and `limit` arguments

### Story US-073: Discover Links via MCP [FEAT-020]

**As an** AI agent building entity relationships
**I want** MCP tools for link discovery and neighbor queries
**So that** I can explore the graph through the standard agent protocol

**Acceptance Criteria:**
- [ ] `beads.link_candidates` tool returns candidate targets with already-linked status
- [ ] `beads.neighbors` tool returns grouped inbound/outbound neighbors
- [ ] Tool descriptions explain the link type's target collection and cardinality
- [ ] Tools are auto-generated for each collection with declared link types
- [ ] Collections without link types do not get link discovery tools

## Edge Cases

- **Link type with no existing links**: Returns all entities from target
  collection (subject to filter/limit). `existing_link_count: 0`
- **Self-referential links**: `depends-on` targeting the same collection.
  The source entity is excluded from candidates (can't link to self)
- **Cardinality violation preview**: For `one-to-one`, if a link already
  exists, the candidate query warns that creating a new link will fail
  (cardinality check happens at link creation, not discovery)
- **Target collection is empty**: Returns empty candidates list. Not an
  error
- **Target collection doesn't exist**: Returns a not-found error
- **Search on non-indexed field**: Falls back to scan on the target
  collection. Slower but correct. Response includes a hint that the
  field could be indexed for faster discovery
- **Deleted entity in link**: If a link target was force-deleted, the
  neighbor query skips it (same behavior as FEAT-009 traversal)
- **High fan-out entity**: An entity with 10K+ links. Neighbor query
  respects `limit` parameter. Consider paginated neighbor queries for
  very high fan-out

## Dependencies

- **FEAT-007** (Entity-Graph Model): Entities and links are the data model
- **FEAT-009** (Graph Traversal): Discovery is the foundation; traversal
  builds on it for multi-hop
- **FEAT-013** (Secondary Indexes): Candidate search uses indexed fields
- **FEAT-015** (GraphQL): Relationship fields and `linkCandidates` query
- **FEAT-016** (MCP): `link_candidates` and `neighbors` tools
- **ADR-010**: Links table with PK and target indexes

## Out of Scope

- **Full graph pattern matching**: Cypher-style `MATCH (a)-[:X]->(b)-[:Y]->(c)` patterns. Not scheduled
- **Graph visualization**: Force-directed layout rendering. UI concern (FEAT-011 V2)
- **Link weight / ranking**: Ordering candidates by relevance or link metadata. Deferred
- **Shortest path**: Finding the shortest path between two entities. Deferred (FEAT-009 traversal covers reachability but not shortest path)
- **Graph analytics**: PageRank, centrality, community detection. Analytical workloads belong in CDC → DuckDB / niflheim

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #7 (Link operations),
  P1 #4 (Graph traversal queries)
- **User Stories**: US-070, US-071, US-072, US-073
- **Implementation**: `crates/axon-api/` (discovery queries),
  `crates/axon-graphql/` (relationship fields, linkCandidates),
  `crates/axon-mcp/` (discovery tools)

### Feature Dependencies
- **Depends On**: FEAT-007, FEAT-009, FEAT-013, FEAT-015, FEAT-016
- **Depended By**: FEAT-011 (Admin UI link builder)
