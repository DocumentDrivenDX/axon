---
dun:
  id: ADR-008
  depends_on:
    - ADR-002
    - ADR-007
    - FEAT-002
    - FEAT-006
---
# ADR-008: Lifecycle State Machines as Schema Declarations

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-002, FEAT-006, ADR-002, ADR-007 | High |

## Context

The bead collection has a lifecycle state machine (draft → pending → ready →
in_progress → review → done, with blocked/cancelled). This is currently
enforced in Rust application code (`bead.rs: BeadStatus::valid_transitions()`).

This is wrong. The state machine defines the structure and constraints of the
collection — it's the same kind of declaration as field types and link types.
It belongs in the schema, not in application code.

| Aspect | Description |
|--------|-------------|
| Problem | Lifecycle transitions are hardcoded in Rust, not declared in schema |
| Current State | `BeadStatus::valid_transitions()` returns allowed transitions per state |
| Requirements | Any collection should be able to declare lifecycle state machines; enforcement should be automatic on update |

## Decision

Add **lifecycle declarations** as Layer 3 of the Entity Schema Format (ESF),
alongside Layer 1 (JSON Schema for field validation) and Layer 2 (link types).

### Data Model

A collection schema can declare zero or more lifecycle state machines. Each
lifecycle governs a specific field in the entity data.

```rust
/// A lifecycle state machine declared in the schema (Layer 3 of ESF).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleDef {
    /// The entity data field this lifecycle governs (e.g., "status").
    pub field: String,

    /// The initial state assigned when the field is not provided on create.
    /// Must be a key in `transitions`.
    pub initial: String,

    /// Map from state → list of valid target states.
    /// A state with an empty list is terminal (no outgoing transitions).
    pub transitions: HashMap<String, Vec<String>>,
}
```

### ESF Representation

```yaml
esf_version: "1.0"
collection: beads

entity_schema:
  type: object
  required: [bead_type, status, title]
  properties:
    bead_type:
      type: string
    status:
      type: string
    title:
      type: string
      minLength: 1
    # ... other fields ...

link_types:
  depends-on:
    target_collection: beads
    cardinality: many-to-many
  parent-of:
    target_collection: beads
    cardinality: one-to-many

lifecycles:
  status:
    field: status
    initial: draft
    transitions:
      draft: [pending, cancelled]
      pending: [ready, in_progress, blocked, cancelled]
      ready: [in_progress, blocked, cancelled]
      in_progress: [review, done, blocked, cancelled]
      review: [in_progress, done, cancelled]
      done: []
      blocked: [pending, cancelled]
      cancelled: []
```

### CollectionSchema Changes

```rust
pub struct CollectionSchema {
    pub collection: CollectionId,
    pub description: Option<String>,
    pub version: u32,
    pub entity_schema: Option<Value>,                       // Layer 1
    pub link_types: HashMap<String, LinkTypeDef>,            // Layer 2
    pub lifecycles: HashMap<String, LifecycleDef>,           // Layer 3 (NEW)
}
```

The `lifecycles` map is keyed by a logical name (usually the same as the
field name, but doesn't have to be — you could have multiple lifecycles
if different fields have independent state machines).

### Enforcement

**On create**:
- If the entity data does not include the lifecycle field, set it to
  `initial`
- If the entity data includes the lifecycle field, verify it's a valid
  state (a key in `transitions`)

**On update**:
- Read the current value of the lifecycle field from the stored entity
- Read the new value from the update payload
- If the value changed, verify the transition is allowed
  (`transitions[current].contains(new)`)
- If the transition is not in the allowed list, reject with
  `AxonError::InvalidOperation` including the current state, attempted
  state, and list of valid transitions

**On delete**:
- No lifecycle check — deletion is always allowed regardless of state
  (the entity ceases to exist, it doesn't transition)

### Validation on Schema Save

When a schema with lifecycles is saved:

1. Every state mentioned in a transition target must also be a key in
   `transitions` (no transitions to undefined states)
2. `initial` must be a key in `transitions`
3. If the entity_schema has an `enum` on the lifecycle field, the enum
   values must be a superset of the lifecycle states (consistency check)
4. The transition graph need not be connected (terminal states with no
   outgoing transitions are expected)

### Migration from Hardcoded Bead Lifecycle

The `BeadStatus` enum and `valid_transitions()` in `bead.rs` are replaced
by the lifecycle declaration in the bead schema. The `transition_bead()`
function becomes a thin wrapper around the generic `update_entity()` — the
handler's lifecycle enforcement does the work.

This means any collection can have lifecycle state machines, not just beads.
An invoices collection could declare `status: draft → submitted → approved →
paid → reconciled`. A contracts collection could declare `stage: negotiation
→ review → signed → active → expired`.

### Future Extensions (Not in V1)

- **Transition guards**: Conditions that must be true for a transition
  (e.g., "can only transition to `approved` if `approved_by` is set").
  These would be cross-field validation rules, potentially expressed as
  simple predicates in the lifecycle declaration
- **Transition hooks**: Side effects on transition (e.g., "on transition
  to `done`, create an audit summary"). Deferred to the plugin system
- **Lifecycle visualization**: The admin UI could render the state machine
  as a directed graph

## Consequences

**Positive**:
- Any collection can declare lifecycle state machines — not just beads
- State machines are visible, inspectable, and versioned (via schema
  versioning, ADR-007)
- The bead module shrinks significantly — `BeadStatus` enum and
  `valid_transitions()` are replaced by schema data
- Schema is the single source of truth for "what are the rules of this
  collection"

**Negative**:
- Adds a new field to `CollectionSchema` (breaking change for existing
  serialized schemas — needs migration or default handling)
- Lifecycle enforcement adds a read-before-write on every update (need
  current state to check transition validity) — but we already do this
  for OCC version checks
- Transition graph validation on schema save adds complexity

**Key principle**: The schema defines the rules. The engine enforces them.
Application code should never hardcode business rules that belong in the
schema.
