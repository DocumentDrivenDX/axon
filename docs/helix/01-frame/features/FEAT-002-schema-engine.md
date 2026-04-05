---
dun:
  id: FEAT-002
  depends_on:
    - helix.prd
---
# Feature Specification: FEAT-002 - Schema Engine

**Feature ID**: FEAT-002
**Status**: Draft
**Priority**: P0
**Owner**: Core Team
**Created**: 2026-04-04
**Updated**: 2026-04-04

## Overview

The schema engine is Axon's type system. Every collection has a schema that defines the structure of its entities. The schema engine validates all writes, provides clear error messages on violations, and supports schema evolution over time. Schemas are defined in a portable format and provide enough value — validation, documentation, migration, query optimization — that defining them is obviously worthwhile.

## Problem Statement

Agents and applications write malformed data, schemas drift silently between environments, and downstream consumers break on unexpected shapes. Existing schemaless stores (Firebase, MongoDB) trade correctness for convenience; SQL databases provide schemas but require DDL expertise and offer poor error messages for programmatic consumers.

- Current situation: Agent-written data has no structural guarantees. Validation is ad-hoc or absent
- Pain points: Silent data corruption, schema drift, poor error messages, migration pain
- Desired outcome: Schemas that are easy to define, provide instant feedback on violations, and evolve gracefully

## Requirements

### Functional Requirements

- **Schema definition format**: Schemas are defined in a portable format (JSON Schema draft 2020-12 as the base, with Axon extensions). YAML and JSON input supported
- **Type system**: Support for standard types — string, integer, float, boolean, datetime, array, object, enum, reference (foreign key to another collection)
- **Validation on write**: Every create and update operation validates the entity against the collection's schema before persisting. Invalid entities are rejected with structured error responses
- **Structured errors**: Validation errors include field path, expected type, actual value, and human-readable message. Agents can parse errors programmatically and self-correct
- **Required/optional fields**: Fields can be marked required or optional with defaults
- **Nested objects**: Schemas support nested object structures to arbitrary depth
- **Flexible zones**: Schemas can designate specific fields as `additionalProperties: true` — typed at the top level, flexible within a subtree. This accommodates agent metadata without sacrificing top-level structure
- **Schema storage**: Schemas are stored within Axon alongside collection metadata. Schema definitions are versioned
- **Schema inspection**: API and CLI can retrieve the schema for any collection

### Schema Evolution (P1)

- **Additive changes**: Adding optional fields is always safe and automatic
- **Breaking change detection**: Removing required fields, changing types, or narrowing constraints are flagged as breaking
- **Migration support**: Breaking changes require explicit migration declarations
- **Version tracking**: Each schema change increments a version. Entities carry the schema version they were validated against

### Non-Functional Requirements

- **Performance**: Schema validation < 1ms for typical entities (< 100 fields). Must not be the bottleneck on writes
- **Error clarity**: A developer or agent reading a validation error should understand what's wrong and how to fix it without reading the schema definition
- **Portability**: Schema format is standard enough that external tools can consume it (JSON Schema compatibility)

## User Stories

### Story US-004: Define a Collection Schema [FEAT-002]

**As a** developer
**I want** to define a schema for my collection in YAML or JSON
**So that** Axon enforces the structure of entities my agents write

**Acceptance Criteria:**
- [ ] Schema defined in YAML or JSON is accepted at collection creation time
- [ ] Schema supports string, integer, float, boolean, datetime, array, object, enum types
- [ ] Required and optional fields are supported with defaults for optional fields
- [ ] Nested objects are supported
- [ ] Schema is stored and retrievable via API and CLI

### Story US-005: Get Clear Validation Errors [FEAT-002]

**As an** agent writing data to a collection
**I want** structured, actionable error messages when my writes are invalid
**So that** I can self-correct without human intervention

**Acceptance Criteria:**
- [ ] Validation errors include: field path, expected type, actual value, human-readable message
- [ ] Multiple violations in a single entity are all reported (not just the first one)
- [ ] Error response is machine-parseable (structured JSON, not just a string)
- [ ] Error messages suggest the correction (e.g., "field 'status' expected one of [pending, active, done], got 'pendng'")

### Story US-006: Inspect a Schema [FEAT-002]

**As a** developer or agent
**I want** to retrieve the schema for a collection
**So that** I know what fields and types are expected before writing

**Acceptance Criteria:**
- [ ] `axon schema show <collection>` displays the full schema
- [ ] API endpoint returns schema in JSON Schema format
- [ ] Schema includes field descriptions if provided in the definition

## Edge Cases and Error Handling

- **Invalid schema definition**: Schema that doesn't parse or has internal contradictions is rejected at collection creation with specific errors
- **Empty entity**: Writing `{}` to a collection with required fields fails validation
- **Extra fields**: By default, fields not in the schema are rejected. Flexible zones opt-in to extra fields
- **Type coercion**: Axon does NOT silently coerce types (e.g., string "123" is not accepted for an integer field). Explicit types only
- **Null handling**: Fields can be explicitly nullable via schema. Non-nullable fields reject null values
- **Deep nesting**: Schemas with excessive nesting depth (>10 levels) emit a warning but are allowed

## Success Metrics

- 100% of writes are validated against schemas (no bypass path)
- Validation errors are actionable — agents can parse and self-correct
- Schema definition takes < 5 minutes for a typical collection

## Constraints and Assumptions

### Constraints
- JSON Schema draft 2020-12 as the base format — don't invent a new schema language
- No silent type coercion — strict typing
- Schema is required for all collections — no schemaless mode

### Assumptions
- Most collection schemas have 10-50 fields
- JSON Schema is familiar enough to developers that adoption friction is low
- Agents benefit more from strict validation with good errors than from permissive schemas

## Dependencies

- None (schema engine is foundational)

## Out of Scope

- Schema generation from existing data (P2)
- Cross-collection referential integrity enforcement (P2)
- UMF/tablespec import (P2)
- Schema diffing and visualization tools

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #2 (Schema Engine)
- **User Stories**: US-004, US-005, US-006
- **Prior Art**: tablespec UMF format, JSON Schema ecosystem
- **Test Suites**: `tests/FEAT-002/`
- **Implementation**: `src/schema/` or equivalent

### Feature Dependencies
- **Depends On**: None
- **Depended By**: FEAT-001 (Collections), FEAT-004 (Entity Operations)
