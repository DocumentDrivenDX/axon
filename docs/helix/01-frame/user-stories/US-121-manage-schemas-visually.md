---
ddx:
  id: US-121
---

# US-121: Manage Schemas Visually

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-16, UI-17, UI-18
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As a** developer defining Axon schemas (Ava, Agent Application Developer persona)
**I want** a database-scoped schema workspace
**So that** I can iterate on collection definitions without CLI round-trips

## Context

Renumbered from US-043 (collision with FEAT-012). Schema-first workflows need
a place to see and evolve collection definitions; the schema workspace lists
collections by schema status, shows structured and raw views, and requires a
preview step before saving changes. Exercises FEAT-011 requirements UI-16
through UI-18.

## Walkthrough

1. Developer opens the database Schemas screen.
2. System lists registered collections with their schema status and opens a
   structured schema view on selection.
3. Developer switches to the raw view to inspect the full schema payload.
4. Developer edits the schema and requests a save.
5. System shows a preview of the change; the developer confirms.
6. System applies the schema and shows any validation errors inline.

## Acceptance Criteria

- [ ] **US-121-AC1** — Given a database with registered collections, when the
  developer opens the Schemas screen, then collections are listed and
  selecting one opens a structured schema view.
- [ ] **US-121-AC2** — Given a selected collection, when the developer opens
  the raw view, then the full collection schema payload is displayed.
- [ ] **US-121-AC3** — Given the schema workspace, when the developer creates
  a collection with entity schema content, then the collection is registered
  in the current tenant/database scope ready for schema-first workflows.
- [ ] **US-121-AC4** — Given a schema edit, when the developer saves, then a
  preview of the change is required before the save is applied.

## Edge Cases

- **Invalid schema content**: validation errors are displayed inline and the
  active schema remains unchanged.
- **Collection with no schema**: listed with an explicit "no schema" status
  rather than omitted.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Schema list and view | US-121-AC1 | 2 collections, one with schema | Open Schemas, select one | Structured view of the schema |
| Raw view | US-121-AC2 | Collection with schema | Open raw view | Full schema payload shown |
| Schema-first create | US-121-AC3 | Schemas screen | Create collection with schema JSON | Collection registered in scope |
| Preview before save | US-121-AC4 | Edited schema | Save | Preview shown first; apply after confirm |
| Invalid edit | US-121-AC4 | Schema with type error | Save | Inline errors; no change applied |

## Dependencies

- **Stories**: US-040 (database workspace navigation)
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-16, UI-17, UI-18
- **PRD Requirements**: FR-24
- **External**: CONTRACT-002 (GraphQL surface), CONTRACT-010 (ESF schema
  format)

## Out of Scope

- Schema-evolution compatibility classification UX (FEAT-017).
- Access-control policy editing beside the schema (FEAT-031).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
