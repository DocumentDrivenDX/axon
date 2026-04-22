---
ddx:
  id: FEAT-019
  depends_on:
    - helix.prd
    - FEAT-002
    - FEAT-004
    - ADR-002
    - ADR-008
---
# Feature Specification: FEAT-019 - Validation Rules and Actionable Errors

**Feature ID**: FEAT-019
**Status**: Draft
**Priority**: P1
**Owner**: Core Team
**Created**: 2026-04-05
**Updated**: 2026-04-05

## Overview

JSON Schema (ESF Layer 1) validates field types and basic constraints.
Validation rules (ESF Layer 5) go further: cross-field conditions,
conditional requirements, **validation gates**, and actionable error
messages that tell agents exactly what's wrong and how to fix it.

Validation gates are named checkpoints that group rules by purpose.
The `save` gate blocks persistence. Other gates (`complete`, `review`,
`processing`) allow saves but track readiness — an entity can be
saved in an incomplete state and progressively improved until it passes
the required gate for a lifecycle transition or workflow step.

Gate pass/fail state is materialized as indexed fields on each entity,
making queries like "show me orders ready for processing" a fast
index lookup.

This is Axon's key differentiator over general-purpose databases. A SQL
constraint violation says "CHECK constraint failed." An Axon validation
error says "Field 'approver_id' is required when status is 'approved'.
Current status: 'approved'. Set approver_id to a valid user ID." — and
it tells you which workflow gates the entity passes and which it doesn't.

## Problem Statement

JSON Schema validates structure but not semantics. Real-world entities
have cross-field dependencies: approved invoices need an approver, bugs
need a priority, completed tasks should have a resolution. These rules
can't be expressed in JSON Schema alone.

Current validation errors from JSON Schema libraries are technical and
generic ("instance failed to match pattern"). Agents need errors that
explain the business rule, identify the fix, and distinguish hard
constraints from soft readiness checks.

In agentic workflows, the save-vs-validate distinction is critical.
Agents create entities early (save a draft with minimal fields), then
progressively fill in data. Hard validation that blocks saves on
incomplete data is hostile to this workflow. But soft validation that
tracks "what's still needed before this can proceed" is essential — the
agent needs to know "I saved it, but 3 things are still needed before
it can move to review."

## Requirements

### Functional Requirements

#### Validation Rule Declarations (ESF Layer 5)

Rules are declared in the collection schema alongside entity schema (L1),
link types (L2), lifecycles (L3), and indexes (L4):

```yaml
esf_version: "1.0"
collection: beads

entity_schema:
  # ... Layer 1 ...

validation_rules:                                    # Layer 5 — NEW

  # ── Save gate: must pass to persist the entity ─────────────

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

  # ── Complete gate: required before status → ready ──────────

  - name: description-for-complete
    gate: complete
    require: { field: description, not_null: true }
    message: "Description is required before marking as ready"
    fix: "Add a description"

  - name: priority-for-complete
    gate: complete
    require: { field: priority, not_null: true }
    message: "Priority must be set before marking as ready"
    fix: "Set priority (0-4)"

  - name: assignee-for-complete
    gate: complete
    require: { field: assignee, not_null: true }
    message: "An assignee is required before marking as ready"
    fix: "Assign someone to this item"

  - name: bugs-need-priority
    gate: complete
    when: { field: bead_type, eq: "bug" }
    require: { field: priority, lte: 2 }
    message: "Bugs must have priority P0-P2 before completion"
    fix: "Set priority to 0, 1, or 2"

  # ── Review gate: required before status → review ───────────

  - name: acceptance-for-review
    gate: review
    require: { field: acceptance, not_null: true }
    message: "Acceptance criteria must be defined before review"
    fix: "Add acceptance criteria"

  - name: high-priority-bugs-need-assignee
    gate: review
    when:
      all:
        - { field: bead_type, eq: "bug" }
        - { field: priority, lte: 1 }
    require: { field: assignee, not_null: true }
    message: "High-priority bugs (P0/P1) must have an assignee for review"
    fix: "Set assignee to the person responsible"

  # ── Advisory: never blocks, always reports ─────────────────

  - name: title-not-placeholder
    advisory: true
    require: { field: title, not_match: "^(TODO|FIXME|untitled|test)$" }
    message: "Title appears to be a placeholder"
    fix: "Replace title with a descriptive name"

  - name: description-recommended
    advisory: true
    require: { field: description, not_null: true }
    message: "Consider adding a description"
    fix: "Add a description for context"
```

#### Gate Integration with Lifecycles

Gates compose with lifecycle transitions. A transition can require that
a specific gate passes:

```yaml
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
      ready:
        - target: in_progress
        - target: cancelled
      in_progress:
        - target: review
          requires_gate: review
        - target: blocked
        - target: done
      review: [done, in_progress, cancelled]
      done: []
      blocked: [pending, cancelled]
      cancelled: []
```

When a lifecycle transition specifies `requires_gate`, the transition
is blocked if the entity fails any rule in that gate. The error response
includes the gate name, failing rules, and fix suggestions.

Gates are **inclusive**: the `review` gate implicitly includes all
`complete` gate rules (you can't be ready for review if you're not
complete). This is declared explicitly in the schema:

```yaml
gates:
  complete:
    description: "Entity has all required fields for processing"
  review:
    includes: [complete]
    description: "Entity is ready for human review"
```

#### Rule Structure

Each validation rule has:

| Field | Required | Description |
|---|---|---|
| `name` | Yes | Unique identifier for the rule within the collection |
| `gate` | No* | Gate this rule belongs to: `save`, or any custom gate name |
| `advisory` | No* | If `true`, rule never blocks — always reports. Mutually exclusive with `gate` |
| `when` | No | Condition that activates the rule. If omitted, rule always applies |
| `require` | Yes | The constraint to enforce when the rule is active |
| `message` | Yes | Human-readable explanation of the business rule |
| `fix` | No | Actionable suggestion for how to resolve the violation. May include `{field}` placeholders substituted with current values |

*Each rule must specify either `gate` or `advisory: true`.

#### Condition Operators (`when`)

| Operator | Description | Example |
|---|---|---|
| `eq` | Field equals value | `{field: status, eq: "approved"}` |
| `ne` | Field not equal | `{field: status, ne: "draft"}` |
| `in` | Field in list | `{field: bead_type, in: ["bug", "task"]}` |
| `not_null` | Field is present and non-null | `{field: owner, not_null: true}` |
| `is_null` | Field is absent or null | `{field: deleted_at, is_null: true}` |
| `gt`, `gte`, `lt`, `lte` | Numeric/date comparison | `{field: priority, lte: 1}` |
| `match` | Regex match | `{field: email, match: "^.+@.+$"}` |
| `all` | All sub-conditions true (AND) | `{all: [{...}, {...}]}` |
| `any` | Any sub-condition true (OR) | `{any: [{...}, {...}]}` |

#### Requirement Operators (`require`)

| Operator | Description | Example |
|---|---|---|
| `not_null` | Field must be present and non-null | `{field: approver_id, not_null: true}` |
| `eq` | Field must equal value | `{field: currency, eq: "USD"}` |
| `in` | Field must be in list | `{field: status, in: ["draft", "pending"]}` |
| `gt_field` | Field must be greater than another field | `{field: due_date, gt_field: created_date}` |
| `gte_field`, `lt_field`, `lte_field` | Cross-field comparison | |
| `match` | Field must match regex | `{field: title, match: "^[A-Z]"}` |
| `not_match` | Field must not match regex | `{field: title, not_match: "^TODO"}` |
| `min_length` | String minimum length | `{field: description, min_length: 10}` |

#### Gate Semantics

| Gate type | Write behavior | Response behavior | Queryable |
|---|---|---|---|
| `save` | **Block** — entity cannot be persisted | Errors in response | N/A (entity doesn't exist) |
| Custom gates (`complete`, `review`, etc.) | **Allow save** | Gate pass/fail + failures in response | Yes — via gate status table |
| `advisory: true` | **Allow save** | Advisories in response | Yes — via gate status table |

Custom gates allow progressive refinement: save early, validate
incrementally, gate transitions on readiness. The agent always knows
exactly what's still needed.

FEAT-019 validation runs on every entity write path that persists entity
payload data: create, update, and patch. The save gate is evaluated after
lifecycle initial-state handling and JSON Schema validation, but before
storage writes, index maintenance, and audit append. A failed save gate
therefore leaves no entity row and no mutation audit entry for that attempted
write. Custom gates and advisories are also evaluated on create, update, and
patch; their pass/fail results are materialized on the saved entity and echoed
in the write response.

Save gates are payload-only in V1. They can validate fields in the submitted
entity, but they cannot branch on caller identity, tenant role, JWT grant, or
other request context. Subject-aware authorization belongs to FEAT-029
policies. Until FEAT-029 is the enforcement layer for an application-specific
role field, browser-writable bootstrap schemas should use conservative
payload gates that allow only the safe default values.

#### Browser-Writable Bootstrap Role Pattern

For a browser-writable bootstrap flow, a self-created `users` row must not be
allowed to self-promote to an application admin role. The recommended interim
Axon rule is a collection-level save gate on the browser-writable users
collection that allows only the safe default `member` role:

```yaml
collection: users

entity_schema:
  type: object
  required: [display_name, role]
  properties:
    display_name: { type: string }
    role:
      type: string
      enum: [member, admin]

validation_rules:
  - name: browser-bootstrap-role-member-only
    gate: save
    require: { field: role, in: [member] }
    message: "Browser bootstrap users must start as member"
    fix: "Set role to member; create admin users through a privileged path"
```

This rule runs on create as well as update. A browser actor submitting
`role: admin` is rejected before persistence; `role: member` is accepted.
Because FEAT-019 rules do not inspect caller context, this schema intentionally
rejects `admin` for every caller on that browser-writable path. Admin or
operator bootstrap must use the control-plane membership/credential flow, an
operator seed script, or a separate admin-only collection/path until FEAT-029
subject-aware policies can express "non-admin callers may only create
members" directly.

#### Materialized Gate Status (1:M Table)

On every entity write (create, update, patch), Axon evaluates all
non-save gates and materializes the results in a dedicated gate status
table:

**Gate registry** — gate definitions are persisted when the schema is
saved, so the system knows which gates exist for each collection:

```
gate_definitions:
    PK: (collection_id, gate_name)
    description:    text
    includes:       text[]      -- gates this gate includes (e.g., review includes complete)
    rule_count:     int         -- number of rules in this gate
    created_at:     timestamp

    FK: collection_id → collections
```

**PostgreSQL materialization:**
```sql
CREATE TABLE gate_definitions (
    collection_id  INT   NOT NULL REFERENCES collections(id),
    gate_name      TEXT  NOT NULL,
    description    TEXT,
    includes       TEXT[],
    rule_count     INT   NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, gate_name)
);
```

When `put_schema` is called with validation rules, the handler:
1. Extracts all unique gate names from the rules
2. Reads the `gates` declaration from the schema for descriptions and
   `includes` relationships
3. Upserts `gate_definitions` — adding new gates, updating rule counts,
   removing gates that no longer have any rules
4. Triggers gate recomputation for entities if rules changed

**Entity gate status** — materialized per-entity pass/fail for each
registered gate:

```
entity_gates:
    PK: (collection_id, entity_id, gate_name)
    pass:           boolean
    failure_count:  int
    evaluated_at:   timestamp
    failures_json:  bytes    -- serialized list of {rule, field, message, fix}

    FK: (collection_id, entity_id) → entities  ON DELETE CASCADE
    FK: (collection_id, gate_name) → gate_definitions

    INDEX: (collection_id, gate_name, pass, entity_id)  -- "all entities passing gate X"
```

**PostgreSQL materialization:**
```sql
CREATE TABLE entity_gates (
    collection_id  INT       NOT NULL,
    entity_id      UUID      NOT NULL,
    gate_name      TEXT      NOT NULL,
    pass           BOOLEAN   NOT NULL,
    failure_count  INT       NOT NULL DEFAULT 0,
    evaluated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    failures_json  JSONB,
    PRIMARY KEY (collection_id, entity_id, gate_name),
    FOREIGN KEY (collection_id, entity_id)
        REFERENCES entities(collection_id, id) ON DELETE CASCADE,
    FOREIGN KEY (collection_id, gate_name)
        REFERENCES gate_definitions(collection_id, gate_name) ON DELETE CASCADE
);

CREATE INDEX idx_gates_by_status
    ON entity_gates (collection_id, gate_name, pass, entity_id);
```

**KV layout:**
```
gates/         {collection_id}/{entity_id}/{gate_name}         → {pass, failure_count, evaluated_at, failures_json}
gates/bystate/ {collection_id}/{gate_name}/{0|1}/{entity_id}   → {}
```

The PK ordering `(collection_id, entity_id, gate_name)` is fast for
the write path (update all gates for one entity). The secondary index
`(collection_id, gate_name, pass, entity_id)` is fast for the query
path ("all entities passing gate X").

`ON DELETE CASCADE` ensures gate rows are cleaned up when entities are
deleted.

Advisory rules are materialized as a special gate named `_advisory`
for queryability ("entities with advisory warnings").

#### Querying by Gate Status

Gate status is queryable via all interfaces:

**Structured API:**
```json
{
  "collection": "beads",
  "filter": {
    "and": [
      { "gate": "complete", "pass": true },
      { "field": "status", "op": "eq", "value": "pending" }
    ]
  }
}
```

**GraphQL:**
```graphql
query {
  beads(filter: {
    gates: { complete: true }
    status: { eq: PENDING }
  }) {
    edges { node { id title status } }
  }
}
```

**MCP:**
```
beads.query with filter: { _gate.complete: true, status: "pending" }
```

The query planner uses the `idx_gates_by_status` index for gate
filters, then intersects with EAV secondary indexes for field filters.

#### Gate Status in Entity Responses

Every entity response includes the current gate status:

```json
{
  "entity": { "id": "bead-42", "version": 4, ... },
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
        { "rule": "assignee-for-complete", ... },
        { "rule": "acceptance-for-review", ... }
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

The agent knows immediately: "I saved the entity, but it won't pass
the `complete` gate until I set an assignee." The agent can fix it now
or come back later.

#### Gate Recomputation

When validation rules change (schema update), gate statuses for existing
entities become stale. A background worker recomputes gates for all
entities in the collection (same mechanism as FEAT-017 revalidation and
FEAT-013 index rebuild).

#### Actionable Error Responses

Every validation failure (from JSON Schema or validation rules) produces
a structured error with enough context for an agent to self-correct:

```json
{
  "code": "VALIDATION_FAILED",
  "errors": [
    {
      "rule": "approved-needs-approver",
      "severity": "error",
      "field": "approver_id",
      "message": "Approved items must have an approver_id set",
      "fix": "Set approver_id to the user who approved this item",
      "context": {
        "trigger_field": "status",
        "trigger_value": "approved"
      }
    }
  ],
  "warnings": [
    {
      "rule": "epics-should-have-description",
      "severity": "warning",
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

**Key properties:**
- All errors and warnings are reported (not just the first)
- Each violation identifies the rule name, field, and triggering condition
- `fix` provides a concrete remediation (not just "invalid value")
- `context` shows which condition activated the rule
- JSON Schema violations are translated into the same format with
  generated `fix` suggestions

#### JSON Schema Error Enhancement

JSON Schema 2020-12 errors are enhanced to match the actionable format:

| JSON Schema error | Enhanced error |
|---|---|
| `"instance failed to match pattern"` | `"Field 'email' must match pattern '^.+@.+$'. Got: 'not-an-email'. Fix: Provide a valid email address"` |
| `"instance is not one of the enum values"` | `"Field 'status' must be one of [draft, pending, ready, ...]. Got: 'pendng'. Fix: Use one of the allowed values. Did you mean 'pending'?"` |
| `"instance type does not match any allowed primitive type"` | `"Field 'priority' must be an integer. Got: string '3'. Fix: Provide an integer value (e.g., 3, not '3')"` |
| `"required property 'title' is missing"` | `"Required field 'title' is missing. Fix: Add a 'title' field with a non-empty string value"` |

Near-match detection (Levenshtein distance) powers "did you mean?"
suggestions for enum values and field names.

#### Rule Evaluation Order

1. JSON Schema validation (Layer 1) runs first
2. If Layer 1 passes, validation rules (Layer 5) run
3. If Layer 1 fails, Layer 5 rules are skipped (entity structure is
   invalid, cross-field rules may not be meaningful)
4. Lifecycle validation (Layer 3) runs independently — it checks
   transition validity, not field constraints
5. Within Layer 5, rules evaluate in declaration order. All rules are
   evaluated (no short-circuit). All errors and warnings are collected

#### Rule Validation on Schema Save

When a schema with validation rules is saved:

1. Rule names must be unique within the collection
2. Referenced fields must exist in the entity schema (or be `*` for any)
3. `severity` must be `error`, `warning`, or `info`
4. `message` is required and non-empty
5. Condition and requirement operators must be syntactically valid
6. `gt_field` / `lt_field` referenced fields must exist in the schema
7. Regex patterns (`match`, `not_match`) must be valid regex

### Non-Functional Requirements

- **Rule evaluation latency**: < 1ms for 20 rules on a typical entity
  (50 fields). Rules must not dominate write latency
- **Error generation**: < 0.5ms to produce the enhanced error response
  including fix suggestions and near-match detection
- **Memory**: Rule definitions are cached per collection. No per-request
  allocation for rule evaluation

## User Stories

### Story US-066: Cross-Field Validation Rules [FEAT-019]

**As a** developer defining collection constraints
**I want** to declare rules like "approved items need an approver"
**So that** business logic is enforced at the data layer, not in application code

**Acceptance Criteria:**
- [ ] Rule `when: {field: status, eq: "approved"} require: {field: approver_id, not_null: true}` rejects entities with status=approved and no approver_id
- [ ] The same entity with status=draft and no approver_id is accepted (rule condition not met)
- [ ] Rule with `all` condition (multiple fields) activates only when all conditions are true
- [ ] Rule with `any` condition activates when any condition is true
- [ ] Rules with `gt_field` / `lt_field` correctly compare two fields in the same entity
- [ ] Rules without a `when` condition always apply

### Story US-067: Validation Gates [FEAT-019]

**As a** developer defining progressive validation
**I want** to group rules into named gates (save, complete, review, etc.)
**So that** entities can be saved early and validated incrementally as they mature

**Acceptance Criteria:**
- [ ] A rule with `gate: save` blocks persistence — entity is not saved if this rule fails
- [ ] A rule with `gate: complete` allows the save — entity is persisted, gate failure reported in response
- [ ] A rule with `advisory: true` allows the save — advisory reported in response, never blocks
- [ ] Write response includes gate pass/fail status for all non-save gates
- [ ] Write response includes failure details (rule name, field, message, fix) for each failing gate
- [ ] Gate results are materialized in the `entity_gates` table on every write
- [ ] Gate definitions are registered in `gate_definitions` when the schema is saved
- [ ] A gate with `includes: [complete]` inherits all `complete` gate rules — failing a `complete` rule also fails the `review` gate
- [ ] Lifecycle transition with `requires_gate: complete` is blocked if the entity fails the `complete` gate
- [ ] Blocked transition error includes the gate name, failing rules, and fix suggestions

### Story US-074b: Query by Gate Status [FEAT-019]

**As an** agent or operator
**I want** to find entities that pass or fail a specific validation gate
**So that** I can find items ready for processing or items that need attention

**Acceptance Criteria:**
- [ ] Filter `{ _gate.complete: true }` returns only entities passing the `complete` gate
- [ ] Filter `{ _gate.complete: false }` returns entities failing the `complete` gate
- [ ] Gate filter combines with field filters: `{ _gate.complete: true, status: "pending" }` returns pending entities ready for completion
- [ ] Gate filter works via GraphQL: `beads(filter: { gates: { complete: true } })`
- [ ] Gate filter works via MCP: `beads.query` with gate filter
- [ ] Gate filter uses the `idx_gates_by_status` index — query latency < 50ms on 100K entities
- [ ] Entities with no gate rows (e.g., collections without validation rules) are not returned by gate filters

### Story US-068: Actionable Error Messages [FEAT-019]

**As an** agent receiving a validation error
**I want** the error to tell me exactly what's wrong and how to fix it
**So that** I can self-correct without human intervention

**Acceptance Criteria:**
- [ ] Every validation error includes `rule`, `severity`, `field`, `message`, and `context`
- [ ] Errors from validation rules include `fix` when provided in the rule definition
- [ ] JSON Schema errors are enhanced with field path, expected type, actual value, and generated fix suggestion
- [ ] Enum mismatch errors include "did you mean?" suggestions using Levenshtein distance when the input is close to a valid value
- [ ] Type mismatch errors show both the expected and actual types
- [ ] Required-field errors name the missing field and suggest a default value if one exists in the schema
- [ ] All violations are reported, not just the first — the agent gets the complete picture in one response

### Story US-069: Validate Rules on Schema Save [FEAT-019]

**As a** developer saving a schema with validation rules
**I want** the rules themselves to be validated
**So that** invalid rules don't silently fail at entity write time

**Acceptance Criteria:**
- [ ] Duplicate rule names in the same collection are rejected
- [ ] Rules referencing non-existent fields are rejected with the invalid field name
- [ ] Invalid severity values are rejected
- [ ] Invalid regex patterns are rejected with the regex parse error
- [ ] `gt_field` referencing a non-existent field is rejected
- [ ] Rules with empty `message` are rejected

## Edge Cases

- **Rule references optional field**: If the `when` condition references
  a field that is absent, the condition evaluates to false (rule doesn't
  fire). This is distinct from the field being `null`
- **Rule on nested field**: Field paths support dot notation
  (`address.city`). If the parent object is absent, the rule doesn't fire
- **Rule on array field**: `not_null` on an array checks that the array
  exists and is non-empty. Comparison operators on arrays are not
  supported (use JSON Schema for array constraints)
- **Circular rule dependencies**: Rules don't depend on each other —
  they all evaluate against the entity data independently. No circular
  dependency is possible
- **Rule evaluation on patch**: Rules evaluate against the **merged
  result**, not the patch document. The agent sees the full entity after
  merge, including fields they didn't change
- **Performance with many rules**: 100+ rules on a single collection
  are unusual but supported. Evaluation is linear in rule count
- **Near-match suggestions**: Levenshtein distance is computed only for
  enum values with ≤ 20 options. Larger enums skip suggestions
  (performance)

## Data Model

```rust
/// A validation rule declared in the schema (Layer 5 of ESF).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Unique name within the collection.
    pub name: String,

    /// Gate this rule belongs to. "save" blocks persistence.
    /// Custom gates (e.g., "complete", "review") allow save but track readiness.
    /// None if advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,

    /// If true, rule never blocks — always reports. Mutually exclusive with gate.
    #[serde(default)]
    pub advisory: bool,

    /// Condition that activates the rule. None = always active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<RuleCondition>,

    /// Constraint to enforce when active.
    pub require: RuleRequirement,

    /// Human-readable explanation of the business rule.
    pub message: String,

    /// Actionable fix suggestion. May include {field} placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Gate definition declared in the schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateDef {
    /// Human-readable description of what this gate means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Other gates whose rules are included in this gate.
    /// e.g., "review" includes ["complete"] means all complete rules
    /// must also pass for review to pass.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RuleCondition {
    FieldCheck {
        field: String,
        #[serde(flatten)]
        op: ConditionOp,
    },
    All { all: Vec<RuleCondition> },
    Any { any: Vec<RuleCondition> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq(Value),
    Ne(Value),
    In(Vec<Value>),
    NotNull(bool),
    IsNull(bool),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    Match(String),
}
```

`CollectionSchema` gains:
```rust
pub struct CollectionSchema {
    pub collection: CollectionId,
    pub description: Option<String>,
    pub version: u32,
    pub entity_schema: Option<Value>,                       // Layer 1
    pub link_types: HashMap<String, LinkTypeDef>,            // Layer 2
    pub lifecycles: HashMap<String, LifecycleDef>,           // Layer 3
    pub indexes: Vec<IndexDef>,                              // Layer 4
    pub gates: HashMap<String, GateDef>,                     // Layer 5 gate definitions
    pub validation_rules: Vec<ValidationRule>,               // Layer 5 rules
}
```

## Dependencies

- **FEAT-002** (Schema Engine): Rules extend the schema validation pipeline
- **FEAT-004** (Entity Operations): Rules execute on every write
- **ADR-002** (Schema Format): Rules are Layer 5 of ESF
- **ADR-008** (Lifecycles): Lifecycle transitions and rules are complementary — lifecycles validate state transitions, rules validate field constraints conditional on state. `requires_gate` field on transitions connects L3 and L5
- **ADR-010** Section 11 (Gate Tables): Physical schema for `gate_definitions` and `entity_gates` tables with indexes

## Out of Scope

- **Cross-entity validation**: Rules that reference other entities
  (e.g., "assignee must exist in users collection"). Deferred — requires
  cross-collection lookups on every write
- **Computed fields**: Rules that derive a field value from other fields.
  Deferred
- **Async validation**: Rules that call external services. Deferred
- **Rule inheritance**: Rules inherited from a parent schema. Deferred

## Traceability

### Related Artifacts
- **Parent PRD Section**: Requirements Overview > P0 #4 (Schema engine — validation)
- **Technical Requirements**: Section 5 (Schema System — validation rules, severity levels)
- **User Stories**: US-066, US-067, US-068, US-069, US-074b
- **Implementation**: `crates/axon-schema/` (rule evaluation),
  `crates/axon-api/` (error enhancement)

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-004, FEAT-013 (gate index queries)
- **Depended By**: FEAT-015 (GraphQL surfaces validation errors),
  FEAT-016 (MCP tools surface validation errors)
