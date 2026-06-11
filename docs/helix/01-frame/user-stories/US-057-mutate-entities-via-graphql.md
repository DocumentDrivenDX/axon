---
ddx:
  id: US-057
---

# US-057: Mutate Entities via GraphQL

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-12, GQL-13, GQL-14
**PRD Requirements**: FR-5, FR-6, FR-20, FR-28
**Priority**: P0
**Status**: Draft

## Story

**As** an agent or UI client built by Ava (agent application developer)
**I want** to create, update, patch, and delete entities via GraphQL
**So that** I can use a single API for both reads and writes

## Context

GraphQL is the primary application write surface; generated mutations must
carry the full governed-write semantics — optimistic concurrency, lifecycle
validation, atomic transactions, and the approval-required safe default.
This story exercises GQL-12 through GQL-14. Mutation names, inputs, and
error extension codes are normative in CONTRACT-002.

## Walkthrough

1. The client creates an entity through the generated create mutation and
   receives it with ID and version.
2. The client updates it supplying the expected version; the update succeeds
   and the version increments.
3. A concurrent client updates first; the original client's next update
   returns a version conflict carrying current state, and it retries
   correctly.
4. The client patches a single field, transitions the entity's lifecycle
   state, and finally commits a multi-operation transaction atomically.
5. A mutation that policy routes for approval returns an approval-required
   result instead of committing.

## Acceptance Criteria

- [ ] **US-057-AC1** — Given a registered collection, when the client runs
  the generated create mutation, then the entity returns with ID and
  version.
- [ ] **US-057-AC2** — Given the current entity version, when the client
  updates with the correct expected version, then the update succeeds.
- [ ] **US-057-AC3** — Given a stale expected version, when the client
  updates, then a version-conflict error returns with the current entity
  state in the error extensions (codes per CONTRACT-002).
- [ ] **US-057-AC4** — Given a JSON merge patch, when the client runs the
  generated patch mutation, then only the specified fields change.
- [ ] **US-057-AC5** — Given an existing entity, when the client runs the
  generated delete mutation, then the entity is removed.
- [ ] **US-057-AC6** — Given a lifecycle-declared collection, when the
  client runs the generated transition mutation with an invalid transition,
  then the error lists the valid target states.
- [ ] **US-057-AC7** — Given a multi-operation transaction mutation, when it
  executes, then either all operations commit or none do — partial success
  is impossible.
- [ ] **US-057-AC8** — Given a direct mutation that policy classifies as
  needing approval, when it executes, then an approval-required result
  returns and no entity or link state mutates.

## Edge Cases

- **Patch removing a required field**: Fails schema validation with a
  field-level structured error; no partial write.
- **Transaction with one invalid operation**: The whole transaction rolls
  back and the error identifies the failing operation.
- **Delete with stale expected version**: Returns the same structured
  version conflict as update.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Create | US-057-AC1 | Valid bead payload | Generated create mutation | Entity with ID, version 1 |
| OCC success | US-057-AC2 | Entity at version 1 | Update with expected version 1 | Version 2 |
| OCC conflict | US-057-AC3 | Entity advanced to version 2 | Update with expected version 1 | Conflict error + current state in extensions |
| Merge patch | US-057-AC4 | Bead with title and status | Patch status only | Title unchanged, status updated |
| Invalid transition | US-057-AC6 | Lifecycle draft → done not allowed | Transition draft→done | Error listing valid targets |
| Atomic transaction | US-057-AC7 | 3 ops, third invalid | Commit transaction | No operation applied; error names op 3 |
| Approval-routed | US-057-AC8 | Policy: amount > 10000 needs approval | Direct update to 12000 | Approval-required result; entity unchanged |

## Dependencies

- **Stories**: US-049 (generated types discoverable)
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-12, GQL-13, GQL-14
- **PRD Requirements**: FR-5, FR-6, FR-20, FR-28
- **External**: CONTRACT-002 (mutation names, inputs, error extensions)

## Out of Scope

- The preview/approve/commit intent workflow itself (US-111).
- MCP write tools (US-053).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
