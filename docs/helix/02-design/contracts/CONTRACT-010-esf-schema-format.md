---
ddx:
  id: CONTRACT-010
  depends_on:
    - ADR-002
    - ADR-007
    - ADR-008
    - FEAT-019
    - FEAT-002
  review:
    self_hash: 9250599003d21f3885a52eb67ad688139715e9aa0497bf1634ad27e2d505e134
    deps:
      ADR-002: 914b8c8b1a9829504c826ae36b8d8b48a6118b0268c6c8c562fc446ee01b9a77
      ADR-007: 5a96b23ec82c256af094753065c60c6862a9a7c2fd8e7db3bb681d896627f727
      ADR-008: 9c129ed10278306924eac7b4b3915894f3584f4cfe22243bbf486afdac2fccfc
      FEAT-002: 84f680ec396f34b25b2a91172d8cab7a8e9204817430b9e3aa8f9ec1ee3afd03
      FEAT-019: ddf48d3192c435e1b9a40b2dc77ec60f363bfd91230e99fab336ebf4232785c4
    reviewed_at: "2026-07-11T03:00:17Z"
---

# Contract

**Contract ID**: CONTRACT-010
**Type**: schema (Entity Schema Format)
**Version**: 0.1.0
**Status**: draft
**Related**: ADR-002, ADR-007, ADR-008, FEAT-002, FEAT-019, FEAT-017, CONTRACT-004, CONTRACT-007

## Purpose

Defines the normative Entity Schema Format (ESF): the layered collection
schema document including lifecycle declarations (Layer 3) and validation
rules (Layer 5) — rule YAML syntax, condition/requirement operator tables,
gate declaration grammar, the gate status response, and the
`VALIDATION_FAILED` error envelope. Schema authors, the schema compiler,
and write-path validators implement against this document.

## Scope and Boundaries

- In scope: ESF document structure and layers, lifecycle declaration
  grammar and enforcement rules, Layer-5 rule grammar, operators, gates,
  gate-status and validation-error wire shapes, schema-save validation.
- Out of scope: `access_control` policy grammar (CONTRACT-004), `queries:`
  named-query grammar (CONTRACT-007), index physical design (ADR-010),
  schema evolution classification (FEAT-017), gate-status storage tables.
- Owning system: `axon-schema`.

## Normative Surface

### Document structure and layers

An ESF document is YAML (or equivalent JSON) with these top-level keys:

| Element | Layer | Required | Rules |
|---------|-------|----------|-------|
| `esf_version` | — | yes | Format version string; V1 is `"1.0"` |
| `collection` | — | yes | Collection name |
| `description` | — | no | Human-readable description |
| `entity_schema` | L1 | yes | Standard JSON Schema Draft 2020-12 for the entity body (ADR-002) |
| `link_types` | L2 | no | Map of link-type name → `{ target_collection, cardinality, required, metadata_schema }`; cardinality ∈ `one-to-one`, `one-to-many`, `many-to-one`, `many-to-many`; `metadata_schema` is JSON Schema |
| `lifecycles` | L3 | no | Map of lifecycle name → lifecycle declaration (below; ADR-008) |
| `indexes` | L4 | no | Secondary index declarations (FEAT-013) |
| `validation_rules` | L5 | no | List of validation rules (below; FEAT-019) |
| `gates` | L5 | no | Gate declarations (below) |
| `access_control` | adjacent | no | Policy metadata — CONTRACT-004 |
| `queries` | adjacent | no | Named graph queries — CONTRACT-007 |

The schema version is system-assigned and auto-incremented on every
`put_schema` (ADR-007). All versions are retained; the latest is active.

The internal contract is fail-closed for governed collection-addressable
state: `entity_schema` is required, and schema/namespace/link failures are
represented by typed internal exceptions that surface as stable external
errors rather than generic failures.

### Structural catalog hash

Axon derives a whole-catalog structural projection named `StructuralSchemaV1`
from the active ESF documents for a `(tenant_id, database_id)` pair. The
projection is serialized with AXON-CJSON-1 canonical bytes and hashed with
SHA-256 to produce `AXON-SCHEMA-CATALOG-HASH-1` (`sha256:...`).

| Field | Meaning |
|---|---|
| `format_version` | Canonical structural-manifest format version (currently `1`) |
| `tenant_id` | Tenant whose active schema catalog is being hashed |
| `database_id` | Database whose active schema catalog is being hashed |
| `active_version` | Schema version currently active for the catalog |
| `collections` | Normalized array of active collection entries, sorted by qualified collection name |

Normalization rules:

- The `collections` array includes only structural declarations from the
  active schema: `entity_schema`, `link_types`, `lifecycles`, `indexes`, and
  other structural layers. It excludes `access_control`, inactive versions or
  history, descriptions, timestamps, and other non-structural annotations.
- Each collection's `link_types` are sorted by qualified target collection and
  then link name.
- Normalized link declarations carry explicit `target_collection`,
  `cardinality`, `required`, and `metadata_schema` fields, and any omitted
  structural values are defaulted before hashing so equivalent source
  documents yield the same bytes.
- Collection and link names are compared in qualified form during
  normalization so the hash is stable across source-order changes.

### Layer 3 — lifecycle declarations

```yaml
lifecycles:
  <name>:                # logical name, usually the field name
    field: <string>      # required; entity data field governed
    initial: <string>    # required; MUST be a key in transitions
    transitions:         # required; state -> list of valid targets
      <state>: [<state>, ...]            # simple form
      <state>:                           # guarded form
        - target: <state>
          requires_gate: <gate name>     # optional transition guard
        - target: <state>
      <terminal_state>: []               # empty list = terminal
```

Enforcement (normative):

- On create: if the lifecycle field is absent, set it to `initial`; if
  present, it MUST be a key in `transitions`.
- On update: if the field value changed, the transition MUST be in
  `transitions[current]`; otherwise reject with an error naming the current
  state, attempted state, and the valid transitions.
- On delete: no lifecycle check.
- `requires_gate`: the transition is blocked unless the entity passes every
  rule in the named gate; the error response includes the gate name,
  failing rules, and fix suggestions.

Schema-save validation for lifecycles:

1. Every transition target MUST be a key in `transitions`.
2. `initial` MUST be a key in `transitions`.
3. If `entity_schema` declares an `enum` on the lifecycle field, the enum
   values MUST be a superset of the lifecycle states.
4. The transition graph need not be connected.

### Layer 5 — validation rule structure

Each entry in `validation_rules` has:

| Field | Required | Description |
|---|---|---|
| `name` | yes | Unique identifier for the rule within the collection |
| `gate` | no* | Gate this rule belongs to: `save`, or any custom gate name |
| `advisory` | no* | If `true`, rule never blocks — always reports. Mutually exclusive with `gate` |
| `when` | no | Condition that activates the rule. If omitted, the rule always applies |
| `require` | yes | The constraint enforced when the rule is active |
| `message` | yes | Human-readable explanation of the business rule |
| `fix` | no | Actionable remediation; MAY include `{field}` placeholders substituted with current values |

\* Each rule MUST specify exactly one of `gate` or `advisory: true`. There
is no `severity` field (see Non-Normative Notes for the resolution).

### Condition operators (`when`)

| Operator | Description | Example |
|---|---|---|
| `eq` | Field equals value | `{field: status, eq: "approved"}` |
| `ne` | Field not equal | `{field: status, ne: "draft"}` |
| `in` | Field in list | `{field: bead_type, in: ["bug", "task"]}` |
| `not_null` | Field present and non-null | `{field: owner, not_null: true}` |
| `is_null` | Field absent or null | `{field: deleted_at, is_null: true}` |
| `gt`, `gte`, `lt`, `lte` | Numeric/date comparison | `{field: priority, lte: 1}` |
| `match` | Regex match | `{field: email, match: "^.+@.+$"}` |
| `all` | All sub-conditions true (AND) | `{all: [{...}, {...}]}` |
| `any` | Any sub-condition true (OR) | `{any: [{...}, {...}]}` |

### Requirement operators (`require`)

| Operator | Description | Example |
|---|---|---|
| `not_null` | Field must be present and non-null | `{field: approver_id, not_null: true}` |
| `eq` | Field must equal value | `{field: currency, eq: "USD"}` |
| `in` | Field must be in list | `{field: status, in: ["draft", "pending"]}` |
| `gt_field` | Field greater than another field | `{field: due_date, gt_field: created_date}` |
| `gte_field`, `lt_field`, `lte_field` | Cross-field comparison | |
| `match` | Field must match regex | `{field: title, match: "^[A-Z]"}` |
| `not_match` | Field must not match regex | `{field: title, not_match: "^TODO"}` |
| `min_length` | String minimum length | `{field: description, min_length: 10}` |

### Gate declaration grammar

```yaml
gates:
  <gate_name>:
    description: <string>        # optional
    includes: [<gate_name>, ...] # optional; inclusion is explicit
```

Gate inclusion is transitive set union: a gate's effective rule set is its
own rules plus every rule of each included gate (e.g. `review` includes
`complete`).

### Gate semantics

| Gate type | Write behavior | Response behavior | Queryable |
|---|---|---|---|
| `save` | **Block** — entity cannot be persisted | Errors in response | N/A (entity doesn't exist) |
| Custom gates (`complete`, `review`, …) | Allow save | Gate pass/fail + failures in response | Yes — via gate status |
| `advisory: true` | Allow save | Advisories in response | Yes — via gate status |

Rule evaluation order (normative):

1. JSON Schema validation (L1) runs first.
2. L5 rules run only if L1 passes.
3. Lifecycle validation (L3) runs independently (transition validity, not
   field constraints).
4. Within L5, rules evaluate in declaration order; all rules are evaluated
   (no short-circuit); all failures and advisories are collected and
   reported together.

The save gate is evaluated on every write path that persists entity payload
data (create, update, patch), after lifecycle initial-state handling and
JSON Schema validation.

### Gate status in entity responses

Every entity response includes current gate status:

```json
{
  "entity": { "id": "bead-42", "version": 4 },
  "gates": {
    "complete": {
      "pass": false,
      "failures": [
        {
          "rule": "assignee-for-complete",
          "field": "assignee",
          "message": "An assignee is required before marking as ready",
          "fix": "Assign someone to this item"
        }
      ]
    },
    "review": {
      "pass": false,
      "failures": [
        { "rule": "assignee-for-complete", "field": "assignee", "message": "...", "fix": "..." },
        { "rule": "acceptance-for-review", "field": "acceptance", "message": "...", "fix": "..." }
      ]
    }
  },
  "advisories": [
    {
      "rule": "description-recommended",
      "field": "description",
      "message": "Consider adding a description"
    }
  ]
}
```

### `VALIDATION_FAILED` envelope

Save-gate failures (including translated JSON Schema violations) return:

```json
{
  "code": "VALIDATION_FAILED",
  "errors": [
    {
      "rule": "approved-needs-approver",
      "gate": "save",
      "field": "approver_id",
      "message": "Approved items must have an approver_id set",
      "fix": "Set approver_id to the user who approved this item",
      "context": {
        "trigger_field": "status",
        "trigger_value": "approved"
      }
    }
  ],
  "advisories": [
    {
      "rule": "epics-should-have-description",
      "field": "description",
      "message": "Epics should have a description for planning purposes",
      "fix": "Add a description explaining the epic's scope and goals",
      "context": {
        "trigger_field": "bead_type",
        "trigger_value": "epic"
      }
    }
  ]
}
```

Required properties:

- All failures and advisories are reported, not just the first.
- Each violation identifies `rule`, `field`, and (for gated rules) `gate`;
  `context` shows the condition that activated the rule.
- `fix` provides concrete remediation.
- JSON Schema (L1) violations are translated into the same shape with
  generated `fix` suggestions and near-match ("did you mean?") hints for
  enum values and field names.
- Blocking failures appear in `errors`; advisory findings appear in
  `advisories`. There is no per-item `severity` field.

### Schema-save validation for Layer 5

When a schema containing validation rules is saved:

1. Rule names MUST be unique within the collection.
2. Referenced fields MUST exist in the entity schema (or be `*` for any).
3. Each rule MUST declare exactly one of `gate` or `advisory: true`; a
   `gate` value MUST be `save` or a declared gate name; `includes` MUST
   reference declared gates and MUST NOT form a cycle.
4. `message` is required and non-empty.
5. Condition and requirement operators MUST be syntactically valid.
6. `gt_field` / `gte_field` / `lt_field` / `lte_field` referenced fields
   MUST exist in the schema.
7. Regex patterns (`match`, `not_match`) MUST be valid.

## Precedence and Compatibility

- Versioning: `esf_version` versions the format; the collection schema
  version is system-assigned, auto-incremented, and fully retained
  (ADR-007). Old versions are viewable but not restorable in V1.
- Layer precedence at write time: L1 (JSON Schema) → L5 (rules) → L3
  transition validity is checked independently; gates compose with L3 via
  `requires_gate`.
- When validation rules change, gate statuses for existing entities are
  recomputed in the background; until recomputation completes, stored gate
  status MAY be stale.
- Compatibility of schema changes is classified per FEAT-017; lifecycle and
  rule changes are schema changes and are versioned, diffed, and audited.
- Unknown top-level ESF keys MUST be rejected (closed format), preserving
  room for new layers via `esf_version` bumps. Reserved namespace collisions
  MUST be rejected with `reserved_namespace`.

## Error Semantics

| Condition | Error / Outcome | Retry | Recovery Expectation |
|-----------|------------------|-------|----------------------|
| Save-gate rule fails on create/update/patch | `VALIDATION_FAILED`; entity not persisted | yes | Apply the `fix` suggestions and resubmit |
| Custom gate fails | Entity persists; gate `pass: false` with failures in response | n/a | Fix incrementally; gate status is queryable |
| Lifecycle transition not in `transitions[current]` | Invalid-operation error naming current state, attempted state, and valid transitions | yes | Use a valid transition |
| Transition with `requires_gate` while gate fails | Transition blocked; response includes gate name, failing rules, fixes | yes | Satisfy the gate, then re-attempt |
| Lifecycle field invalid on create | Rejected (state not in `transitions`) | yes | Use a declared state or omit for `initial` |
| Schema save violates lifecycle or rule validation | Schema rejected; version not created | yes | Fix the declaration per the diagnostics |
| L1 fails | `VALIDATION_FAILED` with translated JSON Schema errors; L5 skipped | yes | Fix structural errors first |

## Examples

```yaml
esf_version: "1.0"
collection: beads

entity_schema:
  type: object
  required: [bead_type, status, title]
  properties:
    bead_type: { type: string }
    status: { type: string }
    title: { type: string, minLength: 1 }
    priority: { type: integer }
    assignee: { type: string }
    description: { type: string }
    due_date: { type: string, format: date }
    created_date: { type: string, format: date }

link_types:
  depends-on:
    target_collection: beads
    cardinality: many-to-many

lifecycles:
  status:
    field: status
    initial: draft
    transitions:
      draft: [pending, cancelled]
      pending:
        - target: ready
          requires_gate: complete
        - target: cancelled
      ready: [in_progress, cancelled]
      in_progress:
        - target: review
          requires_gate: review
        - target: blocked
        - target: done
      review: [done, in_progress, cancelled]
      done: []
      blocked: [pending, cancelled]
      cancelled: []

gates:
  complete:
    description: "Entity has all required fields for processing"
  review:
    includes: [complete]
    description: "Entity is ready for human review"

validation_rules:
  - name: valid-bead-type
    gate: save
    require: { field: bead_type, in: [task, bug, epic, chore, spike, feature] }
    message: "bead_type must be a valid type"

  - name: due-after-created
    gate: save
    when: { field: due_date, not_null: true }
    require: { field: due_date, gt_field: created_date }
    message: "Due date must be after creation date"
    fix: "Set due_date to a date after {created_date}"

  - name: assignee-for-complete
    gate: complete
    require: { field: assignee, not_null: true }
    message: "An assignee is required before marking as ready"
    fix: "Assign someone to this item"

  - name: high-priority-bugs-need-assignee
    gate: review
    when:
      all:
        - { field: bead_type, eq: "bug" }
        - { field: priority, lte: 1 }
    require: { field: assignee, not_null: true }
    message: "High-priority bugs (P0/P1) must have an assignee for review"
    fix: "Set assignee to the person responsible"

  - name: title-not-placeholder
    advisory: true
    require: { field: title, not_match: "^(TODO|FIXME|untitled|test)$" }
    message: "Title appears to be a placeholder"
    fix: "Replace title with a descriptive name"
```

## Non-Normative Notes

**Severity → gate/advisory resolution.** FEAT-019's schema-save checklist
(:566) requires `severity ∈ {error, warning, info}`, while its own rule
structure (:204-215) and gate semantics define `gate`/`advisory` with no
severity field. The gate/advisory model is the current design and is
normative here: blocking behavior is a property of the gate a rule belongs
to (`save` blocks persistence; custom gates block transitions via
`requires_gate`), and `advisory: true` marks never-blocking rules. The
schema-save check that previously validated the severity enum is replaced
by check 3 above (gate XOR advisory). The `VALIDATION_FAILED` envelope
correspondingly separates `errors` from `advisories` instead of carrying
per-item severity. FEAT-019 should be amended to drop the severity check.

ADR-002 lists "validation rules with severity levels" as deferred; FEAT-019
is that deferral landing, in gate/advisory form. Gate-status storage,
recomputation workers, and `_gate.*` query filters are governed by ADR-010
§11 and FEAT-019, not this contract.

## Validation Checklist

- [ ] Normative fields and rules are explicit.
- [ ] Compatibility and precedence rules are explicit.
- [ ] Error handling is explicit.
- [ ] At least one executable test can be derived from this contract.
- [ ] Non-normative notes cannot be mistaken for contract requirements.
