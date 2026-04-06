---
dun:
  id: FEAT-013
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-002
    - FEAT-004
    - ADR-010
---
# Feature Specification: FEAT-013 - Secondary Indexes and Query Acceleration

**Feature ID**: FEAT-013
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Secondary indexes accelerate entity queries by maintaining pre-computed
lookup structures for declared fields. Without indexes, all queries perform
full collection scans with application-layer filtering. With indexes,
equality, range, and sort queries execute in sub-millisecond time against
typed index tables.

Indexes are declared in the Entity Schema Format (ESF Layer 4) and
maintained automatically by the storage layer on every entity write.
The design follows the Salesforce EAV (Entity-Attribute-Value) pattern:
one table per value type, shared across all collections. Compound indexes
use a binary-encoded sort key that is portable across SQL and KV backends.

See [ADR-010](../../02-design/adr/ADR-010-physical-storage-and-secondary-indexes.md)
for the full design.

## Problem Statement

Entity queries currently perform a full range scan of every entity in the
collection, deserializing and filtering in application code. This is
acceptable for small collections but degrades linearly with entity count.
Collections with thousands of entities need sub-millisecond lookups on
common filter fields (status, priority, owner, timestamps).

Agents frequently query by status ("give me all pending beads") and sort
by priority or creation time. Without indexes, these queries are O(n) in
collection size.

## Requirements

### Functional Requirements

#### Index Declarations (ESF Layer 4)

- **Single-field indexes**: Declare a field path and value type for fast
  equality and range lookups
- **Compound indexes**: Declare an ordered list of field+type pairs for
  multi-field queries and multi-field sorts
- **Unique indexes**: Single-field or compound indexes that enforce
  uniqueness — no two entities in the same collection may share the same
  indexed value
- **Index types**: `string`, `integer`, `float`, `datetime`, `boolean`
- **Schema integration**: Indexes are declared in the collection schema
  alongside entity schema (L1), link types (L2), and lifecycles (L3)
- **Null handling**: Null or missing field values are not indexed.
  Entities with null indexed fields are excluded from index lookups but
  included in full scans

#### Index Maintenance

- **Automatic on write**: Every entity create, update, and patch
  operation maintains all declared indexes for that entity
- **Delete-then-insert pattern**: Old index entries are removed and new
  entries are inserted on every write
- **Cascade on entity delete**: When an entity is deleted, its index
  entries are automatically removed
- **Unique violation**: If a write would violate a unique index, the
  operation fails with `AxonError::Conflict` identifying the conflicting
  field and value

#### Query Acceleration

- **Equality queries**: `status = "pending"` uses the string index on
  `status` to return matching entity IDs, then fetches entities by PK
- **Range queries**: `priority > 3` uses the integer index on `priority`
  with a range scan
- **Sort optimization**: If the query's sort field matches an index, the
  index scan produces pre-sorted results with no application-layer sort
- **Compound prefix matching**: A compound index on `(status, priority)`
  accelerates queries filtering on `status` alone (leftmost prefix match)
- **Fallback**: Queries on non-indexed fields fall back to full scan with
  application-layer filtering (current behavior)

#### Index Lifecycle

- **Building state**: New indexes on existing collections start in
  `building` state. A background worker populates the index by scanning
  all existing entities. The query planner does not use `building` indexes
- **Ready state**: Once the build scan completes, the index transitions
  to `ready`. The query planner begins using it
- **Dropping state**: When an index is removed from the schema, it
  enters `dropping` state. A background worker removes index entries in
  batches
- **Rebuild**: An admin operation can trigger a full reindex — the index
  returns to `building`, existing entries are truncated, and entities
  are rescanned

### Non-Functional Requirements

- **Write amplification**: Each entity write touches N index tables where
  N is the number of declared indexes. Acceptable for typical workloads
  (1-5 indexes per collection)
- **Query latency**: Indexed equality query on a 100K-entity collection
  < 5ms p99
- **Index build**: Background build should process at least 10K
  entities/second
- **Backend portability**: Index implementation must work identically on
  PostgreSQL, SQLite, and KV stores. No backend-specific query operators
  (no GIN, no JSONB containment)

## User Stories

### Story US-031: Declare a Secondary Index [FEAT-013]

**As a** developer defining a collection schema
**I want** to declare which fields should be indexed
**So that** queries on those fields are fast

**Acceptance Criteria:**
- [ ] Adding `indexes: [{field: status, type: string}]` to the schema
  creates an index on the `status` field
- [ ] Queries filtering on `status` use the index instead of a full scan
- [ ] Queries filtering on non-indexed fields still work (full scan)
- [ ] Schema validation rejects invalid index declarations (unknown type,
  missing field)

### Story US-032: Enforce Uniqueness via Index [FEAT-013]

**As a** developer modeling entities with unique constraints
**I want** to declare a unique index on a field
**So that** the system prevents duplicate values

**Acceptance Criteria:**
- [ ] Adding `unique: true` to an index declaration enforces uniqueness
- [ ] Creating two entities with the same indexed value in the same
  collection fails with a conflict error
- [ ] The error identifies the conflicting field and value
- [ ] Unique constraint is enforced at the storage level, not just
  application level

### Story US-033: Compound Index for Multi-Field Queries [FEAT-013]

**As a** developer querying entities by multiple fields
**I want** a compound index that accelerates multi-field lookups
**So that** queries like "status=pending AND priority>3" are fast

**Acceptance Criteria:**
- [ ] A compound index on `[status, priority]` accelerates queries on
  both fields together
- [ ] The compound index also accelerates queries on `status` alone
  (prefix match)
- [ ] Sort by the compound index's field order uses the index scan order

### Story US-034: Background Index Build [FEAT-013]

**As an** operator adding an index to an existing collection
**I want** the index to build in the background
**So that** existing entities are indexed without blocking normal operations

**Acceptance Criteria:**
- [ ] Adding a new index to a collection with entities starts a
  background build
- [ ] The index is not used for queries until the build completes
- [ ] Entity writes during the build also populate the new index
- [ ] The build completes and the index becomes available for queries

## Edge Cases

- **Schema change removes an index**: Index enters `dropping` state;
  queries stop using it immediately. Background cleanup follows
- **Index type mismatch**: Entity has a string value for an integer-indexed
  field. The value is not indexed (skip, don't error). Query results for
  that entity come from the fallback scan path
- **Concurrent unique violation**: Two transactions insert entities with
  the same unique-indexed value. Exactly one succeeds; the other gets
  a conflict error
- **Empty collection**: Index on an empty collection is immediately `ready`
- **All entities have null for indexed field**: Index is empty but valid.
  Queries on that field return no results from the index, but a full scan
  would find entities (with null values matching `IS NULL` predicates if
  supported)

## Dependencies

- **FEAT-001** (Collections): Indexes belong to collections
- **FEAT-002** (Schema Engine): Index declarations are part of the schema
- **FEAT-004** (Entity Operations): Index maintenance hooks into write path
- **ADR-010**: Full design for EAV index tables, compound sort key encoding,
  index lifecycle state machine

## Out of Scope

- **Array field indexing**: One row per array element. Deferred
- **Full-text search indexes**: tsvector/FTS5. Separate feature
- **Cost-based query planning**: V1 uses simple rules-based index selection
- **GIN / JSONB containment indexes**: Deliberately omitted for portability
- **Expression indexes**: Indexes on computed values. Deferred

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #9 (Secondary indexes)
- **User Stories**: US-031, US-032, US-033, US-034
- **Architecture**: ADR-010 (Physical Storage and Secondary Indexes)
- **Implementation**: `crates/axon-storage/` (index maintenance),
  `crates/axon-api/` (query planner)

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-004
- **Depended By**: FEAT-011 (Admin UI can display index status)
