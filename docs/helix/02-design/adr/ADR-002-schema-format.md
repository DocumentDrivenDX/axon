---
ddx:
  id: ADR-002
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-007
---
# ADR-002: Schema Format — JSON Schema + Link-Type Definitions

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-04 | Accepted | Erik LaBianca | FEAT-002, FEAT-007 | High |

## Context

Axon needs a schema system that supports deeply nested entities (up to 8 levels), typed directional links with metadata, and sub-millisecond write-time validation in Rust. The system must be understandable by AI agents and human developers without learning a new schema language.

| Aspect | Description |
|--------|-------------|
| Problem | No existing schema format natively supports entity-graph-relational models with typed links |
| Current State | 19 schema formats evaluated. See [schema format research](../../00-discover/schema-format-research.md) |
| Requirements | Entity nesting, typed links, Rust-native sub-ms validation, agent-parseable errors |

## Decision

We will use **JSON Schema Draft 2020-12 for entity bodies** and **Axon link-type definitions for relationships**. Start minimal — these two layers are already far beyond the status quo.

1. **Entity schemas are standard JSON Schema.** The `jsonschema` Rust crate provides sub-microsecond validation with full spec compliance. Any tool that understands JSON Schema can work with Axon entity schemas.

2. **Link-type definitions are Axon-specific.** JSON Schema has no concept of typed directional relationships. Link-types declare: source/target collection, cardinality, required/optional, and a metadata schema (which itself is JSON Schema).

That's it for V1. Validation rules with severity levels, context-specific constraints, state machines, and schema evolution are deferred until the minimal schema proves insufficient. We add complexity only when real use cases demand it.

**Key Points**: JSON Schema for entities (free sub-ms validation, agents already know it) | Minimal link-type vocabulary for relationships | Nothing else until needed

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| A. Pure JSON Schema with `x-` extensions | Largest ecosystem | No native link support; extensions become a disguised custom format | Rejected |
| B. SHACL / ShEx | Graph-native, W3C standard | RDF-based, no Rust validator, steep learning curve | Rejected |
| C. EdgeDB SDL | Best entity-graph fit, links first-class | No Rust parser, requires EdgeDB server | Rejected |
| D. Custom format from scratch | Maximum freedom | Must build all tooling, reinvents JSON Schema validation | Rejected |
| **E. JSON Schema + link-type definitions** | Sub-ms validation via `jsonschema` crate, agents know JSON Schema, custom only for links | Link-type format is Axon-specific | **Selected** |
| F. OWL / RDFS | Formal reasoning, W3C | Open-world wrong for validation, no Rust tooling, every OWL system added JSON later | Rejected |

## Consequences

| Type | Impact |
|------|--------|
| Positive | Free sub-microsecond entity validation. Agents read/write schemas using standard JSON Schema. Bridges to SQL DDL, Protobuf, TypeScript work out of the box |
| Negative | Link-type format is Axon-specific (small surface area). No cross-field or cross-entity validation rules in V1 — application logic handles these |
| Neutral | Schemas stored as versioned entities in Axon (schema-as-data). Schema changes produce audit entries |

## Collection Schema Document (V1)

```yaml
collection: invoices

# Entity body — standard JSON Schema Draft 2020-12
entity_schema:
  type: object
  required: [vendor_id, amount, status]
  properties:
    vendor_id:
      type: string
      format: uuid
    amount:
      type: object
      properties:
        value: { type: number, minimum: 0 }
        currency: { type: string, enum: [USD, EUR, GBP] }
    line_items:
      type: array
      items:
        type: object
        properties:
          description: { type: string }
          quantity: { type: integer, minimum: 1 }
          unit_price: { type: number, minimum: 0 }
    status:
      type: string
      enum: [draft, submitted, approved, paid, reconciled]

# Link types — Axon vocabulary
link_types:
  belongs-to:
    target_collection: vendors
    cardinality: many-to-one
    required: true

  paid-by:
    target_collection: payments
    cardinality: many-to-many
    metadata_schema:
      type: object
      required: [amount_applied]
      properties:
        amount_applied: { type: number, minimum: 0 }

  approved-by:
    target_collection: contacts
    cardinality: many-to-many
    metadata_schema:
      type: object
      properties:
        approved_at: { type: string, format: date-time }
```

### What's in scope for V1

- Entity body validation via `jsonschema` crate
- Link-type declarations: target collection, cardinality (one-to-one, one-to-many, many-to-one, many-to-many), required/optional
- Link metadata validation via `jsonschema` (metadata_schema is JSON Schema)
- Structured validation errors: field path, expected type, actual value, human-readable message
- Schema introspection API: retrieve schema for any collection

### What's deferred

| Feature | When | Trigger |
|---------|------|---------|
| Validation rules with severity (error/warning/info) | When a use case needs cross-field validation that JSON Schema can't express | First real-world request |
| Context-specific constraints (per-LOB nullability) | When multi-tenant or multi-context deployments need it | First multi-context deployment |
| State machines | FEAT-010 (P2) | After core entity/link model is proven |
| Schema evolution / breaking-change detection | P1 | After V1 schemas stabilize |
| Expression language for guards/rules | When simple field predicates prove insufficient | Complexity pressure from real use cases |
| Schema bridges (ESF → SQL DDL, Protobuf, TypeScript) | P2 | When client SDK generation is needed |
| Schema bridge to UMF (tablespec) | P2 | When data pipeline integration is needed |

## Implementation Impact

| Aspect | Assessment |
|--------|------------|
| Effort | Low — `jsonschema` crate handles entity validation. Link-type parser is ~200 lines of serde deserialization |
| Performance | Excellent — sub-microsecond entity validation. Link-type check is a HashMap lookup + optional metadata validation |
| Security | Schemas validated on definition. No expression evaluation in V1 — no injection surface |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| JSON Schema can't express needed constraints | Medium | Low | Defer to application logic in V1. Add validation rules when real need emerges |
| Link-type vocabulary needs to grow | Medium | Low | Small surface area — adding fields to link-type definitions is non-breaking |
| Developers want more schema power sooner | Low | Low | JSON Schema already handles 90% of validation needs. Link-types handle relationships. Most remaining needs are application logic |

## Dependencies

- **Technical**: `jsonschema` crate (Rust, Draft 2020-12), `serde_yaml`, `serde_json`
- **Decisions**: ADR-001 (Rust)
- **Research**: [Schema Format Research](../../00-discover/schema-format-research.md)

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Entity validation p99 < 1ms | If validation becomes bottleneck |
| Schema definition takes < 5 minutes | If developers find it burdensome |
| No workaround requests in first 3 months | If developers need to bypass schema, V1 scope was too narrow |

## References

- [Schema Format Research](../../00-discover/schema-format-research.md) — 19 formats evaluated
- [Technical Requirements](../../01-frame/technical-requirements.md) — schema system section
- JSON Schema Draft 2020-12: https://json-schema.org/draft/2020-12/json-schema-core
