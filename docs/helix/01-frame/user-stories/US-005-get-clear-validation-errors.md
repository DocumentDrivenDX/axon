---
ddx:
  id: US-005
  review:
    self_hash: 221b5cda1147abc47d0962334bc709ef3804e0d61c0b03ba54ada4964068f74d
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-005: Get Clear Validation Errors

**Feature**: FEAT-002 — Schema Engine
**Feature Requirements**: SCH-07, SCH-08, SCH-09, SCH-10
**PRD Requirements**: FR-1
**Priority**: P0
**Status**: Draft

## Story

**As an** agent writing data to a collection
**I want** structured, actionable error messages when my writes are invalid
**So that** I can self-correct without human intervention

## Context

Agents cannot self-correct from opaque rejection strings; they need machine-parseable errors that pinpoint each violation. This story exercises SCH-07 (validation on every write with no bypass), SCH-08 (field path, violated constraint with expected value, actual value, human-readable message), SCH-09 (all violations reported), and SCH-10 (no silent type coercion). The validation error envelope is defined in CONTRACT-010.

## Walkthrough

1. Agent submits an entity write that violates the collection schema in more than one place.
2. System validates the entity against the active schema and rejects the write before anything persists.
3. System returns a structured, machine-parseable error (envelope per CONTRACT-010) listing every violation, each with field path, violated constraint and expected value, actual value, and a human-readable message.
4. Agent parses the error, corrects each named field, and resubmits successfully.

## Acceptance Criteria

- [ ] **US-005-AC1** — Given an entity write violating the schema, when the write is rejected, then each reported violation includes the field path, the violated constraint with its expected value (type, enum members, bound, or pattern), the actual offending value, and a human-readable message.
- [ ] **US-005-AC2** — Given an entity with multiple violations, when the write is rejected, then all violations are reported in a single response, not just the first.
- [ ] **US-005-AC3** — Given a rejected write, when the agent inspects the response, then the error is structured and machine-parseable per the CONTRACT-010 error envelope, not a bare string.
- [ ] **US-005-AC4** — Given a violation where the constraint admits a suggestion (e.g. a near-miss enum value), when the write is rejected, then the message includes the suggested correction.
- [ ] **US-005-AC5** — Given a string value supplied for an integer field, when the write is submitted, then it is rejected as a type violation rather than silently coerced.

## Edge Cases

- **Empty entity against required fields**: Writing `{}` reports every missing required field, one violation each.
- **Violation deep in a nested object**: The field path identifies the full nested location of the failure.
- **Undeclared field outside a flexible zone**: Rejected with a violation naming the unexpected field.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Single violation detail | US-005-AC1 | Schema: `amount` integer; write `amount: "abc"` | Submit write | Rejected; violation has path `amount`, expected integer, actual `"abc"`, readable message |
| All violations reported | US-005-AC2 | Write missing required `id` and with bad `status` | Submit write | Both violations in one response |
| Machine-parseable | US-005-AC3 | Any invalid write | Submit write | Structured error envelope per CONTRACT-010 |
| Suggestion | US-005-AC4 | Enum `[pending, active, done]`; write `status: "pendng"` | Submit write | Message lists allowed values and suggests `pending` |
| No coercion | US-005-AC5 | Integer field `amount`; write `amount: "123"` | Submit write | Rejected as type violation; nothing persisted |

## Dependencies

- **Stories**: US-004 (a schema must be defined before validation errors can be produced)
- **Feature Spec**: FEAT-002
- **Feature Requirements**: SCH-07, SCH-08, SCH-09, SCH-10
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010

## Out of Scope

- Cross-field validation rules and gates (FEAT-019).
- Errors for non-conformance discovered after a schema change (FEAT-017 revalidation).
- Policy-based rejections (FEAT-029) — this story covers schema validation only.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
