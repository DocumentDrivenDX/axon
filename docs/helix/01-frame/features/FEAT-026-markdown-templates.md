---
ddx:
  id: FEAT-026
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-005
---
# Feature Specification: FEAT-026 - Markdown Template Rendering

**Feature ID**: FEAT-026
**Status**: In Progress
**Priority**: P2
**Owner**: Core Team
**Created**: 2026-04-07
**Updated**: 2026-04-08

## Overview

Schemas define what data looks like. Markdown templates define how data
reads. FEAT-026 adds an optional Mustache template to each collection
that renders an entity as a markdown document. This is render-only
(entity to markdown) — there is no parse path (markdown to entity).

Agents communicate in markdown. Users read markdown. By letting the
schema owner define a presentation template, every consumer — agents,
UIs, notifications, exports — gets a consistent, human-readable view
of structured data without building formatting logic.

## Problem Statement

Entity data in Axon is stored as JSON. JSON is excellent for machines
but poor for humans. When an agent needs to present an entity to a
user (in a Slack message, email, report, or chat response), it must
build formatting logic. When a human inspects an entity via CLI or
admin UI, they see raw JSON.

Every consumer that wants a readable view reinvents the same
formatting. The schema owner — who understands the data best — has no
way to define how entities should be presented. The result is
inconsistent, ad-hoc formatting scattered across consumers.

- **Current situation**: Consumers format entity JSON themselves or
  display it raw
- **Pain points**: Inconsistent presentation, duplicated formatting
  logic, poor readability for humans reviewing agent output
- **Desired outcome**: Schema owners define a template once; all
  consumers get a consistent markdown rendering via a single API call

## Requirements

### Functional Requirements

#### Template Definition

- **Mustache syntax**: Templates use Mustache (logic-less templates)
  for field interpolation. Supported constructs:
  - `{{field}}` — scalar field interpolation (HTML-escaped by default)
  - `{{{field}}}` — unescaped interpolation (for fields containing
    markdown)
  - `{{#field}}...{{/field}}` — section (renders if field is truthy /
    non-empty)
  - `{{^field}}...{{/field}}` — inverted section (renders if field is
    falsy / absent)
  - `{{#array}}...{{/array}}` — iteration over arrays
  - `{{nested.field}}` — dot-notation for nested objects
- **One template per collection**: A single optional template. Named
  templates (summary, detail, etc.) are deferred
- **Stored alongside schema, versioned independently**: Templates are
  associated with a collection but do not share the schema version
  counter. Changing a template does not trigger schema evolution
  analysis or entity revalidation

#### Template Storage

Templates are stored as a separate concept from `CollectionSchema`:

```
markdown_templates:
    PK: (collection_id)
    template:       text        -- Mustache template string
    version:        int         -- independent version counter
    updated_at:     timestamp
    updated_by:     text        -- actor who last modified

    FK: collection_id -> collections
```

This separation ensures:
1. Schema version does not bump on template changes
2. Schema evolution analysis is unaffected by templates
3. The `axon-schema` crate has no rendering dependency

#### Template Validation on Save

When a template is saved, Axon validates it against the collection's
entity schema:

1. **Parse check**: Template must be valid Mustache syntax
2. **Field resolution**: All `{{field}}` references are extracted and
   checked against the entity schema
   - Fields that exist in the schema: accepted
   - Fields that don't exist in the schema at all: **rejected** with
     error naming the invalid field
   - System fields (`_id`, `_version`, `_created_at`, `_updated_at`,
     `_created_by`, `_updated_by`): accepted (always available)
3. **Optional field warning**: Fields that exist but are not in the
   schema's `required` list produce an advisory warning ("field
   'notes' is optional — template output may be incomplete for
   entities without this field")

Validation is analogous to `validate_rule_definitions()` (FEAT-019,
US-069): catch errors at definition time, not render time.

#### Rendering

- **Input**: Entity JSON + template string
- **Output**: UTF-8 markdown string
- **Missing optional fields**: Render as empty string (Mustache
  default behavior). Sections (`{{#field}}`) around optional fields
  let the template author handle this gracefully
- **Null values**: Treated as falsy (sections don't render, interpolation
  produces empty string)
- **System fields**: `_id`, `_version`, `_created_at`, `_updated_at`,
  `_created_by`, `_updated_by` are available as template variables
  alongside the entity's `data` fields
- **Performance**: Template compilation is cached per collection.
  Rendering < 0.5ms for typical entities (< 100 fields)

#### API Surface

The markdown rendering path is shipped on the HTTP gateway and the CLI.
Operators can manage templates publicly, then request rendered markdown
for single-entity inspection.

**Shipped HTTP surface:**
```
GET /collections/{collection}/entities/{id}?format=markdown
Accept: text/markdown

200 OK
Content-Type: text/markdown; charset=utf-8

# INV-2026-0042

**Status:** approved
**Vendor:** Acme Corp

## Line Items
- Widget A (qty: 10, $25.00 each)
- Widget B (qty: 5, $12.50 each)

**Total:** $312.50
```

**gRPC:**
- `GetEntity` remains JSON-only in the current slice
- Markdown-specific request fields and a `rendered_markdown` response
  field are deferred until the protobuf contract and service
  implementation are extended together

**Shipped behavior:**
- If `format=markdown` and the collection has no template: return
  `400 Bad Request` with error "collection 'X' has no markdown
  template defined"
- If `format=markdown` and rendering fails: return `500` with error
  details. Entity JSON is still returned in the response body so the
  caller has the data even if rendering failed
- Default format remains JSON. The `format` parameter is opt-in

**Shipped template management:**
```
PUT    /collections/{collection}/template  — save/update template
GET    /collections/{collection}/template  — retrieve current template
DELETE /collections/{collection}/template  — remove template
```

`PUT` accepts either:
- `Content-Type: text/plain` with the raw Mustache template body
- `Content-Type: application/json` with `{ "template": "..." }`

`PUT` validates Mustache syntax and schema field references before
persisting the template. Optional-field warnings are returned to the
caller but do not block the save.

**Shipped CLI surface:**
```bash
axon collection template put <collection> --template '# {{title}}'
axon collection template get <collection>
axon collection template delete <collection>
axon entity get <collection> <id> --render markdown
```

**Deferred surfaces:**
- gRPC `GetEntity` markdown format selection and `rendered_markdown`
  response field
- List/query responses with markdown rendering
- GraphQL `renderedMarkdown` field
- MCP tool returning markdown
- Batch/transaction response rendering

### Non-Functional Requirements

- **Rendering latency**: < 0.5ms for entities with < 100 fields and
  templates < 10KB
- **Template compilation**: Compiled templates are cached per
  collection. Cache invalidated on template update
- **No side effects**: Rendering is pure — no database reads, no
  external calls, no mutation. Template rendering cannot trigger
  validation, audit, or any write path
- **Memory**: Template cache is bounded. Collections without templates
  have no cache entry

## Architecture

### Crate: `axon-render`

A new workspace crate dedicated to template rendering:

```
crates/
  axon-render/
    src/
      lib.rs          -- public API: render(), validate_template()
      mustache.rs     -- Mustache engine wrapper
      fields.rs       -- field extraction from template AST
```

**Dependencies:**
- `axon-core` — `Entity` type, `AxonError`
- `ramhorns` or equivalent zero-copy Mustache crate
- Does NOT depend on `axon-schema` (templates are passed as strings;
  field validation is done by the caller in `axon-api`)

**Depended on by:**
- `axon-api` — calls `render()` in the entity GET handler and
  `validate_template()` in the template PUT handler

This keeps `axon-schema` free of rendering logic and `axon-render`
free of schema knowledge. The `axon-api` handler bridges the two:
it reads the schema, reads the template, validates field references,
and calls render.

### Storage

The storage adapter trait gains three methods:

```rust
/// Template operations on the storage adapter.
fn put_template(
    &self,
    collection: &CollectionId,
    template: &str,
    actor: Option<&str>,
) -> Result<u32, AxonError>;  // returns new version

fn get_template(
    &self,
    collection: &CollectionId,
) -> Result<Option<(String, u32)>, AxonError>;  // (template, version)

fn delete_template(
    &self,
    collection: &CollectionId,
) -> Result<bool, AxonError>;  // true if existed
```

### Handler Flow

**Render on GET:**
```
get_entity(collection, id, format=markdown) → {
    1. Get entity from storage
    2. Get template from storage (or cache)
    3. If no template: return error
    4. Build render context: entity.data + system fields
    5. Call axon_render::render(template, context)
    6. Return rendered markdown on the HTTP surface; retain the entity
       for error responses and future gRPC expansion
}
```

**Validate on PUT template:**
```
put_template(collection, template_text) → {
    1. Get schema from storage
    2. Parse template (Mustache syntax check)
    3. Extract field references from template AST
    4. Validate references against entity_schema
    5. If errors: return 400 with invalid field list
    6. If warnings: note in response
    7. Store template
    8. Invalidate render cache for collection
    9. Audit log: template_updated
}
```

## User Stories

### Story US-075: Define a Markdown Template [FEAT-026]

**As a** schema owner
**I want** to define a Mustache template for my collection
**So that** all consumers get a consistent, readable presentation of
entities

This story must ship before US-076 becomes a public client-facing
surface.

**Acceptance Criteria:**
- [ ] `PUT /collections/{collection}/template` accepts a valid
      Mustache template and stores it
- [ ] Template is validated against the collection's entity schema
      before saving
- [ ] References to nonexistent fields are rejected with error naming
      the invalid field
- [ ] References to optional fields produce an advisory warning
- [ ] `GET /collections/{collection}/template` returns the current
      template
- [ ] `DELETE /collections/{collection}/template` removes the
      template
- [ ] Template changes are audited (actor, timestamp, before/after)

### Story US-076: Render an Entity as Markdown [FEAT-026]

**As an** agent or application
**I want** to retrieve an entity rendered as markdown
**So that** I can present it to users without building formatting logic

Current scope note: the repository contains an internal/test-only HTTP
render path for this story, but it is not yet a shipped user surface
until US-075's template management APIs land.

**Acceptance Criteria:**
- [ ] `GET /collections/{c}/entities/{id}?format=markdown` returns
      rendered markdown with `Content-Type: text/markdown`
- [ ] On successful HTTP render, the response body is the rendered
      markdown document
- [ ] If rendering fails after the entity is fetched, the error
      response includes entity JSON for caller fallback
- [ ] Scalar fields render via `{{field}}` interpolation
- [ ] Nested object fields render via `{{parent.child}}` dot notation
- [ ] Array fields render via `{{#items}}...{{/items}}` iteration
- [ ] Missing optional fields render as empty (no error, no placeholder)
- [ ] Null fields are treated as falsy (sections don't render)
- [ ] System fields (`_id`, `_version`, `_created_at`, etc.) are
      available as template variables
- [ ] If collection has no template, request returns `400` with
      descriptive error
- [ ] Rendering latency < 0.5ms for entities with < 100 fields

### Story US-077: Template Survives Schema Evolution [FEAT-026]

**As a** schema owner evolving my schema
**I want** template changes to be independent of schema versioning
**So that** fixing a typo in my template doesn't trigger evolution
analysis or entity revalidation

**Acceptance Criteria:**
- [ ] Updating a template does not increment `CollectionSchema.version`
- [ ] Schema evolution analysis (`diff_schemas`) does not consider
      template content
- [ ] Adding a field to the schema does not invalidate the template
      (template simply doesn't reference it yet)
- [ ] Removing a field that the template references produces a warning
      at the next template retrieval or render (not at schema save time)

## Edge Cases and Error Handling

- **No entity schema**: If a collection has no `entity_schema` (all
  entities accepted), template field validation is skipped — any field
  reference is accepted since the schema imposes no constraints
- **Template with no field references**: A static markdown template
  (no `{{...}}` expressions) is valid — useful for collections where
  the template is just a header or boilerplate
- **Very large entities**: Entities with deeply nested structures or
  large arrays may produce large markdown output. No output size limit
  in V1; monitoring recommended
- **Template references field added later**: If a template references
  `{{new_field}}` and the schema doesn't have it yet, the template
  save is rejected. The schema must be updated first, then the template
- **Concurrent template and schema update**: Template validation uses
  the schema at save time. If the schema is updated concurrently
  (removing a field), the template may reference a now-removed field.
  Next render produces empty for that field (Mustache default). Next
  template save would catch the stale reference
- **HTML in field values**: Mustache `{{field}}` HTML-escapes by
  default. Fields intended to contain markdown should use `{{{field}}}`
  (triple-mustache, unescaped). Template authors must make this choice
  explicitly

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

## Notes

Expedited shipping requested. See PO-2026-118 for terms.
```

**Rendered output** (for a draft invoice without approver or notes):
```markdown
# Invoice INV-2026-0043

**Vendor:** Globex Inc
**Status:** draft
**Amount:** EUR 150.00

## Line Items

- Consulting hours (qty: 3, 50.00 each)
```

The optional `approver` and `notes` sections are cleanly omitted via
Mustache sections — no "null" text, no empty headers.

## Dependencies

- **FEAT-002** (Schema Engine): Template field validation requires
  the entity schema
- **FEAT-005** (API Surface): New endpoint and query parameter
- **FEAT-003** (Audit Log): Template saves are audited

## Out of Scope

- **Markdown-to-entity parsing**: Render is one-way. Agents POST JSON
- **Multiple named templates**: One template per collection in V1
- **Partials / shared templates**: No cross-collection template reuse
- **Custom Mustache helpers**: Logic-less only. No lambdas, no custom
  functions
- **Template inheritance**: No base templates with override blocks
- **List/query markdown rendering**: Deferred to follow-on work
- **GraphQL/MCP integration**: Deferred to follow-on work
- **Localization**: Templates are not locale-aware. One template per
  collection, not per locale

## Success Metrics

- Template definition takes < 2 minutes for a typical collection
- Agents can retrieve and use rendered markdown without any
  client-side formatting code
- Rendering adds < 1ms to entity GET latency (p99)

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P1 #13 (MCP server —
  agent-native output), P2 #8 (Application substrate — schema-driven
  presentation)
- **User Stories**: US-075, US-076, US-077
- **Review Findings**: axon-abf94a4d (separation of concerns),
  axon-340fa160 (template validation), axon-5076f930 (API scope),
  axon-1360014c (axon-render crate)
- **Test Suites**: `tests/FEAT-026/`
- **Implementation**: `crates/axon-render/`

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-005, FEAT-003
- **Depended By**: FEAT-024 (Application Substrate — rendered views),
  FEAT-016 (MCP Server — markdown entity output, future)
