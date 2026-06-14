---
ddx:
  id: US-069
  review:
    self_hash: 8232ac8959882863535c25443905fb518c62cd221671edb9f7a19d54a72a0684
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-069: Validate Rules on Schema Save

**Feature**: FEAT-019 — Validation Rules and Actionable Errors
**Feature Requirements**: VAL-18
**PRD Requirements**: FR-1
**Priority**: P1
**Status**: Draft

## Story

**As a** business workflow builder (Wei) saving a schema with validation rules
**I want** the rules themselves to be validated at save time
**So that** malformed rules fail loudly at authoring time instead of silently
misbehaving at entity write time

## Context

A typo'd field name, an invalid regex, or a rule that declares both `gate`
and `advisory: true` should never reach the write path. This story exercises
schema-save validation of Layer 5 rules (VAL-18). The save-time validation
rules — including the requirement that each rule declares exactly one of
`gate` or `advisory: true` — are normative in CONTRACT-010.

## Walkthrough

1. Wei saves a schema containing a rule that references a field not present
   in the entity schema.
2. Axon rejects the schema save with a structured error naming the offending
   rule and the invalid field.
3. Wei corrects the field name and saves again; the schema is accepted and
   its gates are registered.

## Acceptance Criteria

- [ ] **US-069-AC1** — Given two rules with the same name in one collection,
  when the schema is saved, then the save is rejected naming the duplicate.
- [ ] **US-069-AC2** — Given a rule referencing a field that does not exist
  in the entity schema, when the schema is saved, then the save is rejected
  with the invalid field name.
- [ ] **US-069-AC3** — Given a rule that declares both `gate` and
  `advisory: true`, or neither, when the schema is saved, then the save is
  rejected for invalid gate/advisory structure.
- [ ] **US-069-AC4** — Given a rule with an invalid regex pattern, when the
  schema is saved, then the save is rejected with the regex parse error.
- [ ] **US-069-AC5** — Given a rule whose cross-field comparison
  (`gt_field` or relatives) references a non-existent field, when the schema
  is saved, then the save is rejected.
- [ ] **US-069-AC6** — Given a rule with an empty `message`, when the schema
  is saved, then the save is rejected.
- [ ] **US-069-AC7** — Given gate declarations whose `includes` reference an
  undeclared gate or form a cycle, when the schema is saved, then the save is
  rejected.

## Edge Cases

- **Multiple defects in one schema**: all rule-definition errors are
  reported together so the author fixes them in one pass.
- **Previously valid schema unaffected**: a rejected schema save leaves the
  active schema version and existing gate registrations unchanged.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Duplicate names | US-069-AC1 | Two rules named `needs-approver` | Save schema | Rejected; duplicate named |
| Unknown field | US-069-AC2 | Rule requiring `aprover_id` (typo) | Save schema | Rejected; invalid field `aprover_id` reported |
| Both gate and advisory | US-069-AC3 | Rule with `gate: save` and `advisory: true` | Save schema | Rejected; gate/advisory mutually exclusive |
| Neither gate nor advisory | US-069-AC3 | Rule with no `gate` and no `advisory` | Save schema | Rejected; one of gate/advisory required |
| Bad regex | US-069-AC4 | Rule `match: "([unclosed"` | Save schema | Rejected with regex parse error |
| Include cycle | US-069-AC7 | `review includes [complete]`, `complete includes [review]` | Save schema | Rejected; cycle reported |

## Dependencies

- **Stories**: US-066 (rule grammar being validated).
- **Feature Spec**: FEAT-019
- **Feature Requirements**: VAL-18
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010 (schema-save validation for Layer 5)

## Out of Scope

- Validation of entity data against the rules (US-066, US-067).
- Schema evolution classification of rule changes (FEAT-017).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
