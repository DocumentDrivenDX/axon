---
dun:
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
conditional requirements, severity levels, and actionable error messages
that tell agents exactly what's wrong and how to fix it.

This is Axon's key differentiator over general-purpose databases. A SQL
constraint violation says "CHECK constraint failed." An Axon validation
error says "Field 'approver_id' is required when status is 'approved'.
Current status: 'approved'. Set approver_id to a valid user ID."

## Problem Statement

JSON Schema validates structure but not semantics. Real-world entities
have cross-field dependencies: approved invoices need an approver, bugs
need a priority, completed tasks should have a resolution. These rules
can't be expressed in JSON Schema alone.

Current validation errors from JSON Schema libraries are technical and
generic ("instance failed to match pattern"). Agents need errors that
explain the business rule, identify the fix, and distinguish blocking
errors from advisory warnings.

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
  # Cross-field: approved items need an approver
  - name: approved-needs-approver
    when:
      field: status
      eq: "approved"
    require:
      field: approver_id
      not_null: true
    severity: error
    message: "Approved items must have an approver_id set"
    fix: "Set approver_id to the user who approved this item"

  # Conditional requirement: bugs need priority
  - name: bugs-need-priority
    when:
      field: bead_type
      eq: "bug"
    require:
      field: priority
      not_null: true
    severity: error
    message: "Bugs must have a priority (0-4)"
    fix: "Set priority to an integer between 0 and 4"

  # Warning: epics should have descriptions
  - name: epics-should-have-description
    when:
      field: bead_type
      eq: "epic"
    require:
      field: description
      not_null: true
    severity: warning
    message: "Epics should have a description for planning purposes"
    fix: "Add a description explaining the epic's scope and goals"

  # Info: resolution recommended for done items
  - name: done-wants-resolution
    when:
      field: status
      eq: "done"
    require:
      field: resolution
      not_null: true
    severity: info
    message: "Consider adding a resolution note for completed items"
    fix: "Set resolution to a summary of what was done"

  # Cross-field comparison: due_date must be after created_date
  - name: due-after-created
    when:
      field: due_date
      not_null: true
    require:
      field: due_date
      gt_field: created_date
    severity: error
    message: "Due date must be after creation date"
    fix: "Set due_date to a date after {created_date}"

  # Multi-condition: high-priority bugs need assignee
  - name: high-priority-bugs-need-assignee
    when:
      all:
        - { field: bead_type, eq: "bug" }
        - { field: priority, lte: 1 }
    require:
      field: assignee
      not_null: true
    severity: error
    message: "High-priority bugs (P0/P1) must have an assignee"
    fix: "Set assignee to the person responsible for this bug"

  # Unconditional: always validate
  - name: title-not-placeholder
    require:
      field: title
      not_match: "^(TODO|FIXME|untitled|test)$"
    severity: warning
    message: "Title appears to be a placeholder"
    fix: "Replace title with a descriptive name"
```

#### Rule Structure

Each validation rule has:

| Field | Required | Description |
|---|---|---|
| `name` | Yes | Unique identifier for the rule within the collection |
| `when` | No | Condition that activates the rule. If omitted, rule always applies |
| `require` | Yes | The constraint to enforce when the rule is active |
| `severity` | Yes | `error` (reject write), `warning` (accept, flag in response), `info` (accept, log only) |
| `message` | Yes | Human-readable explanation of the business rule |
| `fix` | No | Actionable suggestion for how to resolve the violation. May include `{field}` placeholders substituted with current values |

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

#### Severity Levels

| Severity | Write behavior | Response behavior |
|---|---|---|
| `error` | **Reject** the write | Error in response, entity not persisted |
| `warning` | **Accept** the write | Warning in response, entity persisted. Warning recorded in audit entry |
| `info` | **Accept** the write | Info in audit entry only. Not in response unless explicitly requested |

Warnings accumulate — a write with 3 warnings and 0 errors succeeds,
and the response includes all 3 warnings. The agent can choose to fix
them or ignore them.

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

### Story US-067: Severity Levels [FEAT-019]

**As a** developer defining validation rules
**I want** to distinguish errors, warnings, and info
**So that** some rules block writes while others advise

**Acceptance Criteria:**
- [ ] A rule with `severity: error` rejects the write — entity is not persisted
- [ ] A rule with `severity: warning` accepts the write — entity is persisted, warning included in response
- [ ] A rule with `severity: info` accepts the write — info recorded in audit entry only
- [ ] A write with 0 errors and 3 warnings succeeds, response includes all 3 warnings
- [ ] A write with 1 error and 2 warnings fails, response includes the error and both warnings
- [ ] Warnings are recorded in the audit entry for the mutation

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

    /// Condition that activates the rule. None = always active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<RuleCondition>,

    /// Constraint to enforce when active.
    pub require: RuleRequirement,

    /// Error, warning, or info.
    pub severity: RuleSeverity,

    /// Human-readable explanation of the business rule.
    pub message: String,

    /// Actionable fix suggestion. May include {field} placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Error,
    Warning,
    Info,
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
    pub validation_rules: Vec<ValidationRule>,               // Layer 5 (NEW)
}
```

## Dependencies

- **FEAT-002** (Schema Engine): Rules extend the schema validation pipeline
- **FEAT-004** (Entity Operations): Rules execute on every write
- **ADR-002** (Schema Format): Rules are Layer 5 of ESF
- **ADR-008** (Lifecycles): Lifecycle transitions and rules are complementary — lifecycles validate state transitions, rules validate field constraints conditional on state

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
- **User Stories**: US-066, US-067, US-068, US-069
- **Implementation**: `crates/axon-schema/` (rule evaluation),
  `crates/axon-api/` (error enhancement)

### Feature Dependencies
- **Depends On**: FEAT-002, FEAT-004
- **Depended By**: FEAT-015 (GraphQL surfaces validation errors),
  FEAT-016 (MCP tools surface validation errors)
