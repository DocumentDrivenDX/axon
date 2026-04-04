---
dun:
  id: ADR-002
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-007
---
# ADR-002: Schema Format — Hybrid JSON Schema + Axon Vocabulary

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-04 | Proposed | Erik LaBianca | FEAT-002, FEAT-007, FEAT-010 | High |

## Context

Axon needs a schema system that supports deeply nested entities (up to 8 levels), typed directional links with metadata, validation rules with severity levels, context-specific constraints, schema evolution, and multi-format bridging — all with sub-millisecond performance in Rust.

| Aspect | Description |
|--------|-------------|
| Problem | No existing schema format natively supports entity-graph-relational models with audit-first validation, typed links, and agent-friendly structured errors |
| Current State | 19 schema formats evaluated across graph/semantic, document/object, database-native, and data pipeline categories. See [schema format research](../../00-discover/schema-format-research.md) |
| Requirements | Entity nesting, typed links, validation rules with severity, context-specific constraints, schema evolution, Rust-native sub-ms performance, agent-parseable errors |

## Decision

We will implement a **hybrid Entity Schema Format (ESF)** with three layers:

1. **Layer 1 — JSON Schema Draft 2020-12** for entity body structure validation. The `jsonschema` Rust crate provides sub-microsecond validation with full spec compliance. Entity body schemas are valid, standalone JSON Schema documents.

2. **Layer 2 — Axon link-type definitions** for typed relationships between entities. Inspired by EdgeDB SDL's link model and ISO PG-Schema's edge-type definitions. Not expressible in JSON Schema; custom Axon vocabulary.

3. **Layer 3 — Axon validation rules** with severity levels (error/warning/info), context-specific overrides, and structured error reporting. Inspired by UMF's per-LOB nullability and SHACL's validation report format. Custom Axon vocabulary.

ESF documents are **YAML** (with JSON as alternative serialization). Schema evolution uses **Avro-style compatibility modes** (backward, forward, full, none) with automatic breaking-change detection.

**Key Points**: JSON Schema for free sub-ms validation | Custom vocabulary only where JSON Schema can't reach | One constraint language, not three

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| A. Pure JSON Schema with `x-` extensions | Largest ecosystem, universal tooling, agents already know it | No native link-type support, no severity levels, no context-specific constraints, extensions are non-standard | Rejected: extensions would be so extensive they'd be a custom format wearing a JSON Schema disguise |
| B. SHACL / ShEx | Graph-native validation, W3C standard, closed-world validation proven | RDF-based, no Rust validator, poor developer ergonomics, steep learning curve, alienates non-graph developers | Rejected: adoption barrier too high, no Rust ecosystem |
| C. EdgeDB SDL | Best entity-graph-relational fit, links are first-class, computed properties, migration generation | No Rust parser, requires EdgeDB server, proprietary grammar, small community | Rejected: no Rust tooling, server dependency |
| D. Custom ESF from scratch | Maximum design freedom, perfectly tailored | Must build all tooling, no ecosystem leverage, agents unfamiliar with format | Rejected: reinventing JSON Schema validation is wasteful |
| **E. Hybrid (JSON Schema + Axon vocabulary)** | Sub-ms validation via `jsonschema` crate, agents already understand JSON Schema, custom only where needed, bridges naturally | Two-layer mental model, JSON Schema limitations require escape to Layer 3 | **Selected**: best balance of ecosystem leverage and graph-native capability |
| F. OWL / RDFS | Formal reasoning, class hierarchies, W3C standard | Open-world assumption wrong for validation, no Rust tooling, academic adoption only, every system that started with OWL added JSON later | Rejected: wrong model for write-time validation |

## Consequences

| Type | Impact |
|------|--------|
| Positive | Free sub-microsecond entity validation via `jsonschema` crate. Agents can read/write entity schemas using standard JSON Schema knowledge. Bridges to SQL DDL, Protobuf, TypeScript leverage JSON Schema ecosystem |
| Negative | Two-layer model adds conceptual overhead. Link-type and validation-rule formats are Axon-specific (no external ecosystem). Must maintain compatibility with JSON Schema spec evolution |
| Neutral | Schema stored as versioned entities in Axon (schema-as-data pattern from TerminusDB). Schema changes produce audit entries just like data changes |

## ESF Document Structure (Conceptual)

```yaml
esf_version: "1.0"
collection: invoices

# Layer 1: Entity body (valid JSON Schema 2020-12)
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

# Layer 2: Link types (Axon vocabulary)
link_types:
  belongs-to:
    target_collection: vendors
    cardinality: many-to-one
    required: true
    metadata_schema:
      type: object
      properties:
        since: { type: string, format: date }

  paid-by:
    target_collection: payments
    cardinality: many-to-many
    metadata_schema:
      type: object
      required: [amount_applied]
      properties:
        amount_applied: { type: number, minimum: 0 }

# Layer 3: Validation rules (Axon vocabulary)
validation_rules:
  - name: line_items_sum_matches_amount
    severity: error
    expression: "sum(entity.line_items[*].quantity * entity.line_items[*].unit_price) == entity.amount.value"
    message: "Line item total must equal invoice amount"

  - name: approved_requires_approver_link
    severity: error
    condition: "entity.status == 'approved'"
    expression: "links_count('approved-by') >= 1"
    message: "Approved invoices must have an approved-by link"

# State machine (optional, FEAT-010)
state_machine:
  field: status
  initial: draft
  terminal: [paid, reconciled]
  transitions:
    - from: draft
      to: submitted
    - from: submitted
      to: approved
      guard: "links_count('approved-by') >= 1"
    - from: approved
      to: paid
      guard: "links_count('paid-by') >= 1"

# Evolution
evolution:
  compatibility: backward
```

## Implementation Impact

| Aspect | Assessment |
|--------|------------|
| Effort | Medium — JSON Schema validation is free (crate). Link-type and validation-rule parsers are moderate. Bridge generators are P2 |
| Skills | Rust, JSON Schema spec, schema diff algorithms |
| Performance | Excellent — `jsonschema` crate: sub-microsecond for typical entities. Layer 2/3 add minimal overhead |
| Scalability | Schema stored per-collection. Validation is per-entity (no cross-entity joins at write time, except guard conditions) |
| Security | Schema injection prevented: schemas validated on definition, not at runtime eval. Expression language is a restricted predicate DSL, not arbitrary code |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Custom expression language grows complex | Medium | High | Start with minimal predicate set (comparisons, exists, count). No loops, no variables. Add expressiveness only when use cases demand it |
| JSON Schema spec evolves incompatibly | Low | Medium | Pin to Draft 2020-12. Monitor spec via json-schema.org. `jsonschema` crate tracks spec versions |
| Two-layer model confuses developers | Medium | Medium | Clear docs: "JSON Schema = what an entity looks like. Axon vocabulary = how entities relate and what extra rules apply." Examples for every domain |
| Breaking-change detection false positives | Medium | Low | Start conservative (flag all non-additive changes). Refine heuristics based on real usage |

## Dependencies

- **Technical**: `jsonschema` crate (Rust, Draft 2020-12), `serde_yaml`, `serde_json`
- **Decisions**: ADR-001 (Rust)
- **Research**: [Schema Format Research](../../00-discover/schema-format-research.md)

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Entity validation p99 < 1ms | If validation becomes bottleneck, profile and optimize |
| Schema definition takes < 5 minutes for a typical collection | If developers find ESF burdensome, simplify or add scaffolding tools |
| Zero escape-to-raw-JSON workarounds in V1 use cases | If developers bypass ESF, the format is too restrictive |

## References

- [Schema Format Research](../../00-discover/schema-format-research.md) — 19 formats evaluated
- [FoundationDB DST Research](../../00-discover/foundationdb-dst-research.md) — correctness testing approach
- [Technical Requirements](../../01-frame/technical-requirements.md) — ESF section
- EdgeDB SDL: https://www.edgedb.com/docs/datamodel/index
- JSON Schema Draft 2020-12: https://json-schema.org/draft/2020-12/json-schema-core
- SHACL: https://www.w3.org/TR/shacl/
- Avro Schema Evolution: https://avro.apache.org/docs/current/specification/#schema-resolution
