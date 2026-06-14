---
ddx:
  id: US-009
  review:
    self_hash: 4af4b4b7f90ec4c0582e4a817bacce585aecb06af370e58eadee1cbf2b404d68
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-009: Attach Metadata to Mutations

**Feature**: FEAT-003 — Audit Log
**Feature Requirements**: AUD-06
**PRD Requirements**: FR-15
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer instrumenting her agents
**I want** my agents' writes to carry context (reason, session ID, correlation ID)
**So that** the audit trail explains why a change happened, not just what changed

## Context

Repair-grade provenance needs more than before/after state: investigations
hinge on correlating mutations with agent sessions and stated reasons. This
story exercises FEAT-003's AUD-06 — caller-supplied audit metadata stored
with the entry, returned on query, and guaranteed to never influence the
operation itself.

## Walkthrough

1. Ava's agent performs a write and attaches metadata such as a reason and a session correlation ID.
2. The system commits the mutation and stores the metadata on the audit entry.
3. Later, Ava queries audit history and sees each entry's metadata alongside operation, actor, and diff.
4. She filters her investigation by correlation ID in her tooling using the returned metadata values.

## Acceptance Criteria

- [ ] **US-009-AC1** — Given a write operation with optional audit metadata attached, when the mutation commits, then the metadata is stored on the resulting audit entry.
- [ ] **US-009-AC2** — Given a stored entry with metadata, when audit history is queried, then the metadata is returned with the entry.
- [ ] **US-009-AC3** — Given metadata with non-string values or a malformed shape, when the write is submitted, then it is rejected with a structured error (metadata is simple string key-value, per CONTRACT-005).
- [ ] **US-009-AC4** — Given two identical writes, one with and one without metadata, when both commit, then the entity outcomes are identical — metadata never affects the operation.

## Edge Cases

- **Empty metadata object**: treated as absent; the entry is created without metadata.
- **Oversized metadata**: rejected with a structured error per CONTRACT-005 limits; the mutation does not commit partially.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-009-AC1 | Update with metadata `{reason: "retry", session: "s-9"}` | Commit, then query audit | Entry carries both metadata keys |
| Returned on query | US-009-AC2 | Entry with metadata exists | Query audit by entity | Metadata present in response |
| Invalid shape | US-009-AC3 | Metadata value is a nested object | Submit write | Structured validation error; no mutation |
| Purely informational | US-009-AC4 | Same patch with/without metadata | Commit both (fresh entities) | Identical entity end states |

## Dependencies

- **Stories**: None
- **Feature Spec**: FEAT-003
- **Feature Requirements**: AUD-06
- **PRD Requirements**: FR-15
- **External**: CONTRACT-005 (metadata field rules), CONTRACT-001 (write surfaces accepting audit metadata)

## Out of Scope

- Server-side filtering of audit queries by metadata values (unsupported in V1; see CONTRACT-001).
- Structured/nested metadata values.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
