---
ddx:
  id: US-042
  review:
    self_hash: d67386d1586a037888dd7cbda34c2bdb79aa40fa94fd1c81c6d521331f3744b8
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-042: Manage Collections and Entities

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-07, UI-08, UI-09, UI-10, UI-11, UI-12, UI-13, UI-14, UI-15, UI-22
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Approved

## Story

**As a** developer using Axon (Ava, Agent Application Developer persona)
**I want** tenant-scoped collection and entity CRUD
**So that** I can inspect and repair data without writing curl commands

## Context

Exploratory data work — scanning entity data, creating fixtures, repairing a
bad record — is far faster visually than through the CLI. This story
exercises FEAT-011's collection-management and entity-browsing requirements
within an explicit tenant/database scope.

## Walkthrough

1. Developer opens the database Collections screen.
2. System lists registered collections with entity counts and schema
   versions, each linking to a detail view.
3. Developer creates a collection in the current scope.
4. System shows it in the list and opens its detail view.
5. Developer creates, edits, and deletes entities from the collection detail
   view, and inspects an entity's full JSON, version, and history.
6. System validates input, applies changes, and reflects them immediately.

## Acceptance Criteria

- [ ] **US-042-AC1** — Given a database workspace, when the developer opens
  the Collections screen, then registered collections are listed and each
  links to its detail view.
- [ ] **US-042-AC2** — Given the Collections screen, when the developer
  creates a collection, then it is created in the current tenant/database
  scope, appears in the list, and its detail view supports entity creation.
- [ ] **US-042-AC3** — Given an existing collection, when the developer drops
  it, then a confirmation step is required before deletion.
- [ ] **US-042-AC4** — Given a collection detail view, when the developer
  creates, reads, updates, and deletes an entity, then each operation
  succeeds from the UI.
- [ ] **US-042-AC5** — Given an entity, when the developer opens its detail
  view, then version, collection, schema version, and full JSON data are
  shown.
- [ ] **US-042-AC6** — Given an entity with prior mutations, when the
  developer opens its History tab, then its audit versions are listed.
- [ ] **US-042-AC7** — Given a collection with many entities, when the
  developer filters by a field=value expression, then only matching entities
  are listed.

## Edge Cases

- **Schema-violating entity create**: the UI displays the full structured
  error with all field violations, not just the first.
- **Delete with inbound links**: the UI surfaces the referential-integrity
  error instead of silently failing.
- **Large collections**: the entity table paginates at 50 rows.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| List collections | US-042-AC1 | Database with 2 collections | Open Collections | Both listed with counts and schema versions |
| Create collection | US-042-AC2 | Collections screen | Create `invoices` | Listed; detail view supports entity create |
| Drop with confirm | US-042-AC3 | Collection `invoices` | Drop it | Confirmation required, then removed |
| Entity CRUD | US-042-AC4 | Collection detail | Create/read/update/delete entity `e1` | All four operations succeed |
| Entity detail | US-042-AC5 | Entity `e1` exists | Open detail | Version, collection, schema version, JSON shown |
| History tab | US-042-AC6 | `e1` updated twice | Open History tab | Audit versions listed |
| Field filter | US-042-AC7 | Entities with `status=paid` and `status=open` | Filter `status=paid` | Only matching rows shown |

## Dependencies

- **Stories**: US-040 (database workspace navigation)
- **Feature Spec**: FEAT-011
- **Feature Requirements**: UI-07 through UI-15, UI-22
- **PRD Requirements**: FR-24
- **External**: CONTRACT-002 (GraphQL data-plane surface), CONTRACT-001
  (documented REST exceptions)

## Out of Scope

- Schema editing workflows (US-121).
- Rollback and audit-log browsing beyond the entity History tab (US-122).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
