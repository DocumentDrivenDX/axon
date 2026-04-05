---
dun:
  id: FEAT-004
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-002
    - FEAT-003
---
# Feature Specification: FEAT-004 - Entity Operations

**Feature ID**: FEAT-004
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

Entity operations are the core data manipulation interface of Axon. Entities are the units of data within collections. Every entity operation is schema-validated, version-tracked, and audited. The API is designed for programmatic consumption by agents — structured inputs, structured outputs, structured errors.

## Problem Statement

Agents need to create, read, update, delete, and query structured entities with transactional guarantees and clear error handling. Current options require either raw SQL (too low-level for agents), schemaless APIs (too permissive), or file I/O (no concurrency or query).

## Requirements

### Functional Requirements

- **Create entity**: Insert a new entity into a collection. Entity is validated against schema. Returns the created entity with server-assigned metadata (id, version, created_at)
- **Read entity**: Retrieve an entity by ID. Returns full entity with metadata
- **Update entity**: Replace or patch an entity. Full entity replacement or partial field updates. Validated against schema. Version must match (optimistic concurrency)
- **Delete entity**: Soft-delete or hard-delete an entity by ID. Audit record captures the deleted state
- **List entities**: Return all entities in a collection with pagination (cursor-based)
- **Query/filter**: Filter entities by field values with operators (eq, ne, gt, lt, gte, lte, in, contains). Combinable with AND/OR
- **Sort**: Order results by one or more fields, ascending or descending
- **Projection**: Return only specified fields (reduce payload for large entities)
- **Pagination**: Cursor-based pagination for stable iteration over changing data. Limit/offset as convenience alias

### Entity Model

- **Entity ID**: Server-generated UUIDv7 by default, or client-provided string ID. Must be unique within collection
- **Version**: Monotonically increasing integer per entity. Starts at 1. Incremented on every update
- **System metadata**: `_id`, `_version`, `_created_at`, `_updated_at`, `_created_by`, `_updated_by` — always present, not part of user schema
- **User data**: The entity body, validated against the collection schema

### Optimistic Concurrency

- **Version check on update**: Update and delete operations accept an expected `_version`. If the current version doesn't match, the operation fails with a conflict error
- **Conflict response**: Includes the current entity state so the caller can merge and retry
- **Unconditional writes**: If no version is provided, the write succeeds unconditionally (useful for idempotent operations)

### Non-Functional Requirements

- **Performance**: Single-entity read/write < 10ms p99. List/query scales linearly with result set size
- **Consistency**: Within a single Axon instance, read-after-write consistency is guaranteed
- **Concurrency**: Multiple concurrent readers. Writers are serialized per-entity via optimistic concurrency

## User Stories

### Story US-010: CRUD an Entity [FEAT-004]

**As an** agent
**I want** to create, read, update, and delete entities in a collection
**So that** I can store and manage structured state

**Acceptance Criteria:**
- [ ] Create returns the full entity with _id, _version, _created_at
- [ ] Read by ID returns the entity or 404
- [ ] Update with correct _version succeeds and increments version
- [ ] Update with wrong _version fails with conflict error including current state
- [ ] Delete removes the entity and creates an audit entry with the deleted state

### Story US-011: Query Entities [FEAT-004]

**As an** agent
**I want** to find entities matching specific criteria
**So that** I can locate relevant data without knowing entity IDs

**Acceptance Criteria:**
- [ ] Filter by field equality: `status = "pending"`
- [ ] Filter with comparison operators: `priority > 3`
- [ ] Combine filters with AND: `status = "pending" AND assignee = "agent-1"`
- [ ] Sort results: `ORDER BY created_at DESC`
- [ ] Paginate with cursor: returns next cursor for stable iteration
- [ ] Return count of matching entities without fetching all of them

### Story US-012: Partial Update [FEAT-004]

**As an** agent
**I want** to update specific fields without sending the entire entity
**So that** I can make targeted changes efficiently

**Acceptance Criteria:**
- [ ] Patch operation accepts a subset of fields
- [ ] Only specified fields are modified; unmentioned fields are preserved
- [ ] Patch is validated against schema (the resulting entity must be valid)
- [ ] Version conflict detection works the same as full replacement

## Edge Cases and Error Handling

- **Entity not found**: Read/update/delete on non-existent ID returns structured 404 with the ID that was requested
- **Schema violation on update**: Update that would make the entity invalid is rejected. Current entity is unchanged
- **Concurrent updates**: Two agents update the same entity. First succeeds, second gets version conflict with current state. No data loss
- **Large entities**: Entities exceeding 1MB are rejected with a size limit error. Configurable per collection (P2)
- **Empty update**: Patch with no changed fields succeeds as a no-op (version is NOT incremented, no audit entry)
- **Delete then read**: Reading a deleted entity returns 404 (or the deleted state with a tombstone flag, TBD)
- **ID collision**: Client-provided ID that already exists returns conflict error

## Success Metrics

- All CRUD operations complete within latency targets
- Zero data loss from concurrent operations (optimistic concurrency prevents silent overwrites)
- Agents can self-correct from validation and conflict errors programmatically

## Constraints and Assumptions

### Constraints
- Single-entity operations only in V1. Multi-entity transactions are P1 (batch operations)
- Read-after-write consistency within a single instance
- Entity size limit of 1MB by default

### Assumptions
- Most entities are 1-50KB
- Most queries return < 1000 results
- Agents will commonly use version-based concurrency (not unconditional writes)

## Dependencies

- **FEAT-001** (Collections): Entities live in collections
- **FEAT-002** (Schema Engine): All writes are validated
- **FEAT-003** (Audit Log): All mutations are audited

## Out of Scope

- Full-text search (P2)
- Aggregation queries (P2)
- Multi-entity transactions / batch operations (P1)
- Secondary indexes (P1 — V1 uses sequential scan for queries)
- Geospatial queries

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #4 (Entity Operations), P0 #5 (Optimistic Concurrency)
- **User Stories**: US-010, US-011, US-012
- **Test Suites**: `tests/FEAT-004/`
- **Implementation**: `src/entities/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-001, FEAT-002, FEAT-003
- **Depended By**: FEAT-005 (API Surface), FEAT-006 (Bead Storage Adapter)
