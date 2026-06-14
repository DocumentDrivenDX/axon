---
ddx:
  id: US-138
  review:
    self_hash: 8151ea9a09172afc20a1b05308e5e55d5d5e96926e3e4407fd39e7e8092d1d6c
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-138: Template Survives Schema Evolution

**Feature**: FEAT-026 — Markdown Template Rendering
**Feature Requirements**: TPL-02, TPL-11, TPL-12
**PRD Requirements**: FR-24
**Priority**: P2
**Status**: Draft

## Story

**As a** schema owner (Wei) evolving my schema
**I want** template changes to be independent of schema versioning
**So that** fixing a typo in my template doesn't trigger evolution
analysis or entity revalidation

## Context

Renumbered from US-077 (collision with FEAT-009). Templates are
presentation metadata; if editing one bumped the schema version, every
template tweak would look like schema evolution and could trigger
revalidation. This story exercises lifecycle independence (TPL-02,
TPL-11, TPL-12): independent version counters and well-defined staleness
behavior when schemas remove fields a template references.

## Walkthrough

1. Wei fixes a typo in the `invoices` template and saves it.
2. The template's own version increments; the collection schema version
   is unchanged and no evolution analysis or revalidation runs.
3. Wei later adds a new field to the schema; the existing template stays
   valid (it simply doesn't reference the field yet).
4. Wei removes a field the template references; the schema change
   succeeds, and the stale reference surfaces as a warning at the next
   template retrieval or render.

## Acceptance Criteria

- [ ] **US-138-AC1** — Given a stored template, when it is updated, then
      the collection schema version is not incremented.
- [ ] **US-138-AC2** — Given schema evolution analysis runs, when schemas
      are diffed, then template content is not considered.
- [ ] **US-138-AC3** — Given a schema gains a new field, when the existing
      template is validated or used, then it remains valid without
      modification.
- [ ] **US-138-AC4** — Given a schema removes a field that the template
      references, when the template is next retrieved or used for
      rendering, then a warning is produced — and the schema save itself
      is not blocked.

## Edge Cases

- **Template save concurrent with field removal**: validation uses the
  schema at save time; the staleness is caught at the next retrieval,
  render, or save (TPL-12).
- **Rendering with a stale reference**: produces empty output for the
  removed field (Mustache default), never an error.
- **Template version after schema rollback**: template and schema version
  counters never interact in either direction.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Independent versions | US-138-AC1 | Schema v3, template v1 | Update template | Template v2; schema still v3 |
| Diff ignores templates | US-138-AC2 | Template changed between schema diffs | Run evolution analysis | No template-derived differences reported |
| Additive schema change | US-138-AC3 | Add optional `po_number` to schema | Validate existing template | Valid, no warnings about `po_number` |
| Removed referenced field | US-138-AC4 | Template references `notes`; remove `notes` from schema | Retrieve template / render | Warning naming `notes`; schema change unblocked |

## Dependencies

- **Stories**: US-133 (a stored template to evolve around)
- **Feature Spec**: FEAT-026
- **Feature Requirements**: TPL-02, TPL-11, TPL-12
- **PRD Requirements**: FR-24
- **External**: FEAT-017 (schema evolution analysis this story must not
  perturb)

## Out of Scope

- Schema evolution mechanics themselves (FEAT-017); render semantics
  (US-136); template validation rules at save time (US-133).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
