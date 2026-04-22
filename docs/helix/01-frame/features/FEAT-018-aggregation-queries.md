---
ddx:
  id: FEAT-018
  depends_on:
    - helix.prd
    - FEAT-004
    - FEAT-013
    - FEAT-015
---
# Feature Specification: FEAT-018 - Aggregation Queries

**Feature ID**: FEAT-018
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Aggregation queries compute summary statistics over entities in a
collection: COUNT, SUM, AVG, MIN, MAX, and GROUP BY. Results are
accelerated by secondary indexes (FEAT-013) when the aggregation field
is indexed.

Aggregations are exposed through the structured API, GraphQL, and MCP.

## Problem Statement

Agents and dashboards need summary data: "how many beads are blocked?",
"what's the average priority of in-progress tasks?", "count beads by
status". Without aggregation, clients must fetch all entities and compute
summaries client-side, which is wasteful and slow for large collections.

## Requirements

### Functional Requirements

#### Aggregation Functions

| Function | Description | Applicable types |
|---|---|---|
| `COUNT` | Number of matching entities | All |
| `SUM` | Sum of numeric field values | integer, float |
| `AVG` | Average of numeric field values | integer, float |
| `MIN` | Minimum value | integer, float, string, datetime |
| `MAX` | Maximum value | integer, float, string, datetime |

- **COUNT without GROUP BY**: Already partially supported via
  `count_only` in `QueryEntitiesRequest`. This feature formalizes it
- **COUNT with GROUP BY**: Group entities by a field and count each group
- **Multiple aggregations**: A single query can request multiple
  aggregation functions on different fields
- **Filter + aggregate**: Aggregations can be combined with filters
  (e.g., "average priority of beads WHERE status = 'in_progress'")

#### GROUP BY

- **Single-field grouping**: Group by one field (e.g., `GROUP BY status`)
- **Multi-field grouping**: Group by multiple fields (e.g.,
  `GROUP BY status, assignee`)
- **Group keys**: Each group returns its key value(s) and the aggregated
  results
- **Null handling**: Entities where the grouped field is null or missing
  form their own group (labeled `null`)

#### API Integration

**Structured API:**
```json
{
  "collection": "beads",
  "filter": { "field": "bead_type", "op": "eq", "value": "task" },
  "aggregations": [
    { "function": "count" },
    { "function": "avg", "field": "priority" }
  ],
  "group_by": ["status"]
}
```

**GraphQL** (auto-generated per collection):
```graphql
query {
  beadsAggregate(filter: { beadType: { eq: TASK } }, groupBy: [STATUS]) {
    groups {
      keys { status: BeadStatus }
      count
      avgPriority
    }
    totalCount
  }
}
```

**MCP** (auto-generated per collection):
```
beads.aggregate
  parameters:
    filter: object (optional)
    aggregations: array of {function, field}
    group_by: array of field names
  returns: grouped aggregation results
```

#### Index Acceleration

- When the `GROUP BY` field has a secondary index (FEAT-013), the
  aggregation scans the index table instead of the entity table
- When the filter field has a secondary index, the filter is applied
  via index lookup before aggregation
- Non-indexed aggregations fall back to full entity scan

### Non-Functional Requirements

- **Latency**: Aggregation over 10K entities with indexed GROUP BY < 500ms p99
- **Latency**: COUNT without GROUP BY < 50ms (uses collection count or index count)
- **Memory**: Aggregation results are bounded by the number of distinct group keys, not entity count

## User Stories

### Story US-062: Count Entities by Field [FEAT-018]

**As an** agent
**I want** to count entities grouped by a field
**So that** I can understand the distribution of data without fetching all entities

**Acceptance Criteria:**
- [ ] `COUNT GROUP BY status` on beads returns `{draft: 5, pending: 12, in_progress: 3, ...}`
- [ ] COUNT without GROUP BY returns a single total count
- [ ] COUNT with a filter (e.g., `WHERE bead_type = 'task'`) counts only matching entities
- [ ] Empty collection returns `{totalCount: 0, groups: []}`
- [ ] Null/missing field values form their own group labeled `null`

### Story US-063: Compute Numeric Aggregations [FEAT-018]

**As an** agent analyzing entity data
**I want** to compute SUM, AVG, MIN, MAX on numeric fields
**So that** I can derive summary statistics without fetching all entities

**Acceptance Criteria:**
- [ ] `AVG(priority) GROUP BY status` returns average priority per status group
- [ ] `SUM(amount)` on an invoices collection returns the total amount
- [ ] `MIN(priority)` and `MAX(priority)` return the correct extreme values
- [ ] Aggregation on a non-numeric field (e.g., `SUM(title)`) returns a clear type error
- [ ] Entities where the aggregated field is null are excluded from SUM/AVG/MIN/MAX (not treated as 0)
- [ ] AVG returns a float even when the source field is integer

### Story US-064: Aggregate via GraphQL [FEAT-018]

**As a** UI developer building a dashboard
**I want** to query aggregations via GraphQL
**So that** I can build summary views without client-side computation

**Acceptance Criteria:**
- [ ] `beadsAggregate` query is auto-generated for each collection
- [ ] Filter, groupBy, and aggregation functions are available as arguments
- [ ] Response includes `totalCount` and `groups` with keys and aggregated values
- [ ] Aggregation query works alongside regular entity queries in the same GraphQL request

### Story US-065: Aggregate via MCP [FEAT-018]

**As an** AI agent
**I want** to compute aggregations via an MCP tool
**So that** I can understand data distributions through the standard agent protocol

**Acceptance Criteria:**
- [ ] `beads.aggregate` tool is auto-generated for each collection
- [ ] Tool accepts filter, aggregations, and group_by parameters
- [ ] Response is structured JSON with groups and aggregated values
- [ ] Tool description explains available functions and valid field types

## Edge Cases

- **GROUP BY on non-existent field**: Returns all entities in a single
  `null` group
- **Aggregation on empty collection**: Returns `{totalCount: 0}` with
  empty groups
- **Very high cardinality GROUP BY**: Grouping by a unique field (e.g.,
  entity ID) produces one group per entity. Response is bounded by
  entity count — a limit parameter caps the number of groups returned
- **Concurrent writes during aggregation**: Aggregation sees a consistent
  snapshot (same isolation as entity queries). New writes during
  aggregation may or may not be included depending on timing

## Dependencies

- **FEAT-004** (Entity Operations): Aggregation builds on entity query
- **FEAT-013** (Secondary Indexes): Index-accelerated grouping and filtering
- **FEAT-015** (GraphQL): `*Aggregate` query types auto-generated
- **FEAT-016** (MCP): `*.aggregate` tools auto-generated

## Out of Scope

- **HAVING clause**: Filter on aggregated results (e.g., "groups with
  count > 10"). Deferred
- **Window functions**: Running totals, rankings. Deferred
- **Cross-collection aggregation**: Join aggregation across collections.
  Deferred — use CDC → DuckDB for analytical queries
- **Approximate aggregation**: HyperLogLog for approximate distinct
  counts. Deferred

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #3 (Aggregation queries)
- **User Stories**: US-062, US-063, US-064, US-065
- **Implementation**: `crates/axon-api/` (aggregation engine),
  `crates/axon-graphql/` (aggregate query types)

### Feature Dependencies
- **Depends On**: FEAT-004, FEAT-013, FEAT-015, FEAT-016
- **Depended By**: FEAT-011 (Admin UI dashboard could use aggregations)
