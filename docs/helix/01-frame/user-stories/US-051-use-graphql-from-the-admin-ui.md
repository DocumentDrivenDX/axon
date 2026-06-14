---
ddx:
  id: US-051
  review:
    self_hash: 8de90a63b43b3a437d5b20007e69843f7b11dd9faedca40f0d9b16b0ea1e9f53
    deps: {}
    reviewed_at: "2026-06-14T03:52:45Z"
---

# US-051: Use GraphQL from the Admin UI

**Feature**: FEAT-015 — GraphQL Query Layer
**Feature Requirements**: GQL-05, GQL-06, GQL-12
**PRD Requirements**: FR-20, FR-24
**Priority**: P1
**Status**: Draft

## Story

**As** Wei, a business workflow builder operating Axon through the admin web
UI
**I want** the admin UI to drive all tenant data-plane and control-plane
workflows through GraphQL
**So that** the UI is built on efficient, type-safe queries with the same
governed semantics as every other client

## Context

The admin UI (FEAT-011) is itself a GraphQL client; it must not need a
private API. This story exercises FEAT-015's query and mutation surface
(GQL-05, GQL-06, GQL-12) from the perspective of its most demanding
first-party consumer, including the control-plane GraphQL endpoint defined in
CONTRACT-002.

## Walkthrough

1. Wei opens the admin UI; the collection list view loads via the collection
   introspection query.
2. Wei browses a collection; filtering and pagination flow through generated
   filter inputs and connections.
3. Wei creates, edits, links, transitions, and recovers entities; each action
   issues tenant-scoped GraphQL operations.
4. Wei administers tenants, users, members, credentials, and databases; those
   flows use the control-plane GraphQL endpoint.
5. Wei opens an entity detail view; the UI fetches entity, links, and recent
   audit in one consolidated query and renders within the latency target.

## Acceptance Criteria

- [ ] **US-051-AC1** — Given the admin UI, when the collection list view
  loads, then the data comes from the GraphQL collection introspection query
  (per CONTRACT-002).
- [ ] **US-051-AC2** — Given a collection browse view, when Wei filters and
  pages, then the requests use generated GraphQL filter inputs and
  connections.
- [ ] **US-051-AC3** — Given entity management flows (create, read, update,
  delete, links, lifecycle transitions, entity rollback, audit revert,
  markdown template management, and schema/collection administration), when
  Wei performs them in the UI, then each uses tenant-scoped GraphQL
  operations.
- [ ] **US-051-AC4** — Given control-plane flows (tenant, user, tenant
  member, credential, database), when Wei performs them, then they use the
  control-plane GraphQL endpoint (per CONTRACT-002).
- [ ] **US-051-AC5** — Given an entity detail view, when it loads, then
  entity, links, and recent audit are fetched in one consolidated GraphQL
  query where practical.
- [ ] **US-051-AC6** — Given the consolidated entity detail query, when it
  executes against the reference dataset, then it completes in under 200ms
  p99.

## Edge Cases

- **Policy-restricted operator**: UI views render policy-filtered data
  without errors; denied actions surface the policy explanation.
- **Concurrent edit in another client**: The UI's stale update receives the
  structured version conflict and offers a refresh, not a silent overwrite.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| Collection list | US-051-AC1 | 5 registered collections | Load collections view | Introspection query observed; 5 collections rendered |
| Filtered browse | US-051-AC2 | 100 beads, filter `status=pending` | Filter + next page | Generated filter input used; correct page contents |
| Entity lifecycle flow | US-051-AC3 | Bead with lifecycle schema | Create → transition → revert via UI | All operations visible as tenant-scoped GraphQL |
| Control plane | US-051-AC4 | Admin credential | Create a database via UI | Control-plane GraphQL operation observed |
| Detail latency | US-051-AC6 | Entity with 10 links, 20 audit rows | Load detail view 100 times | p99 < 200ms |

## Dependencies

- **Stories**: US-048, US-057
- **Feature Spec**: FEAT-015
- **Feature Requirements**: GQL-05, GQL-06, GQL-12
- **PRD Requirements**: FR-20, FR-24
- **External**: CONTRACT-002 (data-plane and control-plane GraphQL),
  FEAT-011 (admin web UI feature)

## Out of Scope

- Admin UI layout, navigation, and UX (FEAT-011 stories).
- Policy and intent admin screens (FEAT-031 stories).

## Review Checklist

Use this checklist when reviewing a user story:

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
