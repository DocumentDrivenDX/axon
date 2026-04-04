---
dun:
  id: FEAT-001
  depends_on:
    - helix.prd
---
# Feature Specification: FEAT-001 - Collections

**Feature ID**: FEAT-001
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

Collections are the foundational data container in Axon. A collection is a named, schema-bound container of documents within a database. Collections provide the organizational unit for data, schema enforcement, access control, and audit boundaries.

## Problem Statement

Agentic applications need a structured place to store groups of related documents with consistent schemas. Current approaches — raw database tables, JSON files, schemaless document stores — either lack structure enforcement or require too much boilerplate to set up correctly.

- Current situation: Agents dump data into whatever storage is convenient, with no consistency guarantees
- Pain points: No naming conventions, no schema binding, no lifecycle management, no discovery
- Desired outcome: Named collections with schemas, discoverable via API, manageable via CLI

## Requirements

### Functional Requirements

- **Create collection**: Create a named collection bound to a schema. Collection names must be unique within a database
- **Drop collection**: Remove a collection and all its documents (with confirmation). Audit records are retained
- **List collections**: Enumerate all collections in a database with metadata (name, schema version, document count, created/updated timestamps)
- **Describe collection**: Return full collection metadata including schema, indexes, statistics
- **Collection namespacing**: Collections exist within a database. Database provides the isolation boundary
- **Collection metadata**: Each collection tracks creation time, schema version, document count, last modified time

### Non-Functional Requirements

- **Performance**: Collection creation/drop < 100ms. List/describe < 50ms
- **Naming**: Collection names are lowercase alphanumeric with hyphens and underscores. 1-128 characters. Must start with a letter
- **Limits**: No hard limit on collections per database in V1. Document count per collection bounded only by storage
- **Durability**: Collection metadata is durable — survives process restart in both embedded and server modes

## User Stories

### Story US-001: Create a Collection [FEAT-001]

**As a** developer setting up an agentic application
**I want** to create a named collection with a schema
**So that** my agents have a structured place to store documents

**Acceptance Criteria:**
- [ ] `axon collection create <name> --schema <path>` creates a collection
- [ ] Collection name uniqueness is enforced within a database
- [ ] Invalid names are rejected with a clear error message
- [ ] Collection creation is recorded in the audit log
- [ ] Schema is validated before collection creation

### Story US-002: List and Inspect Collections [FEAT-001]

**As a** developer or agent
**I want** to list all collections and inspect their metadata
**So that** I can discover what data is available and its structure

**Acceptance Criteria:**
- [ ] `axon collection list` returns all collections with name, schema version, doc count
- [ ] `axon collection describe <name>` returns full metadata including schema
- [ ] API equivalents return the same information as CLI commands
- [ ] Empty database returns empty list, not an error

### Story US-003: Drop a Collection [FEAT-001]

**As a** developer managing application lifecycle
**I want** to remove a collection that is no longer needed
**So that** I can clean up unused data structures

**Acceptance Criteria:**
- [ ] `axon collection drop <name>` removes the collection and its documents
- [ ] CLI requires `--confirm` flag for destructive operation
- [ ] API requires explicit confirmation parameter
- [ ] Drop operation is recorded in the audit log (including document count at time of drop)
- [ ] Audit records for the dropped collection's documents are retained

## Edge Cases and Error Handling

- **Duplicate name**: Creating a collection with an existing name returns a clear conflict error
- **Drop non-existent**: Dropping a collection that doesn't exist returns a not-found error (not a crash)
- **Concurrent creation**: Two concurrent creates with the same name — exactly one succeeds, one gets conflict error
- **Name validation**: Names with invalid characters get a specific validation error listing the rules
- **Empty schema**: A collection must have a schema. Schemaless collections are not supported (this is a deliberate design choice)

## Success Metrics

- Collection CRUD operations complete within latency targets
- Zero data loss on collection drop (audit records preserved)
- Agent frameworks can discover and use collections programmatically

## Constraints and Assumptions

### Constraints
- Collection names are immutable after creation (rename = create new + migrate + drop old)
- Schema binding is required — no schemaless collections
- Database-level isolation (collections in different databases are fully independent)

### Assumptions
- Most applications will have 5-50 collections
- Collection metadata fits comfortably in memory for reasonable collection counts

## Dependencies

- **FEAT-002** (Schema Engine): Collections require a schema at creation time
- **FEAT-003** (Audit Log): Collection lifecycle events must be audited

## Out of Scope

- Collection-level access control (deferred to auth feature)
- Cross-collection transactions (deferred to P2)
- Collection migration between databases

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #1 (Collections)
- **User Stories**: US-001, US-002, US-003
- **Design Artifacts**: [To be created]
- **Test Suites**: `tests/FEAT-001/`
- **Implementation**: `src/collections/` or equivalent

### Feature Dependencies
- **Depends On**: FEAT-002 (Schema Engine)
- **Depended By**: FEAT-004 (Document Operations), FEAT-006 (Bead Storage Adapter)
