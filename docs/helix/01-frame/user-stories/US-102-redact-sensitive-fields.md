---
ddx:
  id: US-102
  review:
    self_hash: a0bb950c5ac45b546ee38c321ad028f30553914db51dbccdf35bda0bd2ea5287
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-102: Redact Sensitive Fields

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-12
**PRD Requirements**: FR-10, FR-12, FR-13
**Priority**: P0
**Status**: Approved

## Story

**As a** contractor (end user of an application built by Ava, Agent
Application Developer persona)
**I want** visible engagement rows to omit budget and rate-card data
**So that** I can work with assignment context without seeing commercial terms

## Context

Field redaction is the second policy layer after row visibility: a subject
may see a row but not all its fields. Redaction must be uniform across
GraphQL, generic JSON, REST compatibility, and audit reads, with redacted
GraphQL fields generated as nullable.

## Walkthrough

1. Contractor queries engagements they are assigned to.
2. System returns the visible rows with commercial fields redacted to null.
3. Contractor inspects an engagement's audit history.
4. System applies the same redaction to before/after payloads.

## Acceptance Criteria

- [ ] **US-102-AC1** — Given a redactable field, when the GraphQL type is
  generated, then the field is nullable even if the JSON Schema marks it
  required.
- [ ] **US-102-AC2** — Given a matching read-deny field rule, when the
  subject reads the row, then the redacted field returns null.
- [ ] **US-102-AC3** — Given the same subject and row, when generic JSON,
  REST compatibility, and audit read payloads are returned, then they apply
  the same redaction.
- [ ] **US-102-AC4** — Given a JSON Schema required field with a read-deny
  rule, when the subject reads the row, then the field is still redacted on
  read.

## Edge Cases

- **Mixed visibility in one list**: rows redact per-subject rules
  independently of other rows in the same result.
- **Diff payloads**: audit diffs never reveal redacted values through
  before/after comparison output.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Nullable generation | US-102-AC1 | `budget_cents` redactable | Introspect GraphQL type | Field nullable |
| Redacted read | US-102-AC2 | Contractor reads own engagement | Query row | `budget_cents` is null |
| Cross-surface parity | US-102-AC3 | Same subject/row | Read via JSON, REST compat, audit | Identical redaction |
| Required-field redaction | US-102-AC4 | Required field with deny rule | Read | Field redacted |

## Dependencies

- **Stories**: US-101 (row visibility evaluated first)
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-12
- **PRD Requirements**: FR-10, FR-12, FR-13
- **External**: CONTRACT-004 (field rules, redaction), CONTRACT-002 (GraphQL
  nullability), CONTRACT-005 (audit record reads)

## Out of Scope

- Write denial for the same fields (US-103, US-047).
- Partial masking formats (only null redaction is specified).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
