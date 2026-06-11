---
ddx:
  id: FEAT-026
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-003
    - FEAT-005
---
# Feature Specification: FEAT-026 — Markdown Template Rendering

**Feature ID**: FEAT-026
**Status**: approved
**Priority**: P2
**Owner**: Core Team
**Covered PRD Subsystem(s)**: API and Deployment Surfaces
**Covered PRD Requirements**: FR-24 (operator-facing data inspection);
supports PRD P2 #1 (Application substrate — schema-driven presentation)
**Cross-Subsystem Rationale**: None — single subsystem.
**FR Prefix**: TPL

## Overview

Schemas define what data looks like. Markdown templates define how data
reads. FEAT-026 adds an optional Mustache template to each collection that
renders an entity as a markdown document. This is render-only (entity to
markdown) — there is no parse path (markdown to entity). It supports
operator data inspection (PRD FR-24) and schema-driven presentation for
the application substrate (PRD P2 #1).

Agents communicate in markdown. Users read markdown. By letting the schema
owner define a presentation template, every consumer — agents, UIs,
notifications, exports — gets a consistent, human-readable view of
structured data without building formatting logic.

The normative HTTP endpoints for template management and rendered entity
retrieval are defined in
[CONTRACT-001 — HTTP API surface](../../02-design/contracts/CONTRACT-001-http-api-surface.md);
the CLI commands are defined in
[CONTRACT-008 — CLI and config](../../02-design/contracts/CONTRACT-008-cli-and-config.md).

## Ideal Future State

A schema owner writes one Mustache template per collection and every
consumer — an agent composing a Slack message, an operator inspecting an
entity from the CLI, an export job — retrieves the same consistent,
readable markdown view of an entity with a single call. Template mistakes
are caught at save time against the entity schema, not discovered at
render time. Templates evolve on their own cadence: fixing presentation
never triggers schema evolution analysis or entity revalidation.

## Problem Statement

- **Current situation**: Entity data is stored and returned as JSON;
  consumers format it themselves or display it raw.
- **Pain points**: Inconsistent presentation, duplicated formatting logic
  in every consumer, and poor readability for humans reviewing agent
  output. The schema owner — who understands the data best — has no way to
  define how entities should be presented.
- **Desired outcome**: Schema owners define a template once; all consumers
  get a consistent markdown rendering via a single call.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Template definition and validation | "Define how my collection's entities read, and catch mistakes early" | One optional Mustache template per collection, validated against the entity schema at save time |
| Rendering | "Give me this entity as readable markdown" | Pure, fast entity-to-markdown rendering with predictable handling of optional, null, and system fields |
| Lifecycle independence | "Presentation changes must not destabilize my schema" | Template versioning independent of schema versioning and evolution analysis |

## Requirements

### Functional Requirements by Area

#### Template Definition and Validation

- **TPL-01**. Each collection MUST support one optional Mustache
  (logic-less) template. Supported constructs: scalar interpolation
  (HTML-escaped by default), unescaped interpolation for markdown-bearing
  fields, sections (truthy), inverted sections (falsy/absent), array
  iteration, and dot-notation access to nested objects.
- **TPL-02**. Templates MUST be stored separately from the collection
  schema with an independent version counter: saving a template never
  increments the schema version and never triggers schema evolution
  analysis or entity revalidation.
- **TPL-03**. Saving a template MUST validate it: (a) the template parses
  as valid Mustache; (b) every field reference resolves against the
  collection's entity schema; references to fields that do not exist in
  the schema are rejected with an error naming the invalid field; (c)
  system fields (id, version, created/updated timestamps and actors) are
  always accepted.
- **TPL-04**. References to fields that exist but are not required MUST
  produce an advisory warning (output may be incomplete for entities
  without the field); warnings are returned to the caller and do not block
  the save.
- **TPL-05**. Template saves, updates, and deletions MUST be audited
  (actor, timestamp, before/after), per FEAT-003.

#### Rendering

- **TPL-06**. Rendering MUST take an entity and the collection's template
  and produce a UTF-8 markdown document. Missing optional fields render as
  empty strings; null values are treated as falsy (sections do not render,
  interpolation produces empty output); system fields are available as
  template variables alongside the entity's data fields.
- **TPL-07**. Rendering MUST be pure: no database reads beyond fetching
  the entity and template, no external calls, no mutation. Rendering can
  never trigger validation, audit, or any write path.
- **TPL-08**. Rendered-markdown retrieval MUST be available for
  single-entity reads on the HTTP surface and the CLI, as defined in
  CONTRACT-001 and CONTRACT-008. JSON remains the default representation;
  markdown is opt-in per request.
- **TPL-09**. Requesting markdown for a collection with no template MUST
  fail with a descriptive error identifying the collection; if rendering
  fails after the entity is fetched, the error response MUST include the
  entity JSON so the caller still has the data. Exact status codes and
  error envelope are defined in CONTRACT-001.

#### Template Management Surface

- **TPL-10**. Schema owners MUST be able to save/update, retrieve, and
  delete a collection's template through the HTTP surface and the CLI;
  the normative endpoints and commands are defined in CONTRACT-001 and
  CONTRACT-008.

#### Lifecycle Independence

- **TPL-11**. Schema evolution analysis MUST NOT consider template
  content; adding a schema field never invalidates an existing template.
- **TPL-12**. Removing a schema field that an existing template references
  MUST NOT block the schema change; the stale reference produces a warning
  at the next template retrieval or render, and the next template save
  rejects it.

### Non-Functional Requirements

- **Rendering latency**: < 0.5 ms for entities with < 100 fields and
  templates < 10 KB; rendering adds < 1 ms to entity read latency (p99).
- **Caching**: compiled templates are cached per collection; the cache is
  invalidated on template update; the cache is bounded and collections
  without templates hold no cache entry.
- **Purity**: rendering has no side effects (TPL-07) — enforceable by
  contract test.

## User Stories

- [US-133 — Define a Markdown Template](../user-stories/US-133-define-a-markdown-template.md)
- [US-136 — Render an Entity as Markdown](../user-stories/US-136-render-an-entity-as-markdown.md)
- [US-138 — Template Survives Schema Evolution](../user-stories/US-138-template-survives-schema-evolution.md)

## Edge Cases and Error Handling

- **No entity schema**: if a collection has no entity schema (all entities
  accepted), template field validation is skipped — any field reference is
  accepted since the schema imposes no constraints.
- **Template with no field references**: a static markdown template (no
  Mustache expressions) is valid — useful where the template is just a
  header or boilerplate.
- **Very large entities**: deeply nested structures or large arrays may
  produce large markdown output; no output size limit in V1, monitoring
  recommended.
- **Template references a field added later**: saving a template that
  references a field the schema does not yet have is rejected; the schema
  must be updated first, then the template.
- **Concurrent template and schema update**: template validation uses the
  schema at save time. If the schema concurrently removes a field, the
  next render produces empty output for that field (Mustache default) and
  the next template save catches the stale reference (TPL-12).
- **HTML in field values**: escaped interpolation HTML-escapes by default;
  fields intended to contain markdown must use unescaped interpolation —
  an explicit choice by the template author.

## Example

### Invoice Collection Template

**Schema excerpt:**
```yaml
entity_schema:
  type: object
  required: [invoice_number, vendor, status, amount, line_items]
  properties:
    invoice_number: { type: string }
    vendor: { type: string }
    status: { type: string, enum: [draft, submitted, approved, paid] }
    amount:
      type: object
      properties:
        value: { type: number }
        currency: { type: string }
    line_items:
      type: array
      items:
        type: object
        properties:
          description: { type: string }
          quantity: { type: integer }
          unit_price: { type: number }
    notes: { type: string }
    approver: { type: string }
```

**Template:**
```mustache
# Invoice {{invoice_number}}

**Vendor:** {{vendor}}
**Status:** {{status}}
**Amount:** {{amount.currency}} {{amount.value}}
{{#approver}}**Approved by:** {{approver}}{{/approver}}

## Line Items

{{#line_items}}
- {{description}} (qty: {{quantity}}, {{unit_price}} each)
{{/line_items}}

{{#notes}}
## Notes

{{{notes}}}
{{/notes}}
```

**Rendered output** (for an approved invoice):
```markdown
# Invoice INV-2026-0042

**Vendor:** Acme Corp
**Status:** approved
**Amount:** USD 312.50
**Approved by:** jane@example.com

## Line Items

- Widget A (qty: 10, 25.00 each)
- Widget B (qty: 5, 12.50 each)
```

For a draft invoice without `approver` or `notes`, the optional sections
are cleanly omitted via Mustache sections — no "null" text, no empty
headers.

## Success Metrics

- Template definition takes < 2 minutes for a typical collection.
- Agents retrieve and use rendered markdown with zero client-side
  formatting code.
- Rendering adds < 1 ms to entity read latency (p99).

## Constraints and Assumptions

- Mustache (logic-less) is the template language; no custom helpers,
  lambdas, or logic constructs.
- Render is one-way: agents and applications write entities as JSON;
  markdown is presentation only.
- Templates are presentation metadata and are deliberately excluded from
  the schema version counter and evolution analysis (TPL-02, TPL-11).

## Dependencies

- **Other features**:
  - FEAT-002 (Schema Engine) — template field validation requires the
    entity schema.
  - FEAT-005 (API Surface) — rendered retrieval and template management
    ride the public surface (CONTRACT-001, CONTRACT-008).
  - FEAT-003 (Audit Log) — template saves are audited.
- **External services**: none; normative HTTP/CLI surface lives in
  CONTRACT-001 and CONTRACT-008.
- **PRD requirements**: FR-24 (P1); supports P2 #1.

## Out of Scope

- **Markdown-to-entity parsing**: render is one-way; writes are JSON.
- **Multiple named templates** (summary, detail, …): one template per
  collection in V1.
- **Partials / shared templates**: no cross-collection template reuse.
- **Custom Mustache helpers**: logic-less only — no lambdas or custom
  functions.
- **Template inheritance**: no base templates with override blocks.
- **List/query markdown rendering**: single-entity rendering only in V1.
- **gRPC, GraphQL, and MCP rendered output**: deferred follow-on
  surfaces; V1 ships HTTP and CLI only.
- **Localization**: one template per collection, not per locale.
