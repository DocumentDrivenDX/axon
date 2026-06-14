---
ddx:
  id: US-115
  review:
    self_hash: 44b573bc03b40a0d52e71a636a8a51ce2727934dc3bf46e1c7fe7b94b7baf499
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-115: Browse Entities With Policy-Safe UI Semantics

**Feature**: FEAT-031 — Policy and Intents Admin UI
**Feature Requirements**: PUI-08, PUI-09, PUI-10
**PRD Requirements**: FR-24, FR-30
**Priority**: P0
**Status**: Approved

## Story

**As an** operator or developer (Wei, Business Workflow Builder persona)
**I want** entity lists, relationship tabs, and audit views to reflect the same policy results as GraphQL
**So that** the web UI cannot mislead me or leak hidden data

## Context

The console itself must obey the policy engine: hidden rows absent, redacted
fields explicit, denied writes honest. The UI renders policy-filtered GraphQL
results and never reconstructs or caches values the backend withheld.

## Walkthrough

1. Viewer opens a collection list as a policy-limited subject.
2. System renders rows, relationship tabs, cursors, and counts from
   policy-filtered GraphQL results.
3. Viewer opens an entity with redacted fields; the UI shows an explicit
   redacted state.
4. Viewer attempts a denied write; the UI shows the stable code, field path,
   and explanation, and does not optimistically update.

## Acceptance Criteria

- [ ] **US-115-AC1** — Given a policy-limited viewer, when entity lists,
  relationship traversal, cursors, and total counts render, then they match
  policy-filtered GraphQL results.
- [ ] **US-115-AC2** — Given redacted fields, when list, detail,
  relationship, and audit views render, then redaction is shown as an
  explicit state and the original value is absent from the DOM.
- [ ] **US-115-AC3** — Given a denied write, when the UI receives the error,
  then it displays the GraphQL error code, field path, and policy explanation
  without applying an optimistic UI update.
- [ ] **US-115-AC4** — Given audit views, when the current viewer reads them,
  then the same redaction rules apply as for entity reads.

## Edge Cases

- **Clipboard/export**: redacted states are never copyable or exportable as
  original values.
- **Live updates**: any subscription-driven UI refresh must not surface
  hidden rows or redacted values.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Filtered lists | US-115-AC1 | 10 rows, 3 visible to viewer | Open list | 3 rows; counts/cursors match GraphQL |
| Redacted DOM | US-115-AC2 | Field redacted for viewer | Inspect DOM | Redacted state shown; value absent |
| Honest denial | US-115-AC3 | Write denied by policy | Submit edit | Code + field path + explanation; no optimistic state |
| Audit redaction | US-115-AC4 | Audit entry with redacted field | Open audit view | Same redaction as entity read |

## Dependencies

- **Stories**: US-101, US-102, US-103 (backend policy semantics)
- **Feature Spec**: FEAT-031
- **Feature Requirements**: PUI-08, PUI-09, PUI-10
- **PRD Requirements**: FR-24, FR-30
- **External**: CONTRACT-002 (GraphQL shapes), CONTRACT-004 (denial codes)

## Out of Scope

- Enforcement itself (FEAT-029) — the UI renders, it does not decide.
- Mutation preview flows (US-116).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
