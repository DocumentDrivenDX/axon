---
ddx:
  id: US-075
---

# US-075: Schema-Declared Named Query

**Feature**: FEAT-009 — Unified Graph Query (Cypher)
**Feature Requirements**: QRY-04, QRY-05, QRY-06, QRY-07, QRY-08, QRY-09
**PRD Requirements**: FR-3
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer defining a collection schema
**I want** to declare reusable graph queries in the schema itself
**So that** they are type-checked, policy-validated, index-validated, and surfaced as typed fields before anything ships

## Context

Named queries shift query validation to schema-write time: a query that
would scan unindexed data, bypass policy, or reference unknown fields fails
the schema save, not the production request. This story exercises the full
compile pipeline (QRY-04..QRY-07), generation of typed surfaces (QRY-08),
and dry-run compile reports (QRY-09). Grammar and diagnostics are normative
in CONTRACT-007.

## Walkthrough

1. Ava adds a named query to her collection schema (declaration grammar per CONTRACT-007 / CONTRACT-010).
2. On schema save, the compiler type-checks the query against active schemas, validates index usage, and validates policy compatibility.
3. On activation, a typed GraphQL field and an MCP tool appear for the query.
4. For iteration, Ava uses the schema dry-run and reads the compile report without activating.

## Acceptance Criteria

- [ ] **US-075-AC1** — Given a schema with a named-query declaration, when the schema is saved, then the declaration is accepted per the CONTRACT-007 grammar.
- [ ] **US-075-AC2** — Given a named query referencing an unknown label, property, or relationship type, when the schema is saved, then the save fails with a type-check diagnostic identifying the reference.
- [ ] **US-075-AC3** — Given a named query requiring an unindexed scan over a collection above the configured threshold, when the schema is saved, then the save fails with a diagnostic suggesting an index declaration (QRY-06).
- [ ] **US-075-AC4** — Given a named query that would require policy bypass to be useful, when the schema is saved, then the save fails with the documented policy-compatibility error (QRY-07).
- [ ] **US-075-AC5** — Given successful activation, when surfaces are inspected, then the named query exists as a typed GraphQL field and a corresponding MCP tool (per CONTRACT-002/003).
- [ ] **US-075-AC6** — Given a schema dry-run, when submitted, then a compile report including named-query diagnostics is returned and nothing is activated.

## Edge Cases

- **Renaming a declared query**: the old field/tool disappears on activation; existing subscribers are torn down cleanly (see US-077).
- **Query valid against old schema but broken by a concurrent schema change**: compile-time validation runs against the schema state being activated, so the save fails atomically.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Declare | US-075-AC1 | `ready_beads` block in schema | Save schema | Accepted, compiled |
| Unknown label | US-075-AC2 | Query references `:Nope` | Save schema | Type-check failure naming `Nope` |
| Unindexed scan | US-075-AC3 | Predicate on unindexed field, large collection | Save schema | Diagnostic suggesting index |
| Policy bypass | US-075-AC4 | Query needs hidden rows to be useful | Save schema | Policy-compatibility rejection |
| Dry run | US-075-AC6 | Same schema, dry-run flag | Submit | Compile report; no activation |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-009
- **Feature Requirements**: QRY-04, QRY-05, QRY-06, QRY-07, QRY-08, QRY-09
- **PRD Requirements**: FR-3
- **External**: CONTRACT-007 (grammar, diagnostics, error codes), CONTRACT-010 (ESF integration), CONTRACT-002/003 (generated surfaces), FEAT-002 (schema engine), FEAT-013 (indexes), FEAT-029/ADR-019 (policy compilation)

## Out of Scope

- Executing the generated surfaces (US-072, US-073).
- Schema evolution interactions beyond save-time validation (FEAT-017).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
