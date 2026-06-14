---
ddx:
  id: US-136
  review:
    self_hash: 884e339eee19235a529655d0e529cd53c0de110b80f4a6235ae577bfe27e0e13
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-136: Render an Entity as Markdown

**Feature**: FEAT-026 — Markdown Template Rendering
**Feature Requirements**: TPL-06, TPL-07, TPL-08, TPL-09
**PRD Requirements**: FR-24
**Priority**: P2
**Status**: Draft

## Story

**As an** agent or application developer (Ava)
**I want** to retrieve an entity rendered as markdown
**So that** I can present it to users without building formatting logic

## Context

Renumbered from US-076 (collision with FEAT-009). Agents present entities
to humans in Slack messages, emails, and chat responses; today each one
formats JSON itself. This story exercises the rendering area of FEAT-026
(TPL-06..09): opt-in markdown retrieval for single entities on the HTTP
surface and CLI. Depends on US-133 shipping first (a template must be
definable before rendering is a public surface). Endpoints, status codes,
and CLI flags are normative in CONTRACT-001 and CONTRACT-008.

## Walkthrough

1. Ava's agent requests an invoice entity in markdown form via the HTTP
   surface (CONTRACT-001).
2. Axon fetches the entity and the collection's compiled template and
   renders a UTF-8 markdown document.
3. The response is the rendered markdown with the markdown content type;
   scalar, nested, and array fields appear per the template; optional
   missing fields render empty; system fields are available.
4. The agent posts the markdown directly to a Slack channel with no
   formatting code.
5. From a terminal, Ava retrieves the same rendered view through the CLI
   (CONTRACT-008).

## Acceptance Criteria

- [ ] **US-136-AC1** — Given a collection with a template, when an entity
      is requested in markdown form (CONTRACT-001), then the response body
      is the rendered markdown document with the markdown content type.
- [ ] **US-136-AC2** — Given rendering fails after the entity is fetched,
      when the error is returned, then it includes the entity JSON for
      caller fallback (envelope per CONTRACT-001).
- [ ] **US-136-AC3** — Given a template with scalar interpolation, nested
      dot-notation, and array iteration, when an entity renders, then each
      construct produces the corresponding markdown output.
- [ ] **US-136-AC4** — Given missing optional fields and null fields, when
      an entity renders, then missing optionals render empty and null
      values are treated as falsy (sections do not render) — no errors, no
      placeholders.
- [ ] **US-136-AC5** — Given system fields (ID, version, timestamps,
      actors), when a template references them, then they are available as
      template variables.
- [ ] **US-136-AC6** — Given a collection with no template, when markdown
      is requested, then the request fails with a descriptive error naming
      the collection (status per CONTRACT-001).
- [ ] **US-136-AC7** — Given an entity with < 100 fields and a template
      < 10 KB, when it renders, then rendering completes in < 0.5 ms.
- [ ] **US-136-AC8** — Given the CLI render option (CONTRACT-008), when an
      entity is fetched with markdown rendering, then the output matches
      the HTTP-rendered document.

## Edge Cases

- **JSON remains the default**: a request without the markdown opt-in
  returns JSON unchanged.
- **Concurrent schema change removed a referenced field**: render produces
  empty output for that field (Mustache default); no error.
- **HTML in field values**: escaped interpolation HTML-escapes;
  markdown-bearing fields require the template's unescaped form.
- **Very large entity**: rendering succeeds; no output size limit in V1.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy render | US-136-AC1 | Invoice + template from US-133 | Request markdown form | Markdown document, markdown content type |
| Constructs | US-136-AC3 | Entity with nested `amount` and `line_items` array | Render | Dot-notation and iteration output present |
| Optional/null | US-136-AC4 | Draft invoice without `approver`, `notes` | Render | Optional sections cleanly omitted |
| No template | US-136-AC6 | Collection `vendors` without template | Request markdown | Descriptive error naming `vendors` |
| Render failure fallback | US-136-AC2 | Entity fetched; render error induced | Request markdown | Error response includes entity JSON |

## Dependencies

- **Stories**: US-133 (template definition must ship first)
- **Feature Spec**: FEAT-026
- **Feature Requirements**: TPL-06, TPL-07, TPL-08, TPL-09
- **PRD Requirements**: FR-24
- **External**: CONTRACT-001 (HTTP render surface), CONTRACT-008 (CLI
  render option)

## Out of Scope

- List/query rendering; GraphQL, MCP, and gRPC rendered output (deferred
  surfaces); template management (US-133).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
