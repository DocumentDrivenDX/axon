---
ddx:
  id: US-047
  review:
    self_hash: 43bb11bcdf0931947f1a936c574e8ad78118b786f283baf8693fef29e1ad3b3b
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-047: Attribute-Based Write Control

**Feature**: FEAT-029 — Data-Layer Access Control Policies
**Feature Requirements**: ACL-01, ACL-07, ACL-13
**PRD Requirements**: FR-10, FR-11
**Priority**: P0
**Status**: Approved

## Story

**As an** operator (Wei, Business Workflow Builder persona)
**I want** to control who can edit which collections and fields
**So that** PRD authors can't edit technical designs and vice versa

## Context

Originally defined under FEAT-012; moved to FEAT-029 ownership because
collection- and field-scoped write policies based on subject attributes are
schema-declared data policies governed by FEAT-029 (grammar per
CONTRACT-004). Complementary per-user write scopes and field immutability are
expressed as policy rules, not identity-level roles.

## Walkthrough

1. Operator declares write policies giving one user write access on one
   collection and read-only access on another, with the complementary rules
   for a second user.
2. System compiles and activates the policy.
3. The first user updates an entity in their writable collection
   successfully, then attempts an update in the read-only collection and is
   denied with a stable policy error.
4. A field-level write-deny rule prevents non-privileged subjects from
   changing a protected field.

## Acceptance Criteria

- [ ] **US-047-AC1** — Given a policy granting a subject write on collection
  A and read on collection B, when that subject updates an entity in A, then
  the write succeeds.
- [ ] **US-047-AC2** — Given the same policy, when that subject attempts an
  update in collection B, then the write is denied with the stable forbidden
  envelope (CONTRACT-004).
- [ ] **US-047-AC3** — Given the complementary policy for a second subject,
  when the second subject writes in B and is denied in A, then the policy
  applies symmetrically.
- [ ] **US-047-AC4** — Given a field-level write-deny rule on a protected
  field, when a non-privileged subject's write includes that field, then the
  write fails naming the denied field path — the field is never silently
  preserved or dropped.

## Edge Cases

- **Write allowed, field denied**: a subject with row write access still
  fails when the payload touches a denied field.
- **Deny precedence**: a matching deny rule overrides any matching allow for
  the same subject and operation.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Writable collection | US-047-AC1 | erik: write on `technical-designs` | Update entity there | Success |
| Read-only collection | US-047-AC2 | erik: read on `prds` | Update entity in `prds` | Forbidden with stable envelope |
| Complementary policy | US-047-AC3 | mike: write `prds`, read `technical-designs` | Reverse operations | Mirror outcomes |
| Protected field | US-047-AC4 | Write-deny on `approved_by` | Update including `approved_by` | Fails naming the field path |

## Dependencies

- **Stories**: US-103 (denied-write mechanics), US-109 (policy authoring)
- **Feature Spec**: FEAT-029
- **Feature Requirements**: ACL-01, ACL-07, ACL-13
- **PRD Requirements**: FR-10, FR-11
- **External**: CONTRACT-004 (rule grammar, decision combination, denial
  envelope)

## Out of Scope

- Read redaction (US-046, US-102).
- Approval-routed writes (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
