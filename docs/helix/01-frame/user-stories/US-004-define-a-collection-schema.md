---
ddx:
  id: US-004
---

# US-004: Define a Collection Schema

**Feature**: FEAT-002 — Schema Engine
**Feature Requirements**: SCH-01, SCH-02, SCH-03, SCH-04, SCH-11
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As a** developer
**I want** to define a schema for my collection in YAML or JSON
**So that** Axon enforces the structure of entities my agents write

## Context

Without a declared schema there is nothing for Axon to enforce, and agent-written data has no structural guarantees. This story exercises SCH-01 (portable Entity Schema Format with YAML/JSON input), SCH-02 (type system), SCH-03 (required/optional fields with defaults), SCH-04 (nested objects), and SCH-11 (versioned storage). The normative schema document format is CONTRACT-010; submission and retrieval surfaces are CONTRACT-001/CONTRACT-008.

## Walkthrough

1. Developer writes a schema document in YAML or JSON using the Entity Schema Format (CONTRACT-010), declaring fields, types, required/optional markers, defaults, and nested structures.
2. Developer supplies the schema at collection creation time via the CLI or HTTP API (CONTRACT-008/CONTRACT-001).
3. System validates the schema document itself and accepts it.
4. System stores the schema alongside the collection metadata as version 1, and the schema is retrievable via API and CLI.

## Acceptance Criteria

- [ ] **US-004-AC1** — Given a valid schema document in YAML or JSON, when it is supplied at collection creation time, then it is accepted and bound to the collection.
- [ ] **US-004-AC2** — Given a schema using string, integer, float, boolean, datetime, array, object, enum, and reference types, when the schema is submitted, then all of those types are accepted and enforced on subsequent writes.
- [ ] **US-004-AC3** — Given a schema marking some fields required and others optional with defaults, when an entity omits an optional field with a default, then the stored entity carries the declared default.
- [ ] **US-004-AC4** — Given a schema with nested object structures, when entities with matching nested shapes are written, then they validate successfully.
- [ ] **US-004-AC5** — Given a collection with a bound schema, when the schema is requested via API or CLI (CONTRACT-001/CONTRACT-008), then the stored schema document and its version are returned.

## Edge Cases

- **Invalid schema document**: A schema that doesn't parse or contains internal contradictions is rejected at submission with specific errors (SCH-06).
- **Equivalent YAML and JSON input**: The same schema expressed in YAML or JSON produces identical stored schemas.
- **Required field with a default**: Accepted per the Entity Schema Format rules in CONTRACT-010.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-004-AC1 | YAML schema for `invoices` with `id`, `amount`, `status` | Create collection with schema | Schema accepted; collection bound to schema v1 |
| All types | US-004-AC2 | Schema using every supported type including enum and reference | Submit schema, then write a conforming entity | Schema accepted; entity validates |
| Optional default | US-004-AC3 | Optional field `currency` with default `"USD"` | Write entity omitting `currency` | Stored entity has `currency = "USD"` |
| Nested objects | US-004-AC4 | Schema with `address.geo.lat` three levels deep | Write entity with matching nesting | Entity validates |
| Retrieval | US-004-AC5 | Collection bound to schema v1 | Request schema via CLI and API | Same schema document and version returned |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-002
- **Feature Requirements**: SCH-01, SCH-02, SCH-03, SCH-04, SCH-11
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010, CONTRACT-001, CONTRACT-008

## Out of Scope

- Collection creation mechanics and naming (FEAT-001, US-001).
- Validation error content (US-005).
- Schema changes after creation — evolution, diff, migration (FEAT-017).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
