---
ddx:
  id: US-118
---

# US-118: Handle Stale And Mismatched Intents Safely

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-15
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As an** approver (Wei, Business Workflow Builder persona)
**I want** stale or mismatched intents to be obvious and uncommittable
**So that** I cannot approve a different write than the one previewed

## Context

Intent bindings (entity versions, schema version, policy version, grant
version, operation hash) guarantee the committed write equals the reviewed
one. The UI's job is to make a broken binding impossible to miss and
impossible to commit, while preserving lineage through re-preview.

## Walkthrough

1. Approver opens an intent whose target entity changed after preview.
2. System shows the stale state, disables commit, and offers re-preview.
3. Approver re-previews; the system creates a new intent preserving lineage
   to the prior stale one.
4. Expired intents remain visible in history but cannot be approved or
   committed.

## Acceptance Criteria

- [ ] **US-118-AC1** — Given an entity version change after preview, when the
  intent detail view renders, then it shows the stale state, disables commit,
  and offers re-preview.
- [ ] **US-118-AC2** — Given a policy, schema, grant, or operation-hash
  change after preview, when commit is attempted, then the UI shows the
  specific stale or mismatch code and no partial commit occurs.
- [ ] **US-118-AC3** — Given an expired intent, when it is viewed, then it is
  visible in history but cannot be approved or committed.
- [ ] **US-118-AC4** — Given a re-preview, when the new intent is created,
  then it has a new intent identifier and preserves lineage to the prior
  stale intent.

## Edge Cases

- **Staleness while the detail view is open**: the next interaction surfaces
  the stale state rather than submitting against changed bindings.
- **Multiple binding changes**: the UI reports each violated binding, not
  just the first.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Entity staleness | US-118-AC1 | Entity mutated post-preview | Open intent | Stale shown; commit disabled; re-preview offered |
| Mismatch codes | US-118-AC2 | Policy version bumped | Attempt commit | Specific code; no partial commit |
| Expired intent | US-118-AC3 | Intent past expiry | View in history | Visible; approve/commit unavailable |
| Lineage on re-preview | US-118-AC4 | Stale intent | Re-preview | New intent ID linked to prior |

## Dependencies

- **Stories**: US-107 (FEAT-030 stale execution semantics), US-117 (intent
  detail view)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-15
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-002 (intent operations, stale/conflict extensions),
  CONTRACT-004 (reason codes)

## Out of Scope

- The binding/rebinding algorithm itself (FEAT-030).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
