---
ddx:
  id: US-133
  review:
    self_hash: 2f20065df7de1057ca094319b75b7051f8cb2b442b3a9ed9e55ed899321af6fe
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-133: Define a Markdown Template

**Feature**: FEAT-026 — Markdown Template Rendering
**Feature Requirements**: TPL-01, TPL-02, TPL-03, TPL-04, TPL-05, TPL-10
**PRD Requirements**: FR-24
**Priority**: P2
**Status**: Draft

## Story

**As a** schema owner (Wei)
**I want** to define a Mustache template for my collection
**So that** all consumers get a consistent, readable presentation of
entities

## Context

Renumbered from US-075 (collision with FEAT-009). The schema owner knows
the data best but has no way to define presentation. This story exercises
template definition and validation (TPL-01..05) and the management surface
(TPL-10). It must ship before rendered retrieval (US-136) becomes a public
client-facing surface. Endpoints and CLI commands are normative in
CONTRACT-001 and CONTRACT-008.

## Walkthrough

1. Wei saves a Mustache template for the `invoices` collection through
   the template management surface (CONTRACT-001 / CONTRACT-008).
2. Axon validates the template: Mustache syntax, then every field
   reference against the collection's entity schema.
3. A reference to a nonexistent field is rejected with an error naming
   the field; Wei fixes it and resaves.
4. A reference to an optional field returns an advisory warning, but the
   save succeeds.
5. Wei retrieves the stored template to confirm it, and the save appears
   in the audit log with actor and before/after.

## Acceptance Criteria

- [ ] **US-133-AC1** — Given a valid Mustache template, when it is saved
      via the template management surface (CONTRACT-001/008), then it is
      stored for the collection.
- [ ] **US-133-AC2** — Given a save request, when the template is
      processed, then it is validated against the collection's entity
      schema before persisting.
- [ ] **US-133-AC3** — Given a template referencing a field absent from
      the schema, when it is saved, then the save is rejected with an
      error naming the invalid field.
- [ ] **US-133-AC4** — Given a template referencing an optional field,
      when it is saved, then an advisory warning is returned and the save
      succeeds.
- [ ] **US-133-AC5** — Given a stored template, when it is retrieved, then
      the current template body and version are returned.
- [ ] **US-133-AC6** — Given a stored template, when it is deleted, then
      subsequent retrievals report no template.
- [ ] **US-133-AC7** — Given any template save, update, or delete, when it
      completes, then an audit record captures actor, timestamp, and
      before/after.

## Edge Cases

- **Collection with no entity schema**: field validation is skipped — any
  reference is accepted, since the schema imposes no constraints.
- **Static template with no field references**: valid (boilerplate-only
  templates are allowed).
- **Malformed Mustache syntax**: rejected at parse stage with a
  descriptive error before any field checking.
- **System-field references**: always accepted (TPL-03).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Valid save | US-133-AC1 | `invoices` schema with `vendor` | Save `**Vendor:** {{vendor}}` | Stored; version incremented |
| Unknown field | US-133-AC3 | Schema without `supplier` | Save template using `{{supplier}}` | Rejected, error names `supplier` |
| Optional warning | US-133-AC4 | `notes` optional in schema | Save template using `{{notes}}` | Saved; advisory warning about `notes` |
| Audited change | US-133-AC7 | Existing template v1 | Update to v2 | Audit record with actor and before/after |

## Dependencies

- **Stories**: none
- **Feature Spec**: FEAT-026
- **Feature Requirements**: TPL-01, TPL-02, TPL-03, TPL-04, TPL-05, TPL-10
- **PRD Requirements**: FR-24
- **External**: CONTRACT-001 (HTTP template endpoints), CONTRACT-008 (CLI
  commands); FEAT-002 (entity schema), FEAT-003 (audit)

## Out of Scope

- Rendering behavior (US-136); schema-evolution independence (US-138);
  multiple named templates, partials, localization (FEAT-026 Out of
  Scope).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
