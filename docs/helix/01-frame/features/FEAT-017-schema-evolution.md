---
ddx:
  id: FEAT-017
  depends_on:
    - helix.prd
  review:
    self_hash: 7589f2ef1950a23cd5b4572f4ab88b8c30a9cb3421a6a63138dde3e6a0619f97
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-14T04:25:45Z"
---
# Feature Specification: FEAT-017 — Schema Evolution and Migration

**Feature ID**: FEAT-017
**Status**: draft
**Priority**: P1
**Owner**: Core Team
**Requirement Prefix**: EVO
**Covered PRD Subsystem(s)**: Entity-Graph Data Model
**Covered PRD Requirements**: PRD Should-Have P1-1 (schema evolution and migration); FR-1 (validation against the active schema as it changes over time)
**Cross-Subsystem Rationale**: None — single subsystem.

## Overview

Schema evolution allows collection schemas to change over time without downtime or data loss, implementing PRD Should-Have P1-1: classify breaking changes, validate existing data, and support safe additive evolution. Building on schema versioning (ADR-007) and the validation primitives of FEAT-002, this feature is the sole owner of compatibility classification, breaking-change detection, entity revalidation, schema diff, and read compatibility across versions.

## Ideal Future State

A developer evolves a schema confidently: every proposed change is classified as compatible, breaking, or metadata-only before it applies; additive changes go through with zero downtime; breaking changes require an informed, explicit decision backed by an impact report. Operators can find every entity that no longer conforms, see exactly what changed between any two schema versions, and live consumers keep reading old-version entities safely during a rolling schema change — no coordination required between old and new readers and writers.

## Problem Statement

- **Current situation**: Schemas change as applications evolve — new fields are added, types are tightened, required fields are introduced. Without an evolution strategy, schema changes either break existing entities silently or are rejected entirely, forcing manual data migration.
- **Pain points**: Developers cannot tell whether a change is safe before applying it; operators cannot find non-conforming entities after a change; readers of old-version entities fail during rolling changes with live consumers.
- **Desired outcome**: Agents and operators evolve schemas knowing which changes are safe, which require migration, and what the impact on existing data is — with old-version entities remaining readable throughout.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Compatibility classification | Is this schema change safe to apply? | Classify every change; report impact; gate breaking changes behind explicit confirmation |
| Entity revalidation | Which entities no longer conform? | On-demand and post-change validation sweeps with per-entity reports |
| Schema diff | What exactly changed between versions? | Structured field-level diffs in audit, CLI, and API surfaces |
| Read compatibility | Can old-version entities still be read after a bump? | Lazy-read semantics with declared read-time defaults |

## Requirements

### Functional Requirements by Area

#### Compatibility Classification

When a schema update is submitted, the system MUST classify the change:

| Classification | Definition | Example | Behavior |
|---|---|---|---|
| **Compatible** | All existing entities remain valid under the new schema | Add optional field, widen enum, relax constraint | Apply immediately, zero downtime |
| **Breaking** | Some existing entities may be invalid under the new schema | Add required field, remove field, narrow enum, tighten type | Require explicit confirmation or migration plan |
| **Metadata-only** | No entity validation impact | Change description, reorder fields, add/remove index | Apply immediately, zero downtime |

- **EVO-01**. Every submitted schema update MUST be classified as compatible, breaking, or metadata-only per the table above before any change applies.
- **EVO-02**. Before a breaking schema change applies, the system MUST report which fields changed, how many entities are potentially affected, and what the validation failures would be.
- **EVO-03**. Adding a required field MUST require a default value or a migration plan — the system MUST NOT accept a required-field addition without a path for existing entities.
- **EVO-04**. Narrowing a constraint (e.g. removing enum values) MUST validate existing data and report violations; the system MUST NOT silently retain data that violates the new constraint without reporting it.
- **EVO-05**. Entities MUST track which schema version they were created or last validated against (ADR-007).
- **EVO-06**. A breaking change without explicit force confirmation MUST be rejected with the compatibility report; with explicit confirmation it applies and increments the schema version, accepting that some entities may be invalid until migrated. Confirmation surface per CONTRACT-008 (CLI) and CONTRACT-001 (API).
- **EVO-07**. Schema updates MUST support a dry-run mode that reports the compatibility classification and potential impact without applying the change — surface per CONTRACT-008/CONTRACT-001.
- **EVO-08**. Audit entries for applied schema changes MUST include the compatibility classification, and for forced breaking changes the fact that force was used.

#### Entity Revalidation

- **EVO-09**. Operators MUST be able to run an on-demand revalidation of all entities in a collection against the current schema, reporting each invalid entity with its identifier, version, and specific validation errors.
- **EVO-10**. When a breaking schema change is applied, a background revalidation MUST validate all existing entities and report results; invalid entities are flagged but never modified.
- **EVO-11**. Revalidation of large collections MUST run as a background operation with progress reporting (entities scanned / total).

#### Schema Diff

- **EVO-12**. The system MUST produce a structured field-level diff between any two schema versions — including non-adjacent versions — showing added, removed, and modified fields, covering type changes, constraint changes, and enum value changes.
- **EVO-13**. Schema-change audit entries MUST include the field-level diff.
- **EVO-14**. The diff MUST be available to developers and operators through the CLI and the query API — surface per CONTRACT-008 (CLI) and CONTRACT-002 (GraphQL).

#### Read Compatibility

- **EVO-15**. Reading an entity stored at an older schema version against a newer active schema MUST succeed, and the entity MUST report its actual stored schema version.
- **EVO-16**. Schemas MAY declare read-time defaults for fields added in later versions; when an older-version entity is read, those fields are populated from the declared defaults before being returned. Declaration surface per CONTRACT-010 (ESF).
- **EVO-17**. Fields added in a later version with no declared read-time default MUST be returned as null or omitted, per the field's schema declaration.
- **EVO-18**. When a force-applied change adds a required field with no default, reads of older-version entities MUST succeed with the field absent (or null) plus a structured warning; the required-with-no-default constraint applies to writes only.
- **EVO-19**. Lazy read MUST NOT modify storage: an entity remains at its stored schema version until its next write, at which point normal validation against the active schema applies. Operators can opt into eager revalidation via EVO-09; lazy read is the runtime default.

### Non-Functional Requirements

- **Compatibility check latency**: < 100ms for schema comparison (schema-only analysis, no entity scanning).
- **Revalidation throughput**: > 10K entities/second for background validation.
- **Diff generation**: < 10ms for a field-level diff between two versions.

## User Stories

| ID | Title | Link |
|----|-------|------|
| US-058 | Detect Breaking Schema Changes | [US-058](../user-stories/US-058-detect-breaking-schema-changes.md) |
| US-059 | Force-Apply a Breaking Change | [US-059](../user-stories/US-059-force-apply-a-breaking-change.md) |
| US-060 | Revalidate Entities Against Current Schema | [US-060](../user-stories/US-060-revalidate-entities-against-current-schema.md) |
| US-061 | View Schema Diff | [US-061](../user-stories/US-061-view-schema-diff.md) |
| US-125 | Lazy-Read Schema Migration | [US-125](../user-stories/US-125-lazy-read-schema-migration.md) |

## Edge Cases and Error Handling

- **Revalidation of empty collection**: Returns success with zero invalid entities.
- **Compatible change to schema with no entities**: Applied immediately; no revalidation needed.
- **Breaking change with zero entities affected**: Classified as breaking (schema analysis only); revalidation finds zero invalid entities.
- **Schema change that affects indexes**: If an indexed field's type changes, the affected index must be rebuilt (FEAT-013).
- **Concurrent schema change**: Two simultaneous schema updates — one wins via the version increment (ADR-007); the other receives a conflict error.
- **Revalidation during active writes**: New entities are validated against the current schema at write time; background revalidation may report an entity that is subsequently updated to conform.

## Success Metrics

- 100% of schema updates receive a compatibility classification before applying; zero breaking changes apply without explicit confirmation.
- Zero read failures for old-version entities after a schema bump (rolling changes require no reader/writer coordination).
- Operators can produce a complete non-conformance report for any collection after a breaking change.

## Constraints and Assumptions

### Constraints
- Schema versioning semantics (auto-increment, history) are fixed by ADR-007; this feature builds on them rather than redefining them.
- Revalidation is read-only: it reports non-conforming entities but never mutates them.

### Assumptions
- Breaking changes are rare relative to additive changes; the common path is zero-downtime compatible evolution.
- Live consumers at mixed schema versions are a normal operating condition during rolling changes, not an error state.

## Dependencies

- **Other features**: FEAT-002 (Schema Engine — validation primitives and versioned schema storage), FEAT-013 (Secondary Indexes — index rebuild when field types change).
- **External services**: None. Normative interface surface: CONTRACT-008 (CLI and config), CONTRACT-001 (HTTP API), CONTRACT-002 (GraphQL), CONTRACT-010 (ESF schema format, including read-time default declarations).
- **PRD requirements**: Should-Have P1-1 (schema evolution and migration); FR-1 (P0).

## Out of Scope

- **Migration declarations (explicitly deferred to V2 — design only)**: declarative transform rules attached to a schema version ("field X renamed to Y", "field Z default value is 0", "remove field W"), a backfill worker that applies such rules to existing entities, and schema rollback to a previous version (which requires reverse migration rules). V1 provides the foundation — versioning, compatibility detection, revalidation, and lazy-read defaults — but no automatic transformation of stored entities.
- **Schema-version-aware transformation beyond simple read-time defaults** (field renames, nested-shape changes, type conversions) — V2 transform-rule territory.
- **Cross-collection migration**: splitting or merging collections.

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
