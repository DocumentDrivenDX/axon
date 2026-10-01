---
ddx:
  id: US-147
---
# US-147: Author Links, Lifecycles, and Rules Visually

**Feature**: FEAT-011 — Admin Web UI
**Feature Requirements**: UI-33, UI-34, UI-35
**PRD Requirements**: FR-24
**Priority**: P1
**Status**: Draft

## Story

**As a** Wei, a business workflow builder modeling entities with relationships and lifecycle states
**I want** structured, form-based editors for link types, lifecycles, and validation rules/gates in the schema workspace
**So that** I can declare relationships, states, and business rules without hand-authoring the ESF YAML grammar

## Context

Extends the FEAT-011 schema workspace (US-121, UI-16 through UI-18), which
today gives collections a structured view for the entity body but only a raw
JSON/YAML view for `link_types`, `lifecycles`, and `validation_rules`/`gates`.
Exercises UI-33 (link-type editor), UI-34 (lifecycle editor), and UI-35
(validation-rule/gate editor). This is a form-based extension of the existing
ESF surface (CONTRACT-010), not a new modeling language.

## Walkthrough

1. Wei opens a collection's schema workspace and switches to the Links tab.
2. Wei creates a new link type by choosing a target collection, cardinality,
   and required/optional flag from form controls, then adds metadata fields
   through the same structured form.
3. Wei switches to the Lifecycle tab, defines a lifecycle field's states, and
   draws or lists the valid transitions between them, marking one transition
   as gated.
4. Wei switches to the Rules tab and adds a validation rule by picking a
   field, a condition operator, a requirement operator, and a message from
   structured inputs, then assigns it to a gate.
5. Wei saves; the system shows a preview of the resulting schema change and
   inline validation errors before the change is applied, exactly as it does
   for entity-body edits (UI-18).

## Acceptance Criteria

- [ ] **US-147-AC1** — Given a collection's schema workspace, when Wei opens
  the Links tab, then existing `link_types` are listed and Wei can create,
  edit, and remove one through form controls for target collection,
  cardinality, required/optional, and metadata schema, with no raw JSON/YAML
  editing required.
- [ ] **US-147-AC2** — Given a collection's schema workspace, when Wei opens
  the Lifecycle tab for a lifecycle field, then Wei can define states and
  valid transitions, including marking a transition as requiring a named
  gate, through a structured form or diagram view.
- [ ] **US-147-AC3** — Given a collection's schema workspace, when Wei opens
  the Rules tab, then Wei can create, edit, and remove `validation_rules`
  entries (condition, requirement, message, fix) and `gates` declarations
  (including `includes` composition) through form-based inputs.
- [ ] **US-147-AC4** — Given a structured edit to link types, lifecycles, or
  rules, when Wei saves, then a preview of the change is required before
  the save is applied, with the same inline validation-error display as
  entity-schema edits (UI-18).
- [ ] **US-147-AC5** — Given a structured edit that produces an invalid
  declaration (for example, a transition target that is not a declared
  state, or a rule referencing a nonexistent field), then the structured
  editor surfaces the error inline and the active schema remains unchanged.

## Edge Cases

- **Existing raw-authored schema**: a collection whose `link_types`,
  `lifecycles`, or `validation_rules` were authored as raw JSON/YAML opens
  correctly in the structured editors; the structured and raw views stay in
  sync in both directions.
- **Invalid transition target**: declaring a transition to a state not
  present in the lifecycle's state list is rejected inline, matching
  CONTRACT-010's schema-save validation for Layer 3.
- **Gate cycle**: a `gates.includes` declaration that would form a cycle is
  rejected inline, matching CONTRACT-010's schema-save validation for Layer 5.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create link type | US-147-AC1 | Collection with no link types | Create a link type via form | Link type appears in structured and raw views |
| Define lifecycle | US-147-AC2 | Collection with a status field | Add states and transitions, mark one gated | Lifecycle declaration saved with gated transition |
| Add validation rule | US-147-AC3 | Collection with a schema | Add a rule via form, assign to a gate | Rule appears under the gate in structured and raw views |
| Preview before save | US-147-AC4 | Any structured edit | Save | Preview shown first; apply after confirm |
| Invalid transition | US-147-AC5 | Lifecycle editor open | Add a transition to an undeclared state | Inline error; no change applied |

## Dependencies

- **Stories**: US-121 (schema workspace structured/raw views this extends).
- **Feature Spec**: [FEAT-011 — Admin Web UI](../features/FEAT-011-admin-web-ui.md)
- **Feature Requirements**: UI-33, UI-34, UI-35
- **PRD Requirements**: FR-24
- **External**: CONTRACT-010 (ESF `link_types`, `lifecycles`, `validation_rules`, `gates` grammar); CONTRACT-002 (GraphQL surface)

## Out of Scope

- A drag-and-drop or novel visual modeling canvas — these are structured
  form-based editors over the existing ESF grammar, not a new notation.
- Access-control policy editing beside the schema (FEAT-031).
- Schema-evolution compatibility classification UX (FEAT-017).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
