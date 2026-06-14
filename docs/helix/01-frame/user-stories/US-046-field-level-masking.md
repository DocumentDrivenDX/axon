---
ddx:
  id: US-046
  review:
    self_hash: 29d233d608c4adb261fa1fccf6e5cf1fb7c2dfceae9d7283b0b49f8cd13a8d41
    deps: {}
    reviewed_at: "2026-06-14T04:39:42Z"
---

# US-046: Field-Level Masking

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-01, ACL-12
**PRD Requirements**: FR-10, FR-12
**Priority**: P0
**Status**: Approved

## Story

**As a** data steward (Wei, Business Workflow Builder persona)
**I want** sensitive fields hidden from unauthorized users
**So that** PII and confidential data is only visible to those who need it

## Context

Originally defined under FEAT-012; moved to FEAT-029 ownership because
schema-declared field-level policies are governed by FEAT-029 (grammar per
CONTRACT-004). A field-level read-deny policy redacts the field on every read
surface for subjects that match the rule, while authorized subjects see the
full entity.

## Walkthrough

1. Data steward declares a field-level read-deny rule on a sensitive field
   (for example, salary in an employees collection) for low-privilege
   subjects.
2. System compiles and activates the policy with the schema version.
3. A low-privilege user reads an employee entity; the sensitive field is
   redacted.
4. An authorized subject reads the same entity and sees the full data.

## Acceptance Criteria

- [ ] **US-046-AC1** — Given a field-level read-deny policy on a sensitive
  field, when a matching low-privilege subject reads the entity, then the
  field is redacted in the response.
- [ ] **US-046-AC2** — Given the same entity, when a subject the policy
  allows reads it, then the full entity including the sensitive field is
  returned.
- [ ] **US-046-AC3** — Given a redacted field, when any read surface returns
  it, then the redaction shape follows CONTRACT-004 (null redaction; the
  generated GraphQL field is nullable), never the original value.
- [ ] **US-046-AC4** — Given the policy, when the subject reads query
  results, entity detail, or audit after-state payloads, then the same
  redaction applies on all of them.

## Edge Cases

- **Required schema field**: a field marked required by the JSON Schema can
  still be redacted on read.
- **Policy removed**: after a policy version that removes the rule activates,
  new reads return the field; in-flight requests use their snapshot.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Redacted for low privilege | US-046-AC1 | Deny rule on `salary` for `read` role | Read employee as `read` subject | `salary` redacted |
| Visible for authorized | US-046-AC2 | Same entity | Read as authorized subject | Full entity returned |
| Stable redaction shape | US-046-AC3 | Redacted read | Inspect response | Null redaction per CONTRACT-004 |
| Audit parity | US-046-AC4 | Mutated employee | Read audit after-state as `read` subject | Same field redacted |

## Dependencies

- **Stories**: US-102 (redaction mechanics), US-109 (policy authoring)
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-01, ACL-12
- **PRD Requirements**: FR-10, FR-12
- **External**: CONTRACT-004 (field-rule grammar, redaction semantics),
  CONTRACT-002 (GraphQL nullability)

## Out of Scope

- Field write denial (US-047, US-103).
- Masking formats other than redaction-to-null (e.g., partial masking).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
