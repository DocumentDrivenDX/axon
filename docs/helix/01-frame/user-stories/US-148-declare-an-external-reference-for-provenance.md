---
ddx:
  id: US-148
---
# US-148: Declare an External Reference for Provenance

**Feature**: FEAT-007 — Entity-Graph Data Model
**Feature Requirements**: GRF-13, GRF-14, GRF-15
**PRD Requirements**: FR-34
**Priority**: P2
**Status**: Draft

## Story

**As a** Wei, a business workflow builder whose records originate in an ERP, spreadsheet, or SaaS tool
**I want** to declare a field that points at the record's external system and ID
**So that** I can trace an Axon entity back to its source and avoid creating duplicates, without Axon reconciling that external system itself

## Context

Exercises GRF-13 (external-reference field declaration), GRF-14 (indexable
and optionally unique (system, external ID) pair via FEAT-013), and GRF-15
(schema introspection surfaces the field as a reference, not ordinary data).
Motivated by a comparison against Palantir Foundry's Ontology/OSv2: unlike
OSv2's datasource mapping, Axon does not ingest or reconcile the external
system — it only records a provenance pointer.

## Walkthrough

1. Wei declares a `vendor_ref` external-reference field on the `Vendor`
   collection's schema, with a system identifier and an external ID.
2. Wei declares a unique compound index on `(vendor_ref.system,
   vendor_ref.external_id)` using the existing secondary-index mechanism
   (FEAT-013).
3. Wei creates a `Vendor` entity carrying `vendor_ref = { system: "sap",
   external_id: "V-10231" }`.
4. Wei attempts to create a second `Vendor` entity with the same
   `vendor_ref` pair; the write is rejected as a uniqueness conflict.
5. Wei inspects the collection's schema through the admin UI or schema
   introspection API and sees `vendor_ref` identified as an external
   reference, distinct from ordinary business fields.

## Acceptance Criteria

- [ ] **US-148-AC1** — Given a schema declaring an external-reference field
  with a system identifier and external ID, when an entity is created with
  that field populated, then the entity is stored and validated like any
  other schema-declared field, with no referential-integrity check against
  the external system.
- [ ] **US-148-AC2** — Given a unique compound index declared on an
  external-reference field's (system, external ID) pair, when a second
  entity is created with the same pair, then the write is rejected as a
  uniqueness conflict identifying the conflicting field and value.
- [ ] **US-148-AC3** — Given no uniqueness constraint declared on an
  external-reference field, when two entities are created with the same
  (system, external ID) pair, then both writes succeed — Axon does not
  implicitly deduplicate.
- [ ] **US-148-AC4** — Given a schema with an external-reference field, when
  a client queries schema introspection (or generated GraphQL/MCP metadata),
  then the field is identified as an external reference distinct from
  ordinary data fields.

## Edge Cases

- **Missing external ID**: an external-reference field missing its declared
  required parts fails ordinary schema validation, the same as any other
  required field.
- **No last-synced timestamp**: `last_synced_at` is optional; its absence
  does not block writes, and Axon never sets or refreshes it on its own.
- **Traversal attempt**: an external-reference field is not a link and does
  not appear in graph traversal (FEAT-009) results.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Basic declaration | US-148-AC1 | Schema with `vendor_ref` external-reference field | Create entity with `vendor_ref` populated | Entity stored; no external-system check performed |
| Duplicate with unique index | US-148-AC2 | Unique index on `(system, external_id)`; one entity with `sap`/`V-10231` exists | Create second entity with same pair | Uniqueness conflict error naming the field and value |
| Duplicate without unique index | US-148-AC3 | No uniqueness constraint declared | Create two entities with the same pair | Both succeed |
| Introspection identifies the field | US-148-AC4 | Schema with `vendor_ref` | Query schema introspection | `vendor_ref` marked as an external reference |

## Dependencies

- **Stories**: US-032 (enforce uniqueness via index — the mechanism this story reuses).
- **Feature Spec**: [FEAT-007 — Entity-Graph Data Model](../features/FEAT-007-entity-graph-model.md)
- **Feature Requirements**: GRF-13, GRF-14, GRF-15
- **PRD Requirements**: FR-34
- **External**: CONTRACT-010 (external-reference field grammar, to be added when implemented); FEAT-013 / CONTRACT-010 (secondary-index and uniqueness declaration grammar this story reuses)

## Out of Scope

- Ingesting, transforming, deduplicating, or reconciling data from the
  referenced external system.
- Automatically refreshing `last_synced_at` or the referenced record.
- Cross-system identity resolution or fuzzy matching.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
