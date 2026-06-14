---
ddx:
  id: US-068
  review:
    self_hash: 1bfeb0406cb6ffa63b3791d7705924903147ef52e9d2b5680e9f75634448d695
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-068: Actionable Error Messages

**Feature**: FEAT-019 — Validation Rules and Actionable Errors
**Feature Requirements**: VAL-16, VAL-17
**PRD Requirements**: FR-1
**Priority**: P1
**Status**: Draft

## Story

**As an** agent application developer (Ava) whose agent receives a validation
error
**I want** the error to tell the agent exactly what is wrong and how to fix it
**So that** the agent can self-correct without human intervention

## Context

Generic validation errors ("instance failed to match pattern") force a human
into the loop. Agents need structured, complete errors that explain the
business rule, name the field, and propose a concrete fix. This story
exercises the actionable error envelope (VAL-16) and JSON Schema error
enhancement with near-match hints (VAL-17). The `VALIDATION_FAILED` envelope
shape is normative in CONTRACT-010; rules carry gate/advisory structure —
there is no severity field.

## Walkthrough

1. Ava's agent submits a write that violates one validation rule and two
   JSON Schema constraints.
2. Axon rejects the write with a single structured response reporting all
   three violations together.
3. Each violation names the rule (or translated schema constraint), the
   field, the human-readable message, the concrete fix, and the context that
   activated it; blocking failures and advisories are reported separately.
4. The agent repairs the payload from the response alone and resubmits
   successfully.

## Acceptance Criteria

- [ ] **US-068-AC1** — Given any validation failure, when the error is
  returned, then each violation identifies the rule, its gate (or advisory
  classification), the field, the message, and the activating condition
  context — blocking failures and advisories are structurally distinct, with
  no per-item severity field.
- [ ] **US-068-AC2** — Given a rule that declares a `fix`, when it fails,
  then the error includes the fix with current field values substituted into
  any placeholders.
- [ ] **US-068-AC3** — Given a JSON Schema (L1) violation, when the error is
  returned, then it is translated into the same actionable shape with field
  path, expected type/constraint, actual value, and a generated fix
  suggestion.
- [ ] **US-068-AC4** — Given an enum mismatch close to a valid value, when
  the error is returned, then it includes a "did you mean?" suggestion.
- [ ] **US-068-AC5** — Given a type mismatch, when the error is returned,
  then it shows both the expected and the actual type.
- [ ] **US-068-AC6** — Given a missing required field, when the error is
  returned, then it names the missing field and suggests a default value if
  the schema declares one.
- [ ] **US-068-AC7** — Given multiple simultaneous violations, when the error
  is returned, then all violations are reported in one response, not just the
  first.

## Edge Cases

- **Large enums**: near-match suggestions are computed only for enums with
  ≤ 20 options; larger enums omit suggestions rather than degrading latency.
- **Rule without a fix**: the violation is still complete (rule, field,
  message, context); only the fix is absent.
- **L1 failure suppresses L5**: when JSON Schema validation fails, Layer 5
  rules are skipped, and the response contains only the translated L1
  violations (per CONTRACT-010 evaluation order).

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Rule failure with fix | US-068-AC2 | Rule `approved-needs-approver` with fix text | Write `{status: "approved"}` without approver | Violation includes rule name, field `approver_id`, message, fix, and trigger context |
| Enum near-match | US-068-AC4 | Schema enum `[draft, pending, ready]` | Write `{status: "pendng"}` | Error suggests `pending` |
| Type mismatch | US-068-AC5 | `priority` is integer | Write `{priority: "3"}` | Error shows expected integer, got string `"3"`, with fix |
| Complete reporting | US-068-AC7 | Entity violating one rule and two schema constraints | Single write | One response listing all three violations |

## Dependencies

- **Stories**: US-066 (rules that produce the failures).
- **Feature Spec**: FEAT-019
- **Feature Requirements**: VAL-16, VAL-17
- **PRD Requirements**: FR-1
- **External**: CONTRACT-010 (`VALIDATION_FAILED` envelope)

## Out of Scope

- Gate pass/fail reporting on successful saves (US-067).
- Error presentation in specific clients (UI rendering, CLI formatting).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
