---
ddx:
  id: US-066
  review:
    self_hash: 8af46031cdc4d8b0d0cfcac55058f8cfd63bd06d6ba668c894f2799014ded184
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-066: Cross-Field Validation Rules

**Feature**: FEAT-019 — Validation Rules and Actionable Errors
**Feature Requirements**: VAL-01, VAL-02, VAL-03
**PRD Requirements**: FR-1
**Priority**: P1
**Status**: Draft

## Story

**As a** business workflow builder (Wei) defining collection constraints
**I want** to declare rules like "approved items need an approver"
**So that** business logic is enforced at the data layer, not reimplemented in
every application

## Context

JSON Schema can require a field unconditionally, but real business rules are
conditional and cross-field: an approver is required only when status is
approved; a due date must come after the creation date. This story exercises
FEAT-019's Layer 5 rule declaration (VAL-01), the condition/requirement
operator set (VAL-02), and unconditional-rule semantics (VAL-03). The rule
grammar is normative in CONTRACT-010.

## Walkthrough

1. Wei adds a validation rule to the collection schema: when `status` equals
   `approved`, require `approver_id` to be non-null, with a message and fix.
2. Wei saves the schema; Axon accepts the well-formed rule.
3. An application writes an entity with `status: approved` and no
   `approver_id`; Axon rejects the write and reports the failing rule with
   its message and fix.
4. The application writes the same entity with `status: draft`; Axon accepts
   it because the rule's condition is not met.

## Acceptance Criteria

- [ ] **US-066-AC1** — Given a rule conditioned on `status = approved` that
  requires `approver_id` to be non-null, when an entity with
  `status: approved` and no `approver_id` is written, then the write is
  rejected and the failure names that rule.
- [ ] **US-066-AC2** — Given the same rule, when an entity with
  `status: draft` and no `approver_id` is written, then the write is accepted
  because the rule's condition is not met.
- [ ] **US-066-AC3** — Given a rule with an `all` composite condition over
  multiple fields, when only some sub-conditions are true, then the rule does
  not fire; when all are true, it fires.
- [ ] **US-066-AC4** — Given a rule with an `any` composite condition, when
  at least one sub-condition is true, then the rule fires.
- [ ] **US-066-AC5** — Given a rule using a cross-field comparison
  (`gt_field`/`lt_field`), when the two referenced fields violate the
  comparison, then the rule fails; when they satisfy it, the rule passes.
- [ ] **US-066-AC6** — Given a rule with no `when` condition, when any entity
  is written, then the rule's requirement is always evaluated.

## Edge Cases

- **Condition references an absent field**: the condition evaluates to false
  and the rule does not fire — distinct from the field being `null`.
- **Cross-field comparison with one side missing**: the requirement cannot be
  satisfied trivially; behavior follows CONTRACT-010 operator semantics.
- **Rule evaluated on patch**: the rule sees the merged entity, not the patch
  document.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Conditional rule fires | US-066-AC1 | Rule `when status=approved require approver_id not_null` | Write `{status: "approved"}` without `approver_id` | Write rejected; failure names the rule |
| Condition not met | US-066-AC2 | Same rule | Write `{status: "draft"}` without `approver_id` | Write accepted |
| `all` composite | US-066-AC3 | Rule `when all[bead_type=bug, priority<=1] require assignee not_null` | Write `{bead_type: "bug", priority: 3}` without assignee | Rule does not fire; write accepted |
| Cross-field comparison | US-066-AC5 | Rule `require due_date gt_field created_date` | Write `{created_date: 2026-06-10, due_date: 2026-06-01}` | Rule fails with message and fix |
| Unconditional rule | US-066-AC6 | Rule with no `when`, `require bead_type in [task, bug]` | Write `{bead_type: "banana"}` | Rule fails on every write with that value |

## Dependencies

- **Stories**: None.
- **Feature Spec**: FEAT-019
- **Feature Requirements**: VAL-01, VAL-02, VAL-03
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010 (rule structure and operator grammar)

## Out of Scope

- Gate semantics and save-blocking behavior (US-067).
- Error message formatting quality and near-match hints (US-068).
- Validation of the rule definitions themselves (US-069).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
