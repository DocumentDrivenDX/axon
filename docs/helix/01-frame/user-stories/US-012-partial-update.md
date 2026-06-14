---
ddx:
  id: US-012
  review:
    self_hash: 8d9348aa16a05099a94937f0755e324aab93cecfeae23842f304d50846639423
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-012: Partial Update

**Feature**: FEAT-004 — Entity Operations
**Feature Requirements**: ENT-03, ENT-04, ENT-06, ENT-10
**PRD Requirements**: FR-1, FR-6
**Priority**: P0
**Status**: Draft

## Story

**As** Ava, an agent application developer
**I want** my agents to update specific fields without sending the entire entity
**So that** targeted changes are efficient and don't clobber fields the agent never read

## Context

Agents typically change one or two fields of a large record. This story
exercises FEAT-004's patch semantics (ENT-03, ENT-04): only supplied fields
change, the resulting entity must still validate, OCC applies exactly as for
full replacement, and a no-change patch is a true no-op (ENT-06). The patch
wire format is normative in CONTRACT-001.

## Walkthrough

1. Ava's agent reads an entity and decides to change its `status` field only.
2. The agent submits a patch containing just that field plus the expected version (per CONTRACT-001).
3. The system merges the patch, validates the resulting entity against the schema, commits, and increments the version.
4. All unmentioned fields are unchanged; the audit entry shows only the patched field in its diff.

## Acceptance Criteria

- [ ] **US-012-AC1** — Given an entity with many fields, when the agent submits a patch containing a subset of fields, then only those fields are modified and all unmentioned fields are preserved.
- [ ] **US-012-AC2** — Given a patch that would make the resulting entity schema-invalid, when submitted, then it is rejected with field-level validation errors and the stored entity is unchanged.
- [ ] **US-012-AC3** — Given a stale expected version, when a patch is submitted, then it fails with the same version-conflict behavior as full replacement (current state included, per CONTRACT-001).
- [ ] **US-012-AC4** — Given a patch whose values equal the stored values, when submitted, then the operation succeeds as a no-op: the version is not incremented and no audit entry is produced.

## Edge Cases

- **Patch removing a required field**: rejected by schema validation (the resulting entity must be valid).
- **Patch on a missing entity**: structured not-found error naming the ID.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Happy path | US-012-AC1 | Entity with 10 fields | Patch `{status: "done"}` + version | Only `status` changed; version +1 |
| Invalid result | US-012-AC2 | Schema requires `amount >= 0` | Patch `{amount: -5}` | Validation error; entity unchanged |
| Stale version | US-012-AC3 | Entity at version 4 | Patch with expected version 3 | Version conflict + current state |
| No-op | US-012-AC4 | `status` already `"done"` | Patch `{status: "done"}` | Success; version unchanged; no audit entry |

## Dependencies

- **Stories**: US-010 (CRUD basis)
- **Feature Spec**: FEAT-004
- **Feature Requirements**: ENT-03, ENT-04, ENT-06, ENT-10
- **PRD Requirements**: FR-1, FR-6
- **External**: CONTRACT-001 (patch format, OCC and conflict shapes), FEAT-002 (schema validation), FEAT-003 (audit diff)

## Out of Scope

- Multi-entity patches in one atomic operation (FEAT-008 transactions).
- JSON Patch-style positional array operations beyond the contract's merge semantics.

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
