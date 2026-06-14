---
ddx:
  id: FEAT-002
  depends_on:
    - helix.prd
  review:
    self_hash: 0e2c69a223cadb6a5d1421cf36a9f91ce49880b66edb0680fd0c229cf1445533
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:39:42Z"
---
# Feature Specification: FEAT-002 — Schema Engine

**Feature ID**: FEAT-002
**Status**: draft
**Priority**: P0
**Owner**: Core Team
**Requirement Prefix**: SCH
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: FR-1 (the active-schema-validation aspect; entity CRUD itself is owned by FEAT-004)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

The schema engine is Axon's type system, implementing the "active schema validation" requirement of PRD FR-1. Every collection has a schema that defines the structure of its entities; the schema engine validates all writes and provides structured, actionable error messages on violations. Schemas are defined in a portable format and provide enough value — validation, documentation, query support — that defining them is obviously worthwhile.

Schema evolution — breaking-change classification, migration, revalidation, and diff — is owned by FEAT-017; this feature provides the validation primitives FEAT-017 builds on.

## Ideal Future State

A developer defines a collection schema in minutes using a portable, familiar format, and from that moment every write is validated with no bypass path. When an agent submits malformed data, the rejection tells it exactly which field failed, what constraint was expected, and what value it sent — precise enough that the agent self-corrects without human intervention. External tools can consume the schema directly because it stays close to a published standard.

## Problem Statement

- **Current situation**: Agent-written data has no structural guarantees; validation is ad-hoc or absent. Existing schemaless stores trade correctness for convenience; SQL databases provide schemas but require DDL expertise and offer poor error messages for programmatic consumers.
- **Pain points**: Silent data corruption, schema drift between environments, poor error messages that block agent self-correction, downstream consumers breaking on unexpected shapes.
- **Desired outcome**: Schemas that are easy to define, provide instant structured feedback on violations, and are portable to external tooling.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Schema definition | How do I declare the shape of my entities? | Portable definition format, type system, required/optional fields, nesting, flexible zones |
| Write validation | Is this entity acceptable, and if not, why exactly? | Validate every write against the active schema; structured, complete, actionable errors |
| Storage and inspection | What schema governs this collection right now? | Versioned schema storage; retrieval via API and CLI in a portable format |

## Requirements

### Functional Requirements by Area

#### Schema Definition

- **SCH-01**. Schemas MUST be defined in the portable Entity Schema Format (ESF) — JSON Schema draft 2020-12 as the base with Axon extensions. YAML and JSON input are accepted. The normative format is defined in CONTRACT-010.
- **SCH-02**. The type system MUST support string, integer, float, boolean, datetime, array, object, enum, and reference (link to another collection) types.
- **SCH-03**. Fields MUST be markable as required or optional, with declared defaults for optional fields.
- **SCH-04**. Schemas MUST support nested object structures to arbitrary depth.
- **SCH-05**. Schemas MUST support flexible zones: designated subtrees that accept undeclared properties while the top level remains typed, accommodating agent metadata without sacrificing structure. Declaration surface per CONTRACT-010.
- **SCH-06**. A schema that fails to parse or contains internal contradictions MUST be rejected at submission time with specific errors.

#### Write Validation

- **SCH-07**. Every create and update operation MUST validate the entity against the collection's active schema before persisting. Invalid entities are rejected; there is no bypass path.
- **SCH-08**. Every validation error MUST name the failing field path, the violated constraint with its expected value (type, enum members, bound, or pattern), the actual offending value, and a human-readable message. Where the constraint admits a suggestion (e.g. a near-miss enum value), the message includes it. The error envelope is machine-parseable per CONTRACT-010.
- **SCH-09**. A single invalid write MUST report all violations in the entity, not only the first one encountered.
- **SCH-10**. Axon MUST NOT silently coerce types (e.g. string "123" is not accepted for an integer field). Explicit types only.

#### Storage and Inspection

- **SCH-11**. Schemas MUST be stored within Axon alongside collection metadata, and schema definitions MUST be versioned: each accepted schema change increments the version (ADR-007).
- **SCH-12**. The schema for any collection MUST be retrievable via the API and CLI in its portable format, including field descriptions when provided — surface per CONTRACT-001/CONTRACT-008.

### Non-Functional Requirements

- **Performance**: Schema validation < 1ms for typical entities (< 100 fields); validation must not be the bottleneck on writes.
- **Error clarity**: 100% of validation errors carry field path, violated constraint with expected value, and actual value (SCH-08); a developer or agent can correct the write without reading the schema definition.
- **Portability**: Schema format remains consumable by standard JSON Schema tooling for the non-extended subset.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-004 | Define a Collection Schema | [US-004](../user-stories/US-004-define-a-collection-schema.md) |
| US-005 | Get Clear Validation Errors | [US-005](../user-stories/US-005-get-clear-validation-errors.md) |
| US-006 | Inspect a Schema | [US-006](../user-stories/US-006-inspect-a-schema.md) |

## Edge Cases and Error Handling

- **Invalid schema definition**: A schema that doesn't parse or has internal contradictions is rejected at submission with specific errors (SCH-06).
- **Empty entity**: Writing `{}` to a collection with required fields fails validation, reporting every missing required field.
- **Extra fields**: By default, fields not in the schema are rejected. Flexible zones opt in to extra fields (SCH-05).
- **Type coercion**: No silent coercion (SCH-10).
- **Null handling**: Fields can be explicitly nullable via the schema; non-nullable fields reject null values.
- **Deep nesting**: Schemas with excessive nesting depth (> 10 levels) emit a warning but are allowed.

## Success Metrics

- 100% of writes are validated against schemas (no bypass path).
- Validation errors are actionable: an agent can parse the error and produce a corrected write without human intervention.
- Schema definition takes < 5 minutes for a typical collection.

## Constraints and Assumptions

### Constraints
- JSON Schema draft 2020-12 is the base format — do not invent a new schema language.
- No silent type coercion — strict typing.
- A schema is required for every collection — no schemaless mode.

### Assumptions
- Most collection schemas have 10-50 fields.
- JSON Schema is familiar enough to developers that adoption friction is low.
- Agents benefit more from strict validation with good errors than from permissive schemas.

## Dependencies

- **Other features**: None — the schema engine is foundational. FEAT-001 (Collections) and FEAT-004 (Entity Operations) consume it; FEAT-017 (Schema Evolution) extends it.
- **External services**: None. Normative interface surface: CONTRACT-010 (ESF schema format and validation error envelope), CONTRACT-001 (HTTP API), CONTRACT-008 (CLI and config).
- **PRD requirements**: FR-1 (P0).

## Out of Scope

- Schema evolution, breaking-change classification, migration, entity revalidation, and schema diff (owned by FEAT-017).
- Schema generation from existing data (P2).
- Cross-collection referential integrity enforcement (P2).
- UMF/tablespec import (P2).
- Schema visualization tools.

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
