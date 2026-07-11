---
ddx:
  id: FEAT-001
  depends_on:
    - helix.prd
  review:
    self_hash: fef81eac3824d481ea889c8402ec5f2d7e6ecfa7f396186f18fa49ed8319a1cf
    deps:
      helix.prd: 6703170c71275bba7d108c4f9c329d32e4104f9c965278db888ad43cdc3ca367
    reviewed_at: "2026-07-11T02:26:23Z"
---
# Feature Specification: FEAT-001 — Collections

**Feature ID**: FEAT-001
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Requirement Prefix**: COL
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-1 (the collection container, lifecycle, and discovery aspects; entity CRUD itself is owned by FEAT-004)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Collections are the foundational data container in Axon. A collection is a named, schema-bound container of entities within a schema namespace inside a database. This feature implements the container half of PRD FR-1: entities are stored, validated, governed, and audited within named collections, which provide the organizational unit for schema enforcement, policy scope, and audit boundaries.

## Ideal Future State

A developer or agent creates a named, schema-bound collection in a single command or API call, discovers existing collections and their structure programmatically, renames a collection without breaking references or history, and removes a collection knowing its audit trail survives. Nothing is stored "loose": every entity lives in a collection, and every collection carries a schema, a stable identity, and an audit boundary from the moment it exists.

## Problem Statement

- **Current situation**: Agents dump data into whatever storage is convenient — raw database tables, JSON files, schemaless document stores — with no consistency guarantees.
- **Pain points**: No naming conventions, no schema binding, no lifecycle management, no discovery. Setting up structured storage correctly requires too much boilerplate.
- **Desired outcome**: Named collections with schemas, discoverable via API, manageable via CLI, with audited lifecycle events.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Naming and identity | How are collections addressed and kept unambiguous? | Unique naming within the schema namespace; stable identity independent of name |
| Lifecycle | Create, rename, and remove containers safely | Schema-bound creation, rename without data rewrite, confirmed drop with audit retention |
| Discovery | What collections exist and what do they hold? | List and describe with metadata via API and CLI |

## Requirements

### Functional Requirements by Area

#### Naming and Identity

- **COL-01**. Collection names MUST be unique within their schema namespace. The fully qualified name of a collection is `database.schema.collection` (FEAT-014, ADR-011); the same collection name MAY exist in different schema namespaces or databases without conflict.
- **COL-02**. Each collection MUST carry a stable identity independent of its name (ADR-010 stable numeric collection IDs), so that renaming a collection does not rewrite entity data, audit records, or references.
- **COL-03**. Invalid collection names MUST be rejected with a structured error naming the violated rule. The normative naming grammar lives in CONTRACT-001/CONTRACT-008.

#### Lifecycle

- **COL-04**. Axon MUST create a named collection bound to a schema. The schema is validated (FEAT-002) before the collection is created, and creation is recorded in the audit log (FEAT-003).
- **COL-05**. Axon MUST support renaming a collection without downtime and without rewriting entity data; existing audit records and references remain valid via the stable identity (COL-02, ADR-010). Renames are recorded in the audit log.
- **COL-06**. Axon MUST drop a collection and its entities only with explicit confirmation. Audit records for the collection and its entities MUST be retained, and the drop event MUST be audited including the entity count at drop time.
- **COL-07**. Schema binding is mandatory: a collection MUST NOT exist without a schema. Schemaless collections are not supported. The internal contract is fail-closed for governed collection-addressable state.

#### Discovery

- **COL-08**. Axon MUST enumerate all collections in scope with metadata: name, schema version, entity count, created/updated timestamps.
- **COL-09**. Axon MUST describe a single collection with full metadata including its schema, declared indexes, and statistics.
- **COL-10**. Collections MUST be manageable via the CLI and the HTTP API with equivalent behavior and information — surface per CONTRACT-008 and CONTRACT-001.
- **COL-11**. Each collection MUST track creation time, schema version, entity count, and last modified time.

### Non-Functional Requirements

- **Performance**: Collection create/rename/drop < 100ms; list/describe < 50ms (95th percentile).
- **Limits**: No hard limit on collections per database in V1. Entity count per collection bounded only by storage.
- **Durability**: Collection metadata survives process restart in both embedded and server modes.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-001 | Create a Collection | [US-001](../user-stories/US-001-create-a-collection.md) |
| US-002 | List and Inspect Collections | [US-002](../user-stories/US-002-list-and-inspect-collections.md) |
| US-003 | Drop a Collection | [US-003](../user-stories/US-003-drop-a-collection.md) |

## Edge Cases and Error Handling

- **Duplicate name**: Creating a collection whose name already exists in the same schema namespace returns a structured conflict error.
- **Drop non-existent**: Dropping a collection that does not exist returns a not-found error, not a crash.
- **Concurrent creation**: Two concurrent creates with the same fully qualified name — exactly one succeeds; the other receives a conflict error.
- **Rename collision**: Renaming a collection to a name already taken in the same schema namespace returns a conflict error and leaves both collections unchanged.
- **Name validation**: Names with invalid characters receive a specific validation error citing the naming rules.
- **Empty schema**: Creating a collection without a schema is rejected (COL-07 — a deliberate design choice).

## Success Metrics

- Collection lifecycle and discovery operations complete within the NFR latency targets.
- Zero audit data loss on collection drop or rename (audit records preserved and addressable).
- Agent frameworks can discover and use collections programmatically without out-of-band knowledge.

## Constraints and Assumptions

### Constraints
- Schema binding is required — no schemaless collections; governed collection-addressable state fails closed without an active schema.
- Database-level isolation: collections in different databases are fully independent.
- Collections exist within a schema namespace within a database (FEAT-014, ADR-011).

### Assumptions
- Most applications will have 5-50 collections.
- Collection metadata fits comfortably in memory for reasonable collection counts.

## Dependencies

- **Other features**: FEAT-002 (Schema Engine — collections require a validated schema at creation), FEAT-003 (Audit Log — lifecycle events must be audited), FEAT-014 (Multi-Tenancy — schema namespace and database hierarchy).
- **External services**: None. Normative interface surface: CONTRACT-001 (HTTP API), CONTRACT-008 (CLI and config).
- **PRD requirements**: FR-1 (P0).

## Out of Scope

- Collection-level access control and visibility policy (owned by the policy enforcement features, FEAT-012/FEAT-029).
- Transactional semantics for multi-entity and cross-collection writes (owned by FEAT-008).
- Entity CRUD and query behavior inside a collection (owned by FEAT-004).
- Collection migration between databases.

## Review Checklist

Use this checklist when reviewing this feature specification:

- [ ] Covered PRD Subsystem(s) and Requirements (`FR-n`) are listed; a feature spanning >1 subsystem carries an explicit cross-subsystem rationale (else split per the Decomposition test)
- [ ] Functional areas (if any) are subordinate parts of this one capability, not separate capabilities
- [ ] Overview connects this feature to a specific PRD requirement
- [ ] Ideal future state describes the desired user-visible outcome, not only current problems
- [ ] Problem statement describes what exists now and what is broken — not just what is wanted
- [ ] Every functional requirement is testable — you can write an assertion for it
- [ ] Acceptance criteria are defined in the user stories that decompose this feature, not here (ADR-009)
- [ ] Non-functional requirements have specific numeric targets
- [ ] Edge cases cover realistic failure scenarios, not just happy paths
- [ ] Success metrics are specific to this feature, not product-level metrics
- [ ] Dependencies reference real artifact IDs
- [ ] Out of scope excludes things someone might reasonably assume are in scope
- [ ] No implementation details — WHAT not HOW
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
- [ ] Feature is consistent with governing PRD requirements
- [ ] No `[NEEDS CLARIFICATION]` markers remain unresolved for P0 features
