---
ddx:
  id: US-099
  review:
    self_hash: 67c31a3bf25cd2d351b68f3a72fd504c1be8c0c08f096e32460abecd758f0f28
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-099: Generate a Schema-Driven Admin App

**Feature**: FEAT-024 — Application Substrate
**Feature Requirements**: SUB-05, SUB-06, SUB-07, SUB-08, SUB-09
**PRD Requirements**: PRD P2 #1 (Application substrate); builds on FR-20, FR-24
**Priority**: P2
**Status**: Draft

## Story

**As a** workflow builder (Wei)
**I want** a usable admin application generated from schema metadata
**So that** I can browse, edit, validate, and audit domain data without
building forms by hand

## Context

Internal-tool teams rebuild the same browse/edit/audit surfaces per
project. This story exercises the admin-app area of FEAT-024
(SUB-05..09): a generated application extending the base admin UI
(FEAT-011), with form controls, validation, and audit views all derived
from the schema, and tenant/database scope preserved throughout.

## Walkthrough

1. Wei runs admin-app generation against his collections' schemas.
2. The generated app presents collection navigation, an entity browser
   with search/filter, an entity editor, a link viewer, and a per-entity
   audit viewer.
3. Wei opens the editor for an invoice; required fields, enums, arrays,
   and nested objects render as appropriate controls.
4. He submits an invalid entity; the client blocks it, and a forced
   submission is rejected server-side with a matching message.
5. After adding a schema field, regeneration updates the visible form
   fields.

## Acceptance Criteria

- [ ] **US-099-AC1** — Given generated output, when the app loads, then it
      includes collection navigation, entity browser, entity editor, link
      viewer, and audit viewer.
- [ ] **US-099-AC2** — Given a schema with required fields, optional
      fields, enums, arrays, and nested objects, when the editor renders,
      then each renders as an editable control derived from the schema.
- [ ] **US-099-AC3** — Given an invalid form submission, when it is
      attempted, then it is blocked client-side and, if forced through,
      rejected server-side with a matching validation message.
- [ ] **US-099-AC4** — Given a tenant/database scope, when any generated
      route or API call executes, then the scope is preserved.
- [ ] **US-099-AC5** — Given a schema change, when regeneration runs, then
      the visible form fields update accordingly.

## Edge Cases

- **Collection without an entity schema**: browser and editor degrade to
  raw JSON editing rather than failing.
- **Deeply nested objects**: controls nest to the schema's depth; very
  deep structures remain editable (scrolling, not truncation).
- **Concurrent edit conflict**: a stale-version submission surfaces the
  server's version-conflict error in the UI (FR-6 semantics).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Surface completeness | US-099-AC1 | Generated app for 2 collections | Load app | All five areas reachable |
| Control derivation | US-099-AC2 | Invoice schema with `status` enum and `line_items` array | Open editor | Enum renders as select; array as repeatable rows |
| Validation parity | US-099-AC3 | `amount` below schema minimum | Submit | Client blocks; forced API call rejected with same constraint |
| Scope preservation | US-099-AC4 | Tenant `acme`, database `prod` | Browse and edit | Every request carries the `acme`/`prod` scope |

## Dependencies

- **Stories**: US-098 (generated client underpins the app's API calls)
- **Feature Spec**: FEAT-024
- **Feature Requirements**: SUB-05, SUB-06, SUB-07, SUB-08, SUB-09
- **PRD Requirements**: PRD P2 #1; FR-20, FR-24
- **External**: FEAT-011 (base admin UI); CONTRACT-010 (ESF)

## Out of Scope

- Custom theming/branding beyond the base admin UI; client generation
  internals (US-098); deployment (US-100).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
