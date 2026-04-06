---
dun:
  id: FEAT-017
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-013
    - ADR-007
---
# Feature Specification: FEAT-017 - Schema Evolution and Migration

**Feature ID**: FEAT-017
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

Schema evolution allows collection schemas to change over time without
requiring downtime or data loss. ADR-007 provides schema versioning
(auto-increment, history table). This feature builds on that foundation
with breaking change detection, compatibility classification, and
entity revalidation.

## Problem Statement

Schemas change as applications evolve: new fields are added, field types
are tightened, required fields are introduced. Without a migration
strategy, schema changes either break existing entities silently or are
rejected entirely, forcing manual data migration.

Agents and operators need to evolve schemas confidently — knowing which
changes are safe, which require migration, and what the impact is on
existing data.

## Requirements

### Functional Requirements

#### Compatibility Classification

When a schema is updated via `put_schema`, the system classifies the
change:

| Classification | Definition | Example | Behavior |
|---|---|---|---|
| **Compatible** | All existing entities remain valid under the new schema | Add optional field, widen enum, relax constraint | Apply immediately |
| **Breaking** | Some existing entities may be invalid under the new schema | Add required field, remove field, narrow enum, tighten type | Require explicit confirmation or migration plan |
| **Metadata-only** | No entity validation impact | Change description, reorder fields, add/remove index | Apply immediately |

- **Breaking change detection**: Before applying a breaking schema change,
  the system reports which fields changed, how many entities are
  potentially affected, and what the validation failures would be
- **Force flag**: Breaking changes can be applied with `--force` /
  `force: true`, accepting that some entities may be invalid until
  migrated
- **Dry-run mode**: `put_schema --dry-run` reports the compatibility
  classification and potential impact without applying the change

#### Entity Revalidation

- **On-demand revalidation**: Admin operation to validate all entities in
  a collection against the current schema. Reports invalid entities with
  their validation errors
- **Background revalidation**: When a breaking schema change is applied,
  a background worker validates all existing entities and reports results.
  Invalid entities are flagged but not modified
- **Validation report**: Returns a list of `{entity_id, version, errors}`
  for all entities that fail validation

#### Schema Diff

- **Field-level diff**: When a schema changes, produce a structured diff
  showing added, removed, modified, and unchanged fields
- **Diff in audit log**: Schema change audit entries include the
  field-level diff
- **Diff in CLI/UI**: `axon schema diff <collection> <version-a> <version-b>`
  shows the diff between two schema versions
- **Diff in GraphQL**: `schemaVersion` query includes a `diff` field
  showing changes from the previous version

#### Migration Declarations (V2 — Design Only)

For V1, schema evolution is limited to compatibility detection and
force-apply. Full migration support (transform rules, backfill) is
designed but deferred:

- **Migration rules**: Declarative transformations attached to a schema
  version — "field X renamed to Y", "field Z default value is 0",
  "remove field W"
- **Backfill worker**: Background process that applies migration rules
  to existing entities
- **Rollback**: Revert to a previous schema version (requires reverse
  migration rules)

These are V2 capabilities. V1 provides the foundation: versioning,
compatibility detection, and revalidation.

### Non-Functional Requirements

- **Compatibility check latency**: < 100ms for schema comparison
  (schema-only analysis, no entity scanning)
- **Revalidation throughput**: > 10K entities/second for background
  validation
- **Diff generation**: < 10ms for field-level diff between two versions

## User Stories

### Story US-058: Detect Breaking Schema Changes [FEAT-017]

**As a** developer evolving a collection schema
**I want** the system to tell me if my change is breaking
**So that** I don't accidentally invalidate existing data

**Acceptance Criteria:**
- [ ] Adding an optional field is classified as `compatible`
- [ ] Adding a required field is classified as `breaking`
- [ ] Removing a field that exists in stored entities is classified as `breaking`
- [ ] Widening an enum (adding values) is classified as `compatible`
- [ ] Narrowing an enum (removing values) is classified as `breaking`
- [ ] Tightening a type constraint (e.g., `minLength: 1` → `minLength: 5`) is classified as `breaking`
- [ ] Breaking change response includes count of potentially affected entities
- [ ] `put_schema --dry-run` reports classification without applying the change

### Story US-059: Force-Apply a Breaking Change [FEAT-017]

**As a** developer who understands the impact
**I want** to apply a breaking schema change with explicit confirmation
**So that** I can evolve the schema even when existing data doesn't conform

**Acceptance Criteria:**
- [ ] Breaking schema change without `force` flag is rejected with the compatibility report
- [ ] Breaking schema change with `force: true` succeeds and increments schema version
- [ ] Audit entry for forced breaking change includes the compatibility classification and diff
- [ ] After force-apply, `revalidate` reports which entities are now invalid

### Story US-060: Revalidate Entities Against Current Schema [FEAT-017]

**As an** operator after a schema change
**I want** to find all entities that don't conform to the current schema
**So that** I can fix or migrate them

**Acceptance Criteria:**
- [ ] `axon schema revalidate <collection>` scans all entities and reports invalid ones
- [ ] Report includes entity ID, version, and specific validation errors per entity
- [ ] Valid entities are not modified or flagged
- [ ] Revalidation runs as a background operation for large collections (> 1000 entities)
- [ ] Progress is reported (entities scanned / total)

### Story US-061: View Schema Diff [FEAT-017]

**As a** developer debugging a schema change
**I want** to see exactly what changed between two schema versions
**So that** I can understand the evolution history

**Acceptance Criteria:**
- [ ] `axon schema diff <collection> <v1> <v2>` shows added, removed, and modified fields
- [ ] Diff includes field type changes, constraint changes, and enum value changes
- [ ] Schema change audit entry includes the field-level diff
- [ ] Diff between non-adjacent versions (e.g., v1 to v5) works correctly

## Edge Cases

- **Revalidation of empty collection**: Returns success with zero invalid
  entities
- **Compatible change to schema with no entities**: Applied immediately,
  no revalidation needed
- **Breaking change with zero entities affected**: Classified as breaking
  (schema analysis only) but revalidation finds zero invalid entities
- **Schema change that affects indexes**: If an indexed field's type
  changes, the index must be rebuilt (FEAT-013 rebuild operation)
- **Concurrent schema change**: Two `put_schema` calls — one wins via
  the version auto-increment (ADR-007); the other gets a conflict
- **Revalidation during active writes**: New entities are validated
  against the current schema at write time. Background revalidation
  may report an entity that is subsequently updated to conform

## Dependencies

- **FEAT-002** (Schema Engine): Schema validation primitives
- **FEAT-013** (Secondary Indexes): Index rebuild when field types change
- **ADR-007**: Schema versioning and history table

## Out of Scope

- **Automatic migration rules**: V2. V1 detects breaking changes but
  doesn't auto-fix entities
- **Migration backfill worker**: V2. V1 provides revalidation (read-only)
  but not transformation
- **Schema rollback**: V2. V1 keeps history (ADR-007) but doesn't
  support reverting to an old version
- **Cross-collection migration**: Splitting/merging collections

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #1 (Schema evolution)
- **User Stories**: US-058, US-059, US-060, US-061
- **Architecture**: ADR-007 (Schema Versioning)
- **Implementation**: `crates/axon-schema/` (diff, compatibility),
  `crates/axon-api/` (revalidation)

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-013
- **Depended By**: FEAT-011 (Admin UI schema diff view)
